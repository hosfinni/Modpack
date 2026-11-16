//! Thirst: a second bar beside hunger that empties far more slowly, faster in heat
//! and under effort, and is filled by drinking and by some foods.
//!
//! The level runs from 0 to 20 like the food level, so the HUD shows it as ten
//! droplets of two halves.

use std::collections::BTreeMap;

use serde::Deserialize;

/// A full bar.
pub const MAX: f32 = 20.0;
/// The channel the server tells each client its level on.
pub const CHANNEL: &str = "hz_survival:thirst";
/// The config key that matches any potion the table doesn't name.
const ANY_POTION: &str = "potion:*";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ThirstConfig {
    pub enabled: bool,
    /// How long a full bar lasts standing still in a mild biome.
    pub minutes_per_bar: f32,
    pub sprint_multiplier: f32,
    pub hot_multiplier: f32,
    pub cold_multiplier: f32,
    pub nether_multiplier: f32,
    pub burning_multiplier: f32,
    /// In water or rain.
    pub wet_multiplier: f32,
    /// At or below this the player is slowed.
    pub parched_at: f32,
    /// At or below this the player is weakened too and stops healing.
    pub dehydrated_at: f32,
    pub damage_interval_seconds: u32,
    pub damage: f32,
    /// false: thirst alone never takes the last half heart.
    pub lethal: bool,
    pub drain_on_peaceful: bool,
    /// The level a player has after respawning.
    pub respawn_level: f32,
    /// The chance that drinking something in `untreated` brings on Hunger.
    pub untreated_sickness_chance: f32,
    pub untreated_sickness_seconds: u32,
    pub untreated: Vec<String>,
    /// What each item gives when eaten or drunk; a negative value takes.
    pub drinks: BTreeMap<String, f32>,
}

impl Default for ThirstConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            minutes_per_bar: 100.0,
            sprint_multiplier: 2.0,
            hot_multiplier: 1.75,
            cold_multiplier: 0.8,
            nether_multiplier: 2.5,
            burning_multiplier: 3.0,
            wet_multiplier: 0.6,
            parched_at: 6.0,
            dehydrated_at: 2.0,
            damage_interval_seconds: 4,
            damage: 1.0,
            lethal: false,
            drain_on_peaceful: false,
            respawn_level: MAX,
            untreated_sickness_chance: 0.25,
            untreated_sickness_seconds: 20,
            untreated: Vec::new(),
            drinks: BTreeMap::new(),
        }
    }
}

/// How hot the place a player stands in is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Climate {
    Cold,
    Mild,
    Hot,
    /// The Nether.
    Scorching,
}

/// What a player was doing and where, over the last second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exposure {
    pub climate: Climate,
    pub sprinting: bool,
    pub burning: bool,
    pub wet: bool,
}

impl Exposure {
    pub const RESTING: Self = Self {
        climate: Climate::Mild,
        sprinting: false,
        burning: false,
        wet: false,
    };
}

/// What a thirst level does to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Quenched = 0,
    /// Slowed.
    Parched = 1,
    /// Slowed, weakened, and no longer healing.
    Dehydrated = 2,
    /// Empty: taking damage.
    Dying = 3,
}

/// What eating or drinking an item does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drink {
    pub points: f32,
    /// May bring on sickness (plain water from a bottle).
    pub untreated: bool,
}

impl ThirstConfig {
    pub fn check(&self) -> Result<(), String> {
        let positive = |name: &str, value: f32| {
            if value.is_finite() && value > 0.0 {
                Ok(())
            } else {
                Err(format!("thirst.{name} must be more than 0, not {value}"))
            }
        };
        positive("minutes_per_bar", self.minutes_per_bar)?;
        for (name, value) in [
            ("sprint_multiplier", self.sprint_multiplier),
            ("hot_multiplier", self.hot_multiplier),
            ("cold_multiplier", self.cold_multiplier),
            ("nether_multiplier", self.nether_multiplier),
            ("burning_multiplier", self.burning_multiplier),
            ("wet_multiplier", self.wet_multiplier),
            ("damage", self.damage),
        ] {
            positive(name, value)?;
        }
        if self.damage_interval_seconds == 0 {
            return Err("thirst.damage_interval_seconds must be at least 1".into());
        }
        if !(0.0..=1.0).contains(&self.untreated_sickness_chance) {
            return Err("thirst.untreated_sickness_chance is a chance, 0 to 1".into());
        }
        if !(0.0..=MAX).contains(&self.respawn_level) {
            return Err(format!("thirst.respawn_level is 0 to {MAX}"));
        }
        if !(self.dehydrated_at <= self.parched_at && self.parched_at <= MAX) {
            return Err("thirst: dehydrated_at must not be above parched_at".into());
        }
        if let Some((item, points)) = self.drinks.iter().find(|(_, points)| !points.is_finite()) {
            return Err(format!(
                "thirst.drinks.\"{item}\" must be a number, not {points}"
            ));
        }
        Ok(())
    }

