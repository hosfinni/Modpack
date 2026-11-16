//! hz_arsenal: weapons that fight differently. The movesets are `hz-arsenal-core`;
//! this crate hooks them into Horizon.
//!
//! How one attack flows through the hooks, all on the server:
//! 1. `attack_entity`: the player swings at a target. The move is chosen here, from
//!    the weapon, the chain so far and how the attack is made, and kept as the
//!    player's pending strike.
//! 2. `living_incoming_damage_v3`: the hit's damage is multiplied by the move's. The
//!    same hook answers blows on a player who holds a stance.
//! 3. `living_knockback_v2`: the hit's knockback is scaled, or turned into a pull.
//! 4. `hurt_enemy`: the hit landed. The move's stagger, area and dash happen here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use horizon_client::mod_api::{
    AttackEntityEvent, ConfigLoadingEvent, ConfigReloadingEvent, CriticalHitEvent, Flow, Hooks,
    Host, HurtEnemyEvent, LivingIncomingDamageV3Event, LivingKnockbackV2Event, NativeConfig,
    NativeConfigKind, NativeServerPlayer, NetFromServerEvent, ParticleOptions, ParticleType,
    PlayerLoggedOutEvent, RegisterConfigsEvent, ServerAboutToStartEvent, ServerTickEvent,
    ServerTickPhase, Text, TooltipEvent, UseItemEvent,
};
use hz_arsenal_core::{
    Area, Around, Art, Attack, Config, DASH_CHANNEL, DEFAULT_CONFIG, Defence, Fighter,
    RAPIER_CRITICAL, Strike, Weapon, decode_dash, encode_dash, in_sight, is_behind, pull_toward,
};

const MOD_ID: &str = "hz_arsenal";
const CONFIG_FILE: &str = "hz_arsenal.toml";
const PLAYER_ATTACK: &str = "minecraft:player_attack";
/// The blows a stance answers: something swung at the player, not an arrow or a fall.
const MELEE: [&str; 4] = [
    "minecraft:player_attack",
    "minecraft:mob_attack",
    "minecraft:mob_attack_no_aggro",
    "minecraft:sting",
];
/// `SoundSource.PLAYERS`.
const PLAYERS: usize = 7;
const GOLD: u32 = 0xFFAA00FF;
const GREY: u32 = 0xAAAAAAFF;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Arsenal {
        config: RwLock::new(Arc::new(Config::shipped())),
        tick: AtomicU64::new(0),
        fighters: Mutex::new(HashMap::new()),
        by_entity: Mutex::new(HashMap::new()),
        staggered: Mutex::new(Vec::new()),
        deferred: Mutex::new(Vec::new()),
    })
}

/// A move chosen in `attack_entity`, waiting for its hit to go through.
#[derive(Clone, Copy)]
struct Pending {
    target: u32,
    strike: Strike,
    /// The wielder's attack damage, for the share an area deals.
    weapon_damage: f32,
    tick: u64,
}

#[derive(Default)]
struct Entry {
    fighter: Fighter,
    pending: Option<Pending>,
    /// The damage of the weapon last swung, for a riposte's share.
    weapon_damage: f32,
}

/// A mob that can't move for a while.
struct Staggered {
    dimension: String,
    mob: u32,
    until: u64,
}

/// Something decided inside a hook that has no host, done at the next tick's start.
enum Deferred {
    Announce {
        player: u128,
        text: &'static str,
    },
    StrikeBack {
        player: u128,
        attacker: u32,
        share: f32,
    },
}

