//! hz_seasons' rules, with no game types: when the climate turns, how far each place
//! moves along its climate ladder, and what the ground does about it. The trait mod
//! in `../api` feeds these from Horizon's hooks.
//!
//! Time is counted in shifts (the "epoch"): 0 is the world as it was generated. Each
//! biome's place at an epoch is worked out from the biome it was generated with, so
//! shifting never accumulates error: a place always returns to its own biome when
//! its climate returns to normal.

use std::collections::{HashSet, VecDeque};

use serde::Deserialize;

pub mod biomes;
pub mod ground;
pub mod noise;

/// The config the mod writes on its first run.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");
/// The channel the server tells each client the epoch on.
pub const CHANNEL: &str = "hz_seasons:epoch";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub sleeps_per_shift: u32,
    pub shifts_per_year: u32,
    pub season_steps: f64,
    pub drift_steps: f64,
    pub drift_scale: f64,
    pub drift_speed: f64,
    pub max_steps: i32,
    pub chunks_per_tick: u32,
    pub priority_radius: i32,
    pub features: Features,
    pub anchor: Anchor,
    pub client: Client,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            sleeps_per_shift: 3,
            shifts_per_year: 8,
            season_steps: 1.0,
            drift_steps: 1.0,
            drift_scale: 900.0,
            drift_speed: 260.0,
            max_steps: 2,
            chunks_per_tick: 4,
            priority_radius: 5,
            features: Features::default(),
            anchor: Anchor::default(),
            client: Client::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Features {
    pub ground: bool,
    pub ice_and_snow: bool,
    pub plants: bool,
    pub leaf_fall: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            ground: true,
            ice_and_snow: true,
            plants: true,
            leaf_fall: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Anchor {
    pub radius: i32,
}

impl Default for Anchor {
    fn default() -> Self {
        Self { radius: 48 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Client {
    pub tints: bool,
    pub banner: bool,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            tints: true,
            banner: true,
        }
    }
}

impl Config {
    /// Reads a config file. Missing keys take their defaults; an unknown key is an
    /// error, so a typo doesn't silently leave a rule off.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        let at_least_one = |name: &str, value: u32| {
            if value >= 1 {
                Ok(())
            } else {
                Err(format!("{name} must be at least 1"))
            }
        };
        at_least_one("sleeps_per_shift", config.sleeps_per_shift)?;
        at_least_one("chunks_per_tick", config.chunks_per_tick)?;
        if !(4..=4096).contains(&config.shifts_per_year) {
            return Err("shifts_per_year must be 4 to 4096".into());
        }
        for (name, value, low, high) in [
            ("season_steps", config.season_steps, 0.0, 4.0),
            ("drift_steps", config.drift_steps, 0.0, 4.0),
            ("drift_scale", config.drift_scale, 64.0, 1_000_000.0),
            ("drift_speed", config.drift_speed, 0.0, 1_000_000.0),
        ] {
            if !(value >= low && value <= high) {
                return Err(format!(
                    "{name} must be between {low} and {high}, not {value}"
                ));
            }
        }
        if !(0..=4).contains(&config.max_steps) {
            return Err("max_steps must be 0 to 4".into());
        }
        if !(0..=32).contains(&config.priority_radius) {
            return Err("priority_radius must be 0 to 32".into());
        }
        if !(0..=512).contains(&config.anchor.radius) {
            return Err("anchor.radius must be 0 to 512".into());
        }
        Ok(config)
    }

    /// The config a fresh install runs with.
    pub fn shipped() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the shipped config parses")
    }

    /// How many steps hotter (or colder, when negative) the climate is at block
    /// `(x, z)` at `epoch`, in a world of `seed`. Epoch 0 is 0 everywhere.
    pub fn steps_at(&self, seed: i64, x: i32, z: i32, epoch: i64) -> i32 {
        if epoch == 0 || !self.enabled {
            return 0;
        }
        let season = self.season_steps * warmth(epoch, self.shifts_per_year);
        // The pattern of warm and cold patches slides across the map a little
        // further each shift, in a direction of the world's own.
        let seed = seed as u64;
        let (dx, dz) = DIRECTIONS[(noise::hash(seed, 0xD21F7, 1) % 8) as usize];
        let travel = self.drift_speed * epoch as f64;
        let drift = self.drift_steps
            * noise::simplex(
                seed ^ 0x5EA5_0115,
                (f64::from(x) + dx * travel) / self.drift_scale,
                (f64::from(z) + dz * travel) / self.drift_scale,
            );
        ((season + drift).round() as i32).clamp(-self.max_steps, self.max_steps)
    }

    /// Whether block `(x, z)` is held by one of `anchors` (the blocks climate
    /// anchors stand on).
    pub fn anchored(&self, anchors: &[[i32; 3]], x: i32, z: i32) -> bool {
        let radius = i64::from(self.anchor.radius);
        anchors.iter().any(|anchor| {
            let (dx, dz) = (i64::from(anchor[0] - x), i64::from(anchor[2] - z));
            dx * dx + dz * dz <= radius * radius
        })
    }
}

