//! hz_arsenal's movesets, with no game types: for each weapon, which move an attack
//! is and what it does. The trait mod in `../api` feeds these from Horizon's hooks.
//!
//! A weapon has three things of its own:
//! - a combo: hits landed soon after one another walk a chain of moves, ending in a
//!   finisher;
//! - openers: an attack made while sprinting, falling, sneaking or from behind is a
//!   move of its own, when the weapon has one for it;
//! - an art: using the weapon (right click) takes a stance, strikes around the
//!   wielder, lashes out at range or dashes.
//!
//! Time is counted in server ticks, 20 a second.

use serde::Deserialize;

/// The config the mod writes on its first run.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");
/// The channel the server asks a client to move its own player on.
pub const DASH_CHANNEL: &str = "hz_arsenal:dash";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub bonus_scale: f32,
    pub cooldown_scale: f32,
    pub announce_moves: bool,
    pub disabled: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            bonus_scale: 1.0,
            cooldown_scale: 1.0,
            announce_moves: true,
            disabled: Vec::new(),
        }
    }
}

impl Config {
    /// Reads a config file. Missing keys take their defaults; an unknown key is an
    /// error, so a typo doesn't silently leave a rule off.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        for (name, value) in [
            ("bonus_scale", config.bonus_scale),
            ("cooldown_scale", config.cooldown_scale),
        ] {
            if !(0.0..=10.0).contains(&value) {
                return Err(format!("{name} must be between 0 and 10, not {value}"));
            }
        }
        Ok(config)
    }

    /// The config a fresh install runs with.
    pub fn shipped() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the shipped config parses")
    }

    /// The weapon item `item` is, when its moveset is on.
    pub fn weapon(&self, item: &str) -> Option<Weapon> {
        if !self.enabled || self.disabled.iter().any(|disabled| disabled == item) {
            return None;
        }
        Weapon::of(item)
    }

    /// A move's multiplier with the config's scale on its bonus.
    fn scaled(&self, multiplier: f32) -> f32 {
        (1.0 + (multiplier - 1.0) * self.bonus_scale).max(0.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weapon {
    Greatsword,
    Katana,
    Dagger,
    Glaive,
    Warhammer,
    Rapier,
    Halberd,
    ChainWhip,
}

/// Where a strike's area is centred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Around {
    Target,
    Wielder,
}

/// A blow that also hits what stands near.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub radius: f32,
    /// The share of the weapon's damage the others take.
    pub damage: f32,
    pub around: Around,
    /// How hard the others are thrown upward, in blocks a tick.
    pub lift: f32,
}

/// What one attack does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub name: &'static str,
    /// Multiplies the hit's damage.
    pub damage: f32,
    /// Multiplies the hit's knockback; negative pulls the target in.
    pub knockback: f32,
    /// How long the target can't move, in ticks.
    pub stagger: u32,
    pub area: Option<Area>,
    /// How hard the wielder is carried forward, in blocks a tick.
    pub dash: f32,
    /// A plain hit of the chain, not worth announcing.
    pub plain: bool,
}

const fn hit(name: &'static str, damage: f32, knockback: f32) -> Strike {
    Strike {
        name,
        damage,
        knockback,
        stagger: 0,
        area: None,
        dash: 0.0,
        plain: true,
    }
}

const fn special(name: &'static str, damage: f32, knockback: f32) -> Strike {
    Strike {
        name,
        damage,
        knockback,
        stagger: 0,
        area: None,
        dash: 0.0,
        plain: false,
    }
}

impl Strike {
    const fn stagger(mut self, ticks: u32) -> Self {
        self.stagger = ticks;
        self
    }

    const fn area(mut self, radius: f32, damage: f32) -> Self {
        self.area = Some(Area {
            radius,
            damage,
            around: Around::Target,
            lift: 0.0,
        });
        self
    }

    const fn dash(mut self, speed: f32) -> Self {
        self.dash = speed;
        self
    }
}

/// What using a weapon does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Art {
    /// Brace: blows taken while it lasts are weakened, and the next attack hits
    /// harder.
    Guard {
        ticks: u32,
        taken: f32,
        then: Strike,
        primed_ticks: u32,
    },
    /// Turn aside the next blow within the window; doing so readies a counter.
    Parry {
        ticks: u32,
        then: Strike,
        primed_ticks: u32,
    },
    /// Turn aside the next blow within the window and strike back at once.
    Riposte { ticks: u32, damage: f32 },
    /// Strike everything around the wielder.
    Sweep {
        name: &'static str,
        area: Area,
        knockback: f32,
        stagger: u32,
    },
    /// Strike the first thing in front, out to `reach` blocks.
    Lash {
        name: &'static str,
        reach: f32,
        damage: f32,
        stagger: u32,
        pull: f32,
    },
    /// Leap forward, untouchable for a moment.
    Dash {
        name: &'static str,
        speed: f32,
        evade_ticks: u32,
    },
}

