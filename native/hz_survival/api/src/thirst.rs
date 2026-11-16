//! Thirst in the game: the server drains and fills each player's bar, the client
//! draws it. The rules are `hz_survival_core::thirst`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Flow, HealthHudEvent, Holder, Host,
    HudEvent, LivingEntityUseItemFinishEvent, LivingHealEvent, NativeEffect, NativeServerPlayer,
    NetFromServerEvent, Tag, Text, TooltipEvent,
};
use hz_survival_core::thirst::{
    self, BOTTLE, CHANNEL, Droplet, Exposure, MAX, Stage, Thirst, ThirstConfig, bottle_pixel,
};

/// A player's thirst, kept with them in the save (hundredths of a point).
static LEVEL: AttachmentType<f32> = AttachmentType {
    namespace: "hz_survival",
    name: "thirst",
    default: || MAX,
    codec: Some(AttachmentCodec {
        encode: |level| Ok([("level".into(), Tag::Int((*level * 100.0).round() as i32))].into()),
        decode: |data: &Compound| match data.get(&"level".into()) {
            Some(Tag::Int(hundredths)) => Ok(*hundredths as f32 / 100.0),
            _ => Err(AttachmentError::Decode),
        },
    }),
    // A respawn starts from the config's level instead.
    copy_on_death: false,
    sync: false,
};

const TICKS_PER_SECOND: u32 = 20;
/// A client is told again this often even when nothing changed, so one that missed
/// a message (a dimension change, a reload) catches up.
const RESEND_SECONDS: u32 = 10;
/// How long each debuff is given for; refreshed every second while it applies, so
/// it ends three seconds after the player drinks.
const EFFECT_TICKS: i32 = 60;
/// Natural healing and the Regeneration effect come a point at a time; a potion's
/// or a golden apple's instant healing is more, and is never stopped.
const SLOW_HEAL: f32 = 1.0;
const CREATIVE: i32 = 1;
const SPECTATOR: i32 = 3;

/// One player while they are on the server.
struct Online {
    thirst: Thirst,
    entity: u32,
    ticks: u32,
    /// What their client was last told.
    sent: Option<[u8; 2]>,
    since_sent: u32,
    /// The bar as last saved with the player.
    saved: u8,
}

#[derive(Default)]
pub struct State {
    online: Mutex<HashMap<u128, Online>>,
    /// The stage of each player by entity id, for `living_heal`, which names only
    /// the entity.
    stages: Mutex<HashMap<u32, Stage>>,
    rolls: AtomicU64,
    client: Client,
}

/// What the window knows: the last message from its server, and where the health
/// rows are this frame.
struct Client {
    /// `display | stage << 8`, or `UNKNOWN` before the server said anything.
    bar: AtomicU32,
    /// When the health rows were last laid out (milliseconds since `clock()`), and
    /// the screen and rows they were laid out for.
    rows_at: AtomicU64,
    width: AtomicI32,
    height: AtomicI32,
    rows_above_food: AtomicI32,
}

const UNKNOWN: u32 = u32::MAX;

impl Default for Client {
    fn default() -> Self {
        Self {
            bar: AtomicU32::new(UNKNOWN),
            rows_at: AtomicU64::new(0),
            width: AtomicI32::new(0),
            height: AtomicI32::new(0),
            rows_above_food: AtomicI32::new(1),
        }
    }
}

