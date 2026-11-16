//! hz_worldgen's terrain, with no game types: for a seed, what every column of the
//! world is made of. The trait mod in `../api` writes it into Horizon's chunks.
//!
//! [`World::column`] is the whole terrain function: a column's height, biome and
//! blocks depend only on the seed, the config and its position, so chunks can be
//! built in any order on any thread, and far terrain can be drawn before a chunk
//! exists.

use serde::Deserialize;

pub mod noise;
pub mod plates;

use noise::{derive, fbm, ridged, simplex, smoothstep, wave};
use plates::Plates;

/// The config the mod writes on its first run.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Terrain {
    /// This mod builds the overworld.
    #[default]
    Tectonic,
    /// The game's own generator, or a data pack's, builds it.
    Vanilla,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub terrain: Terrain,
    pub sea_level: i32,
    pub plate_size: f64,
    pub ocean_fraction: f64,
    pub mountain_scale: f64,
    pub climate_band: f64,
    pub rivers: bool,
    pub structures: bool,
    pub carvers: bool,
    pub decorations: bool,
    /// Keep vanilla's caves and structures out of the chunks where they would meet
    /// this terrain's water ([`additions`]).
    pub fit_to_sea: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            terrain: Terrain::Tectonic,
            sea_level: 63,
            plate_size: 2400.0,
            ocean_fraction: 0.45,
            mountain_scale: 1.0,
            climate_band: 9000.0,
            rivers: true,
            structures: true,
            carvers: true,
            decorations: true,
            fit_to_sea: true,
        }
    }
}

impl Config {
    /// Reads a config file. Missing keys take their defaults; an unknown key is an
    /// error, so a typo doesn't silently change the world.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        let within = |name: &str, value: f64, low: f64, high: f64| {
            if value >= low && value <= high {
                Ok(())
            } else {
                Err(format!(
                    "{name} must be between {low} and {high}, not {value}"
                ))
            }
        };
        within("sea_level", f64::from(config.sea_level), 0.0, 200.0)?;
        within("plate_size", config.plate_size, 400.0, 100_000.0)?;
        within("ocean_fraction", config.ocean_fraction, 0.0, 1.0)?;
        within("mountain_scale", config.mountain_scale, 0.0, 1.6)?;
        within("climate_band", config.climate_band, 500.0, 1_000_000.0)?;
        Ok(config)
    }

    /// The config a fresh install runs with.
    pub fn shipped() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the shipped config parses")
    }
}

/// The biomes the terrain uses, all vanilla's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Biome {
    DeepFrozenOcean,
    DeepColdOcean,
    DeepOcean,
    DeepLukewarmOcean,
    FrozenOcean,
    ColdOcean,
    Ocean,
    LukewarmOcean,
    WarmOcean,
    SnowyBeach,
    Beach,
    StonyShore,
    FrozenRiver,
    River,
    FrozenPeaks,
    JaggedPeaks,
    StonyPeaks,
    SnowySlopes,
    Grove,
    Meadow,
    WindsweptHills,
    SnowyPlains,
    SnowyTaiga,
    Taiga,
    OldGrowthSpruceTaiga,
    Plains,
    Forest,
    BirchForest,
    DarkForest,
    Swamp,
    Savanna,
    SavannaPlateau,
    Jungle,
    SparseJungle,
    MangroveSwamp,
    Desert,
    Badlands,
}

impl Biome {
    pub const ALL: [Biome; 37] = [
        Biome::DeepFrozenOcean,
        Biome::DeepColdOcean,
        Biome::DeepOcean,
        Biome::DeepLukewarmOcean,
        Biome::FrozenOcean,
        Biome::ColdOcean,
        Biome::Ocean,
        Biome::LukewarmOcean,
        Biome::WarmOcean,
        Biome::SnowyBeach,
        Biome::Beach,
        Biome::StonyShore,
        Biome::FrozenRiver,
        Biome::River,
        Biome::FrozenPeaks,
        Biome::JaggedPeaks,
        Biome::StonyPeaks,
        Biome::SnowySlopes,
        Biome::Grove,
        Biome::Meadow,
        Biome::WindsweptHills,
        Biome::SnowyPlains,
        Biome::SnowyTaiga,
        Biome::Taiga,
        Biome::OldGrowthSpruceTaiga,
        Biome::Plains,
        Biome::Forest,
        Biome::BirchForest,
        Biome::DarkForest,
        Biome::Swamp,
        Biome::Savanna,
        Biome::SavannaPlateau,
        Biome::Jungle,
        Biome::SparseJungle,
        Biome::MangroveSwamp,
        Biome::Desert,
        Biome::Badlands,
    ];