impl Art {
    pub fn name(&self) -> &'static str {
        match self {
            Art::Guard { .. } => "Guard",
            Art::Parry { .. } => "Counter Stance",
            Art::Riposte { .. } => "Riposte Stance",
            Art::Sweep { name, .. } | Art::Lash { name, .. } | Art::Dash { name, .. } => name,
        }
    }
}

/// A weapon's whole moveset.
pub struct Moveset {
    pub chain: &'static [Strike],
    /// How long after a hit the next one still continues the chain, in ticks.
    pub window: u32,
    pub sprint: Option<Strike>,
    pub air: Option<Strike>,
    pub sneak: Option<Strike>,
    /// From behind the target.
    pub behind: Option<Strike>,
    pub art: Art,
    pub art_cooldown: u32,
}

impl Weapon {
    pub const ALL: [Weapon; 8] = [
        Weapon::Greatsword,
        Weapon::Katana,
        Weapon::Dagger,
        Weapon::Glaive,
        Weapon::Warhammer,
        Weapon::Rapier,
        Weapon::Halberd,
        Weapon::ChainWhip,
    ];

    /// The item this weapon is in the content pack.
    pub fn item(self) -> &'static str {
        match self {
            Weapon::Greatsword => "hz_content:greatsword",
            Weapon::Katana => "hz_content:katana",
            Weapon::Dagger => "hz_content:dagger",
            Weapon::Glaive => "hz_content:glaive",
            Weapon::Warhammer => "hz_content:warhammer",
            Weapon::Rapier => "hz_content:rapier",
            Weapon::Halberd => "hz_content:halberd",
            Weapon::ChainWhip => "hz_content:chain_whip",
        }
    }

    pub fn of(item: &str) -> Option<Weapon> {
        Weapon::ALL.into_iter().find(|weapon| weapon.item() == item)
    }

    pub fn moveset(self) -> &'static Moveset {
        match self {
            Weapon::Greatsword => &GREATSWORD,
            Weapon::Katana => &KATANA,
            Weapon::Dagger => &DAGGER,
            Weapon::Glaive => &GLAIVE,
            Weapon::Warhammer => &WARHAMMER,
            Weapon::Rapier => &RAPIER,
            Weapon::Halberd => &HALBERD,
            Weapon::ChainWhip => &CHAIN_WHIP,
        }
    }

    /// The moveset in words, for the item's tooltip: a label and what it names.
    pub fn describe(self) -> Vec<(&'static str, String)> {
        let moves = self.moveset();
        let mut lines = Vec::new();
        let chain: Vec<&str> = moves.chain.iter().map(|strike| strike.name).collect();
        if self == Weapon::Rapier {
            lines.push((
                "Combo",
                "Thrust, stronger with each hit on one target".to_owned(),
            ));
        } else {
            lines.push(("Combo", chain.join(", ")));
        }
        for (label, strike) in [
            ("Sprinting", moves.sprint),
            ("Falling", moves.air),
            ("Sneaking", moves.sneak),
            ("From behind", moves.behind),
        ] {
            if let Some(strike) = strike {
                lines.push((label, strike.name.to_owned()));
            }
        }
        lines.push(("Use", moves.art.name().to_owned()));
        lines
    }
}

// Slow and heavy: a three-hit chain ending in a staggering overhead, and a guard
// that turns into a guard break.
static GREATSWORD: Moveset = Moveset {
    chain: &[
        hit("Cleave", 1.0, 1.0),
        hit("Cleave", 1.1, 1.0),
        special("Overhead", 1.6, 1.5).stagger(20),
    ],
    window: 50,
    sprint: Some(special("Charging Thrust", 1.3, 1.8)),
    air: Some(special("Plunging Cut", 1.5, 1.2).area(2.5, 0.5)),
    sneak: None,
    behind: None,
    art: Art::Guard {
        ticks: 30,
        taken: 0.4,
        then: special("Guard Break", 1.5, 1.6).stagger(15),
        primed_ticks: 50,
    },
    art_cooldown: 100,
};

// Fast and flowing: a four-hit chain, a dash cut, a draw that rewards waiting, and a
// counter.
static KATANA: Moveset = Moveset {
    chain: &[
        hit("Slash", 1.0, 0.6),
        hit("Slash", 1.0, 0.6),
        special("Cross Cut", 1.2, 0.6),
        special("Flourish", 1.45, 1.0),
    ],
    window: 25,
    sprint: Some(special("Dash Cut", 1.25, 0.3).dash(0.9)),
    air: None,
    // Only from rest: see `IAI_REST`.
    sneak: Some(special("Iai Draw", 1.8, 0.8)),
    behind: None,
    art: Art::Parry {
        ticks: 12,
        then: special("Counter", 2.0, 1.0),
        primed_ticks: 30,
    },
    art_cooldown: 60,
};

