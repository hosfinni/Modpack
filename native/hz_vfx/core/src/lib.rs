//! hz_vfx's rules, with no game types: the config, which effect a block, biome or
//! weapon calls for, how a player's motion turns into a landing or a footstep, and how
//! the camera shakes. The trait mod in `../api` feeds these from Horizon's hooks and
//! turns what they answer into particles.

use serde::Deserialize;

/// The config the mod writes on its first run.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");

/// The server tells a client about a shake it should feel (an explosion nearby).
pub const SHAKE_CHANNEL: &str = "hz_vfx:shake";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub intensity: f64,
    pub blocks: Blocks,
    pub combat: Combat,
    pub movement: Movement,
    pub ambient: Ambient,
    pub shake: ShakeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            intensity: 1.0,
            blocks: Blocks::default(),
            combat: Combat::default(),
            movement: Movement::default(),
            ambient: Ambient::default(),
            shake: ShakeConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Blocks {
    pub place: bool,
    #[serde(rename = "break")]
    pub broken: bool,
}

impl Default for Blocks {
    fn default() -> Self {
        Self {
            place: true,
            broken: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Combat {
    pub hits: bool,
    pub swing_trails: bool,
}

impl Default for Combat {
    fn default() -> Self {
        Self {
            hits: true,
            swing_trails: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Movement {
    pub landing: bool,
    pub footsteps: bool,
    pub splash: bool,
    pub breath: bool,
}

impl Default for Movement {
    fn default() -> Self {
        Self {
            landing: true,
            footsteps: true,
            splash: true,
            breath: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Ambient {
    pub fireflies: bool,
    pub embers: bool,
    pub cave_dust: bool,
    pub radius: i32,
}

impl Default for Ambient {
    fn default() -> Self {
        Self {
            fireflies: true,
            embers: true,
            cave_dust: true,
            radius: 12,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ShakeConfig {
    pub enabled: bool,
    pub strength: f64,
}

impl Default for ShakeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 1.0,
        }
    }
}

impl Config {
    /// Reads a config file. Missing keys take their defaults; an unknown key is an
    /// error, so a typo doesn't silently do nothing.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        let within = |name: &str, value: f64, low: f64, high: f64| {
            if (low..=high).contains(&value) {
                Ok(())
            } else {
                Err(format!(
                    "{name} must be between {low} and {high}, not {value}"
                ))
            }
        };
        within("intensity", config.intensity, 0.0, 2.0)?;
        within("shake.strength", config.shake.strength, 0.0, 2.0)?;
        within(
            "ambient.radius",
            f64::from(config.ambient.radius),
            4.0,
            32.0,
        )?;
        Ok(config)
    }

    /// The config a fresh install runs with.
    pub fn shipped() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the shipped config parses")
    }

    /// `count` particles scaled by the intensity, rounding so that a small burst
    /// at a low intensity still sometimes shows one (`roll` is 0..1).
    pub fn count(&self, count: u32, roll: f64) -> u32 {
        let scaled = f64::from(count) * self.intensity;
        let whole = scaled.floor();
        whole as u32 + u32::from(roll < scaled - whole)
    }

    /// How hard the camera shakes for `amount`, after the config.
    pub fn shake(&self, amount: f32) -> f32 {
        if !self.shake.enabled {
            return 0.0;
        }
        amount * (self.shake.strength * self.intensity.min(1.0)) as f32
    }
}

/// A small, fast random source: effects need variety, not quality.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0..1.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// -1..1.
    pub fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }

    /// `low..=high`.
    pub fn range(&mut self, low: i32, high: i32) -> i32 {
        let span = (high - low + 1).max(1) as u64;
        low + (self.next_u64() % span) as i32
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
}

/// The colour of the dust a block's ground kicks up, 0..1 RGB.
pub fn dust_color(block: &str) -> [f32; 3] {
    let name = block.strip_prefix("minecraft:").unwrap_or(block);
    let has = |part: &str| name.contains(part);
    if has("snow") || has("powder_snow") {
        [0.93, 0.95, 0.98]
    } else if has("ice") {
        [0.76, 0.86, 0.96]
    } else if has("red_sand") || has("terracotta") {
        [0.74, 0.42, 0.22]
    } else if has("sand") || has("end_stone") {
        [0.86, 0.80, 0.60]
    } else if has("soul") {
        [0.33, 0.26, 0.21]
    } else if has("netherrack") || has("nylium") {
        [0.48, 0.18, 0.17]
    } else if has("mud") || has("clay") {
        [0.40, 0.36, 0.33]
    } else if has("gravel") || has("tuff") || has("andesite") {
        [0.55, 0.53, 0.51]
    } else if has("deepslate") || has("basalt") || has("blackstone") {
        [0.30, 0.30, 0.33]
    } else if has("stone") || has("cobble") || has("ore") {
        [0.52, 0.52, 0.52]
    } else if has("grass")
        || has("dirt")
        || has("podzol")
        || has("mycelium")
        || has("farmland")
        || has("path")
        || has("moss")
    {
        [0.48, 0.38, 0.27]
    } else if has("planks") || has("log") || has("wood") {
        [0.60, 0.47, 0.30]
    } else {
        [0.55, 0.50, 0.44]
    }
}

/// Whether nothing would be kicked up from `block`: air, fluids, and the plants and
/// thin things a foot passes through.
pub fn is_soft(block: &str) -> bool {
    let name = block.strip_prefix("minecraft:").unwrap_or(block);
    name.ends_with("air")
        || matches!(name, "water" | "lava" | "bubble_column" | "light")
        || name.contains("grass") && !name.contains("block")
        || name.contains("fern")
        || name.contains("flower")
        || name.ends_with("_bush")
        || name.contains("torch")
        || name.contains("carpet")
        || name.contains("rail")
        || name.contains("button")
        || name.contains("sapling")
        || name.contains("vine")
        || name == "snow"
}

/// A biome cold enough for a player's breath to show.
pub fn is_cold(biome: &str) -> bool {
    let name = biome.strip_prefix("minecraft:").unwrap_or(biome);
    name.contains("snowy")
        || name.contains("frozen")
        || name.contains("ice")
        || matches!(name, "grove" | "jagged_peaks" | "deep_dark")
}

/// A biome warm and green enough for fireflies on a summer night.
pub fn has_fireflies(biome: &str) -> bool {
    let name = biome.strip_prefix("minecraft:").unwrap_or(biome);
    matches!(
        name,
        "plains"
            | "sunflower_plains"
            | "meadow"
            | "forest"
            | "flower_forest"
            | "birch_forest"
            | "old_growth_birch_forest"
            | "dark_forest"
            | "swamp"
            | "mangrove_swamp"
            | "river"
            | "jungle"
            | "sparse_jungle"
            | "bamboo_jungle"
            | "cherry_grove"
    )
}

/// Ground fireflies hover over.
pub fn firefly_ground(block: &str) -> bool {
    let name = block.strip_prefix("minecraft:").unwrap_or(block);
    matches!(
        name,
        "grass_block" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "moss_block"
    ) || name.ends_with("_tulip")
        || matches!(
            name,
            "dandelion" | "poppy" | "oxeye_daisy" | "cornflower" | "azure_bluet"
        )
}

/// Night by the time of day (0 at sunrise, 24000 a day).
pub fn is_night(day_time: i64) -> bool {
    (13_000..23_000).contains(&day_time.rem_euclid(24_000))
}

/// What rises from a burning block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ember {
    /// Sparks that arc up and fall back: a fire, a campfire.
    Sparks,
    /// Pops of lava.
    Lava,
    /// Pale flecks of soul fire.
    Soul,
}

pub fn ember(block: &str) -> Option<Ember> {
    let name = block.strip_prefix("minecraft:").unwrap_or(block);
    match name {
        "fire" | "campfire" => Some(Ember::Sparks),
        "lava" | "magma_block" => Some(Ember::Lava),
        "soul_fire" | "soul_campfire" => Some(Ember::Soul),
        _ => None,
    }
}

/// How a weapon's blow looks where it lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blow {
    /// A thin, quick cut: katana, dagger, rapier.
    Slash,
    /// A wide sweep: greatsword, glaive, halberd, and vanilla's swords.
    Sweep,
    /// A heavy blow that throws up dust: warhammer, mace, axes.
    Crush,
    /// A crack of the whip.
    Lash,
    /// Anything else (a fist, a tool): a few sparks.
    Plain,
}

pub fn blow(item: &str) -> Blow {
    let name = item.split_once(':').map_or(item, |(_, path)| path);
    match name {
        "katana" | "dagger" | "rapier" => Blow::Slash,
        "greatsword" | "glaive" | "halberd" => Blow::Sweep,
        "warhammer" | "mace" => Blow::Crush,
        "chain_whip" => Blow::Lash,
        _ if name.ends_with("_sword") => Blow::Sweep,
        _ if name.ends_with("_axe") => Blow::Crush,
        _ => Blow::Plain,
    }
}

/// What a tick of the local player's motion amounted to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Step {
    /// They came down this many blocks and stopped.
    pub landed: Option<f64>,
    /// Blocks a tick along the ground.
    pub speed: f64,
    /// Blocks a tick up (negative: down).
    pub rise: f64,
}

/// Follows the local player from tick to tick. The client is told only where the
/// player is, so falling and landing are read from how that changes.
#[derive(Clone, Debug, Default)]
pub struct Motion {
    last: Option<[f64; 3]>,
    /// Where a fall began, while falling.
    fall_top: Option<f64>,
}

/// Falls shorter than this land without dust.
pub const LANDING_MIN: f64 = 1.5;
/// A tick that moves more than this was a teleport, not a move.
const TELEPORT: f64 = 8.0;

impl Motion {
    pub fn step(&mut self, position: [f64; 3]) -> Step {
        let Some(last) = self.last.replace(position) else {
            return Step::default();
        };
        let (dx, dy, dz) = (
            position[0] - last[0],
            position[1] - last[1],
            position[2] - last[2],
        );
        if dx.abs() + dy.abs() + dz.abs() > TELEPORT {
            self.fall_top = None;
            return Step::default();
        }
        let mut step = Step {
            landed: None,
            speed: (dx * dx + dz * dz).sqrt(),
            rise: dy,
        };
        if dy < -0.05 {
            self.fall_top.get_or_insert(last[1]);
        } else if let Some(top) = self.fall_top.take() {
            // Down to a stop, not up: a fall that ended.
            if dy.abs() < 0.05 {
                let height = top - position[1];
                if height >= LANDING_MIN {
                    step.landed = Some(height);
                }
            }
        }
        step
    }

    /// Forget the player (a new world, a death).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// How hard a landing from `height` blocks shakes the camera.
pub fn landing_shake(height: f64) -> f32 {
    if height < 4.0 {
        0.0
    } else {
        (0.25 + (height - 4.0) * 0.08).min(1.2) as f32
    }
}

/// How hard an explosion `distance` blocks away shakes the camera.
pub fn explosion_shake(distance: f64) -> f32 {
    if distance >= 40.0 {
        0.0
    } else {
        (1.4 * (1.0 - distance / 40.0).powi(2)) as f32
    }
}

pub fn encode_shake(amount: f32) -> [u8; 4] {
    amount.to_le_bytes()
}

pub fn decode_shake(data: &[u8]) -> Option<f32> {
    let amount = f32::from_le_bytes(data.try_into().ok()?);
    (amount.is_finite() && amount >= 0.0).then_some(amount.min(2.0))
}

/// The camera's shake: kicks that die away over a moment.
#[derive(Clone, Debug, Default)]
pub struct Shake {
    /// (when it started, in seconds; how hard)
    kicks: Vec<(f64, f32)>,
}

/// How long a kick takes to fade to a third, in seconds.
const SHAKE_FADE: f64 = 0.18;
/// Kicks older than this are forgotten.
const SHAKE_LIFE: f64 = 1.0;

/// What the camera does this frame, in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sway {
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
}

impl Shake {
    pub fn kick(&mut self, now: f64, amount: f32) {
        if amount > 0.0 {
            self.kicks.retain(|&(start, _)| now - start < SHAKE_LIFE);
            if self.kicks.len() < 16 {
                self.kicks.push((now, amount));
            }
        }
    }

    pub fn sway(&mut self, now: f64) -> Sway {
        self.kicks.retain(|&(start, _)| now - start < SHAKE_LIFE);
        let mut sway = Sway::default();
        for &(start, amount) in &self.kicks {
            let t = (now - start).max(0.0);
            let fade = (-t / SHAKE_FADE).exp() * f64::from(amount);
            // Three waves at unrelated rates, so it doesn't read as a wobble.
            sway.pitch += (fade * 1.6 * (t * 47.0).sin()) as f32;
            sway.yaw += (fade * 0.9 * (t * 31.0 + 1.3).sin()) as f32;
            sway.roll += (fade * 1.1 * (t * 23.0 + 2.1).sin()) as f32;
        }
        let bound = |v: f32| v.clamp(-4.0, 4.0);
        Sway {
            pitch: bound(sway.pitch),
            yaw: bound(sway.yaw),
            roll: bound(sway.roll),
        }
    }
}

/// The way a player faces, a unit vector, from their yaw and pitch in degrees
/// (yaw 0 faces +z, 90 faces -x; pitch 90 looks down).
pub fn facing(yaw: f32, pitch: f32) -> [f64; 3] {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}

/// Points along a swing from `from` to `to`, bowed out sideways by `bow` blocks:
/// where a swing trail's particles go.
pub fn arc(from: [f64; 3], to: [f64; 3], points: usize, bow: f64) -> Vec<[f64; 3]> {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    // Sideways: across the swing, along the ground.
    let len = (d[0] * d[0] + d[2] * d[2]).sqrt().max(1e-6);
    let side = [-d[2] / len, 0.0, d[0] / len];
    (0..points)
        .map(|i| {
            let t = (i as f64 + 0.5) / points as f64;
            let out = bow * (std::f64::consts::PI * t).sin();
            [
                from[0] + d[0] * t + side[0] * out,
                from[1] + d[1] * t + 0.15 * out,
                from[2] + d[2] * t + side[2] * out,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_parses_and_bad_values_are_refused() {
        let config = Config::shipped();
        assert_eq!(config, Config::default());
        assert!(Config::parse("intensity = 3.0").is_err());
        assert!(Config::parse("[ambient]\nradius = 100").is_err());
        assert!(Config::parse("sparkles = true").is_err());
        let off = Config::parse("[blocks]\nbreak = false").unwrap();
        assert!(!off.blocks.broken && off.blocks.place);
    }

    #[test]
    fn intensity_scales_counts_and_shake() {
        let config = Config::shipped();
        assert_eq!(config.count(8, 0.5), 8);
        let half = Config::parse("intensity = 0.5").unwrap();
        assert_eq!(half.count(8, 0.9), 4);
        // 1 at half: sometimes one, sometimes none.
        assert_eq!(half.count(1, 0.2), 1);
        assert_eq!(half.count(1, 0.8), 0);
        let none = Config::parse("intensity = 0.0").unwrap();
        assert_eq!(none.count(20, 0.0), 0);
        assert_eq!(none.shake(1.0), 0.0);
        let still = Config::parse("[shake]\nenabled = false").unwrap();
        assert_eq!(still.shake(1.0), 0.0);
        assert_eq!(config.shake(1.0), 1.0);
    }

    #[test]
    fn the_random_source_spreads_and_repeats() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let xs: Vec<f64> = (0..1000).map(|_| a.unit()).collect();
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        assert_eq!(xs[10], {
            (0..10).for_each(|_| {
                b.unit();
            });
            b.unit()
        });
        let mean = xs.iter().sum::<f64>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05);
        let mut r = Rng::new(1);
        assert!(
            (0..500)
                .map(|_| r.range(-3, 3))
                .all(|v| (-3..=3).contains(&v))
        );
    }

    #[test]
    fn ground_has_the_dust_of_its_kind() {
        assert_eq!(
            dust_color("minecraft:sand"),
            dust_color("minecraft:sandstone")
        );
        assert_ne!(
            dust_color("minecraft:sand"),
            dust_color("minecraft:red_sand")
        );
        assert_ne!(
            dust_color("minecraft:grass_block"),
            dust_color("minecraft:snow_block")
        );
        assert_eq!(
            dust_color("minecraft:dirt"),
            dust_color("minecraft:grass_block")
        );
        for block in [
            "minecraft:air",
            "minecraft:water",
            "minecraft:short_grass",
            "minecraft:torch",
        ] {
            assert!(is_soft(block), "{block}");
        }
        for block in [
            "minecraft:grass_block",
            "minecraft:stone",
            "minecraft:oak_planks",
        ] {
            assert!(!is_soft(block), "{block}");
        }
    }

    #[test]
    fn biomes_and_time_pick_the_ambient_effects() {
        assert!(is_cold("minecraft:snowy_plains") && is_cold("minecraft:frozen_peaks"));
        assert!(!is_cold("minecraft:plains") && !is_cold("minecraft:desert"));
        assert!(has_fireflies("minecraft:swamp") && !has_fireflies("minecraft:desert"));
        assert!(is_night(18_000) && is_night(18_000 + 24_000 * 3) && !is_night(6_000));
        assert!(firefly_ground("minecraft:grass_block") && !firefly_ground("minecraft:stone"));
        assert_eq!(ember("minecraft:campfire"), Some(Ember::Sparks));
        assert_eq!(ember("minecraft:soul_fire"), Some(Ember::Soul));
        assert_eq!(ember("minecraft:lava"), Some(Ember::Lava));
        assert_eq!(ember("minecraft:torch"), None);
    }

    #[test]
    fn each_weapon_lands_its_own_blow() {
        assert_eq!(blow("hz_content:katana"), Blow::Slash);
        assert_eq!(blow("hz_content:greatsword"), Blow::Sweep);
        assert_eq!(blow("hz_content:warhammer"), Blow::Crush);
        assert_eq!(blow("hz_content:chain_whip"), Blow::Lash);
        assert_eq!(blow("minecraft:diamond_sword"), Blow::Sweep);
        assert_eq!(blow("minecraft:iron_axe"), Blow::Crush);
        assert_eq!(blow("minecraft:stick"), Blow::Plain);
    }

    #[test]
    fn a_fall_lands_once_with_its_height() {
        let mut motion = Motion::default();
        let mut landed = Vec::new();
        // Walk, jump off a 5 block ledge, land, walk on.
        let mut path = vec![[0.0, 70.0, 0.0], [0.2, 70.0, 0.0], [0.4, 70.0, 0.0]];
        let mut y: f64 = 70.0;
        let mut v: f64 = 0.0;
        while y > 65.0 {
            v = (v - 0.08) * 0.98;
            y = (y + v).max(65.0);
            path.push([0.5, y, 0.0]);
        }
        path.extend([[0.6, 65.0, 0.0], [0.8, 65.0, 0.0], [1.0, 65.0, 0.0]]);
        for p in path {
            if let Some(h) = motion.step(p).landed {
                landed.push(h);
            }
        }
        assert_eq!(landed.len(), 1, "{landed:?}");
        assert!((landed[0] - 5.0).abs() < 0.01);
        // A step down a block lands without dust.
        let mut motion = Motion::default();
        let small: Vec<_> = [
            [0.0, 70.0, 0.0],
            [0.0, 69.5, 0.0],
            [0.0, 69.0, 0.0],
            [0.0, 69.0, 0.0],
        ]
        .into_iter()
        .filter_map(|p| motion.step(p).landed)
        .collect();
        assert!(small.is_empty());
        // A teleport down is no fall.
        let mut motion = Motion::default();
        motion.step([0.0, 200.0, 0.0]);
        assert_eq!(motion.step([0.0, 64.0, 0.0]).landed, None);
        assert_eq!(motion.step([0.0, 64.0, 0.0]).landed, None);
    }

    #[test]
    fn shakes_grow_with_the_cause_and_die_away() {
        assert_eq!(landing_shake(3.0), 0.0);
        assert!(landing_shake(10.0) > landing_shake(5.0));
        assert!(landing_shake(100.0) <= 1.2);
        assert!(explosion_shake(4.0) > explosion_shake(20.0));
        assert_eq!(explosion_shake(50.0), 0.0);
        assert_eq!(decode_shake(&encode_shake(0.7)), Some(0.7));
        assert_eq!(decode_shake(&[1, 2]), None);
        assert_eq!(decode_shake(&encode_shake(f32::NAN)), None);

        let mut shake = Shake::default();
        shake.kick(10.0, 1.0);
        let strength = |s: Sway| s.pitch.abs() + s.yaw.abs() + s.roll.abs();
        let early: f32 = (1..10)
            .map(|i| strength(shake.sway(10.0 + i as f64 * 0.01)))
            .sum();
        let late: f32 = (1..10)
            .map(|i| strength(shake.sway(10.5 + i as f64 * 0.01)))
            .sum();
        assert!(early > late * 5.0, "{early} {late}");
        assert_eq!(shake.sway(12.0), Sway::default());
        // However many kicks pile up, the camera stays in bounds.
        let mut pile = Shake::default();
        for _ in 0..40 {
            pile.kick(0.0, 2.0);
        }
        let s = pile.sway(0.005);
        assert!(s.pitch.abs() <= 4.0 && s.yaw.abs() <= 4.0 && s.roll.abs() <= 4.0);
    }

    #[test]
    fn facing_and_arcs_point_the_right_way() {
        let south = facing(0.0, 0.0);
        assert!((south[2] - 1.0).abs() < 1e-9);
        let west = facing(90.0, 0.0);
        assert!((west[0] + 1.0).abs() < 1e-9);
        let down = facing(0.0, 90.0);
        assert!((down[1] + 1.0).abs() < 1e-9);
        let points = arc([0.0, 0.0, 0.0], [0.0, 0.0, 3.0], 6, 0.5);
        assert_eq!(points.len(), 6);
        assert!(points.windows(2).all(|w| w[1][2] > w[0][2]));
        // Bowed out to one side, most in the middle.
        assert!(
            points
                .iter()
                .all(|p| p[0] != 0.0 && p[0].signum() == points[0][0].signum())
        );
        assert!(points[2][0].abs() > points[0][0].abs());
    }
}