    /// The biome's id in the game.
    pub fn id(self) -> &'static str {
        match self {
            Biome::DeepFrozenOcean => "minecraft:deep_frozen_ocean",
            Biome::DeepColdOcean => "minecraft:deep_cold_ocean",
            Biome::DeepOcean => "minecraft:deep_ocean",
            Biome::DeepLukewarmOcean => "minecraft:deep_lukewarm_ocean",
            Biome::FrozenOcean => "minecraft:frozen_ocean",
            Biome::ColdOcean => "minecraft:cold_ocean",
            Biome::Ocean => "minecraft:ocean",
            Biome::LukewarmOcean => "minecraft:lukewarm_ocean",
            Biome::WarmOcean => "minecraft:warm_ocean",
            Biome::SnowyBeach => "minecraft:snowy_beach",
            Biome::Beach => "minecraft:beach",
            Biome::StonyShore => "minecraft:stony_shore",
            Biome::FrozenRiver => "minecraft:frozen_river",
            Biome::River => "minecraft:river",
            Biome::FrozenPeaks => "minecraft:frozen_peaks",
            Biome::JaggedPeaks => "minecraft:jagged_peaks",
            Biome::StonyPeaks => "minecraft:stony_peaks",
            Biome::SnowySlopes => "minecraft:snowy_slopes",
            Biome::Grove => "minecraft:grove",
            Biome::Meadow => "minecraft:meadow",
            Biome::WindsweptHills => "minecraft:windswept_hills",
            Biome::SnowyPlains => "minecraft:snowy_plains",
            Biome::SnowyTaiga => "minecraft:snowy_taiga",
            Biome::Taiga => "minecraft:taiga",
            Biome::OldGrowthSpruceTaiga => "minecraft:old_growth_spruce_taiga",
            Biome::Plains => "minecraft:plains",
            Biome::Forest => "minecraft:forest",
            Biome::BirchForest => "minecraft:birch_forest",
            Biome::DarkForest => "minecraft:dark_forest",
            Biome::Swamp => "minecraft:swamp",
            Biome::Savanna => "minecraft:savanna",
            Biome::SavannaPlateau => "minecraft:savanna_plateau",
            Biome::Jungle => "minecraft:jungle",
            Biome::SparseJungle => "minecraft:sparse_jungle",
            Biome::MangroveSwamp => "minecraft:mangrove_swamp",
            Biome::Desert => "minecraft:desert",
            Biome::Badlands => "minecraft:badlands",
        }
    }

    /// The biome's place in [`Biome::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The blocks the terrain is made of, all vanilla's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    Bedrock,
    Deepslate,
    Stone,
    Water,
    Dirt,
    GrassBlock,
    Sand,
    Sandstone,
    RedSand,
    Terracotta,
    OrangeTerracotta,
    Gravel,
    SnowBlock,
    Podzol,
    Mud,
    CoarseDirt,
}

impl Block {
    pub const ALL: [Block; 16] = [
        Block::Bedrock,
        Block::Deepslate,
        Block::Stone,
        Block::Water,
        Block::Dirt,
        Block::GrassBlock,
        Block::Sand,
        Block::Sandstone,
        Block::RedSand,
        Block::Terracotta,
        Block::OrangeTerracotta,
        Block::Gravel,
        Block::SnowBlock,
        Block::Podzol,
        Block::Mud,
        Block::CoarseDirt,
    ];

    /// The block's id in the game; its default state is the one placed.
    pub fn id(self) -> &'static str {
        match self {
            Block::Bedrock => "minecraft:bedrock",
            Block::Deepslate => "minecraft:deepslate",
            Block::Stone => "minecraft:stone",
            Block::Water => "minecraft:water",
            Block::Dirt => "minecraft:dirt",
            Block::GrassBlock => "minecraft:grass_block",
            Block::Sand => "minecraft:sand",
            Block::Sandstone => "minecraft:sandstone",
            Block::RedSand => "minecraft:red_sand",
            Block::Terracotta => "minecraft:terracotta",
            Block::OrangeTerracotta => "minecraft:orange_terracotta",
            Block::Gravel => "minecraft:gravel",
            Block::SnowBlock => "minecraft:snow_block",
            Block::Podzol => "minecraft:podzol",
            Block::Mud => "minecraft:mud",
            Block::CoarseDirt => "minecraft:coarse_dirt",
        }
    }

    /// The block's place in [`Block::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// One column of the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// The y of its highest solid block.
    pub height: i32,
    pub biome: Biome,
    /// A river runs here: its bed is cut down to below the sea's level.
    pub river: bool,
    /// How much the land is folded here: 0 on a plain, 1 in a high range.
    pub ruggedness: f64,
    /// -1 (polar) to 1 (tropical).
    pub temperature: f64,
    /// -1 (arid) to 1 (soaked).
    pub humidity: f64,
}