// Close and quick: barely any knockback, so the wielder stays on the target; double
// damage from behind, and a dash to get there.
static DAGGER: Moveset = Moveset {
    chain: &[
        hit("Stab", 1.0, 0.2),
        hit("Stab", 1.0, 0.2),
        special("Twist", 1.3, 0.2),
    ],
    window: 15,
    sprint: None,
    air: None,
    sneak: Some(special("Ambush", 1.4, 0.2)),
    behind: Some(special("Backstab", 2.2, 0.2)),
    art: Art::Dash {
        name: "Shadow Step",
        speed: 1.1,
        evade_ticks: 10,
    },
    art_cooldown: 50,
};

// Reach and crowd control: the chain ends in a sweep that hits everything near the
// target, and the art whirls around the wielder.
static GLAIVE: Moveset = Moveset {
    chain: &[
        hit("Thrust", 1.0, 1.0),
        hit("Thrust", 1.05, 1.0),
        special("Wide Sweep", 1.2, 1.2).area(3.0, 0.6),
    ],
    window: 40,
    sprint: Some(special("Lancing Charge", 1.4, 1.6)),
    air: None,
    sneak: None,
    behind: None,
    art: Art::Sweep {
        name: "Whirl",
        area: Area {
            radius: 3.5,
            damage: 0.8,
            around: Around::Wielder,
            lift: 0.0,
        },
        knockback: 1.2,
        stagger: 0,
    },
    art_cooldown: 90,
};

// Impact: every hit staggers and throws, the finisher shakes the ground, and the
// art slams it.
static WARHAMMER: Moveset = Moveset {
    chain: &[
        hit("Smash", 1.0, 1.6).stagger(10),
        hit("Smash", 1.15, 1.6).stagger(10),
        special("Earthshaker", 1.5, 2.0).stagger(30).area(3.0, 0.6),
    ],
    window: 55,
    sprint: None,
    air: Some(special("Meteor", 1.6, 2.0).stagger(30).area(3.5, 0.7)),
    sneak: None,
    behind: None,
    art: Art::Sweep {
        name: "Ground Slam",
        area: Area {
            radius: 4.0,
            damage: 0.7,
            around: Around::Wielder,
            lift: 0.45,
        },
        knockback: 1.0,
        stagger: 30,
    },
    art_cooldown: 120,
};

// Precision: no chain, but every hit in a row on one target hits harder (see
// `FOCUS_STEP`); a lunge, and a riposte.
static RAPIER: Moveset = Moveset {
    chain: &[hit("Thrust", 1.0, 0.5)],
    window: 22,
    sprint: Some(special("Lunge", 1.35, 0.8).dash(0.8)),
    air: None,
    sneak: None,
    behind: None,
    art: Art::Riposte {
        ticks: 10,
        damage: 1.5,
    },
    art_cooldown: 50,
};

// Control: the second hit of the chain hooks the target and pulls it in for the
// third; the art hooks at range.
static HALBERD: Moveset = Moveset {
    chain: &[
        hit("Chop", 1.0, 1.0),
        special("Hook", 0.8, -1.2),
        special("Cleave", 1.5, 1.4).stagger(15),
    ],
    window: 45,
    sprint: Some(special("Impale", 1.5, 1.8)),
    air: None,
    sneak: None,
    behind: None,
    art: Art::Lash {
        name: "Hook and Drag",
        reach: 5.0,
        damage: 0.5,
        stagger: 15,
        pull: 1.0,
    },
    art_cooldown: 80,
};

// Range: light hits from far away, a snare, and an entangling lash.
static CHAIN_WHIP: Moveset = Moveset {
    chain: &[
        hit("Lash", 1.0, 0.3),
        hit("Lash", 1.0, 0.3),
        special("Crack", 1.3, 0.5).stagger(10),
    ],
    window: 35,
    sprint: None,
    air: None,
    sneak: Some(special("Snare", 0.9, 0.0).stagger(25)),
    behind: None,
    art: Art::Lash {
        name: "Entangle",
        reach: 7.0,
        damage: 0.6,
        stagger: 40,
        pull: 0.4,
    },
    art_cooldown: 100,
};

/// How long a katana must rest (no attack) before a sneaking attack is an Iai Draw.
pub const IAI_REST: u64 = 60;
/// A rapier's bonus for each hit in a row on one target, and how many count.
pub const FOCUS_STEP: f32 = 0.08;
pub const FOCUS_MAX: u32 = 5;
/// A rapier's critical hits multiply by this much more than another weapon's.
pub const RAPIER_CRITICAL: f32 = 0.25;

/// How an attack was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attack {
    pub tick: u64,
    /// The entity attacked.
    pub target: u32,
    pub sprinting: bool,
    pub sneaking: bool,
    /// Off the ground.
    pub airborne: bool,
    /// The wielder stands behind the target's back.
    pub behind: bool,
}

