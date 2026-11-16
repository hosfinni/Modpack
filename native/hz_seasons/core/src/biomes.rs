//! Which biome a biome becomes as the climate warms or cools, and what each biome's
//! ground and plants are.
//!
//! Biomes sit on ladders that run from cold to hot. A shift moves a biome some
//! steps along its ladder; a biome on no ladder (badlands, mushroom fields, caves,
//! the Nether and the End) never changes.

const NAMESPACE: &str = "minecraft:";

/// The ladders, cold to hot. A shifted biome becomes the entry it lands on.
const LADDERS: [&[&str]; 9] = [
    &["snowy_plains", "plains", "savanna", "desert"],
    &["snowy_taiga", "taiga", "forest", "jungle"],
    &["frozen_peaks", "jagged_peaks", "stony_peaks"],
    &["snowy_slopes", "grove", "meadow", "savanna_plateau"],
    &["swamp", "mangrove_swamp"],
    &["snowy_beach", "beach"],
    &["frozen_river", "river"],
    &[
        "frozen_ocean",
        "cold_ocean",
        "ocean",
        "lukewarm_ocean",
        "warm_ocean",
    ],
    &[
        "deep_frozen_ocean",
        "deep_cold_ocean",
        "deep_ocean",
        "deep_lukewarm_ocean",
    ],
];

/// Biomes that stand on a ladder's rung without being its entry: they shift like
/// the rung's biome, and stay themselves while the climate is as generated.
const VARIANTS: [(&str, usize, usize); 14] = [
    ("ice_spikes", 0, 0),
    ("sunflower_plains", 0, 1),
    ("windswept_savanna", 0, 2),
    ("old_growth_pine_taiga", 1, 1),
    ("old_growth_spruce_taiga", 1, 1),
    ("birch_forest", 1, 2),
    ("old_growth_birch_forest", 1, 2),
    ("dark_forest", 1, 2),
    ("flower_forest", 1, 2),
    ("cherry_grove", 1, 2),
    ("windswept_forest", 1, 2),
    ("sparse_jungle", 1, 3),
    ("bamboo_jungle", 1, 3),
    ("windswept_hills", 3, 2),
];

fn rung(name: &str) -> Option<(usize, usize)> {
    for (ladder, rungs) in LADDERS.iter().enumerate() {
        if let Some(index) = rungs.iter().position(|rung| *rung == name) {
            return Some((ladder, index));
        }
    }
    VARIANTS
        .iter()
        .find(|(variant, _, _)| *variant == name)
        .map(|(_, ladder, index)| (*ladder, *index))
}

/// The biome `biome` (`minecraft:plains`) becomes `steps` rungs up its ladder
/// (hotter), or down it (colder) when negative. A biome at the end of its ladder
/// stays there; a biome on no ladder, or another mod's, is returned as it is.
pub fn shifted(biome: &str, steps: i32) -> &str {
    if steps == 0 {
        return biome;
    }
    let Some(name) = biome.strip_prefix(NAMESPACE) else {
        return biome;
    };
    let Some((ladder, index)) = rung(name) else {
        return biome;
    };
    let rungs = LADDERS[ladder];
    let landed = (index as i32 + steps).clamp(0, rungs.len() as i32 - 1) as usize;
    if landed == index {
        return biome;
    }
    FULL_IDS[ladder][landed]
}

/// The ladders' entries with their namespace, so that `shifted` can hand out
/// `'static` ids.
const FULL_IDS: [&[&str]; 9] = [
    &[
        "minecraft:snowy_plains",
        "minecraft:plains",
        "minecraft:savanna",
        "minecraft:desert",
    ],
    &[
        "minecraft:snowy_taiga",
        "minecraft:taiga",
        "minecraft:forest",
        "minecraft:jungle",
    ],
    &[
        "minecraft:frozen_peaks",
        "minecraft:jagged_peaks",
        "minecraft:stony_peaks",
    ],
    &[
        "minecraft:snowy_slopes",
        "minecraft:grove",
        "minecraft:meadow",
        "minecraft:savanna_plateau",
    ],
    &["minecraft:swamp", "minecraft:mangrove_swamp"],
    &["minecraft:snowy_beach", "minecraft:beach"],
    &["minecraft:frozen_river", "minecraft:river"],
    &[
        "minecraft:frozen_ocean",
        "minecraft:cold_ocean",
        "minecraft:ocean",
        "minecraft:lukewarm_ocean",
        "minecraft:warm_ocean",
    ],
    &[
        "minecraft:deep_frozen_ocean",
        "minecraft:deep_cold_ocean",
        "minecraft:deep_ocean",
        "minecraft:deep_lukewarm_ocean",
    ],
];