fn clock() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    // Never 0, which means "not yet".
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl State {
    /// A world is about to open: nobody is online yet, and entity ids start over.
    pub fn reset(&self) {
        lock(&self.online).clear();
        lock(&self.stages).clear();
    }

    /// A living player's server tick. Once a second: drain, effects, damage, and
    /// word to their client.
    pub fn player_tick(&self, host: &mut dyn Host, config: &ThirstConfig) {
        let Some(player) = host.acting_player() else {
            return;
        };
        if player.fake || !config.enabled {
            return;
        }
        let uuid = player.uuid;
        let entity = player.entity_id as u32;
        // Looked up and inserted under separate locks: no lock is held across a call
        // into the game, where a hook can fire.
        let known = lock(&self.online).contains_key(&uuid);
        if !known {
            let level = LEVEL.get(&mut *host, Holder::Player(uuid)).unwrap_or(MAX);
            let thirst = Thirst::new(level);
            let saved = thirst.display();
            lock(&self.online).insert(
                uuid,
                Online {
                    thirst,
                    entity,
                    ticks: 0,
                    sent: None,
                    since_sent: 0,
                    saved,
                },
            );
        }
        let due = {
            let mut online = lock(&self.online);
            let Some(state) = online.get_mut(&uuid) else {
                return;
            };
            state.entity = entity;
            state.ticks = state.ticks.wrapping_add(1);
            state.ticks % TICKS_PER_SECOND == 0
        };
        if !due {
            return;
        }
        let drains = !matches!(player.game_mode, CREATIVE | SPECTATOR)
            && (config.drain_on_peaceful || !peaceful(host, &player.dimension));
        let exposure = exposure(host, &player);
        let (second, message, save) = {
            let mut online = lock(&self.online);
            let Some(state) = online.get_mut(&uuid) else {
                return;
            };
            let second = if drains {
                state.thirst.second(config, &exposure, player.health)
            } else {
                thirst::Second {
                    stage: Stage::Quenched,
                    hurt: None,
                }
            };
            let display = state.thirst.display();
            let bytes = thirst::encode(display, second.stage);
            state.since_sent += 1;
            let message =
                (state.sent != Some(bytes) || state.since_sent >= RESEND_SECONDS).then(|| {
                    state.sent = Some(bytes);
                    state.since_sent = 0;
                    bytes
                });
            let save = (state.saved != display).then(|| {
                state.saved = display;
                state.thirst.level
            });
            (second, message, save)
        };
        lock(&self.stages).insert(entity, second.stage);
        if let Some(level) = save {
            let _ = LEVEL.set(&mut *host, Holder::Player(uuid), level);
        }
        if let Some(bytes) = message {
            // A vanilla client isn't sent it, which is fine: it has no bar to draw.
            let _ = host.net_send_to_player(uuid, CHANNEL, &bytes);
        }
        apply_stage(host, uuid, second.stage);
        if let Some(damage) = second.hurt {
            // Vanilla's own dehydration damage (a dolphin out of water), whose death
            // message is "<player> died from dehydration".
            host.hurt_entity(&player.dimension, entity, damage, "minecraft:dry_out", None);
        }
    }

    /// A player finished eating or drinking `event.stack`.
    pub fn use_item_finish(
        &self,
        host: &mut dyn Host,
        config: &ThirstConfig,
        event: &LivingEntityUseItemFinishEvent<'_>,
    ) {
        // A mob's use names its entity; a player's doesn't.
        if event.entity.is_some() || !config.enabled {
            return;
        }
        let Some(player) = host.acting_player() else {
            return;
        };
        let Some(drink) = config.drink(&event.stack.item, event.stack.potion()) else {
            return;
        };
        let uuid = player.uuid;
        let bytes = {
            let mut online = lock(&self.online);
            let Some(state) = online.get_mut(&uuid) else {
                return;
            };
            state.thirst.drink(drink.points);
            let bytes = thirst::encode(state.thirst.display(), config.stage(state.thirst.level));
            state.sent = Some(bytes);
            state.since_sent = 0;
            bytes
        };
        let _ = host.net_send_to_player(uuid, CHANNEL, &bytes);
        if drink.untreated {
            let seed = (uuid as u64) ^ self.rolls.fetch_add(1, Ordering::Relaxed);
            if thirst::roll(seed) < config.untreated_sickness_chance {
                host.add_effect(
                    uuid,
                    &NativeEffect {
                        id: "minecraft:hunger".into(),
                        amplifier: 0,
                        duration: (config.untreated_sickness_seconds * TICKS_PER_SECOND) as i32,
                        ambient: false,
                        particles: true,
                    },
                );
                host.action_bar(uuid, Text::literal("The water was not clean"));
            }
        }
    }

    /// A living entity is about to heal: a dehydrated player's slow healing stops.
    pub fn living_heal(&self, config: &ThirstConfig, event: &LivingHealEvent) -> Flow {
        if !config.enabled || event.amount > SLOW_HEAL {
            return Flow::Continue;
        }
        match lock(&self.stages).get(&event.entity) {
            Some(stage) if *stage >= Stage::Dehydrated => Flow::Cancel,
            _ => Flow::Continue,
        }
    }

    /// A respawn starts from the config's level, whatever the bar was at death.
    pub fn respawned(&self, host: &mut dyn Host, config: &ThirstConfig, uuid: u128) {
        let forgotten = lock(&self.online).remove(&uuid);
        if let Some(state) = forgotten {
            lock(&self.stages).remove(&state.entity);
        }
        let _ = LEVEL.set(&mut *host, Holder::Player(uuid), config.respawn_level);
    }

    /// A player left: their bar is saved with them.
    pub fn logged_out(&self, host: &mut dyn Host, uuid: u128) {
        let Some(state) = lock(&self.online).remove(&uuid) else {
            return;
        };
        lock(&self.stages).remove(&state.entity);
        let _ = LEVEL.set(&mut *host, Holder::Player(uuid), state.thirst.level);
    }

    // The window's side.

    pub fn net_from_server(&self, event: &NetFromServerEvent<'_>) {
        if event.channel != CHANNEL {
            return;
        }
        if let Some((display, stage)) = thirst::decode(event.data) {
            self.client
                .bar
                .store(u32::from(display) | (stage as u32) << 8, Ordering::Relaxed);
        }
    }

    /// The window left its server: the next one may not have the mod.
    pub fn client_left(&self) {
        self.client.bar.store(UNKNOWN, Ordering::Relaxed);
    }

    /// The health rows are being laid out: remember where, so that the thirst row
    /// goes above the food row, the air bubbles and a mount's hearts.
    pub fn health_rows(&self, event: &HealthHudEvent<'_>) {
        let inputs = event.inputs;
        let air_row = inputs.under_water || inputs.air < inputs.max_air;
        // A mount's hearts take the food row's place, ten to a row.
        let base = if inputs.vehicle_hearts > 0 {
            (inputs.vehicle_hearts + 9) / 10
        } else {
            1
        };
        self.client
            .rows_above_food
            .store(base + i32::from(air_row), Ordering::Relaxed);
        self.client.width.store(event.wi, Ordering::Relaxed);
        self.client.height.store(event.hi, Ordering::Relaxed);
        self.client.rows_at.store(clock(), Ordering::Relaxed);
    }

    /// Draws the thirst row: the content pack's droplets, or plain bars without it.
    pub fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        let bar = self.client.bar.load(Ordering::Relaxed);
        if bar == UNKNOWN {
            return;
        }
        // Only while the health rows show (not in creative, nor on a hidden HUD).
        let rows_at = self.client.rows_at.load(Ordering::Relaxed);
        if rows_at == 0 || clock().saturating_sub(rows_at) > 250 {
            return;
        }
        let display = (bar & 0xFF) as u8;
        let dying = (bar >> 8) == Stage::Dying as u32;
        let row = thirst::hud_row(
            display,
            self.client.width.load(Ordering::Relaxed),
            self.client.height.load(Ordering::Relaxed),
            self.client.rows_above_food.load(Ordering::Relaxed),
        );
        // An empty bar shakes, as the food row does when starving.
        let shake = |i: usize| -> f32 {
            if dying && (clock() / 100 + i as u64 * 7).is_multiple_of(5) {
                -1.0
            } else {
                0.0
            }
        };
        for (i, (x, y, droplet)) in row.into_iter().enumerate() {
            let (x, y) = (x as f32, y as f32 + shake(i));
            let sprite = match droplet {
                Droplet::Empty => "hz_content:gui/thirst_empty",
                Droplet::Half => "hz_content:gui/thirst_half",
                Droplet::Full => "hz_content:gui/thirst_full",
            };
            if host.sprite(event.batch, sprite, [0.0, 0.0, 9.0, 9.0], [x, y, 9.0, 9.0]) {
                continue;
            }
            // No texture (the content pack is off, or its GUI textures didn't load):
            // the same bottle, a pixel at a time.
            for (py, row) in BOTTLE.iter().enumerate() {
                for px in 0..row.len() {
                    if let Some(colour) = bottle_pixel(droplet, px, py) {
                        host.fill(
                            event.batch,
                            [x + px as f32, y + py as f32, 1.0, 1.0],
                            colour,
                        );
                    }
                }
            }
        }
    }

    /// Says what an item does to thirst where the player reads about it.
    pub fn tooltip(&self, config: &ThirstConfig, event: &mut TooltipEvent<'_>) {
        if !config.enabled {
            return;
        }
        if let Some(drink) = config.drink(&event.stack.item, event.stack.potion()) {
            let (line, color) = thirst::tooltip(drink.points);
            event.lines.push((Text::literal(line), color));
            if drink.untreated && config.untreated_sickness_chance > 0.0 {
                event
                    .lines
                    .push((Text::literal("Untreated: may make you sick"), 0xAAAAAAFF));
            }
        }
    }
}