/// One instance serves the window's game and the server thread's, so everything it
/// keeps is behind a lock or an atomic. No lock is held across a call into the
/// game: hurting an entity from a hook fires this mod's own damage hooks.
struct Arsenal {
    /// The shipped rules until the game hands over `config/hz_arsenal.toml`.
    config: RwLock<Arc<Config>>,
    /// Server ticks since the world opened.
    tick: AtomicU64,
    fighters: Mutex<HashMap<u128, Entry>>,
    /// Each fighter's entity id, for the hooks that name only the entity.
    by_entity: Mutex<HashMap<u32, u128>>,
    staggered: Mutex<Vec<Staggered>>,
    deferred: Mutex<Vec<Deferred>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The way a player faces along the ground, from their yaw (0 faces +z, 90 faces -x).
fn facing(player: &NativeServerPlayer) -> [f64; 2] {
    let yaw = f64::from(player.yaw).to_radians();
    [-yaw.sin(), yaw.cos()]
}

fn eye(player: &NativeServerPlayer) -> [f64; 3] {
    [
        player.position[0],
        player.position[1] + f64::from(player.eye_height),
        player.position[2],
    ]
}

impl Arsenal {
    fn config(&self) -> Arc<Config> {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn now(&self) -> u64 {
        self.tick.load(Ordering::Relaxed)
    }

    fn load(&self, host: &dyn Host, mod_id: &str, file: &str, text: &str) {
        if mod_id != MOD_ID || file != CONFIG_FILE {
            return;
        }
        match Config::parse(text) {
            Ok(config) => {
                *self.config.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(config);
            }
            // A broken file keeps the rules in use, and says so.
            Err(error) => host.log(
                "ERROR",
                &format!("[hz_arsenal] config/{CONFIG_FILE}: {error}"),
            ),
        }
    }

    /// The player's attack damage with the weapon in hand.
    fn weapon_damage(host: &mut dyn Host, player: u128) -> f32 {
        host.attribute(player, "minecraft:attack_damage")
            .map_or(1.0, |(_, value)| value as f32)
    }

    fn stagger(&self, dimension: &str, mob: u32, ticks: u32) {
        if ticks > 0 {
            let until = self.now() + u64::from(ticks);
            lock(&self.staggered).push(Staggered {
                dimension: dimension.to_owned(),
                mob,
                until,
            });
        }
    }

    /// Holds staggered mobs still for another tick, and lets go of the ones whose
    /// time is up.
    fn hold_staggered(&self, host: &mut dyn Host) {
        let now = self.now();
        let held: Vec<(String, u32)> = {
            let mut staggered = lock(&self.staggered);
            staggered.retain(|entry| entry.until >= now);
            staggered
                .iter()
                .map(|entry| (entry.dimension.clone(), entry.mob))
                .collect()
        };
        for (dimension, mob) in held {
            host.mob_stop(&dimension, mob);
            if let Some(entity) = host.entity(mob)
                && let Some([_, fall, _]) = host.entity_velocity(entity)
            {
                host.entity_set_velocity(entity, [0.0, fall.min(0.0), 0.0]);
            }
        }
    }

    /// Does what hooks without a host put off.
    fn run_deferred(&self, host: &mut dyn Host) {
        let deferred = std::mem::take(&mut *lock(&self.deferred));
        for item in deferred {
            match item {
                Deferred::Announce { player, text } => {
                    host.action_bar(player, Text::colored(text, GOLD >> 8));
                }
                Deferred::StrikeBack {
                    player,
                    attacker,
                    share,
                } => {
                    let Some(wielder) =
                        host.server_players().into_iter().find(|p| p.uuid == player)
                    else {
                        continue;
                    };
                    let damage = lock(&self.fighters)
                        .get(&player)
                        .map_or(1.0, |entry| entry.weapon_damage);
                    host.hurt_entity(
                        &wielder.dimension,
                        attacker,
                        damage * share,
                        PLAYER_ATTACK,
                        Some(wielder.entity_id as u32),
                    );
                    host.play_sound(
                        "minecraft:entity.player.attack.crit",
                        wielder.position,
                        PLAYERS,
                        1.0,
                        1.2,
                    );
                }
            }
        }
    }

    /// Hurts every living thing within `area` of `centre`, apart from the wielder
    /// and `spare`, and throws and staggers it as told.
    #[allow(clippy::too_many_arguments)]
    fn strike_area(
        &self,
        host: &mut dyn Host,
        wielder: &NativeServerPlayer,
        centre: [f64; 3],
        area: &Area,
        weapon_damage: f32,
        spare: Option<u32>,
        push: f64,
        stagger: u32,
    ) {
        let radius = f64::from(area.radius);
        let (low, high) = (centre.map(|v| v - radius), centre.map(|v| v + radius));
        let own = wielder.entity_id as u32;
        for id in host.entities_in(&wielder.dimension, low, high) {
            if id == own || Some(id) == spare {
                continue;
            }
            let Some(entity) = host.entity(id) else {
                continue;
            };
            if host.living(entity).is_none() {
                continue;
            }
            let Some(at) = host.entity_position(entity) else {
                continue;
            };
            let offset = [at[0] - centre[0], at[1] - centre[1], at[2] - centre[2]];
            if offset.iter().map(|v| v * v).sum::<f64>() > radius * radius {
                continue;
            }
            host.hurt_entity(
                &wielder.dimension,
                id,
                weapon_damage * area.damage,
                PLAYER_ATTACK,
                Some(own),
            );
            if push != 0.0 || area.lift != 0.0 {
                // Thrown away from the centre, and up.
                let mut thrown = pull_toward(centre, at, push);
                thrown[1] += f64::from(area.lift);
                host.push_mob(&wielder.dimension, id, thrown);
            }
            self.stagger(&wielder.dimension, id, stagger);
        }
        host.send_particles(
            &ParticleOptions::simple(ParticleType::SweepAttack),
            None,
            centre,
            [radius * 0.5, 0.2, radius * 0.5],
            0.0,
            6,
        );
        host.play_sound(
            "minecraft:entity.player.attack.sweep",
            centre,
            PLAYERS,
            1.0,
            0.8,
        );
    }

    /// Asks the wielder's client to carry its player forward: the server can't move
    /// a player, whose own game does.
    fn dash(host: &mut dyn Host, wielder: &NativeServerPlayer, speed: f32) {
        let [x, z] = facing(wielder);
        let velocity = [x as f32 * speed, 0.12, z as f32 * speed];
        let _ = host.net_send_to_player(wielder.uuid, DASH_CHANNEL, &encode_dash(velocity));
    }

    /// The nearest living thing along the wielder's line of sight, within `reach`.
    fn in_front(
        host: &mut dyn Host,
        wielder: &NativeServerPlayer,
        reach: f32,
    ) -> Option<(u32, [f64; 3])> {
        let own = wielder.entity_id as u32;
        let look = host
            .entity(own)
            .and_then(|entity| host.entity_view_vector(entity))?;
        let (eye, reach) = (eye(wielder), f64::from(reach));
        let (low, high) = (eye.map(|v| v - reach), eye.map(|v| v + reach));
        let mut nearest: Option<(f64, u32, [f64; 3])> = None;
        for id in host.entities_in(&wielder.dimension, low, high) {
            if id == own {
                continue;
            }
            let Some(entity) = host.entity(id) else {
                continue;
            };
            if host.living(entity).is_none() {
                continue;
            }
            let Some(feet) = host.entity_position(entity) else {
                continue;
            };
            // Aim at the middle of the body, not the feet.
            let middle = match host.entity_bounding_box(entity) {
                Some([low, high]) => [feet[0], (low[1] + high[1]) * 0.5, feet[2]],
                None => [feet[0], feet[1] + 0.9, feet[2]],
            };
            if let Some(distance) = in_sight(eye, look, middle, reach)
                && nearest.is_none_or(|(best, _, _)| distance < best)
            {
                nearest = Some((distance, id, feet));
            }
        }
        nearest.map(|(_, id, feet)| (id, feet))
    }
}

impl Hooks for Arsenal {
    fn register_configs(&self, _host: &mut dyn Host, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(
            NativeConfig::new(MOD_ID, NativeConfigKind::Common, DEFAULT_CONFIG).file(CONFIG_FILE),
        );
    }