/// What a biome's soil is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Soil {
    Grass,
    Sand,
    /// Whatever is there: this biome's ground is not its climate's doing (a beach, a
    /// sea floor, a peak).
    Keep,
}

/// What small plants grow in a biome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flora {
    /// Whatever is there.
    Keep,
    /// Nothing: bare or snowed in.
    Bare,
    Meadow,
    Taiga,
    Jungle,
    Savanna,
    Desert,
}

/// What follows a biome onto the land it moves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub soil: Soil,
    /// Water is iced over and the ground snowed on.
    pub frozen: bool,
    pub flora: Flora,
    /// The sapling that takes root where this biome arrives.
    pub sapling: Option<&'static str>,
    /// Trees don't live here: their leaves fall.
    pub barren: bool,
}

impl Profile {
    const KEEP: Self = Self {
        soil: Soil::Keep,
        frozen: false,
        flora: Flora::Keep,
        sapling: None,
        barren: false,
    };
}

/// The profile of `biome` (`minecraft:plains`), from its id: vanilla names its
/// biomes after what grows there, and a data pack's own usually follow.
pub fn profile(biome: &str) -> Profile {
    let name = biome.rsplit(':').next().unwrap_or(biome);
    let has = |word: &str| name.contains(word);
    let frozen = has("snowy") || has("frozen") || has("ice_spikes") || name == "grove";
    if has("badlands") || has("mushroom") || has("cave") || has("deep_dark") {
        return Profile::KEEP;
    }
    if has("ocean")
        || has("river")
        || has("beach")
        || has("shore")
        || has("peaks")
        || has("slopes")
        || name == "grove"
        || has("swamp")
    {
        return Profile {
            frozen,
            ..Profile::KEEP
        };
    }
    if has("desert") {
        return Profile {
            soil: Soil::Sand,
            frozen: false,
            flora: Flora::Desert,
            sapling: None,
            barren: true,
        };
    }
    let grass = |flora, sapling| Profile {
        soil: Soil::Grass,
        frozen,
        flora,
        sapling,
        barren: false,
    };
    if has("taiga") {
        let flora = if frozen { Flora::Bare } else { Flora::Taiga };
        grass(flora, Some("minecraft:spruce_sapling"))
    } else if has("jungle") {
        grass(Flora::Jungle, Some("minecraft:jungle_sapling"))
    } else if has("savanna") {
        grass(Flora::Savanna, Some("minecraft:acacia_sapling"))
    } else if has("birch") {
        grass(Flora::Meadow, Some("minecraft:birch_sapling"))
    } else if has("cherry") {
        grass(Flora::Meadow, Some("minecraft:cherry_sapling"))
    } else if has("forest") {
        grass(Flora::Meadow, Some("minecraft:oak_sapling"))
    } else if has("plains") || has("meadow") || has("hills") || name == "ice_spikes" {
        let flora = if frozen { Flora::Bare } else { Flora::Meadow };
        grass(flora, None)
    } else {
        Profile::KEEP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biomes_climb_and_descend_their_ladders() {
        assert_eq!(shifted("minecraft:plains", 1), "minecraft:savanna");
        assert_eq!(shifted("minecraft:plains", 2), "minecraft:desert");
        assert_eq!(shifted("minecraft:plains", -1), "minecraft:snowy_plains");
        assert_eq!(shifted("minecraft:taiga", 1), "minecraft:forest");
        assert_eq!(shifted("minecraft:forest", -2), "minecraft:snowy_taiga");
        assert_eq!(shifted("minecraft:ocean", -2), "minecraft:frozen_ocean");
        assert_eq!(shifted("minecraft:river", -1), "minecraft:frozen_river");
        assert_eq!(shifted("minecraft:beach", -1), "minecraft:snowy_beach");
    }

    #[test]
    fn a_ladder_has_ends() {
        assert_eq!(shifted("minecraft:desert", 2), "minecraft:desert");
        assert_eq!(
            shifted("minecraft:snowy_plains", -5),
            "minecraft:snowy_plains"
        );
        assert_eq!(shifted("minecraft:plains", 9), "minecraft:desert");
        assert_eq!(shifted("minecraft:beach", 3), "minecraft:beach");
    }

    #[test]
    fn variants_stay_themselves_until_they_move() {
        assert_eq!(
            shifted("minecraft:cherry_grove", 0),
            "minecraft:cherry_grove"
        );
        assert_eq!(shifted("minecraft:cherry_grove", 1), "minecraft:jungle");
        assert_eq!(shifted("minecraft:cherry_grove", -1), "minecraft:taiga");
        assert_eq!(
            shifted("minecraft:bamboo_jungle", 1),
            "minecraft:bamboo_jungle",
            "already hottest"
        );
        assert_eq!(shifted("minecraft:ice_spikes", 1), "minecraft:plains");
    }

    #[test]
    fn what_is_on_no_ladder_never_changes() {
        for biome in [
            "minecraft:badlands",
            "minecraft:mushroom_fields",
            "minecraft:lush_caves",
            "minecraft:deep_dark",
            "minecraft:nether_wastes",
            "minecraft:the_end",
            "minecraft:stony_shore",
            "othermod:crystal_fields",
        ] {
            assert_eq!(shifted(biome, 2), biome);
            assert_eq!(shifted(biome, -2), biome);
        }
    }

    #[test]
    fn every_rung_is_spelled_the_same_with_and_without_its_namespace() {
        for (short, full) in LADDERS.iter().zip(FULL_IDS) {
            assert_eq!(short.len(), full.len());
            for (name, id) in short.iter().zip(full.iter()) {
                assert_eq!(format!("minecraft:{name}"), *id);
            }
        }
        for (_, ladder, index) in VARIANTS {
            assert!(index < LADDERS[ladder].len());
        }
    }

    #[test]
    fn profiles_follow_the_names() {
        let desert = profile("minecraft:desert");
        assert_eq!(
            (desert.soil, desert.flora, desert.barren),
            (Soil::Sand, Flora::Desert, true)
        );
        let plains = profile("minecraft:plains");
        assert_eq!(
            (plains.soil, plains.frozen, plains.flora),
            (Soil::Grass, false, Flora::Meadow)
        );
        let snowy = profile("minecraft:snowy_plains");
        assert_eq!(
            (snowy.soil, snowy.frozen, snowy.flora),
            (Soil::Grass, true, Flora::Bare)
        );
        assert_eq!(
            profile("minecraft:forest").sapling,
            Some("minecraft:oak_sapling")
        );
        assert_eq!(
            profile("minecraft:snowy_taiga").sapling,
            Some("minecraft:spruce_sapling")
        );
        assert!(profile("minecraft:frozen_ocean").frozen);
        assert_eq!(profile("minecraft:frozen_ocean").soil, Soil::Keep);
        assert!(!profile("minecraft:ocean").frozen);
        assert_eq!(profile("minecraft:badlands"), Profile::KEEP);
        assert_eq!(profile("othermod:crystal_fields"), Profile::KEEP);
    }

    #[test]
    fn every_rung_has_a_profile_that_differs_from_its_neighbours_where_it_should() {
        // Moving along the plains ladder changes something each step.
        let ladder = [
            "minecraft:snowy_plains",
            "minecraft:plains",
            "minecraft:savanna",
            "minecraft:desert",
        ];
        for pair in ladder.windows(2) {
            assert_ne!(
                profile(pair[0]),
                profile(pair[1]),
                "{} and {}",
                pair[0],
                pair[1]
            );
        }
    }
}