    /// The points lost in one second of `exposure`.
    pub fn drain_per_second(&self, exposure: &Exposure) -> f32 {
        let mut rate = MAX / (self.minutes_per_bar * 60.0);
        rate *= match exposure.climate {
            Climate::Cold => self.cold_multiplier,
            Climate::Mild => 1.0,
            Climate::Hot => self.hot_multiplier,
            Climate::Scorching => self.nether_multiplier,
        };
        if exposure.sprinting {
            rate *= self.sprint_multiplier;
        }
        if exposure.burning {
            rate *= self.burning_multiplier;
        } else if exposure.wet {
            rate *= self.wet_multiplier;
        }
        rate
    }

    pub fn stage(&self, level: f32) -> Stage {
        if level <= 0.0 {
            Stage::Dying
        } else if level <= self.dehydrated_at {
            Stage::Dehydrated
        } else if level <= self.parched_at {
            Stage::Parched
        } else {
            Stage::Quenched
        }
    }

    /// What using up `item` gives; `potion` is its potion contents (`minecraft:water`)
    /// when it has any. None for an item that doesn't touch thirst.
    pub fn drink(&self, item: &str, potion: Option<&str>) -> Option<Drink> {
        let lookup = |key: &str| {
            let points = *self.drinks.get(key)?;
            Some(Drink {
                points,
                untreated: self.untreated.iter().any(|u| u == key),
            })
        };
        // What a bottle holds first, then the item, then any potion at all.
        potion
            .and_then(|potion| lookup(&format!("potion:{potion}")))
            .or_else(|| lookup(item))
            .or_else(|| potion.and_then(|_| lookup(ANY_POTION)))
    }
}

/// How hot `biome` of `dimension` is, from their ids: vanilla names its biomes after
/// their climate, and a data pack's own usually follow.
pub fn climate_of(dimension: &str, biome: Option<&str>) -> Climate {
    if dimension == "minecraft:the_nether" {
        return Climate::Scorching;
    }
    let Some(biome) = biome else {
        return Climate::Mild;
    };
    let name = biome.rsplit(':').next().unwrap_or(biome);
    const HOT: [&str; 5] = ["desert", "badlands", "savanna", "jungle", "mangrove"];
    const COLD: [&str; 6] = [
        "snowy",
        "frozen",
        "ice_spikes",
        "grove",
        "jagged_peaks",
        "cold",
    ];
    if HOT.iter().any(|word| name.contains(word)) {
        Climate::Hot
    } else if COLD.iter().any(|word| name.contains(word)) {
        Climate::Cold
    } else {
        Climate::Mild
    }
}

/// One player's thirst while they are online.
#[derive(Clone, Debug, PartialEq)]
pub struct Thirst {
    pub level: f32,
    /// Seconds spent empty since the last damage.
    empty_seconds: u32,
}

/// What a second of thirst asks of the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Second {
    pub stage: Stage,
    /// Damage to deal now.
    pub hurt: Option<f32>,
}

impl Thirst {
    pub fn new(level: f32) -> Self {
        Self {
            level: clamp(level),
            empty_seconds: 0,
        }
    }

    /// The level as the HUD shows it: half droplets, 0 to 20, rounded up so that the
    /// bar only reads empty when it is.
    pub fn display(&self) -> u8 {
        self.level.ceil().clamp(0.0, MAX) as u8
    }

    /// One second passes for a player with `health` health points.
    pub fn second(&mut self, config: &ThirstConfig, exposure: &Exposure, health: f32) -> Second {
        self.level = clamp(self.level - config.drain_per_second(exposure));
        let stage = config.stage(self.level);
        let mut hurt = None;
        if stage == Stage::Dying {
            self.empty_seconds += 1;
            if self.empty_seconds >= config.damage_interval_seconds {
                self.empty_seconds = 0;
                // Unless lethal, the last half heart is left, as vanilla's starving
                // leaves it below hard difficulty.
                if config.lethal || health - config.damage >= 1.0 {
                    hurt = Some(config.damage);
                }
            }
        } else {
            self.empty_seconds = 0;
        }
        Second { stage, hurt }
    }