/// A stance a fighter holds for a while.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stance {
    Guard {
        until: u64,
        taken: f32,
    },
    Parry {
        until: u64,
        then: Strike,
        primed_ticks: u32,
    },
    Riposte {
        until: u64,
        damage: f32,
    },
    Evade {
        until: u64,
    },
}

/// What a blow does to a fighter who may be in a stance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Defence {
    /// It lands as it is.
    Taken,
    /// It lands weakened: multiply its damage by this.
    Guarded(f32),
    /// It is turned aside. `strike_back` is the share of the fighter's weapon damage
    /// dealt to the attacker at once.
    Deflected {
        name: &'static str,
        strike_back: Option<f32>,
    },
}

/// One player's place in their movesets.
#[derive(Clone, Debug, Default)]
pub struct Fighter {
    weapon: Option<Weapon>,
    /// How many hits of the chain have landed.
    combo: usize,
    last_attack: Option<u64>,
    last_target: u32,
    /// Hits in a row on `last_target` (the rapier's focus).
    focus: u32,
    stance: Option<Stance>,
    /// A move readied by a stance, and the tick it is lost at.
    primed: Option<(Strike, u64)>,
    art_ready: u64,
}

impl Fighter {
    /// The move an attack with `weapon` is, with the config's scale on its damage.
    /// The fighter's chain and focus move on.
    pub fn attack(&mut self, config: &Config, weapon: Weapon, attack: &Attack) -> Strike {
        let moves = weapon.moveset();
        let rested = self
            .last_attack
            .is_none_or(|last| attack.tick.saturating_sub(last) >= IAI_REST);
        let in_window = self.weapon == Some(weapon)
            && self
                .last_attack
                .is_some_and(|last| attack.tick.saturating_sub(last) <= u64::from(moves.window));
        if !in_window {
            self.combo = 0;
        }
        if !in_window || self.last_target != attack.target {
            self.focus = 0;
        }

        let primed = self
            .primed
            .take()
            .filter(|(_, until)| attack.tick <= *until)
            .map(|(strike, _)| strike);
        // The first of these that applies is the move; an opener starts the chain over.
        let opener = primed
            .or(moves.behind.filter(|_| attack.behind))
            .or(moves.air.filter(|_| attack.airborne))
            .or(moves.sprint.filter(|_| attack.sprinting))
            .or(moves
                .sneak
                .filter(|_| attack.sneaking && (weapon != Weapon::Katana || rested)));
        let mut strike = match opener {
            Some(strike) => {
                self.combo = 0;
                strike
            }
            None => {
                let strike = moves.chain[self.combo % moves.chain.len()];
                self.combo = (self.combo + 1) % moves.chain.len();
                strike
            }
        };
        if weapon == Weapon::Rapier {
            strike.damage += FOCUS_STEP * self.focus.min(FOCUS_MAX) as f32;
            self.focus = (self.focus + 1).min(FOCUS_MAX);
        }
        strike.damage = config.scaled(strike.damage);

        self.weapon = Some(weapon);
        self.last_attack = Some(attack.tick);
        self.last_target = attack.target;
        // Attacking drops a guard.
        if matches!(self.stance, Some(Stance::Guard { .. })) {
            self.stance = None;
        }
        strike
    }

    /// Uses `weapon`'s art at `tick`. `Err` is how many ticks until it is ready.
    pub fn art(&mut self, config: &Config, weapon: Weapon, tick: u64) -> Result<Art, u64> {
        if tick < self.art_ready {
            return Err(self.art_ready - tick);
        }
        let moves = weapon.moveset();
        self.art_ready = tick + (moves.art_cooldown as f32 * config.cooldown_scale).round() as u64;
        let mut art = moves.art;
        match &mut art {
            Art::Guard {
                ticks,
                taken,
                then,
                primed_ticks,
            } => {
                self.stance = Some(Stance::Guard {
                    until: tick + u64::from(*ticks),
                    taken: *taken,
                });
                then.damage = config.scaled(then.damage);
                self.primed = Some((*then, tick + u64::from(*primed_ticks)));
            }
            Art::Parry {
                ticks,
                then,
                primed_ticks,
            } => {
                then.damage = config.scaled(then.damage);
                self.stance = Some(Stance::Parry {
                    until: tick + u64::from(*ticks),
                    then: *then,
                    primed_ticks: *primed_ticks,
                });
            }
            Art::Riposte { ticks, damage } => {
                *damage = config.scaled(*damage);
                self.stance = Some(Stance::Riposte {
                    until: tick + u64::from(*ticks),
                    damage: *damage,
                });
            }
            Art::Dash { evade_ticks, .. } => {
                self.stance = Some(Stance::Evade {
                    until: tick + u64::from(*evade_ticks),
                });
            }
            Art::Sweep { area, .. } => area.damage *= config.scaled(1.0).max(1.0),
            Art::Lash { .. } => {}
        }
        Ok(art)
    }