    fn config_loading(&self, host: &mut dyn Host, event: &mut ConfigLoadingEvent<'_>) {
        let config = event.config;
        self.load(host, &config.mod_id, &config.file, &config.text);
    }

    fn config_reloading(&self, host: &mut dyn Host, event: &mut ConfigReloadingEvent<'_>) {
        let config = event.config;
        self.load(host, &config.mod_id, &config.file, &config.text);
    }

    fn server_about_to_start(&self, _host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        self.tick.store(0, Ordering::Relaxed);
        lock(&self.fighters).clear();
        lock(&self.by_entity).clear();
        lock(&self.staggered).clear();
        lock(&self.deferred).clear();
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        if !matches!(event.phase, ServerTickPhase::ServerTickStart { .. }) {
            return;
        }
        self.tick.fetch_add(1, Ordering::Relaxed);
        self.run_deferred(host);
        self.hold_staggered(host);
    }

    fn player_logged_out(&self, _host: &mut dyn Host, event: &mut PlayerLoggedOutEvent<'_>) {
        lock(&self.fighters).remove(&event.player);
        lock(&self.by_entity).retain(|_, player| *player != event.player);
    }

    /// A player swings at a target: choose the move.
    fn attack_entity(&self, host: &mut dyn Host, event: &mut AttackEntityEvent<'_>) -> Flow {
        let config = self.config();
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        let Some(weapon) = host
            .held_stack(player.uuid)
            .and_then(|stack| config.weapon(&stack.item))
        else {
            // Another weapon in hand: no move waits from an earlier swing.
            if let Some(entry) = lock(&self.fighters).get_mut(&player.uuid) {
                entry.pending = None;
            }
            return Flow::Continue;
        };
        let target = event.entity as u32;
        let yaw = host
            .entity(target)
            .and_then(|entity| host.entity_head_yaw(entity));
        let attack = Attack {
            tick: self.now(),
            target,
            sprinting: player.sprinting,
            sneaking: player.sneaking,
            airborne: !player.on_ground,
            behind: yaw.is_some_and(|yaw| is_behind(player.position, event.pos, yaw)),
        };
        let weapon_damage = Self::weapon_damage(host, player.uuid);
        lock(&self.by_entity).insert(player.entity_id as u32, player.uuid);
        let mut fighters = lock(&self.fighters);
        let entry = fighters.entry(player.uuid).or_default();
        let strike = entry.fighter.attack(&config, weapon, &attack);
        entry.weapon_damage = weapon_damage;
        entry.pending = Some(Pending {
            target,
            strike,
            weapon_damage,
            tick: attack.tick,
        });
        Flow::Continue
    }

