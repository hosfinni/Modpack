//! Skills and strength in numbers: experience for work done, more of it in
//! company, and blows that land harder and hurt less beside other players.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, BreakSpeedEvent, Compound, Flow, Holder,
    Host, ItemStack, LivingIncomingDamageV3Event, NativeBlockEdit, NativeBlockRun,
    NetFromServerEvent, Tag, Text, UseItemOnEvent,
};
use hz_survival_core::Config;
use hz_survival_core::skills::{CHANNEL, Skill, Skills, decode_levels};
use hz_survival_core::thirst::roll;
use hz_survival_core::world::{is_ingot, near, washed};

/// A player's experience, kept with them in the save and through death.
static SKILLS: AttachmentType<Skills> = AttachmentType {
    namespace: "hz_survival",
    name: "skills",
    default: Skills::default,
    codec: Some(AttachmentCodec {
        encode: |skills| {
            Ok([(
                "xp".into(),
                Tag::IntArray(skills.xp.iter().map(|xp| *xp as i32).collect()),
            )]
            .into())
        },
        decode: |data: &Compound| match data.get(&"xp".into()) {
            Some(Tag::IntArray(xp)) if xp.len() == 3 => Ok(Skills {
                xp: [
                    xp[0].max(0) as u32,
                    xp[1].max(0) as u32,
                    xp[2].max(0) as u32,
                ],
            }),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: true,
    sync: false,
};

const SILK_TOUCH: &str = "minecraft:silk_touch";
const WATER_CAULDRON: &str = "minecraft:water_cauldron";
const GREEN: u32 = 0x55FF55;
/// `SoundSource.PLAYERS`.
const PLAYERS: usize = 7;

#[derive(Default)]
pub struct State {
    /// The players online, with their experience.
    skills: Mutex<HashMap<u128, Skills>>,
    /// Where each player was at their last tick: dimension and position.
    places: Mutex<HashMap<u128, (String, [f64; 3])>>,
    rolls: AtomicU64,
    /// This window's own levels, as its server last said them.
    own_levels: [AtomicU8; 3],
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl State {
    /// A world is about to open: nobody is online yet.
    pub fn reset(&self) {
        lock(&self.skills).clear();
        lock(&self.places).clear();
    }

    /// A living player's server tick: where they are, and their skills the first
    /// time they are seen.
    pub fn player_tick(&self, host: &mut dyn Host, config: &Config) {
        let Some(player) = host.acting_player() else {
            return;
        };
        if player.fake {
            return;
        }
        {
            // One guard for the look-up and the insert: locking again inside a match
            // on the first lock would wait on itself for ever.
            let mut places = lock(&self.places);
            match places.get_mut(&player.uuid) {
                Some((dimension, position)) => {
                    if *dimension != player.dimension {
                        dimension.clone_from(&player.dimension);
                    }
                    *position = player.position;
                }
                None => {
                    places.insert(player.uuid, (player.dimension.clone(), player.position));
                }
            }
        }
        // Looked up and inserted under separate locks: reading the save in between calls
        // into the game, and no lock is held across that.
        let known = lock(&self.skills).contains_key(&player.uuid);
        if !known {
            let skills = SKILLS
                .get(&mut *host, Holder::Player(player.uuid))
                .unwrap_or_default();
            lock(&self.skills).insert(player.uuid, skills);
            let _ = host.net_send_to_player(player.uuid, CHANNEL, &skills.encode(&config.skills));
        }
    }

    pub fn logged_out(&self, player: u128) {
        lock(&self.skills).remove(&player);
        lock(&self.places).remove(&player);
    }

    /// The other players within `range` of `player`.
    fn party(&self, player: u128, range: f64) -> Vec<u128> {
        let places = lock(&self.places);
        let Some((dimension, at)) = places.get(&player) else {
            return Vec::new();
        };
        places
            .iter()
            .filter(|(other, _)| **other != player)
            .filter(|(_, (other_dimension, position))| {
                near(
                    (dimension.as_str(), *at),
                    std::iter::once((other_dimension.as_str(), *position)),
                    range,
                ) == 1
            })
            .map(|(other, _)| *other)
            .collect()
    }

    /// `player` earned `xp` in `skill`: more of it in company, and the company gets a
    /// share.
    fn gain(&self, host: &mut dyn Host, config: &Config, player: u128, skill: Skill, xp: u32) {
        if !config.skills.enabled || xp == 0 {
            return;
        }
        let party = self.party(player, config.skills.party_range);
        let (earned, share) = config.skills.party(xp, party.len() as u32);
        let mut awards = vec![(player, earned)];
        awards.extend(party.into_iter().map(|member| (member, share)));
        for (who, amount) in awards {
            let changed = {
                let mut all = lock(&self.skills);
                let Some(skills) = all.get_mut(&who) else {
                    continue;
                };
                let level = skills.gain(&config.skills, skill, amount);
                (*skills, level)
            };
            let (skills, level) = changed;
            let _ = SKILLS.set(&mut *host, Holder::Player(who), skills);
            if let Some(level) = level {
                host.tell(
                    who,
                    Text::colored(format!("{} is now level {level}", skill.name()), GREEN),
                );
                let _ = host.net_send_to_player(who, CHANNEL, &skills.encode(&config.skills));
            }
        }
    }

    fn level(&self, config: &Config, player: u128, skill: Skill) -> u32 {
        lock(&self.skills)
            .get(&player)
            .map_or(0, |skills| skills.level(&config.skills, skill))
    }

    /// A player breaks a block: ore is Mining's work, unless Silk Touch lifts it
    /// whole (to be placed and broken again).
    pub fn block_broken(&self, host: &mut dyn Host, config: &Config, pos: [i32; 3]) {
        let Some(xp) = config.skills.mining_xp.get(&host.block(pos)).copied() else {
            return;
        };
        let Some(player) = host.acting_player() else {
            return;
        };
        if host
            .held_stack(player.uuid)
            .is_some_and(|stack| stack.enchantment_level(SILK_TOUCH) > 0)
        {
            return;
        }
        self.gain(host, config, player.uuid, Skill::Mining, xp);
    }

    pub fn mob_killed(&self, host: &mut dyn Host, config: &Config, player: u128, kind: &str) {
        self.gain(
            host,
            config,
            player,
            Skill::Combat,
            config.skills.kill_xp(kind),
        );
    }

    pub fn furnace_extract(
        &self,
        host: &mut dyn Host,
        config: &Config,
        player: u128,
        item: &str,
        count: u32,
    ) {
        if is_ingot(item) {
            self.gain(
                host,
                config,
                player,
                Skill::Smithing,
                config.skills.smelting_xp.saturating_mul(count),
            );
        }
    }

    /// How fast a block is dug: ore goes by the Mining level. Asked on the server for
    /// the digging player and on that player's own client, which must agree, so the
    /// client answers from the levels its server sent.
    pub fn break_speed(&self, host: &dyn Host, config: &Config, event: &mut BreakSpeedEvent) {
        if !config.skills.enabled || !config.skills.is_ore(&host.block(event.pos)) {
            return;
        }
        let level = match host.acting_player() {
            Some(player) => self.level(config, player.uuid, Skill::Mining),
            None => u32::from(self.own_levels[Skill::Mining.index()].load(Ordering::Relaxed)),
        };
        event.speed *= config.skills.mining_speed(level);
    }

    /// A hit is about to hurt: a player's blows go by their Combat level and their
    /// party, a monster takes less and deals more, and a player in a party is hurt
    /// less.
    pub fn incoming_damage(&self, config: &Config, event: &mut LivingIncomingDamageV3Event<'_>) {
        if let Some(attacker) = event.attacker_uuid {
            let allies = self.party(attacker, config.party.range).len() as u32;
            event.amount *= config.party.damage(allies);
            if config.skills.enabled {
                event.amount *=
                    config
                        .skills
                        .combat_damage(self.level(config, attacker, Skill::Combat));
            }
            if config.mobs.is_monster(event.kind) {
                event.amount /= config.mobs.toughness;
            }
        }
        if let Some(victim) = event.victim_uuid {
            // Hurt by a mob, or by something a mob shot.
            if config.mobs.enabled && event.attacker.is_some() && event.attacker_uuid.is_none() {
                event.amount *= config.mobs.damage;
            }
            let allies = self.party(victim, config.party.range).len() as u32;
            event.amount *= config.party.taken(allies);
        }
    }

    /// `/skills`: a player's levels, and how many are near enough to be their party.
    pub fn report(&self, host: &mut dyn Host, config: &Config, player: u128) {
        let Some(skills) = lock(&self.skills).get(&player).copied() else {
            return;
        };
        for line in skills.report(&config.skills) {
            host.tell(player, Text::literal(line));
        }
        let party = self.party(player, config.skills.party_range).len();
        let line = match party {
            0 => "Nobody is near: experience is worth more in company".to_owned(),
            1 => "1 player near: you both earn more".to_owned(),
            n => format!("{n} players near: you all earn more"),
        };
        host.tell(player, Text::colored(line, 0xAAAAAA));
    }

    /// Crushed ore used on a cauldron of water is washed: the last step before it
    /// smelts into a whole ingot.
    pub fn use_item_on(
        &self,
        host: &mut dyn Host,
        config: &Config,
        event: &UseItemOnEvent<'_>,
    ) -> Flow {
        if !config.refining.enabled {
            return Flow::Continue;
        }
        let Some(clean) = washed(&event.stack.item) else {
            return Flow::Continue;
        };
        if host.block(event.pos) != WATER_CAULDRON {
            return Flow::Continue;
        }
        let Some(player) = host.acting_player() else {
            return Flow::Continue;
        };
        if host.take_items(&event.stack.item, 1) == 0 {
            return Flow::Continue;
        }
        let draw = || {
            roll(
                (player.uuid as u64)
                    ^ self
                        .rolls
                        .fetch_add(1, Ordering::Relaxed)
                        .wrapping_mul(0x9E37_79B9),
            )
        };
        let smithing = self.level(config, player.uuid, Skill::Smithing);
        let count = if draw() < config.skills.washing_bonus(smithing) {
            2
        } else {
            1
        };
        if host
            .give_stack(player.uuid, ItemStack::new(clean, count))
            .is_some()
        {
            // No room: the ore goes back as it was.
            host.give_stack(player.uuid, ItemStack::new(&event.stack.item, 1));
            host.action_bar(player.uuid, Text::literal("No room for the washed ore"));
            return Flow::Cancel;
        }
        if draw() < config.refining.water_use {
            use_water(host, &player.dimension, event.pos);
        }
        let [x, y, z] = event.pos;
        host.play_sound(
            "minecraft:entity.generic.splash",
            [f64::from(x) + 0.5, f64::from(y) + 1.0, f64::from(z) + 0.5],
            PLAYERS,
            0.4,
            1.4,
        );
        self.gain(
            host,
            config,
            player.uuid,
            Skill::Smithing,
            config.skills.washing_xp,
        );
        Flow::Cancel
    }

    // The window's side.

    /// The server said this player's levels.
    pub fn net_from_server(&self, event: &NetFromServerEvent<'_>) {
        if event.channel != CHANNEL {
            return;
        }
        if let Some(levels) = decode_levels(event.data) {
            for (own, level) in self.own_levels.iter().zip(levels) {
                own.store(level, Ordering::Relaxed);
            }
        }
    }

    pub fn client_left(&self) {
        for own in &self.own_levels {
            own.store(0, Ordering::Relaxed);
        }
    }
}

/// Takes a level of water out of the cauldron at `pos`; the last one leaves it empty.
fn use_water(host: &mut dyn Host, dimension: &str, pos: [i32; 3]) {
    match host.block_property(pos, "level").as_deref() {
        Some("3") => {
            host.set_block_property(pos, "level", "2");
        }
        Some("2") => {
            host.set_block_property(pos, "level", "1");
        }
        _ => {
            let edit = NativeBlockEdit {
                palette: vec!["minecraft:cauldron".into()],
                runs: vec![NativeBlockRun {
                    x: pos[0],
                    z: pos[2],
                    y: pos[1],
                    height: 1,
                    surface: None,
                    block: 0,
                    only: None,
                }],
                neighbour_updates: true,
                ..NativeBlockEdit::default()
            };
            let _ = host.edit_blocks(dimension, &edit);
        }
    }
}
