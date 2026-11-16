//! What happens to one column of ground when the biome over it changes.
//!
//! Only blocks nature puts there are touched: grass, dirt, sand, water, ice, snow,
//! small plants, and leaves that grew on a tree. A column topped with anything else
//! (a floor, a road, a roof) is left alone, so builds keep standing through a shift.

use crate::Features;
use crate::biomes::{Flora, Profile, Soil};

/// How many blocks under a column's height the plan reads.
pub const BELOW: usize = 3;
/// How many blocks from a column's height upward the plan reads: the plant on the
/// ground, and the crown of a tree over it.
pub const ABOVE: usize = 12;

/// One block to set, `dy` blocks from the column's height: -1 is the top block of the
/// ground, 0 the block over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    pub dy: i32,
    pub block: &'static str,
    /// Set the block only where this holds. The plan sees blocks without their
    /// properties, so what depends on one is left to the game to check.
    pub only: Option<Guard>,
}

/// A condition on the block an edit replaces, checked by the game when it is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// Still water, not a stream running over the ground.
    StillWater,
    /// A leaf block that grew on a tree, rather than one a player placed.
    GrownLeaf,
}

impl Guard {
    /// The condition as a block predicate, the way commands write one.
    pub fn predicate(self) -> &'static str {
        match self {
            Guard::StillWater => "minecraft:water[level=0]",
            Guard::GrownLeaf => "#minecraft:leaves[persistent=false]",
        }
    }
}

const AIR: &str = "minecraft:air";
const WATER: &str = "minecraft:water";
const ICE: &str = "minecraft:ice";
const SNOW: &str = "minecraft:snow";
const SAND: &str = "minecraft:sand";
const DIRT: &str = "minecraft:dirt";
const GRASS: &str = "minecraft:grass_block";
const SNOWY_GRASS: &str = "minecraft:grass_block[snowy=true]";

/// Soil that turns to sand when a desert arrives.
const EARTH: [&str; 5] = [
    "minecraft:grass_block",
    "minecraft:dirt",
    "minecraft:podzol",
    "minecraft:coarse_dirt",
    "minecraft:rooted_dirt",
];
/// Ground that snow settles on.
const SNOWABLE: [&str; 10] = [
    "minecraft:grass_block",
    "minecraft:dirt",
    "minecraft:podzol",
    "minecraft:coarse_dirt",
    "minecraft:sand",
    "minecraft:red_sand",
    "minecraft:gravel",
    "minecraft:stone",
    "minecraft:mud",
    "minecraft:snow_block",
];
/// Small plants a change of climate clears away. The second value says the plant is
/// two blocks tall.
const PLANTS: [(&str, bool); 24] = [
    ("minecraft:short_grass", false),
    ("minecraft:fern", false),
    ("minecraft:dead_bush", false),
    ("minecraft:short_dry_grass", false),
    ("minecraft:tall_dry_grass", false),
    ("minecraft:bush", false),
    ("minecraft:firefly_bush", false),
    ("minecraft:leaf_litter", false),
    ("minecraft:wildflowers", false),
    ("minecraft:dandelion", false),
    ("minecraft:poppy", false),
    ("minecraft:azure_bluet", false),
    ("minecraft:oxeye_daisy", false),
    ("minecraft:cornflower", false),
    ("minecraft:allium", false),
    ("minecraft:blue_orchid", false),
    ("minecraft:red_tulip", false),
    ("minecraft:orange_tulip", false),
    ("minecraft:white_tulip", false),
    ("minecraft:pink_tulip", false),
    ("minecraft:lily_of_the_valley", false),
    ("minecraft:cactus_flower", false),
    ("minecraft:tall_grass", true),
    ("minecraft:large_fern", true),
];
const CACTUS: &str = "minecraft:cactus";

/// A block state's block: `minecraft:grass_block[snowy=true]` is `minecraft:grass_block`.
pub fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

fn is_air(state: &str) -> bool {
    matches!(block_of(state), "minecraft:air" | "minecraft:cave_air")
}