/// A run of one block in a column: from `low` up to but not including `high`.
pub type Span = (i32, i32, Block);

pub struct World {
    config: Config,
    plates: Plates,
    seeds: Seeds,
}

struct Seeds {
    warp: u64,
    detail: u64,
    rough: u64,
    river: u64,
    temperature: u64,
    humidity: u64,
    variety: u64,
    strata: u64,
}

/// The lowest the sea floor goes.
const FLOOR: i32 = 8;

impl World {
    pub fn new(seed: i64, config: Config) -> Self {
        let seed = seed as u64;
        let plates = Plates::new(
            derive(seed, 1),
            config.plate_size,
            config.ocean_fraction,
            config.mountain_scale,
        );
        let seeds = Seeds {
            warp: derive(seed, 2),
            detail: derive(seed, 3),
            rough: derive(seed, 4),
            river: derive(seed, 5),
            temperature: derive(seed, 6),
            humidity: derive(seed, 7),
            variety: derive(seed, 8),
            strata: derive(seed, 9),
        };
        Self {
            config,
            plates,
            seeds,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The column at block `(x, z)`.
    pub fn column(&self, x: i32, z: i32) -> Column {
        let (fx, fz) = (f64::from(x), f64::from(z));
        let size = self.config.plate_size;
        let sea = self.config.sea_level;

        // Bend the plate edges so that coasts and ranges curve instead of running
        // straight between plate corners: a broad bend, a finer one, and a ripple.
        let (broad, fine) = (size * 0.9, size * 0.24);
        let wx = fx
            + size * 0.34 * fbm(self.seeds.warp, fx / broad, fz / broad, 3)
            + size * 0.07 * fbm(self.seeds.warp ^ 4, fx / fine, fz / fine, 2)
            + 28.0 * simplex(self.seeds.warp ^ 1, fx / 130.0, fz / 130.0);
        let wz = fz
            + size * 0.34 * fbm(self.seeds.warp ^ 2, fx / broad, fz / broad, 3)
            + size * 0.07 * fbm(self.seeds.warp ^ 5, fx / fine, fz / fine, 2)
            + 28.0 * simplex(self.seeds.warp ^ 3, fx / 130.0, fz / 130.0);
        let tectonics = self.plates.sample(wx, wz);

        // Rolling land inside a plate, and rough land where it is folded.
        let ruggedness = tectonics.uplift.clamp(0.0, 1.2);
        // Swells wider than a plate, so that a coast is not simply a plate's edge:
        // shelves drown, and bays and inland seas open.
        let swell = 0.2
            * fbm(
                self.seeds.detail ^ 2,
                fx / (size * 1.15),
                fz / (size * 1.15),
                3,
            );
        let rolling = swell
            + 0.12 * fbm(self.seeds.detail, fx / 820.0, fz / 820.0, 4)
            + 0.04 * fbm(self.seeds.detail ^ 1, fx / 190.0, fz / 190.0, 3);
        let crests = ridged(self.seeds.rough, fx / 170.0, fz / 170.0, 4) - 0.45;
        let rough = crests * (0.03 + 0.3 * ruggedness)
            + 0.012 * fbm(self.seeds.rough ^ 1, fx / 40.0, fz / 40.0, 2);
        let mut elevation = soft_cap(tectonics.elevation + rolling + rough);

        // Rivers wind across the lowlands, cut down to the sea's level; higher up
        // they fade into valleys.
        let mut river = false;
        if self.config.rivers && elevation > -0.02 {
            let course = simplex(self.seeds.river, fx / 1050.0, fz / 1050.0)
                + 0.1 * simplex(self.seeds.river ^ 1, fx / 160.0, fz / 160.0);
            let nearness = 1.0 - smoothstep(0.0, 0.075, course.abs());
            let lowland = 1.0 - smoothstep(0.35, 0.8, elevation);
            let cut = nearness * lowland;
            if cut > 0.0 {
                let bed = -0.045;
                elevation = elevation + (bed - elevation) * cut;
                river = cut > 0.72 && elevation < 0.0;
            }
        }

        let height = if elevation >= 0.0 {
            sea + (30.0 * elevation + 95.0 * elevation * elevation) as i32
        } else {
            (sea - 1 + (58.0 * elevation) as i32).max(FLOOR)
        };

        // Climate: warm and cold belts by latitude, their edges bent by noise, colder
        // with height. z = 0 lies half way between a warm belt and a cold one.
        let band = self.config.climate_band;
        let latitude = fz
            + band * 0.5
            + band
                * 0.32
                * fbm(
                    self.seeds.temperature ^ 1,
                    fx / (band * 0.7),
                    fz / (band * 0.7),
                    2,
                );
        let lapse = f64::from((height - 96).max(0)) / 240.0;
        let temperature = (0.52 * wave(latitude, band * 2.0)
            + 0.44 * fbm(self.seeds.temperature, fx / 2300.0, fz / 2300.0, 3)
            - lapse)
            .clamp(-1.0, 1.0);
        let coastal = 1.0 - smoothstep(0.0, 0.45, tectonics.elevation);
        let humidity =
            (0.72 * fbm(self.seeds.humidity, fx / 1150.0, fz / 1150.0, 3) + 0.3 * coastal - 0.12)
                .clamp(-1.0, 1.0);
        let variety = simplex(self.seeds.variety, fx / 420.0, fz / 420.0);

        let biome = pick_biome(&Site {
            height,
            sea,
            river,
            ruggedness,
            temperature,
            humidity,
            variety,
        });
        Column {
            height,
            biome,
            river,
            ruggedness,
            temperature,
            humidity,
        }
    }

    /// Water stands over the column: sea, a river, a flooded valley.
    pub fn under_sea(&self, column: &Column) -> bool {
        column.height < self.config.sea_level
    }

    /// The blocks of the column at `(x, z)`, bottom up, between `min_y` and
    /// `max_y` (the level's lowest block and one above its highest). The spans
    /// touch: what is above the last one is air.
    pub fn spans(&self, x: i32, z: i32, min_y: i32, max_y: i32, out: &mut Vec<Span>) -> Column {
        out.clear();
        let column = self.column(x, z);
        let top = column.height.clamp(min_y + 1, max_y - 1);
        let sea = self.config.sea_level;
        let jitter = |salt: i64, range: u64| {
            (noise::hash(self.seeds.strata, i64::from(x) ^ (salt << 40), i64::from(z)) % range)
                as i32
        };

        let surface = surface_of(column.biome, top, sea, column.ruggedness, jitter(1, 4));
        // The top layer, then a few blocks of the layer under it, then rock.
        let under_depth = if surface.under == Block::Stone {
            0
        } else {
            3 + jitter(2, 2)
        };
        let under_from = (top - under_depth).max(min_y + 1);
        // Bedrock is ragged on top, deepslate takes over from stone around y 0.
        let bedrock_top = min_y + 1 + jitter(3, 4);
        let deepslate_top = (jitter(4, 8)).min(under_from).max(bedrock_top);

        let mut push = |low: i32, high: i32, block: Block| {
            if high > low {
                out.push((low, high, block));
            }
        };
        push(min_y, bedrock_top.min(top + 1), Block::Bedrock);
        push(bedrock_top, deepslate_top, Block::Deepslate);
        push(deepslate_top.max(bedrock_top), under_from, Block::Stone);
        push(under_from.max(bedrock_top), top, surface.under);
        push(top.max(bedrock_top), top + 1, surface.top);
        if top < sea {
            push(top + 1, (sea + 1).min(max_y), Block::Water);
        }
        column
    }
}

/// What vanilla adds to one chunk of this terrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Additions {
    pub carvers: bool,
    pub structures: bool,
}

/// The sea in one chunk, as this terrain and as vanilla's would have it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sea {
    /// Water stands over some column of the chunk.
    pub wet: bool,
    /// Water stands over a column next to the chunk, in a neighbouring one.
    pub wet_beside: bool,
    /// Vanilla's biome at the chunk's middle is an ocean, so vanilla puts its sea
    /// structures here and none of its land ones.
    pub vanilla_ocean: bool,
}