fn peaceful(host: &dyn Host, dimension: &str) -> bool {
    host.level(dimension)
        .and_then(|level| host.level_difficulty(level))
        == Some("peaceful")
}

fn exposure(host: &dyn Host, player: &NativeServerPlayer) -> Exposure {
    let [x, y, z] = player.position;
    let block = [x.floor() as i32, y.floor() as i32, z.floor() as i32];
    let biome = host.biome(block);
    let entity = host.entity(player.entity_id as u32);
    let burning = entity
        .and_then(|e| host.entity_fire_ticks(e))
        .is_some_and(|ticks| ticks > 0);
    let wet = entity
        .and_then(|e| host.entity_is_in_water(e))
        .unwrap_or(false);
    Exposure {
        climate: thirst::climate_of(&player.dimension, biome.as_deref()),
        sprinting: player.sprinting,
        burning,
        wet,
    }
}

/// The debuffs of a stage, refreshed while it lasts. The content pack's own effect
/// is only the icon and the name; the vanilla ones do the work.
fn apply_stage(host: &mut dyn Host, uuid: u128, stage: Stage) {
    let mut give = |id: &str, amplifier: u8| {
        host.add_effect(
            uuid,
            &NativeEffect {
                id: id.into(),
                amplifier,
                duration: EFFECT_TICKS,
                ambient: true,
                particles: false,
            },
        );
    };
    match stage {
        Stage::Quenched => {}
        Stage::Parched => {
            give("hz_content:parched", 0);
            give("minecraft:slowness", 0);
        }
        Stage::Dehydrated => {
            give("hz_content:parched", 1);
            give("minecraft:slowness", 0);
            give("minecraft:weakness", 0);
        }
        Stage::Dying => {
            give("hz_content:parched", 2);
            give("minecraft:slowness", 1);
            give("minecraft:weakness", 0);
        }
    }
}