fn plant(state: &str) -> Option<bool> {
    let block = block_of(state);
    PLANTS
        .iter()
        .find(|(name, _)| *name == block)
        .map(|(_, tall)| *tall)
}

fn is_leaf(state: &str) -> bool {
    block_of(state).ends_with("_leaves")
}

/// Draws out of a thousand, taken from one roll three decimal digits at a time: a
/// 64-bit roll holds six of them.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u32 {
        let value = (self.0 % 1000) as u32;
        self.0 /= 1000;
        value
    }
}

/// The plants a biome's flora scatters on its ground, each with its chance out of a
/// thousand columns.
fn scatter(flora: Flora) -> &'static [(&'static str, u32)] {
    match flora {
        Flora::Keep | Flora::Bare => &[],
        Flora::Meadow => &[
            ("minecraft:short_grass", 170),
            ("minecraft:dandelion", 8),
            ("minecraft:poppy", 6),
        ],
        Flora::Taiga => &[("minecraft:fern", 110), ("minecraft:short_grass", 50)],
        Flora::Jungle => &[("minecraft:short_grass", 250), ("minecraft:fern", 110)],
        Flora::Savanna => &[("minecraft:short_grass", 210)],
        Flora::Desert => &[("minecraft:dead_bush", 14), (CACTUS, 4)],
    }
}

/// How often a sapling takes root where a forest arrives, out of a thousand columns.
const SAPLING_CHANCE: u32 = 16;