    /// Eating or drinking: `points` may be negative (salty food).
    pub fn drink(&mut self, points: f32) {
        self.level = clamp(self.level + points);
    }
}

fn clamp(level: f32) -> f32 {
    if level.is_finite() {
        level.clamp(0.0, MAX)
    } else {
        MAX
    }
}

/// The message a server sends a client: the bar and what it does to them.
pub fn encode(display: u8, stage: Stage) -> [u8; 2] {
    [display.min(MAX as u8), stage as u8]
}

/// Reads [`encode`]'s bytes. None for anything else, which a client ignores.
pub fn decode(data: &[u8]) -> Option<(u8, Stage)> {
    let [display, stage] = *data else {
        return None;
    };
    let stage = match stage {
        0 => Stage::Quenched,
        1 => Stage::Parched,
        2 => Stage::Dehydrated,
        3 => Stage::Dying,
        _ => return None,
    };
    (display <= MAX as u8).then_some((display, stage))
}

/// One droplet of the HUD row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Droplet {
    Empty,
    Half,
    Full,
}

/// The 9x9 bottle each place on the thirst row shows: `c` the cork, `#` the glass's
/// rim, `o` what the bottle holds. tools/make_content.py draws the same bottle into
/// the content pack's textures; the mod draws it pixel by pixel when they are missing.
pub const BOTTLE: [&str; 9] = [
    "...ccc...",
    "...#o#...",
    "..#ooo#..",
    ".#ooooo#.",
    ".#ooooo#.",
    ".#ooooo#.",
    ".#ooooo#.",
    ".#ooooo#.",
    "..#####..",
];
/// The bottle's colours, 0xAARRGGBB.
pub const CORK: u32 = 0xFF8A_5A2B;
pub const RIM: u32 = 0xFF10_182A;
pub const WATER: u32 = 0xFF3F_76E4;
pub const GLASS: u32 = 0xFF2A_3350;
pub const SHINE: u32 = 0xFF9C_C4FF;
/// The first row of the bottle a half bottle has water in.
const HALF_LINE: usize = 5;

/// The colour of pixel `(x, y)` of a bottle filled as `droplet`, None where it is clear.
pub fn bottle_pixel(droplet: Droplet, x: usize, y: usize) -> Option<u32> {
    let key = *BOTTLE.get(y)?.as_bytes().get(x)?;
    match key {
        b'c' => Some(CORK),
        b'#' => Some(RIM),
        b'o' => {
            let wet = match droplet {
                Droplet::Full => y >= 2,
                Droplet::Half => y >= HALF_LINE,
                Droplet::Empty => false,
            };
            Some(if !wet {
                GLASS
            } else if (x, y) == (2, 4) || (x, y) == (2, 5) {
                SHINE
            } else {
                WATER
            })
        }
        _ => None,
    }
}

/// The ten droplets of the HUD row as `(x, y, droplet)`, for a screen of `width` x
/// `height` GUI pixels. They mirror vanilla's food row (right of the hotbar's centre,
/// filled from the right) and sit `rows_above_food` rows of 10 pixels above it.
pub fn hud_row(
    display: u8,
    width: i32,
    height: i32,
    rows_above_food: i32,
) -> [(i32, i32, Droplet); 10] {
    let y = height - 39 - 10 * rows_above_food;
    std::array::from_fn(|i| {
        let x = width / 2 + 91 - (i as i32) * 8 - 9;
        let droplet = match display.saturating_sub(i as u8 * 2) {
            0 => Droplet::Empty,
            1 => Droplet::Half,
            _ => Droplet::Full,
        };
        (x, y, droplet)
    })
}

/// A tooltip line for an item that changes thirst by `points`, with its colour as
/// 0xRRGGBBAA.
pub fn tooltip(points: f32) -> (String, u32) {
    let droplets = points / 2.0;
    let amount = if droplets.fract() == 0.0 {
        format!("{}", droplets.abs() as i32)
    } else {
        format!("{:.1}", droplets.abs())
    };
    if points >= 0.0 {
        (format!("Thirst +{amount}"), 0x55FFFFFF)
    } else {
        (format!("Thirst -{amount}"), 0xFF5555FF)
    }
}