/// What vanilla may add to a chunk of this terrain.
///
/// Vanilla's carvers fill what they cut from vanilla's own aquifer, which follows
/// vanilla's terrain: under this terrain's sea, where vanilla had land, a carver
/// would cut air out of the water, leaving dry shafts and walls of water in the sea.
/// So a chunk with water in it, or beside it, gets no carved caves. (A carver writes
/// only into its own chunk, so a dry chunk's caves stop at its edge, a block short of
/// any water beside it.)
///
/// Vanilla places structures by its own biomes: villages, temples and outposts where
/// it had land, monuments, ruins and shipwrecks where it had sea. A chunk keeps them
/// only where both terrains agree on whether there is water: no village stands in this
/// terrain's sea, no monument on its land. Pieces of a structure that reach into a
/// chunk that keeps none are cut off at that chunk's edge.
pub fn additions(config: &Config, sea: Sea) -> Additions {
    if !config.fit_to_sea {
        return Additions {
            carvers: config.carvers,
            structures: config.structures,
        };
    }
    Additions {
        carvers: config.carvers && !sea.wet && !sea.wet_beside,
        structures: config.structures && sea.wet == sea.vanilla_ocean,
    }
}

/// Where ranges stop growing in step with the folding under them.
const KNEE: f64 = 1.05;
/// The elevation no peak reaches: about y 300.
const CEILING: f64 = 1.42;