/// What to change in a column whose biome went from `old` to `new`.
///
/// `states` are the column's blocks bottom up (with or without their properties:
/// only the block is read), `BELOW` of them under the
/// column's height (the first block above its ground or water, leaves aside) and
/// `ABOVE` from it upward. `roll` is a random number of the column's own. The edits
/// are written to `out`, at most one for each block.
pub fn plan(
    old: &Profile,
    new: &Profile,
    states: &[&str],
    roll: u64,
    features: &Features,
    out: &mut Vec<Edit>,
) {
    out.clear();
    if old == new || states.len() < BELOW + ABOVE {
        return;
    }
    let mut dice = Dice(roll);
    let at = |dy: i32| states[(BELOW as i32 + dy) as usize];
    let put = |out: &mut Vec<Edit>, dy: i32, block: &'static str, only: Option<Guard>| match out
        .iter_mut()
        .find(|edit| edit.dy == dy)
    {
        Some(edit) => (edit.block, edit.only) = (block, only),
        None => out.push(Edit { dy, block, only }),
    };
    let set = |out: &mut Vec<Edit>, dy: i32, block: &'static str| put(out, dy, block, None);
    // What the ground and the block over it are once the edits so far are made.
    let mut ground = block_of(at(-1));
    let mut over_is_air = is_air(at(0));

    // The soil: earth to sand where a desert arrives, sand to earth where it leaves.
    if features.ground && old.soil != new.soil {
        match (old.soil, new.soil) {
            (Soil::Grass, Soil::Sand) if EARTH.contains(&ground) => {
                set(out, -1, SAND);
                ground = SAND;
                for dy in [-2, -3] {
                    if EARTH.contains(&block_of(at(dy))) {
                        set(out, dy, SAND);
                    }
                }
            }
            (Soil::Sand, Soil::Grass) if ground == SAND && block_of(at(0)) != WATER => {
                set(out, -1, GRASS);
                ground = GRASS;
                if block_of(at(-2)) == SAND {
                    set(out, -2, DIRT);
                }
            }
            _ => {}
        }
    }

    // Small plants: the old climate's are cleared, the new one's scattered below.
    let flora_changes = features.plants && old.flora != new.flora && new.flora != Flora::Keep;
    let snow_arrives = features.ice_and_snow && new.frozen && !old.frozen;
    if flora_changes || snow_arrives {
        if let Some(tall) = plant(at(0)) {
            set(out, 0, AIR);
            over_is_air = true;
            if tall && plant(at(1)) == Some(true) {
                set(out, 1, AIR);
            }
        } else if flora_changes && block_of(at(0)) == CACTUS {
            // A cactus is cleared whole, bottom to top.
            for dy in 0..ABOVE as i32 {
                if block_of(at(dy)) != CACTUS {
                    break;
                }
                set(out, dy, AIR);
            }
            over_is_air = true;
        }
    }

    // Ice and snow.
    if features.ice_and_snow && old.frozen != new.frozen {
        if new.frozen {
            if ground == WATER && over_is_air {
                put(out, -1, ICE, Some(Guard::StillWater));
                ground = ICE;
            } else if SNOWABLE.contains(&ground) && over_is_air {
                set(out, 0, SNOW);
                over_is_air = false;
                if ground == GRASS {
                    set(out, -1, SNOWY_GRASS);
                }
            }
        } else if ground == ICE {
            set(out, -1, WATER);
            ground = WATER;
        } else if block_of(at(0)) == SNOW {
            set(out, 0, AIR);
            over_is_air = true;
            if ground == GRASS {
                set(out, -1, GRASS);
            }
        } else if ground == SNOW {
            // A deep layer of snow counts as ground: the soil is under it.
            set(out, -1, AIR);
            if block_of(at(-2)) == GRASS {
                set(out, -2, GRASS);
            }
            ground = AIR;
        }
    }

    // New growth, on bare ground of the right kind.
    if features.plants && over_is_air {
        // Desert plants root in sand, the others in grass.
        let fits = if new.flora == Flora::Desert {
            ground == SAND
        } else {
            ground == GRASS
        };
        let sapling = new
            .sapling
            .filter(|sapling| old.sapling != Some(*sapling) && ground == GRASS);
        if let Some(sapling) = sapling.filter(|_| dice.next() < SAPLING_CHANCE) {
            set(out, 0, sapling);
        } else if flora_changes {
            let draw = dice.next();
            let mut threshold = 0;
            for (block, chance) in scatter(new.flora) {
                threshold += chance;
                if draw < threshold {
                    if fits {
                        set(out, 0, block);
                    }
                    break;
                }
            }
        }
    }

    // Trees left standing in a desert lose their leaves.
    if features.leaf_fall && new.barren && !old.barren {
        for dy in 0..ABOVE as i32 {
            if is_leaf(at(dy)) {
                put(out, dy, AIR, Some(Guard::GrownLeaf));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biomes::profile;

    const ALL: Features = Features {
        ground: true,
        ice_and_snow: true,
        plants: true,
        leaf_fall: true,
    };

    /// A column: `ground` are its blocks under the height, top last; `over` the ones
    /// from the height upward. The rest is stone below and air above.
    fn column(ground: &[&'static str], over: &[&'static str]) -> Vec<&'static str> {
        let mut states = vec!["minecraft:stone"; BELOW - ground.len()];
        states.extend_from_slice(ground);
        states.extend_from_slice(over);
        states.resize(BELOW + ABOVE, AIR);
        states
    }

    fn run(old: &str, new: &str, states: &[&'static str], roll: u64) -> Vec<Edit> {
        let mut out = Vec::new();
        plan(&profile(old), &profile(new), states, roll, &ALL, &mut out);
        out.sort_by_key(|edit| edit.dy);
        out
    }

    fn edit(dy: i32, block: &'static str) -> Edit {
        Edit {
            dy,
            block,
            only: None,
        }
    }

    fn guarded(dy: i32, block: &'static str, only: Guard) -> Edit {
        Edit {
            dy,
            block,
            only: Some(only),
        }
    }

    /// A roll that grows nothing: every draw of a thousand is 999.
    const BARREN: u64 = 999_999_999_999_999_999;

    #[test]
    fn a_desert_arriving_turns_earth_to_sand_and_clears_the_grass() {
        let states = column(&[DIRT, DIRT, GRASS], &["minecraft:short_grass"]);
        let edits = run("minecraft:savanna", "minecraft:desert", &states, BARREN);
        assert_eq!(
            edits,
            [edit(-3, SAND), edit(-2, SAND), edit(-1, SAND), edit(0, AIR)]
        );
    }

    #[test]
    fn a_desert_leaving_greens_the_sand() {
        let states = column(
            &["minecraft:sandstone", SAND, SAND],
            &["minecraft:dead_bush"],
        );
        let edits = run("minecraft:desert", "minecraft:savanna", &states, BARREN);
        assert_eq!(edits, [edit(-2, DIRT), edit(-1, GRASS), edit(0, AIR)]);
    }

    #[test]
    fn winter_ices_water_and_snows_on_the_ground() {
        let lake = column(&[WATER, WATER, WATER], &[]);
        assert_eq!(
            run("minecraft:plains", "minecraft:snowy_plains", &lake, BARREN),
            [guarded(-1, ICE, Guard::StillWater)],
            "the game freezes it only if it is still water"
        );
        let field = column(&[DIRT, DIRT, GRASS], &[]);
        assert_eq!(
            run("minecraft:plains", "minecraft:snowy_plains", &field, BARREN),
            [edit(-1, SNOWY_GRASS), edit(0, SNOW)]
        );
        // Grass in the way is snowed under.
        let meadow = column(
            &[DIRT, DIRT, GRASS],
            &["minecraft:tall_grass", "minecraft:tall_grass"],
        );
        assert_eq!(
            run(
                "minecraft:plains",
                "minecraft:snowy_plains",
                &meadow,
                BARREN
            ),
            [edit(-1, SNOWY_GRASS), edit(0, SNOW), edit(1, AIR)]
        );
    }

    #[test]
    fn spring_thaws_it_again() {
        let lake = column(&[WATER, WATER, ICE], &[]);
        assert_eq!(
            run("minecraft:snowy_plains", "minecraft:plains", &lake, BARREN),
            [edit(-1, WATER)]
        );
        let field = column(&[DIRT, DIRT, SNOWY_GRASS], &[SNOW]);
        assert_eq!(
            run("minecraft:snowy_plains", "minecraft:plains", &field, BARREN),
            [edit(-1, GRASS), edit(0, AIR)]
        );
        // Packed and blue ice are not the weather's: they stay.
        let glacier = column(&[WATER, WATER, "minecraft:packed_ice"], &[]);
        assert_eq!(
            run(
                "minecraft:snowy_plains",
                "minecraft:plains",
                &glacier,
                BARREN
            ),
            []
        );
    }

    #[test]
    fn a_roofed_pond_is_not_frozen() {
        let roofed = column(&[WATER, WATER, WATER], &["minecraft:oak_planks"]);
        assert_eq!(
            run(
                "minecraft:plains",
                "minecraft:snowy_plains",
                &roofed,
                BARREN
            ),
            []
        );
    }

    #[test]
    fn builds_are_left_alone() {
        for top in [
            "minecraft:cobblestone",
            "minecraft:oak_planks",
            "minecraft:farmland",
            "minecraft:dirt_path",
        ] {
            let floor = column(&[DIRT, DIRT, top], &[]);
            assert_eq!(
                run("minecraft:plains", "minecraft:desert", &floor, 0),
                [],
                "{top}"
            );
            assert_eq!(
                run("minecraft:plains", "minecraft:snowy_plains", &floor, 0),
                [],
                "{top}"
            );
        }
    }

    #[test]
    fn guards_are_block_predicates() {
        assert_eq!(Guard::StillWater.predicate(), "minecraft:water[level=0]");
        assert_eq!(
            Guard::GrownLeaf.predicate(),
            "#minecraft:leaves[persistent=false]"
        );
    }

    #[test]
    fn leaves_fall_from_trees_a_desert_overtakes() {
        let leaf = "minecraft:acacia_leaves";
        let tree = column(&[DIRT, DIRT, GRASS], &[AIR, AIR, leaf, leaf, AIR, leaf]);
        let edits = run("minecraft:savanna", "minecraft:desert", &tree, BARREN);
        // Only leaves that grew there: the game checks that a player didn't place them.
        for dy in [2, 3, 5] {
            assert!(
                edits.contains(&guarded(dy, AIR, Guard::GrownLeaf)),
                "{edits:?}"
            );
        }
        // Leaves stay where trees still live.
        let edits = run("minecraft:plains", "minecraft:savanna", &tree, BARREN);
        assert!(edits.iter().all(|e| e.dy < 2));
    }

    #[test]
    fn a_cactus_is_cleared_whole() {
        let states = column(&[SAND, SAND, SAND], &[CACTUS, CACTUS, CACTUS]);
        let edits = run("minecraft:desert", "minecraft:savanna", &states, BARREN);
        assert_eq!(
            edits,
            [
                edit(-2, DIRT),
                edit(-1, GRASS),
                edit(0, AIR),
                edit(1, AIR),
                edit(2, AIR)
            ]
        );
    }

    #[test]
    fn new_plants_grow_at_about_their_chances_and_only_on_their_soil() {
        let field = column(&[DIRT, DIRT, GRASS], &[]);
        let (mut grass, mut saplings, mut other) = (0, 0, 0);
        for roll in 0..20_000u64 {
            let roll = crate::noise::hash(7, roll as i64, 3);
            for e in run("minecraft:snowy_taiga", "minecraft:forest", &field, roll) {
                match (e.dy, e.block) {
                    (0, "minecraft:short_grass") => grass += 1,
                    (0, "minecraft:oak_sapling") => saplings += 1,
                    (0, "minecraft:dandelion" | "minecraft:poppy") => {}
                    _ => other += 1,
                }
            }
        }
        assert_eq!(other, 0);
        assert!(
            (2800..3900).contains(&grass),
            "{grass} of 20000 columns grew grass"
        );
        assert!(
            (200..460).contains(&saplings),
            "{saplings} of 20000 columns took a sapling"
        );
        // Nothing grows on a stone floor, whatever the roll.
        let floor = column(&[DIRT, DIRT, "minecraft:stone_bricks"], &[]);
        for roll in 0..2000 {
            assert_eq!(
                run("minecraft:snowy_taiga", "minecraft:forest", &floor, roll),
                []
            );
        }
    }

    #[test]
    fn desert_plants_take_to_new_sand() {
        let field = column(&[DIRT, DIRT, GRASS], &[]);
        let mut bushes = 0;
        for roll in 0..20_000u64 {
            let roll = crate::noise::hash(11, roll as i64, 5);
            let edits = run("minecraft:savanna", "minecraft:desert", &field, roll);
            assert!(edits.contains(&edit(-1, SAND)));
            if edits.contains(&edit(0, "minecraft:dead_bush")) {
                bushes += 1;
            }
        }
        assert!(
            (150..420).contains(&bushes),
            "{bushes} dead bushes on 20000 columns"
        );
    }

    #[test]
    fn nothing_changes_when_the_biome_does_not_or_a_feature_is_off() {
        let states = column(&[DIRT, DIRT, GRASS], &["minecraft:short_grass"]);
        assert_eq!(run("minecraft:plains", "minecraft:plains", &states, 0), []);
        assert_eq!(
            run("minecraft:plains", "minecraft:sunflower_plains", &states, 0),
            []
        );
        let off = Features {
            ground: false,
            ice_and_snow: false,
            plants: false,
            leaf_fall: false,
        };
        let mut out = Vec::new();
        plan(
            &profile("minecraft:plains"),
            &profile("minecraft:desert"),
            &states,
            0,
            &off,
            &mut out,
        );
        assert_eq!(out, []);
    }

    #[test]
    fn converting_there_and_back_restores_the_ground() {
        // Plains to snowy plains and back leaves a field as it was.
        let mut states = column(&[DIRT, DIRT, GRASS], &[]);
        for (old, new) in [
            ("minecraft:plains", "minecraft:snowy_plains"),
            ("minecraft:snowy_plains", "minecraft:plains"),
        ] {
            for e in run(old, new, &states, BARREN) {
                states[(BELOW as i32 + e.dy) as usize] = block_of(e.block);
            }
        }
        assert_eq!(states, column(&[DIRT, DIRT, GRASS], &[]));
    }
}