    /// A blow is about to land on the fighter at `tick`. `melee` blows are the ones a
    /// stance answers; arrows, falls and fire go through.
    pub fn defend(&mut self, tick: u64, melee: bool) -> Defence {
        let Some(stance) = self.stance else {
            return Defence::Taken;
        };
        let until = match stance {
            Stance::Guard { until, .. }
            | Stance::Parry { until, .. }
            | Stance::Riposte { until, .. }
            | Stance::Evade { until } => until,
        };
        if tick > until {
            self.stance = None;
            return Defence::Taken;
        }
        if !melee {
            return Defence::Taken;
        }
        match stance {
            // A guard holds for its whole time.
            Stance::Guard { taken, .. } => Defence::Guarded(taken),
            Stance::Evade { .. } => Defence::Deflected {
                name: "Evaded",
                strike_back: None,
            },
            // A parry or a riposte answers one blow.
            Stance::Parry {
                then, primed_ticks, ..
            } => {
                self.stance = None;
                self.primed = Some((then, tick + u64::from(primed_ticks)));
                Defence::Deflected {
                    name: "Parried",
                    strike_back: None,
                }
            }
            Stance::Riposte { damage, .. } => {
                self.stance = None;
                Defence::Deflected {
                    name: "Riposte",
                    strike_back: Some(damage),
                }
            }
        }
    }

    /// How many ticks until the weapon art is ready again at `tick`.
    pub fn art_wait(&self, tick: u64) -> u64 {
        self.art_ready.saturating_sub(tick)
    }
}

/// Whether someone at `attacker` stands behind the back of a target at `target`
/// facing `target_yaw` (degrees, as the game counts them: 0 faces +z, 90 faces -x).
/// "Behind" is the 120 degrees of arc opposite the way the target faces.
pub fn is_behind(attacker: [f64; 3], target: [f64; 3], target_yaw: f32) -> bool {
    let yaw = f64::from(target_yaw).to_radians();
    let facing = (-yaw.sin(), yaw.cos());
    let (dx, dz) = (attacker[0] - target[0], attacker[2] - target[2]);
    let distance = (dx * dx + dz * dz).sqrt();
    if distance < 1e-6 {
        return false;
    }
    (facing.0 * dx + facing.1 * dz) / distance < -0.5
}

/// How far along the line of sight `look` (a unit vector from `eye`) the point
/// `point` is, when it lies within `reach` and within about 25 degrees of that line.
/// A lash picks the nearest such target.
pub fn in_sight(eye: [f64; 3], look: [f64; 3], point: [f64; 3], reach: f64) -> Option<f64> {
    let to = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
    let distance = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
    if distance > reach || distance < 1e-6 {
        return None;
    }
    let along = (to[0] * look[0] + to[1] * look[1] + to[2] * look[2]) / distance;
    (along > 0.9).then_some(distance)
}

/// The push that carries something at `from` toward `to` at `speed` blocks a tick,
/// flat along the ground with a small hop.
pub fn pull_toward(from: [f64; 3], to: [f64; 3], speed: f64) -> [f64; 3] {
    let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
    let distance = (dx * dx + dz * dz).sqrt();
    if distance < 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    [
        dx / distance * speed,
        0.15 * speed.min(1.0),
        dz / distance * speed,
    ]
}