const DIAGONAL: f64 = std::f64::consts::FRAC_1_SQRT_2;
const DIRECTIONS: [(f64, f64); 8] = [
    (1.0, 0.0),
    (DIAGONAL, DIAGONAL),
    (0.0, 1.0),
    (-DIAGONAL, DIAGONAL),
    (-1.0, 0.0),
    (-DIAGONAL, -DIAGONAL),
    (0.0, -1.0),
    (DIAGONAL, -DIAGONAL),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Autumn => "Autumn",
            Season::Winter => "Winter",
        }
    }
}

/// How far into its year `epoch` is, 0 to 1: 0 is the start of spring.
pub fn year_fraction(epoch: i64, shifts_per_year: u32) -> f64 {
    let year = i64::from(shifts_per_year.max(1));
    epoch.rem_euclid(year) as f64 / year as f64
}

/// The season at `epoch`. A year starts in the middle of spring, so that the world
/// as generated (epoch 0) is a spring day.
pub fn season(epoch: i64, shifts_per_year: u32) -> Season {
    match year_fraction(epoch, shifts_per_year) {
        f if !(0.125..0.875).contains(&f) => Season::Spring,
        f if f < 0.375 => Season::Summer,
        f if f < 0.625 => Season::Autumn,
        _ => Season::Winter,
    }
}

/// The year `epoch` falls in, counting from 1.
pub fn year(epoch: i64, shifts_per_year: u32) -> i64 {
    epoch.div_euclid(i64::from(shifts_per_year.max(1))) + 1
}

/// How warm the season is at `epoch`: 1 at midsummer, -1 at midwinter, 0 in the
/// middle of spring and autumn.
pub fn warmth(epoch: i64, shifts_per_year: u32) -> f64 {
    let year = f64::from(shifts_per_year.max(1));
    noise::sine(epoch.rem_euclid(shifts_per_year.max(1) as i64) as f64, year)
}

/// Counts nights slept through toward the next shift.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calendar {
    pub epoch: i64,
    pub sleeps: u32,
}

impl Calendar {
    /// A night was slept through. True when that turns the climate: the epoch has
    /// moved on.
    pub fn slept(&mut self, config: &Config) -> bool {
        if !config.enabled {
            return false;
        }
        self.sleeps += 1;
        if self.sleeps < config.sleeps_per_shift {
            return false;
        }
        self.sleeps = 0;
        self.epoch += 1;
        true
    }
}

/// What the server tells a client about the epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    /// The climate just turned: show the banner. Otherwise the client only catches
    /// up (it joined, or changed dimension).
    pub shift: bool,
    pub epoch: i64,
    pub shifts_per_year: u32,
}

impl Message {
    pub fn encode(&self) -> [u8; 13] {
        let mut bytes = [0; 13];
        bytes[0] = u8::from(self.shift);
        bytes[1..9].copy_from_slice(&self.epoch.to_le_bytes());
        bytes[9..13].copy_from_slice(&self.shifts_per_year.to_le_bytes());
        bytes
    }

    /// Reads [`Message::encode`]'s bytes. None for anything else, which a client
    /// ignores.
    pub fn decode(data: &[u8]) -> Option<Self> {
        let bytes: &[u8; 13] = data.try_into().ok()?;
        let shift = match bytes[0] {
            0 => false,
            1 => true,
            _ => return None,
        };
        let epoch = i64::from_le_bytes(bytes[1..9].try_into().ok()?);
        let shifts_per_year = u32::from_le_bytes(bytes[9..13].try_into().ok()?);
        (4..=4096).contains(&shifts_per_year).then_some(Self {
            shift,
            epoch,
            shifts_per_year,
        })
    }
}