/// Elevation as it is below `KNEE`, then flattening toward `CEILING`: several plate
/// edges meeting never pile a peak through the top of the world.
fn soft_cap(elevation: f64) -> f64 {
    if elevation <= KNEE {
        return elevation;
    }
    let room = CEILING - KNEE;
    KNEE + room * (1.0 - 1.0 / (1.0 + (elevation - KNEE) / room))
}

struct Site {
    height: i32,
    sea: i32,
    river: bool,
    ruggedness: f64,
    temperature: f64,
    humidity: f64,
    variety: f64,
}

fn pick_biome(site: &Site) -> Biome {
    let (t, h) = (site.temperature, site.humidity);
    let above = site.height - site.sea;
    if site.river {
        return if t < -0.45 {
            Biome::FrozenRiver
        } else {
            Biome::River
        };
    }
    if above < 0 {
        let deep = above < -24;
        return match (deep, t) {
            (true, t) if t < -0.45 => Biome::DeepFrozenOcean,
            (true, t) if t < -0.1 => Biome::DeepColdOcean,
            (true, t) if t < 0.35 => Biome::DeepOcean,
            (true, _) => Biome::DeepLukewarmOcean,
            (false, t) if t < -0.45 => Biome::FrozenOcean,
            (false, t) if t < -0.1 => Biome::ColdOcean,
            (false, t) if t < 0.35 => Biome::Ocean,
            (false, t) if t < 0.62 => Biome::LukewarmOcean,
            (false, _) => Biome::WarmOcean,
        };
    }
    if above <= 2 {
        return if site.ruggedness > 0.25 {
            Biome::StonyShore
        } else if t < -0.45 {
            Biome::SnowyBeach
        } else {
            Biome::Beach
        };
    }
    // The heights: peaks above the tree line, slopes and meadows below them.
    if above > 110 {
        return if t < -0.1 {
            if site.variety > 0.0 {
                Biome::JaggedPeaks
            } else {
                Biome::FrozenPeaks
            }
        } else {
            Biome::StonyPeaks
        };
    }
    if above > 62 {
        return if t < -0.3 {
            Biome::SnowySlopes
        } else if t < 0.0 {
            Biome::Grove
        } else if t > 0.55 && h < -0.1 {
            Biome::Badlands
        } else if t > 0.3 && h < 0.2 {
            Biome::SavannaPlateau
        } else if h < -0.25 {
            Biome::WindsweptHills
        } else {
            Biome::Meadow
        };
    }
    let low = above < 8;
    if t < -0.45 {
        if h > 0.05 {
            Biome::SnowyTaiga
        } else {
            Biome::SnowyPlains
        }
    } else if t < -0.12 {
        if h > 0.4 {
            Biome::OldGrowthSpruceTaiga
        } else if h > -0.2 {
            Biome::Taiga
        } else {
            Biome::Plains
        }
    } else if t < 0.3 {
        if h > 0.5 && low {
            Biome::Swamp
        } else if h > 0.42 {
            Biome::DarkForest
        } else if h > 0.0 {
            if site.variety > 0.35 {
                Biome::BirchForest
            } else {
                Biome::Forest
            }
        } else {
            Biome::Plains
        }
    } else if t < 0.6 {
        if h > 0.5 && low {
            Biome::MangroveSwamp
        } else if h > 0.35 {
            Biome::Jungle
        } else if h > 0.12 {
            Biome::SparseJungle
        } else if h > -0.3 {
            Biome::Savanna
        } else {
            Biome::Desert
        }
    } else if h > 0.4 {
        Biome::Jungle
    } else if h > 0.05 {
        Biome::Savanna
    } else if site.variety > 0.45 && above > 20 {
        Biome::Badlands
    } else {
        Biome::Desert
    }
}

struct Surface {
    top: Block,
    under: Block,
}