    /// A hit is about to hurt: a pending move multiplies it, and a stance answers it.
    fn living_incoming_damage_v3(&self, event: &mut LivingIncomingDamageV3Event<'_>) -> Flow {
        let now = self.now();
        let mut fighters = lock(&self.fighters);
        if event.damage_type == PLAYER_ATTACK
            && let Some(pending) = event
                .attacker_uuid
                .and_then(|uuid| fighters.get(&uuid))
                .and_then(|e| e.pending)
            && pending.target == event.victim
            && pending.tick == now
        {
            event.amount *= pending.strike.damage;
        }
        let Some(victim) = event.victim_uuid else {
            return Flow::Continue;
        };
        let Some(entry) = fighters.get_mut(&victim) else {
            return Flow::Continue;
        };
        match entry
            .fighter
            .defend(now, MELEE.contains(&event.damage_type))
        {
            Defence::Taken => Flow::Continue,
            Defence::Guarded(taken) => {
                event.amount *= taken;
                Flow::Continue
            }
            Defence::Deflected { name, strike_back } => {
                drop(fighters);
                let mut deferred = lock(&self.deferred);
                deferred.push(Deferred::Announce {
                    player: victim,
                    text: name,
                });
                if let (Some(share), Some(attacker)) = (strike_back, event.attacker) {
                    deferred.push(Deferred::StrikeBack {
                        player: victim,
                        attacker,
                        share,
                    });
                }
                Flow::Cancel
            }
        }
    }

    /// The hit's knockback: scaled by the move, or turned around into a pull.
    fn living_knockback_v2(&self, event: &mut LivingKnockbackV2Event<'_>) -> Flow {
        let Some(player) = lock(&self.by_entity).get(&(event.attacker as u32)).copied() else {
            return Flow::Continue;
        };
        let Some(pending) = lock(&self.fighters)
            .get(&player)
            .and_then(|entry| entry.pending)
        else {
            return Flow::Continue;
        };
        if pending.target != event.entity as u32 || pending.tick != self.now() {
            return Flow::Continue;
        }
        let knockback = f64::from(pending.strike.knockback);
        event.strength *= knockback.abs();
        if knockback < 0.0 {
            (event.x, event.z) = (-event.x, -event.z);
        }
        Flow::Continue
    }

    /// The hit landed on a mob: the move's stagger, area and dash.
    fn hurt_enemy(&self, host: &mut dyn Host, event: &mut HurtEnemyEvent<'_>) {
        let Some(player) = host.acting_player() else {
            return;
        };
        let pending = lock(&self.fighters)
            .get_mut(&player.uuid)
            .and_then(|entry| entry.pending.take());
        let Some(pending) = pending.filter(|pending| pending.target == event.entity) else {
            return;
        };
        let strike = pending.strike;
        let at = host
            .entity(event.entity)
            .and_then(|entity| host.entity_position(entity));
        self.stagger(&player.dimension, event.entity, strike.stagger);
        if let Some(area) = &strike.area {
            let centre = match area.around {
                Around::Target => at.unwrap_or(player.position),
                Around::Wielder => player.position,
            };
            self.strike_area(
                host,
                &player,
                centre,
                area,
                pending.weapon_damage,
                Some(event.entity),
                0.4,
                strike.stagger,
            );
        }
        if strike.dash > 0.0 {
            Self::dash(host, &player, strike.dash);
        }
        if !strike.plain {
            if self.config().announce_moves {
                host.action_bar(player.uuid, Text::colored(strike.name, GOLD >> 8));
            }
            if let Some(at) = at {
                host.send_particles(
                    &ParticleOptions::simple(ParticleType::Crit),
                    None,
                    [at[0], at[1] + 1.0, at[2]],
                    [0.3, 0.4, 0.3],
                    0.2,
                    8,
                );
            }
        }
    }