/// The bytes that ask a client to push its own player by `velocity`.
pub fn encode_dash(velocity: [f32; 3]) -> [u8; 12] {
    let mut bytes = [0; 12];
    for (chunk, value) in bytes.chunks_exact_mut(4).zip(velocity) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Reads [`encode_dash`]'s bytes. None for anything else, or for a push no dash
/// makes, which a client ignores.
pub fn decode_dash(data: &[u8]) -> Option<[f32; 3]> {
    let bytes: &[u8; 12] = data.try_into().ok()?;
    let mut velocity = [0.0; 3];
    for (value, chunk) in velocity.iter_mut().zip(bytes.chunks_exact(4)) {
        *value = f32::from_le_bytes(chunk.try_into().ok()?);
    }
    velocity
        .iter()
        .all(|value| value.is_finite() && value.abs() <= 2.0)
        .then_some(velocity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::shipped()
    }

    fn plain(tick: u64) -> Attack {
        Attack {
            tick,
            target: 7,
            sprinting: false,
            sneaking: false,
            airborne: false,
            behind: false,
        }
    }

    fn names(fighter: &mut Fighter, weapon: Weapon, ticks: &[u64]) -> Vec<&'static str> {
        ticks
            .iter()
            .map(|tick| fighter.attack(&config(), weapon, &plain(*tick)).name)
            .collect()
    }

    #[test]
    fn the_shipped_config_parses_and_bad_values_are_refused() {
        let config = config();
        assert!(config.enabled && config.announce_moves);
        assert_eq!(config.weapon("hz_content:katana"), Some(Weapon::Katana));
        assert_eq!(config.weapon("minecraft:iron_sword"), None);
        assert!(Config::parse("bonus_scale = -1.0").is_err());
        assert!(Config::parse("bonus_scal = 1.0").is_err());
        let off = Config::parse("disabled = [\"hz_content:katana\"]").unwrap();
        assert_eq!(off.weapon("hz_content:katana"), None);
        assert_eq!(off.weapon("hz_content:dagger"), Some(Weapon::Dagger));
        assert_eq!(
            Config::parse("enabled = false")
                .unwrap()
                .weapon("hz_content:dagger"),
            None
        );
    }

    #[test]
    fn every_weapon_has_an_item_and_a_moveset_of_its_own() {
        for weapon in Weapon::ALL {
            assert_eq!(Weapon::of(weapon.item()), Some(weapon));
            let moves = weapon.moveset();
            assert!(!moves.chain.is_empty() && moves.window > 0 && moves.art_cooldown > 0);
            assert!(!weapon.describe().is_empty());
        }
        // No two weapons share a chain or an art.
        for (i, a) in Weapon::ALL.iter().enumerate() {
            for b in &Weapon::ALL[i + 1..] {
                let chain =
                    |w: &Weapon| w.moveset().chain.iter().map(|s| s.name).collect::<Vec<_>>();
                assert_ne!(chain(a), chain(b), "{a:?} and {b:?}");
                assert_ne!(
                    a.moveset().art.name(),
                    b.moveset().art.name(),
                    "{a:?} and {b:?}"
                );
            }
        }
    }

    #[test]
    fn hits_in_quick_succession_walk_the_chain_and_start_it_over() {
        let mut fighter = Fighter::default();
        assert_eq!(
            names(&mut fighter, Weapon::Greatsword, &[0, 30, 60, 90]),
            ["Cleave", "Cleave", "Overhead", "Cleave"]
        );
        let mut fighter = Fighter::default();
        assert_eq!(
            names(&mut fighter, Weapon::Katana, &[0, 10, 20, 30, 40]),
            ["Slash", "Slash", "Cross Cut", "Flourish", "Slash"]
        );
    }

    #[test]
    fn waiting_too_long_or_changing_weapon_drops_the_chain() {
        let mut fighter = Fighter::default();
        // The greatsword's window is 50 ticks.
        assert_eq!(
            names(&mut fighter, Weapon::Greatsword, &[0, 30, 200]),
            ["Cleave", "Cleave", "Cleave"]
        );
        let mut fighter = Fighter::default();
        fighter.attack(&config(), Weapon::Katana, &plain(0));
        fighter.attack(&config(), Weapon::Katana, &plain(10));
        assert_eq!(
            fighter.attack(&config(), Weapon::Halberd, &plain(20)).name,
            "Chop"
        );
    }

    #[test]
    fn the_finisher_hits_hardest_and_staggers() {
        let mut fighter = Fighter::default();
        let strikes: Vec<Strike> = [0, 30, 60]
            .iter()
            .map(|t| fighter.attack(&config(), Weapon::Greatsword, &plain(*t)))
            .collect();
        assert!(strikes[2].damage > strikes[1].damage && strikes[1].damage > strikes[0].damage);
        assert_eq!(strikes[2].stagger, 20);
        assert!(strikes[0].plain && !strikes[2].plain);
    }

    #[test]
    fn openers_depend_on_how_the_attack_is_made() {
        let attack = |weapon, change: fn(&mut Attack)| {
            let mut attack_ = plain(0);
            change(&mut attack_);
            Fighter::default().attack(&config(), weapon, &attack_).name
        };
        assert_eq!(
            attack(Weapon::Greatsword, |a| a.sprinting = true),
            "Charging Thrust"
        );
        assert_eq!(
            attack(Weapon::Greatsword, |a| a.airborne = true),
            "Plunging Cut"
        );
        assert_eq!(attack(Weapon::Warhammer, |a| a.airborne = true), "Meteor");
        assert_eq!(attack(Weapon::Dagger, |a| a.behind = true), "Backstab");
        assert_eq!(attack(Weapon::Dagger, |a| a.sneaking = true), "Ambush");
        assert_eq!(attack(Weapon::Rapier, |a| a.sprinting = true), "Lunge");
        assert_eq!(attack(Weapon::ChainWhip, |a| a.sneaking = true), "Snare");
        // From behind beats sneaking; a weapon without an opener for it makes its plain hit.
        assert_eq!(
            attack(Weapon::Dagger, |a| {
                a.behind = true;
                a.sneaking = true
            }),
            "Backstab"
        );
        assert_eq!(attack(Weapon::Glaive, |a| a.sneaking = true), "Thrust");
        assert_eq!(attack(Weapon::Katana, |a| a.behind = true), "Slash");
    }

    #[test]
    fn a_backstab_doubles_the_damage() {
        let mut attack = plain(0);
        attack.behind = true;
        let strike = Fighter::default().attack(&config(), Weapon::Dagger, &attack);
        assert!((strike.damage - 2.2).abs() < 1e-6);
        assert!(strike.knockback < 0.5, "and leaves the target in reach");
    }

    #[test]
    fn the_iai_draw_needs_a_rested_blade() {
        let sneak = |tick| Attack {
            sneaking: true,
            ..plain(tick)
        };
        let mut fighter = Fighter::default();
        assert_eq!(
            fighter.attack(&config(), Weapon::Katana, &sneak(0)).name,
            "Iai Draw"
        );
        // Straight after, it is an ordinary slash.
        assert_eq!(
            fighter.attack(&config(), Weapon::Katana, &sneak(10)).name,
            "Slash"
        );
        // After a rest it is a draw again.
        assert_eq!(
            fighter
                .attack(&config(), Weapon::Katana, &sneak(10 + IAI_REST))
                .name,
            "Iai Draw"
        );
    }

    #[test]
    fn the_rapier_focuses_on_one_target() {
        let mut fighter = Fighter::default();
        let damage: Vec<f32> = (0..8)
            .map(|i| {
                fighter
                    .attack(&config(), Weapon::Rapier, &plain(i * 10))
                    .damage
            })
            .collect();
        for pair in damage[..6].windows(2) {
            assert!(
                pair[1] > pair[0],
                "each hit in a row is stronger: {damage:?}"
            );
        }
        assert_eq!(damage[6], damage[5], "up to a limit");
        assert!((damage[5] - (1.0 + FOCUS_STEP * FOCUS_MAX as f32)).abs() < 1e-6);
        // Another target starts over.
        let other = Attack {
            target: 99,
            ..plain(80)
        };
        assert_eq!(
            fighter.attack(&config(), Weapon::Rapier, &other).damage,
            1.0
        );
    }

    #[test]
    fn the_halberd_hooks_before_it_cleaves() {
        let mut fighter = Fighter::default();
        let strikes: Vec<Strike> = [0, 20, 40]
            .iter()
            .map(|t| fighter.attack(&config(), Weapon::Halberd, &plain(*t)))
            .collect();
        assert!(strikes[1].knockback < 0.0, "the hook pulls");
        assert!(strikes[2].damage > 1.0 && strikes[2].stagger > 0);
    }

    #[test]
    fn a_guard_weakens_blows_and_readies_a_guard_break() {
        let mut fighter = Fighter::default();
        let art = fighter.art(&config(), Weapon::Greatsword, 100).unwrap();
        assert!(matches!(art, Art::Guard { .. }));
        assert_eq!(fighter.defend(110, true), Defence::Guarded(0.4));
        assert_eq!(
            fighter.defend(120, true),
            Defence::Guarded(0.4),
            "it holds for its whole time"
        );
        assert_eq!(
            fighter.defend(120, false),
            Defence::Taken,
            "an arrow goes through"
        );
        let strike = fighter.attack(&config(), Weapon::Greatsword, &plain(125));
        assert_eq!(strike.name, "Guard Break");
        assert_eq!(
            fighter.defend(126, true),
            Defence::Taken,
            "attacking drops the guard"
        );
        // Left unused, the guard break is lost.
        let mut idle = Fighter::default();
        idle.art(&config(), Weapon::Greatsword, 0).unwrap();
        assert_eq!(
            idle.attack(&config(), Weapon::Greatsword, &plain(200)).name,
            "Cleave"
        );
    }

    #[test]
    fn a_parry_turns_one_blow_and_readies_a_counter() {
        let mut fighter = Fighter::default();
        fighter.art(&config(), Weapon::Katana, 0).unwrap();
        assert_eq!(
            fighter.defend(5, true),
            Defence::Deflected {
                name: "Parried",
                strike_back: None
            }
        );
        assert_eq!(fighter.defend(6, true), Defence::Taken, "only one blow");
        let counter = fighter.attack(&config(), Weapon::Katana, &plain(20));
        assert_eq!(counter.name, "Counter");
        assert!((counter.damage - 2.0).abs() < 1e-6);
        // Too late and there is no parry.
        let mut slow = Fighter::default();
        slow.art(&config(), Weapon::Katana, 0).unwrap();
        assert_eq!(slow.defend(13, true), Defence::Taken);
        assert_eq!(
            slow.attack(&config(), Weapon::Katana, &plain(20)).name,
            "Slash"
        );
    }

    #[test]
    fn a_riposte_strikes_back_and_a_dash_evades() {
        let mut fighter = Fighter::default();
        fighter.art(&config(), Weapon::Rapier, 0).unwrap();
        assert_eq!(
            fighter.defend(4, true),
            Defence::Deflected {
                name: "Riposte",
                strike_back: Some(1.5)
            }
        );
        assert_eq!(fighter.defend(5, true), Defence::Taken);
        let mut rogue = Fighter::default();
        assert!(matches!(
            rogue.art(&config(), Weapon::Dagger, 0).unwrap(),
            Art::Dash { .. }
        ));
        assert_eq!(
            rogue.defend(3, true),
            Defence::Deflected {
                name: "Evaded",
                strike_back: None
            }
        );
        assert_eq!(
            rogue.defend(8, true),
            Defence::Deflected {
                name: "Evaded",
                strike_back: None
            }
        );
        assert_eq!(rogue.defend(11, true), Defence::Taken);
    }

    #[test]
    fn arts_have_cooldowns_the_config_scales() {
        let mut fighter = Fighter::default();
        assert!(fighter.art(&config(), Weapon::Warhammer, 0).is_ok());
        assert_eq!(fighter.art(&config(), Weapon::Warhammer, 20), Err(100));
        assert_eq!(fighter.art_wait(20), 100);
        assert!(fighter.art(&config(), Weapon::Warhammer, 120).is_ok());
        let quick = Config {
            cooldown_scale: 0.5,
            ..config()
        };
        let mut fighter = Fighter::default();
        fighter.art(&quick, Weapon::Warhammer, 0).unwrap();
        assert!(fighter.art(&quick, Weapon::Warhammer, 60).is_ok());
    }

    #[test]
    fn the_bonus_scale_flattens_or_sharpens_every_move() {
        let flat = Config {
            bonus_scale: 0.0,
            ..config()
        };
        let mut attack = plain(0);
        attack.behind = true;
        assert_eq!(
            Fighter::default()
                .attack(&flat, Weapon::Dagger, &attack)
                .damage,
            1.0
        );
        let sharp = Config {
            bonus_scale: 2.0,
            ..config()
        };
        let strike = Fighter::default().attack(&sharp, Weapon::Dagger, &attack);
        assert!((strike.damage - 3.4).abs() < 1e-6);
        // A move weaker than a plain hit is flattened too, never below nothing.
        let mut fighter = Fighter::default();
        fighter.attack(&flat, Weapon::Halberd, &plain(0));
        assert_eq!(
            fighter.attack(&flat, Weapon::Halberd, &plain(10)).damage,
            1.0
        );
    }

    #[test]
    fn behind_is_the_arc_at_the_targets_back() {
        let target = [0.0, 64.0, 0.0];
        // Yaw 0 faces +z: someone at -z is behind, someone at +z in front.
        assert!(is_behind([0.0, 64.0, -2.0], target, 0.0));
        assert!(!is_behind([0.0, 64.0, 2.0], target, 0.0));
        assert!(
            !is_behind([2.0, 64.0, 0.0], target, 0.0),
            "the side is not the back"
        );
        assert!(
            is_behind([1.0, 64.0, -2.0], target, 0.0),
            "a little off centre still is"
        );
        // Yaw 90 faces -x: behind is +x.
        assert!(is_behind([2.0, 64.0, 0.0], target, 90.0));
        assert!(!is_behind([-2.0, 64.0, 0.0], target, 90.0));
        assert!(!is_behind(target, target, 0.0));
    }

    #[test]
    fn a_lash_reaches_what_is_in_front_and_near_enough() {
        let (eye, look) = ([0.0, 65.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(in_sight(eye, look, [0.0, 65.0, 4.0], 5.0), Some(4.0));
        assert_eq!(in_sight(eye, look, [0.0, 65.0, 6.0], 5.0), None, "too far");
        assert_eq!(in_sight(eye, look, [0.0, 65.0, -3.0], 5.0), None, "behind");
        assert_eq!(
            in_sight(eye, look, [3.0, 65.0, 3.0], 5.0),
            None,
            "too far to the side"
        );
        assert!(
            in_sight(eye, look, [0.5, 65.0, 4.0], 5.0).is_some(),
            "a little off the line"
        );
    }

    #[test]
    fn a_pull_points_at_the_wielder() {
        let push = pull_toward([10.0, 64.0, 0.0], [0.0, 64.0, 0.0], 1.0);
        assert!((push[0] + 1.0).abs() < 1e-9 && push[2].abs() < 1e-9 && push[1] > 0.0);
        assert_eq!(
            pull_toward([1.0, 0.0, 1.0], [1.0, 5.0, 1.0], 1.0),
            [0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn dashes_survive_the_wire_and_nonsense_is_refused() {
        assert_eq!(
            decode_dash(&encode_dash([0.5, 0.1, -0.9])),
            Some([0.5, 0.1, -0.9])
        );
        assert_eq!(decode_dash(&encode_dash([f32::NAN, 0.0, 0.0])), None);
        assert_eq!(
            decode_dash(&encode_dash([50.0, 0.0, 0.0])),
            None,
            "a launch no dash makes"
        );
        assert_eq!(decode_dash(&[0; 5]), None);
    }
}