/// A draw in 0..1 from `seed` (SplitMix64), for the sickness roll: the same seed
/// always gives the same draw.
pub fn roll(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn config() -> ThirstConfig {
        Config::shipped().thirst
    }

    #[test]
    fn a_resting_bar_lasts_as_long_as_the_config_says() {
        let config = config();
        let mut thirst = Thirst::new(MAX);
        let seconds = (config.minutes_per_bar * 60.0) as u32;
        for _ in 0..seconds - 60 {
            thirst.second(&config, &Exposure::RESTING, 20.0);
        }
        assert!(thirst.level > 0.0, "not empty a minute early");
        for _ in 0..120 {
            thirst.second(&config, &Exposure::RESTING, 20.0);
        }
        assert_eq!(thirst.level, 0.0);
    }

    #[test]
    fn thirst_outlasts_hunger_even_when_sprinting_in_a_desert() {
        // A sprinting player empties vanilla's food bar in about 12 minutes of running
        // (0.1 exhaustion a metre at 5.6 m/s, 4 exhaustion a point, saturation aside).
        let config = config();
        let worst = Exposure {
            climate: Climate::Hot,
            sprinting: true,
            burning: false,
            wet: false,
        };
        let minutes = MAX / config.drain_per_second(&worst) / 60.0;
        assert!(
            minutes > 24.0,
            "a full bar lasts {minutes} minutes at worst"
        );
    }

    #[test]
    fn heat_and_effort_drain_faster_and_water_slower() {
        let config = config();
        let rest = config.drain_per_second(&Exposure::RESTING);
        let with = |change: fn(&mut Exposure)| {
            let mut exposure = Exposure::RESTING;
            change(&mut exposure);
            config.drain_per_second(&exposure)
        };
        assert!(with(|e| e.sprinting = true) > rest);
        assert!(with(|e| e.climate = Climate::Hot) > rest);
        assert!(with(|e| e.climate = Climate::Scorching) > with(|e| e.climate = Climate::Hot));
        assert!(with(|e| e.climate = Climate::Cold) < rest);
        assert!(with(|e| e.wet = true) < rest);
        // Fire wins over rain.
        assert!(
            with(|e| {
                e.wet = true;
                e.burning = true
            }) > rest
        );
    }

    #[test]
    fn biomes_are_sorted_by_their_names() {
        assert_eq!(
            climate_of("minecraft:overworld", Some("minecraft:desert")),
            Climate::Hot
        );
        assert_eq!(
            climate_of("minecraft:overworld", Some("minecraft:wooded_badlands")),
            Climate::Hot
        );
        assert_eq!(
            climate_of("minecraft:overworld", Some("minecraft:snowy_taiga")),
            Climate::Cold
        );
        assert_eq!(
            climate_of("minecraft:overworld", Some("minecraft:frozen_ocean")),
            Climate::Cold
        );
        assert_eq!(
            climate_of("minecraft:overworld", Some("minecraft:plains")),
            Climate::Mild
        );
        assert_eq!(climate_of("minecraft:overworld", None), Climate::Mild);
        assert_eq!(
            climate_of("minecraft:the_nether", Some("minecraft:soul_sand_valley")),
            Climate::Scorching
        );
    }

    #[test]
    fn the_stages_follow_the_level() {
        let config = config();
        assert_eq!(config.stage(20.0), Stage::Quenched);
        assert_eq!(config.stage(6.5), Stage::Quenched);
        assert_eq!(config.stage(6.0), Stage::Parched);
        assert_eq!(config.stage(2.0), Stage::Dehydrated);
        assert_eq!(config.stage(0.0), Stage::Dying);
    }

    #[test]
    fn an_empty_bar_hurts_at_intervals_and_spares_the_last_half_heart() {
        let config = config();
        let mut thirst = Thirst::new(0.0);
        let mut hurts = 0;
        for _ in 0..config.damage_interval_seconds * 3 {
            if thirst
                .second(&config, &Exposure::RESTING, 20.0)
                .hurt
                .is_some()
            {
                hurts += 1;
            }
        }
        assert_eq!(hurts, 3);
        // At one health point left, nothing more is taken.
        let mut low = Thirst::new(0.0);
        for _ in 0..config.damage_interval_seconds * 3 {
            assert_eq!(low.second(&config, &Exposure::RESTING, 1.0).hurt, None);
        }
        // Unless the config makes it lethal.
        let lethal = ThirstConfig {
            lethal: true,
            ..config
        };
        let mut dying = Thirst::new(0.0);
        let hurt = (0..lethal.damage_interval_seconds)
            .filter_map(|_| dying.second(&lethal, &Exposure::RESTING, 1.0).hurt)
            .count();
        assert_eq!(hurt, 1);
    }

    #[test]
    fn drinking_fills_the_bar_and_stops_at_its_ends() {
        let mut thirst = Thirst::new(15.0);
        thirst.drink(8.0);
        assert_eq!(thirst.level, MAX);
        thirst.drink(-30.0);
        assert_eq!(thirst.level, 0.0);
        thirst.drink(f32::NAN);
        assert_eq!(thirst.level, MAX, "a broken value never empties the bar");
    }

    #[test]
    fn the_drinks_table_knows_water_bottles_other_potions_and_food() {
        let config = config();
        let water = config
            .drink("minecraft:potion", Some("minecraft:water"))
            .unwrap();
        assert!(water.points > 0.0 && water.untreated);
        let healing = config
            .drink("minecraft:potion", Some("minecraft:healing"))
            .unwrap();
        assert!(healing.points > 0.0 && !healing.untreated);
        let melon = config.drink("minecraft:melon_slice", None).unwrap();
        assert!(melon.points > 0.0 && !melon.untreated);
        assert!(config.drink("minecraft:dried_kelp", None).unwrap().points < 0.0);
        assert_eq!(config.drink("minecraft:bread", None), None);
        // The pack's own drinks beat plain water, and are safe.
        let purified = config.drink("hz_content:purified_water", None).unwrap();
        assert!(purified.points > water.points && !purified.untreated);
    }

    #[test]
    fn the_display_rounds_up_and_survives_the_wire() {
        assert_eq!(Thirst::new(20.0).display(), 20);
        assert_eq!(Thirst::new(0.2).display(), 1);
        assert_eq!(Thirst::new(0.0).display(), 0);
        for stage in [
            Stage::Quenched,
            Stage::Parched,
            Stage::Dehydrated,
            Stage::Dying,
        ] {
            assert_eq!(decode(&encode(13, stage)), Some((13, stage)));
        }
        assert_eq!(decode(&[21, 0]), None);
        assert_eq!(decode(&[5, 9]), None);
        assert_eq!(decode(&[5]), None);
    }

    #[test]
    fn the_hud_row_mirrors_the_food_row() {
        // 13 half droplets: six full, one half, three empty, from the right.
        let row = hud_row(13, 320, 240, 1);
        assert_eq!(row[0], (320 / 2 + 91 - 9, 240 - 49, Droplet::Full));
        assert_eq!(row[5].2, Droplet::Full);
        assert_eq!(row[6].2, Droplet::Half);
        assert_eq!(row[7].2, Droplet::Empty);
        assert_eq!(row[9].0, 320 / 2 + 91 - 72 - 9);
    }

    #[test]
    fn tooltips_count_droplets() {
        assert_eq!(tooltip(6.0).0, "Thirst +3");
        assert_eq!(tooltip(1.0).0, "Thirst +0.5");
        assert_eq!(tooltip(-2.0).0, "Thirst -1");
    }

    #[test]
    fn rolls_are_fractions_and_repeat() {
        for seed in 0..1000u64 {
            let draw = roll(seed);
            assert!((0.0..1.0).contains(&draw));
            assert_eq!(draw, roll(seed));
        }
        let low = (0..10_000u64).filter(|s| roll(*s) < 0.25).count();
        assert!((2200..2800).contains(&low), "{low} of 10000 under 0.25");
    }

    #[test]
    fn a_bottle_holds_water_as_its_droplet_says() {
        let count = |droplet, colour| {
            (0..9)
                .flat_map(|y| (0..9).map(move |x| (x, y)))
                .filter(|&(x, y)| bottle_pixel(droplet, x, y) == Some(colour))
                .count()
        };
        let wet = |droplet| count(droplet, WATER) + count(droplet, SHINE);
        assert_eq!(wet(Droplet::Empty), 0);
        assert!(wet(Droplet::Half) > 0 && wet(Droplet::Half) < wet(Droplet::Full));
        assert!(count(Droplet::Full, CORK) == 3 && count(Droplet::Full, RIM) > 10);
        assert_eq!(bottle_pixel(Droplet::Full, 0, 0), None);
        assert_eq!(bottle_pixel(Droplet::Full, 9, 0), None);
        assert!(BOTTLE.iter().all(|row| row.len() == 9));
    }
}