/// The banner's timing, in milliseconds.
pub const FADE_IN_MS: u64 = 700;
pub const HOLD_MS: u64 = 3400;
pub const FADE_OUT_MS: u64 = 1300;
pub const BANNER_MS: u64 = FADE_IN_MS + HOLD_MS + FADE_OUT_MS;

/// What the banner looks like some time after it started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Banner {
    /// How black the screen is, 0 to 1.
    pub black: f32,
    /// How bright the words are, 0 to 1.
    pub words: f32,
    /// How far the rules beside the words have drawn out, 0 to 1.
    pub rule: f32,
}

/// The banner `elapsed_ms` after the shift; None once it is over.
pub fn banner(elapsed_ms: u64) -> Option<Banner> {
    if elapsed_ms >= BANNER_MS {
        return None;
    }
    let ramp = |from: u64, length: u64| -> f32 {
        ((elapsed_ms.saturating_sub(from)) as f32 / length as f32).clamp(0.0, 1.0)
    };
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let leaving = ease(ramp(FADE_IN_MS + HOLD_MS, FADE_OUT_MS));
    let black = ease(ramp(0, FADE_IN_MS)) * (1.0 - leaving);
    // The words come once the screen is dark, and leave a little before it clears.
    let words =
        ease(ramp(FADE_IN_MS / 2, 900)) * (1.0 - ease(ramp(FADE_IN_MS + HOLD_MS - 200, 900)));
    let rule = ease(ramp(FADE_IN_MS / 2, 1600));
    Some(Banner { black, words, rule })
}