    /// A rapier's critical hits cut deeper.
    fn critical_hit(&self, host: &mut dyn Host, event: &mut CriticalHitEvent<'_>) {
        if !event.critical {
            return;
        }
        let config = self.config();
        let rapier = host
            .acting_player()
            .and_then(|player| host.held_stack(player.uuid))
            .is_some_and(|stack| config.weapon(&stack.item) == Some(Weapon::Rapier));
        if rapier {
            event.damage_multiplier += RAPIER_CRITICAL * config.bonus_scale;
        }
    }

    /// Using a weapon is its art.
    fn use_item(&self, host: &mut dyn Host, event: &mut UseItemEvent<'_>) -> Flow {
        let config = self.config();
        let Some(weapon) = config.weapon(&event.stack.item) else {
            return Flow::Continue;
        };
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        let now = self.now();
        let art = lock(&self.fighters)
            .entry(player.uuid)
            .or_default()
            .fighter
            .art(&config, weapon, now);
        let art = match art {
            Ok(art) => art,
            Err(wait) => {
                let seconds = wait as f32 / 20.0;
                let line = format!("{} ready in {seconds:.1}s", weapon.moveset().art.name());
                host.action_bar(player.uuid, Text::colored(line, GREY >> 8));
                return Flow::Continue;
            }
        };
        lock(&self.by_entity).insert(player.entity_id as u32, player.uuid);
        let weapon_damage = Self::weapon_damage(host, player.uuid);
        if let Some(entry) = lock(&self.fighters).get_mut(&player.uuid) {
            entry.weapon_damage = weapon_damage;
        }
        match art {
            Art::Guard { .. } | Art::Parry { .. } | Art::Riposte { .. } => {
                host.play_sound(
                    "minecraft:item.armor.equip_iron",
                    player.position,
                    PLAYERS,
                    1.0,
                    0.7,
                );
            }
            Art::Sweep {
                area,
                knockback,
                stagger,
                ..
            } => {
                self.strike_area(
                    host,
                    &player,
                    player.position,
                    &area,
                    weapon_damage,
                    None,
                    f64::from(knockback) * 0.6,
                    stagger,
                );
            }
            Art::Lash {
                reach,
                damage,
                stagger,
                pull,
                ..
            } => match Self::in_front(host, &player, reach) {
                Some((target, at)) => {
                    let own = player.entity_id as u32;
                    host.hurt_entity(
                        &player.dimension,
                        target,
                        weapon_damage * damage,
                        PLAYER_ATTACK,
                        Some(own),
                    );
                    host.push_mob(
                        &player.dimension,
                        target,
                        pull_toward(at, player.position, f64::from(pull)),
                    );
                    self.stagger(&player.dimension, target, stagger);
                    host.play_sound(
                        "minecraft:entity.fishing_bobber.retrieve",
                        at,
                        PLAYERS,
                        1.0,
                        0.6,
                    );
                }
                // A miss still spends the art.
                None => {
                    host.play_sound(
                        "minecraft:entity.fishing_bobber.throw",
                        player.position,
                        PLAYERS,
                        1.0,
                        0.6,
                    );
                }
            },
            Art::Dash { speed, .. } => Self::dash(host, &player, speed),
        }
        if config.announce_moves {
            host.action_bar(player.uuid, Text::colored(art.name(), GOLD >> 8));
        }
        // The use is this mod's: the hand swings.
        Flow::Cancel
    }

    // The window's side.

    /// The server asks this client to carry its own player forward.
    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        if event.channel != DASH_CHANNEL {
            return;
        }
        if let Some(velocity) = decode_dash(event.data) {
            host.push_player(velocity);
        }
    }

    /// A weapon's tooltip lists its moves.
    fn tooltip(&self, _host: &dyn Host, event: &mut TooltipEvent<'_>) {
        let Some(weapon) = self.config().weapon(&event.stack.item) else {
            return;
        };
        for (label, moves) in weapon.describe() {
            event
                .lines
                .push((Text::literal(format!("{label}: {moves}")), GREY));
        }
    }
}