/// What a biome's ground is made of. The game's own surface rule reads vanilla's
/// biome map, not this terrain's, so the terrain paints its own ground.
fn surface_of(biome: Biome, top: i32, sea: i32, ruggedness: f64, jitter: i32) -> Surface {
    use Biome::*;
    let (top_block, under) = match biome {
        // Under water: sand where it is warm, gravel where it is cold or deep.
        WarmOcean | LukewarmOcean | DeepLukewarmOcean | Beach | SnowyBeach => {
            (Block::Sand, Block::Sandstone)
        }
        Ocean | DeepOcean | River | FrozenRiver => {
            if top < sea - 6 + jitter {
                (Block::Gravel, Block::Gravel)
            } else {
                (Block::Sand, Block::Sand)
            }
        }
        ColdOcean | DeepColdOcean | FrozenOcean | DeepFrozenOcean => (Block::Gravel, Block::Gravel),
        StonyShore | StonyPeaks | JaggedPeaks => (Block::Stone, Block::Stone),
        FrozenPeaks | SnowySlopes => (Block::SnowBlock, Block::SnowBlock),
        Grove => (Block::SnowBlock, Block::Dirt),
        WindsweptHills => {
            if ruggedness > 0.55 || jitter == 0 {
                (Block::Stone, Block::Stone)
            } else {
                (Block::GrassBlock, Block::Dirt)
            }
        }
        Desert => (Block::Sand, Block::Sandstone),
        Badlands => {
            // Banded hills over red sand flats.
            if top > sea + 30 {
                let band = if (top + jitter / 2) % 5 < 2 {
                    Block::OrangeTerracotta
                } else {
                    Block::Terracotta
                };
                (band, Block::Terracotta)
            } else {
                (Block::RedSand, Block::Terracotta)
            }
        }
        OldGrowthSpruceTaiga => {
            if jitter == 0 {
                (Block::CoarseDirt, Block::Dirt)
            } else {
                (Block::Podzol, Block::Dirt)
            }
        }
        MangroveSwamp => (Block::Mud, Block::Mud),
        Savanna | SavannaPlateau => {
            if jitter == 0 && ruggedness > 0.2 {
                (Block::CoarseDirt, Block::Dirt)
            } else {
                (Block::GrassBlock, Block::Dirt)
            }
        }
        _ => (Block::GrassBlock, Block::Dirt),
    };
    // Ground under water is never grass.
    if top < sea && top_block == Block::GrassBlock {
        return Surface {
            top: Block::Dirt,
            under,
        };
    }
    // The steepest folded land sheds its soil.
    if ruggedness > 0.95 && matches!(top_block, Block::GrassBlock | Block::Podzol) {
        return Surface {
            top: Block::Stone,
            under: Block::Stone,
        };
    }
    Surface {
        top: top_block,
        under,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn world(seed: i64) -> World {
        World::new(seed, Config::shipped())
    }

    #[test]
    fn the_shipped_config_parses_and_bad_values_are_refused() {
        let config = Config::shipped();
        assert_eq!(config.terrain, Terrain::Tectonic);
        assert_eq!(config.sea_level, 63);
        assert_eq!(
            Config::parse("terrain = \"vanilla\"").unwrap().terrain,
            Terrain::Vanilla
        );
        assert!(Config::parse("terrain = \"flat\"").is_err());
        assert!(Config::parse("plate_size = 10.0").is_err());
        assert!(Config::parse("ocean_fraction = 1.5").is_err());
        assert!(Config::parse("see_level = 63").is_err());
    }

    #[test]
    fn a_seed_always_gives_the_same_world_and_seeds_differ() {
        let (a, b, other) = (world(1234), world(1234), world(1235));
        let mut differ = 0;
        for i in 0..400 {
            let (x, z) = (i * 37 - 5000, i * 91 - 9000);
            assert_eq!(a.column(x, z), b.column(x, z));
            if a.column(x, z).height != other.column(x, z).height {
                differ += 1;
            }
        }
        assert!(
            differ > 300,
            "another seed is another world ({differ} of 400 columns differ)"
        );
    }

    #[test]
    fn heights_stay_inside_the_level_and_ranges_rise_high() {
        let world = world(99);
        let (mut highest, mut lowest) = (i32::MIN, i32::MAX);
        for i in 0..220 {
            for j in 0..220 {
                let column = world.column(i * 110 - 12_000, j * 110 - 12_000);
                highest = highest.max(column.height);
                lowest = lowest.min(column.height);
            }
        }
        assert!(lowest >= FLOOR, "the sea floor stops at {lowest}");
        assert!(
            highest < 316,
            "peaks stop below the build limit ({highest})"
        );
        assert!(highest > 190, "there are high ranges ({highest})");
        assert!(lowest < 30, "there are deep seas ({lowest})");
    }

    #[test]
    fn the_ground_is_walkable() {
        // Along long walks, the height changes gently almost everywhere: cliffs are
        // rare, and no column stands walls above its neighbour.
        let world = world(7);
        let (mut steps, mut cliffs, mut worst) = (0, 0, 0);
        for (dx, dz) in [(1, 0), (0, 1)] {
            let mut last = world.column(-6000, 3000).height;
            for i in 1..20_000 {
                let now = world.column(-6000 + dx * i, 3000 + dz * i).height;
                let step = (now - last).abs();
                steps += 1;
                worst = worst.max(step);
                if step > 2 {
                    cliffs += 1;
                }
                last = now;
            }
        }
        assert!(worst <= 14, "a wall of {worst} blocks between neighbours");
        assert!(
            cliffs * 100 / steps < 8,
            "{cliffs} of {steps} steps are cliffs"
        );
    }

    #[test]
    fn the_world_has_sea_land_and_many_biomes() {
        let world = world(2026);
        let sea = world.config().sea_level;
        let mut biomes: BTreeMap<Biome, u32> = BTreeMap::new();
        let (mut land, mut total) = (0, 0);
        for i in 0..300 {
            for j in 0..300 {
                let column = world.column(i * 120 - 18_000, j * 120 - 18_000);
                *biomes.entry(column.biome).or_default() += 1;
                total += 1;
                if column.height >= sea {
                    land += 1;
                }
            }
        }
        let share = land * 100 / total;
        assert!((30..75).contains(&share), "{share}% of the world is land");
        assert!(
            biomes.len() >= 28,
            "only {} biomes: {:?}",
            biomes.len(),
            biomes.keys()
        );
        // No single land biome swallows the map.
        let largest_land = biomes
            .iter()
            .filter(|(biome, _)| biome.index() > Biome::WarmOcean.index())
            .map(|(_, count)| *count)
            .max()
            .unwrap();
        assert!(largest_land * 100 / total < 25);
    }

    #[test]
    fn rivers_run_at_sea_level_and_are_rare() {
        let world = world(31);
        let sea = world.config().sea_level;
        let (mut rivers, mut total) = (0, 0);
        for i in 0..400 {
            for j in 0..400 {
                let column = world.column(i * 23 - 4000, j * 23 - 4000);
                total += 1;
                if column.river {
                    rivers += 1;
                    assert!(column.height < sea, "a river's bed is under water");
                    assert!(matches!(column.biome, Biome::River | Biome::FrozenRiver));
                }
            }
        }
        assert!(rivers > 100, "there are rivers ({rivers})");
        assert!(
            rivers * 100 / total < 10,
            "but not everywhere ({rivers} of {total})"
        );
    }

    #[test]
    fn climate_follows_latitude_and_height() {
        let world = world(5);
        let band = world.config().climate_band as i32;
        let mean = |z: i32| -> f64 {
            (0..400)
                .map(|i| world.column(i * 397 - 80_000, z).temperature)
                .sum::<f64>()
                / 400.0
        };
        // z 0 is temperate; the warm belt lies half a band to its north (negative z),
        // the cold one half a band to its south.
        let (warm, cold) = (mean(-band / 2), mean(band / 2));
        assert!(
            warm > cold + 0.45,
            "a warm belt ({warm}) and a cold belt ({cold})"
        );
        assert!(
            mean(0) < warm && mean(0) > cold,
            "with the temperate land between"
        );
        assert!(
            mean(-band / 2 - band * 2) > cold + 0.45,
            "and the belts repeat"
        );
        // The coldest biomes sit in the cold belt or on the heights.
        for i in 0..4000 {
            let column = world.column(i * 53 - 100_000, (i * 29) % 600 - 300);
            if matches!(column.biome, Biome::SnowyPlains | Biome::SnowyTaiga) {
                assert!(column.temperature < -0.4);
            }
        }
    }

    #[test]
    fn a_column_is_solid_from_bedrock_to_its_top_then_water_to_the_sea() {
        let world = world(77);
        let (min_y, max_y) = (-64, 320);
        let sea = world.config().sea_level;
        let mut spans = Vec::new();
        for i in 0..3000 {
            let (x, z) = (i * 211 - 300_000, i * 173 - 250_000);
            let column = world.spans(x, z, min_y, max_y, &mut spans);
            assert_eq!(spans[0].0, min_y);
            assert_eq!(spans[0].2, Block::Bedrock);
            for pair in spans.windows(2) {
                assert_eq!(
                    pair[0].1, pair[1].0,
                    "no gap or overlap at {x} {z}: {spans:?}"
                );
            }
            assert!(spans.iter().all(|(low, high, _)| low < high));
            let solid_top = spans
                .iter()
                .rev()
                .find(|(_, _, block)| *block != Block::Water)
                .unwrap();
            assert_eq!(solid_top.1 - 1, column.height);
            let last = spans.last().unwrap();
            if column.height < sea {
                assert_eq!(*last, (column.height + 1, sea + 1, Block::Water));
            } else {
                assert_ne!(last.2, Block::Water);
            }
            // The same answer again: chunks may be asked for twice.
            let mut again = Vec::new();
            world.spans(x, z, min_y, max_y, &mut again);
            assert_eq!(spans, again);
        }
    }

    #[test]
    fn the_ground_matches_its_biome() {
        let world = world(404);
        let mut spans = Vec::new();
        let mut seen: BTreeMap<Biome, Block> = BTreeMap::new();
        for i in 0..60_000 {
            let (x, z) = (i * 131 - 4_000_000, (i * 577) % 30_000 - 15_000);
            let column = world.spans(x, z, -64, 320, &mut spans);
            let top = spans
                .iter()
                .rev()
                .find(|(_, _, block)| *block != Block::Water)
                .unwrap()
                .2;
            seen.insert(column.biome, top);
            match column.biome {
                Biome::Desert => assert_eq!(top, Block::Sand),
                Biome::Beach | Biome::SnowyBeach => assert_eq!(top, Block::Sand),
                Biome::StonyPeaks | Biome::JaggedPeaks => assert_eq!(top, Block::Stone),
                Biome::FrozenPeaks | Biome::SnowySlopes => assert_eq!(top, Block::SnowBlock),
                Biome::Plains | Biome::Forest if column.ruggedness <= 0.95 => {
                    assert_eq!(top, Block::GrassBlock)
                }
                _ => {}
            }
            if column.height < world.config().sea_level {
                assert_ne!(top, Block::GrassBlock, "no grass under water");
            }
        }
        assert!(seen.len() >= 25);
    }

    #[test]
    fn ids_are_vanillas_and_indexes_match_the_lists() {
        for (i, biome) in Biome::ALL.iter().enumerate() {
            assert_eq!(biome.index(), i);
            assert!(biome.id().starts_with("minecraft:"));
        }
        for (i, block) in Block::ALL.iter().enumerate() {
            assert_eq!(block.index(), i);
            assert!(block.id().starts_with("minecraft:"));
        }
    }

    #[test]
    fn water_is_exactly_where_a_column_is_under_the_sea() {
        let world = world(2026);
        let mut spans = Vec::new();
        let (mut wet, mut dry) = (0, 0);
        for i in 0..4000 {
            let (x, z) = (i * 37 - 70_000, i * 53 - 90_000);
            let column = world.spans(x, z, -64, 320, &mut spans);
            let has_water = spans.iter().any(|&(_, _, block)| block == Block::Water);
            assert_eq!(world.under_sea(&column), has_water, "at {x} {z}");
            if has_water { wet += 1 } else { dry += 1 }
        }
        assert!(wet > 400 && dry > 400, "{wet} wet, {dry} dry");
    }

    #[test]
    fn caves_stay_out_of_the_sea_and_structures_go_where_the_terrains_agree() {
        let config = Config::shipped();
        let dry = Sea::default();
        let all = Additions {
            carvers: true,
            structures: true,
        };
        assert_eq!(additions(&config, dry), all);
        // Water in the chunk or beside it: no caves.
        for sea in [
            Sea {
                wet: true,
                vanilla_ocean: true,
                ..dry
            },
            Sea {
                wet_beside: true,
                ..dry
            },
        ] {
            assert!(!additions(&config, sea).carvers, "{sea:?}");
        }
        // Structures where both have sea, or both land.
        let ocean = Sea {
            wet: true,
            vanilla_ocean: true,
            ..dry
        };
        assert!(additions(&config, ocean).structures);
        let village_at_sea = Sea { wet: true, ..dry };
        assert!(!additions(&config, village_at_sea).structures);
        let monument_on_land = Sea {
            vanilla_ocean: true,
            ..dry
        };
        assert!(!additions(&config, monument_on_land).structures);
        // A coast beside a dry chunk keeps its structures, not its caves.
        assert_eq!(
            additions(
                &config,
                Sea {
                    wet_beside: true,
                    ..dry
                }
            ),
            Additions {
                carvers: false,
                structures: true
            }
        );
        // Switched off, or turned off by the config.
        let unfitted = Config::parse("fit_to_sea = false").unwrap();
        assert_eq!(additions(&unfitted, village_at_sea), all);
        let none = Config::parse("carvers = false\nstructures = false").unwrap();
        assert_eq!(
            additions(&none, dry),
            Additions {
                carvers: false,
                structures: false
            }
        );
    }
}