/// A grass or leaf colour (0xRRGGBB) as the season tints it. `evergreen` biomes
/// (taiga, jungle, swamp) keep their leaves' colour.
pub fn tinted(season: Season, colour: u32, foliage: bool, evergreen: bool) -> u32 {
    let (toward, amount) = match (season, foliage) {
        (Season::Spring, true) => (0x7ED05A, 0.14),
        (Season::Spring, false) => (0x86CC5A, 0.10),
        (Season::Summer, _) => return colour,
        (Season::Autumn, true) => (0xC8742A, 0.48),
        (Season::Autumn, false) => (0xB59E4A, 0.26),
        (Season::Winter, true) => (0x8E9070, 0.32),
        (Season::Winter, false) => (0xA7B3A0, 0.30),
    };
    if foliage && evergreen {
        return colour;
    }
    let mix = |shift: u32| {
        let (from, to) = (
            f64::from((colour >> shift) & 255),
            f64::from((toward >> shift) & 255),
        );
        ((from + (to - from) * amount).round() as u32).min(255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

/// Whether a biome's trees keep their leaves' colour through the year.
pub fn evergreen(biome: &str) -> bool {
    let name = biome.rsplit(':').next().unwrap_or(biome);
    ["taiga", "jungle", "swamp", "grove", "mangrove"]
        .iter()
        .any(|word| name.contains(word))
}

/// The chunks within `radius` chunks of any of `centres`, nearest to a centre first.
pub fn chunks_around(centres: &[[i32; 2]], radius: i32) -> Vec<[i32; 2]> {
    let mut found: Vec<(i64, [i32; 2])> = Vec::new();
    let mut seen = HashSet::new();
    for centre in centres {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let chunk = [centre[0] + dx, centre[1] + dz];
                if seen.insert(chunk) {
                    let nearest = centres
                        .iter()
                        .map(|c| {
                            let (x, z) = (i64::from(c[0] - chunk[0]), i64::from(c[1] - chunk[1]));
                            x * x + z * z
                        })
                        .min()
                        .unwrap_or(0);
                    found.push((nearest, chunk));
                }
            }
        }
    }
    found.sort();
    found.into_iter().map(|(_, chunk)| chunk).collect()
}

/// The chunks waiting to be converted to the current epoch: each waits once, the
/// ones near players ahead of the rest.
#[derive(Debug, Default)]
pub struct Queue {
    order: VecDeque<[i32; 2]>,
    waiting: HashSet<[i32; 2]>,
}

impl Queue {
    /// Chunks to convert before anything already waiting, in the order given.
    pub fn first(&mut self, chunks: &[[i32; 2]]) {
        for chunk in chunks.iter().rev() {
            if !self.waiting.insert(*chunk) {
                self.order.retain(|waiting| waiting != chunk);
            }
            self.order.push_front(*chunk);
        }
    }

    /// A chunk to convert after everything already waiting.
    pub fn later(&mut self, chunk: [i32; 2]) {
        if self.waiting.insert(chunk) {
            self.order.push_back(chunk);
        }
    }

    /// The chunk to convert next.
    pub fn pop(&mut self) -> Option<[i32; 2]> {
        let chunk = self.order.pop_front()?;
        self.waiting.remove(&chunk);
        Some(chunk)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.waiting.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_parses_and_bad_values_are_refused() {
        let config = Config::shipped();
        assert!(config.enabled && config.features.ground && config.client.banner);
        assert_eq!(config.sleeps_per_shift, 3);
        assert_eq!(config.anchor.radius, 48);
        assert!(Config::parse("sleeps_per_shift = 0").is_err());
        assert!(Config::parse("shifts_per_year = 2").is_err());
        assert!(Config::parse("max_steps = 9").is_err());
        assert!(Config::parse("[features]\ngrond = true").is_err());
        assert_eq!(Config::parse("").unwrap(), Config::default());
    }

    #[test]
    fn the_calendar_turns_after_enough_nights() {
        let config = Config::shipped();
        let mut calendar = Calendar::default();
        let turned: Vec<bool> = (0..7).map(|_| calendar.slept(&config)).collect();
        assert_eq!(turned, [false, false, true, false, false, true, false]);
        assert_eq!(
            calendar,
            Calendar {
                epoch: 2,
                sleeps: 1
            }
        );
        let off = Config {
            enabled: false,
            ..config
        };
        assert!(!(0..10).any(|_| calendar.slept(&off)));
    }

    #[test]
    fn a_year_runs_spring_summer_autumn_winter() {
        let seasons: Vec<Season> = (0..8).map(|epoch| season(epoch, 8)).collect();
        use Season::*;
        assert_eq!(
            seasons,
            [
                Spring, Summer, Summer, Autumn, Autumn, Winter, Winter, Spring
            ]
        );
        assert_eq!(season(8, 8), Spring);
        assert_eq!((year(0, 8), year(7, 8), year(8, 8)), (1, 1, 2));
        assert!(warmth(0, 8).abs() < 1e-9);
        assert_eq!(warmth(2, 8), 1.0);
        assert!(warmth(4, 8).abs() < 1e-9);
        assert_eq!(warmth(6, 8), -1.0);
        assert_eq!(warmth(10, 8), 1.0, "and again the next year");
    }

    #[test]
    fn the_generated_world_is_untouched_until_the_first_shift() {
        let config = Config::shipped();
        for i in 0..2000 {
            assert_eq!(config.steps_at(42, i * 37, -i * 91, 0), 0);
        }
    }

    #[test]
    fn summer_warms_and_winter_cools_most_of_the_land() {
        let config = Config::shipped();
        let mean = |epoch: i64| -> f64 {
            (0..4000)
                .map(|i| f64::from(config.steps_at(9, i * 53, i * 97 % 7919, epoch)))
                .sum::<f64>()
                / 4000.0
        };
        assert!(mean(2) > 0.7, "midsummer: {}", mean(2));
        assert!(mean(6) < -0.7, "midwinter: {}", mean(6));
        assert!(mean(4).abs() < 0.3, "autumn: {}", mean(4));
        // Never beyond the limit.
        for epoch in 0..40 {
            for i in 0..500 {
                let steps = config.steps_at(9, i * 211, i * 173, epoch);
                assert!(steps.abs() <= config.max_steps);
            }
        }
    }

    #[test]
    fn the_patches_wander_so_no_two_springs_are_alike() {
        let config = Config::shipped();
        // Epochs 8 and 16 are both mid-spring; the patches have moved between them.
        let differing = (0..4000)
            .filter(|i| {
                config.steps_at(3, i * 61, i * 29, 8) != config.steps_at(3, i * 61, i * 29, 16)
            })
            .count();
        assert!(
            differing > 400,
            "{differing} of 4000 places differ between two springs"
        );
        // The same epoch is always the same.
        for i in 0..500 {
            assert_eq!(
                config.steps_at(3, i * 61, i * 29, 8),
                config.steps_at(3, i * 61, i * 29, 8)
            );
        }
    }

    #[test]
    fn neighbouring_blocks_shift_together() {
        // Patches are hundreds of blocks across: the step changes rarely along a walk.
        let config = Config::shipped();
        let changes = (1..6000)
            .filter(|x| config.steps_at(5, *x, 40, 3) != config.steps_at(5, x - 1, 40, 3))
            .count();
        assert!(changes < 40, "{changes} changes of step in 6000 blocks");
    }

    #[test]
    fn anchors_hold_the_land_around_them() {
        let config = Config::shipped();
        let anchors = [[100, 64, 100]];
        assert!(config.anchored(&anchors, 100, 100));
        assert!(config.anchored(&anchors, 148, 100));
        assert!(!config.anchored(&anchors, 149, 100));
        assert!(!config.anchored(&anchors, 140, 140));
        assert!(!config.anchored(&[], 0, 0));
    }

    #[test]
    fn messages_survive_the_wire() {
        for message in [
            Message {
                shift: true,
                epoch: 5,
                shifts_per_year: 8,
            },
            Message {
                shift: false,
                epoch: -3,
                shifts_per_year: 4096,
            },
        ] {
            assert_eq!(Message::decode(&message.encode()), Some(message));
        }
        assert_eq!(Message::decode(&[]), None);
        assert_eq!(Message::decode(&[2; 13]), None);
        let mut bad = Message {
            shift: true,
            epoch: 1,
            shifts_per_year: 8,
        }
        .encode();
        bad[9..13].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(Message::decode(&bad), None, "a year of no shifts");
    }

    #[test]
    fn the_banner_darkens_holds_and_clears() {
        assert_eq!(banner(0).unwrap().black, 0.0);
        let dark = banner(FADE_IN_MS + 1000).unwrap();
        assert_eq!(dark.black, 1.0);
        assert_eq!(dark.words, 1.0);
        assert!(
            banner(FADE_IN_MS / 4).unwrap().words == 0.0,
            "words wait for the dark"
        );
        let leaving = banner(FADE_IN_MS + HOLD_MS + FADE_OUT_MS / 2).unwrap();
        assert!(leaving.black > 0.3 && leaving.black < 0.7);
        assert!(leaving.words < leaving.black, "the words leave first");
        assert_eq!(banner(BANNER_MS), None);
        // Black never jumps.
        let mut last = 0.0;
        for ms in (0..BANNER_MS).step_by(16) {
            let now = banner(ms).unwrap().black;
            assert!((now - last).abs() < 0.08, "a jump at {ms} ms");
            last = now;
        }
    }

    #[test]
    fn autumn_turns_leaves_and_evergreens_keep_theirs() {
        let green = 0x59AE30;
        let autumn = tinted(Season::Autumn, green, true, false);
        assert!(autumn >> 16 > green >> 16, "redder");
        assert!((autumn >> 8) & 255 < (green >> 8) & 255, "less green");
        assert_eq!(tinted(Season::Autumn, green, true, true), green);
        assert_eq!(tinted(Season::Summer, green, true, false), green);
        assert_ne!(
            tinted(Season::Winter, green, false, true),
            green,
            "grass pales even under pines"
        );
        assert!(evergreen("minecraft:old_growth_pine_taiga"));
        assert!(evergreen("minecraft:jungle"));
        assert!(!evergreen("minecraft:forest"));
    }

    #[test]
    fn chunks_are_ordered_nearest_first_and_listed_once() {
        let chunks = chunks_around(&[[0, 0], [3, 0]], 2);
        assert_eq!(chunks.len(), 5 * 5 + 5 * 3, "two overlapping squares");
        assert!(chunks[..2].contains(&[0, 0]) && chunks[..2].contains(&[3, 0]));
        let unique: HashSet<_> = chunks.iter().collect();
        assert_eq!(unique.len(), chunks.len());
        assert!(chunks_around(&[], 4).is_empty());
    }

    #[test]
    fn the_queue_serves_urgent_chunks_first_and_each_chunk_once() {
        let mut queue = Queue::default();
        queue.later([9, 9]);
        queue.later([8, 8]);
        queue.later([9, 9]);
        assert_eq!(queue.len(), 2);
        queue.first(&[[1, 1], [2, 2], [8, 8]]);
        let served: Vec<_> = std::iter::from_fn(|| queue.pop()).collect();
        assert_eq!(served, [[1, 1], [2, 2], [8, 8], [9, 9]]);
        assert!(queue.is_empty());
        // Once served, a chunk can wait again.
        queue.later([1, 1]);
        assert_eq!(queue.pop(), Some([1, 1]));
    }
}
