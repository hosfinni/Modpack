//! A harder world that rewards company: tougher monsters, strength in numbers, ore
//! that has to be refined, and monsters that fight as a pack.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MobsConfig {
    pub enabled: bool,
    /// The monsters made tougher and smarter, by entity id.
    pub monsters: Vec<String>,
    /// How much more health they have, in effect: a player's blows on them are
    /// divided by this.
    pub toughness: f32,
    /// Multiplies the damage they deal to players.
    pub damage: f32,
    /// A monster without a target joins one of its kind this near that has one.
    pub pack_range: f64,
    /// Melee monsters with company circle round to the side of their target.
    pub flank: bool,
    /// Under this share of its health, a monster with no allies near backs off.
    pub retreat_below: f32,
    /// The monsters that never back off.
    pub fearless: Vec<String>,
}

impl Default for MobsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            monsters: Vec::new(),
            toughness: 1.4,
            damage: 1.25,
            pack_range: 20.0,
            flank: true,
            retreat_below: 0.3,
            fearless: Vec::new(),
        }
    }
}

impl MobsConfig {
    pub fn check(&self) -> Result<(), String> {
        for (name, value, low, high) in [
            ("toughness", self.toughness, 0.1, 20.0),
            ("damage", self.damage, 0.0, 20.0),
            ("retreat_below", self.retreat_below, 0.0, 1.0),
            ("pack_range", self.pack_range as f32, 0.0, 64.0),
        ] {
            if !(value >= low && value <= high) {
                return Err(format!(
                    "mobs.{name} must be between {low} and {high}, not {value}"
                ));
            }
        }
        Ok(())
    }

    pub fn is_monster(&self, kind: &str) -> bool {
        self.enabled && self.monsters.iter().any(|monster| monster == kind)
    }

    /// Whether a monster of `kind` at `health` of `max_health`, with `allies` of its
    /// kind near, backs off from its target.
    pub fn retreats(&self, kind: &str, health: f32, max_health: f32, allies: usize) -> bool {
        self.is_monster(kind)
            && allies == 0
            && max_health > 0.0
            && health / max_health < self.retreat_below
            && !self.fearless.iter().any(|fearless| fearless == kind)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PartyConfig {
    pub enabled: bool,
    /// Players this near one another fight as a party, in blocks.
    pub range: f64,
    /// How much harder a player hits for each party member near.
    pub damage_per_member: f32,
    /// How much less a player is hurt for each party member near.
    pub protection_per_member: f32,
    /// The most members that count.
    pub members: u32,
}

impl Default for PartyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            range: 16.0,
            damage_per_member: 0.1,
            protection_per_member: 0.08,
            members: 3,
        }
    }
}

impl PartyConfig {
    pub fn check(&self) -> Result<(), String> {
        for (name, value) in [
            ("damage_per_member", self.damage_per_member),
            ("protection_per_member", self.protection_per_member),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("party.{name} must be between 0 and 1, not {value}"));
            }
        }
        if self.protection_per_member * self.members as f32 >= 1.0 {
            return Err("party: protection_per_member times members must stay under 1".into());
        }
        Ok(())
    }

    fn counted(&self, near: u32) -> f32 {
        if self.enabled {
            near.min(self.members) as f32
        } else {
            0.0
        }
    }

    /// What a player's blows are multiplied by with `near` party members beside them.
    pub fn damage(&self, near: u32) -> f32 {
        1.0 + self.damage_per_member * self.counted(near)
    }

    /// What the damage a player takes is multiplied by with `near` party members
    /// beside them.
    pub fn taken(&self, near: u32) -> f32 {
        1.0 - self.protection_per_member * self.counted(near)
    }
}

/// How many of `others` are within `range` of `at` in the same dimension: a
/// player's party, themselves left out by the caller.
pub fn near<'a>(
    at: (&str, [f64; 3]),
    others: impl Iterator<Item = (&'a str, [f64; 3])>,
    range: f64,
) -> u32 {
    others
        .filter(|(dimension, position)| {
            *dimension == at.0
                && position
                    .iter()
                    .zip(at.1)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f64>()
                    <= range * range
        })
        .count() as u32
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RefiningConfig {
    pub enabled: bool,
    /// What smelting raw ore or an ore block gives instead of an ingot: this many
    /// nuggets. An ingot is nine. 0 leaves vanilla's recipes alone.
    pub crude_nuggets: u32,
    /// The chance that washing uses up a level of the cauldron's water.
    pub water_use: f32,
}

impl Default for RefiningConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            crude_nuggets: 3,
            water_use: 0.25,
        }
    }
}

impl RefiningConfig {
    pub fn check(&self) -> Result<(), String> {
        if self.crude_nuggets > 9 {
            return Err("refining.crude_nuggets must be 0 to 9: an ingot is nine nuggets".into());
        }
        if !(0.0..=1.0).contains(&self.water_use) {
            return Err("refining.water_use is a chance, 0 to 1".into());
        }
        Ok(())
    }

    /// The vanilla recipes that turn ore straight into ingots, rewritten to give
    /// nuggets: each as its data pack key (`recipe/minecraft:<name>`) and its JSON.
    /// Empty when crude smelting is left as vanilla's.
    pub fn crude_recipes(&self) -> Vec<(String, String)> {
        if !self.enabled || self.crude_nuggets == 0 {
            return Vec::new();
        }
        let mut recipes = Vec::new();
        for metal in METALS {
            for (kind, time) in [("smelting", 200), ("blasting", 100)] {
                for source in metal.sources {
                    let name = format!("{}_ingot_from_{kind}_{source}", metal.name);
                    let json = format!(
                        "{{\"type\":\"minecraft:{kind}\",\"category\":\"misc\",\"group\":\"{}_ingot\",\
                         \"ingredient\":\"minecraft:{source}\",\"result\":{{\"id\":\"{}\",\"count\":{}}},\
                         \"experience\":0.1,\"cookingtime\":{time}}}",
                        metal.name, metal.nugget, self.crude_nuggets
                    );
                    recipes.push((format!("recipe/minecraft:{name}"), json));
                }
            }
        }
        recipes
    }
}

struct Metal {
    name: &'static str,
    nugget: &'static str,
    /// The items vanilla smelts into this metal's ingot.
    sources: &'static [&'static str],
}

const METALS: [Metal; 3] = [
    Metal {
        name: "iron",
        nugget: "minecraft:iron_nugget",
        sources: &["raw_iron", "iron_ore", "deepslate_iron_ore"],
    },
    Metal {
        name: "gold",
        nugget: "minecraft:gold_nugget",
        sources: &[
            "raw_gold",
            "gold_ore",
            "deepslate_gold_ore",
            "nether_gold_ore",
        ],
    },
    Metal {
        name: "copper",
        nugget: "minecraft:copper_nugget",
        sources: &["raw_copper", "copper_ore", "deepslate_copper_ore"],
    },
];

/// What washing a crushed ore in a water cauldron gives, when `item` is one.
pub fn washed(item: &str) -> Option<&'static str> {
    match item {
        "hz_content:crushed_iron" => Some("hz_content:washed_iron"),
        "hz_content:crushed_copper" => Some("hz_content:washed_copper"),
        "hz_content:crushed_gold" => Some("hz_content:washed_gold"),
        _ => None,
    }
}

/// The ingots that count as smelted for Smithing experience.
pub fn is_ingot(item: &str) -> bool {
    matches!(
        item,
        "minecraft:iron_ingot"
            | "minecraft:gold_ingot"
            | "minecraft:copper_ingot"
            | "minecraft:netherite_scrap"
    )
}

/// Where a monster at `me` goes to come at `target` from the side: a quarter turn
/// round it (`left` or right), `radius` blocks out.
pub fn flank_point(me: [f64; 3], target: [f64; 3], left: bool, radius: f64) -> [f64; 3] {
    let (dx, dz) = (me[0] - target[0], me[2] - target[2]);
    let length = (dx * dx + dz * dz).sqrt().max(1e-6);
    let (ux, uz) = (dx / length, dz / length);
    // A quarter turn of the line from the target to the monster.
    let (sx, sz) = if left { (-uz, ux) } else { (uz, -ux) };
    [target[0] + sx * radius, target[1], target[2] + sz * radius]
}

/// Where a monster at `me` goes to get `distance` blocks further from `threat`.
pub fn flee_point(me: [f64; 3], threat: [f64; 3], distance: f64) -> [f64; 3] {
    let (dx, dz) = (me[0] - threat[0], me[2] - threat[2]);
    let length = (dx * dx + dz * dz).sqrt().max(1e-6);
    [
        me[0] + dx / length * distance,
        me[1],
        me[2] + dz / length * distance,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[test]
    fn monsters_are_tougher_and_hit_harder() {
        let mobs = Config::shipped().mobs;
        assert!(mobs.toughness > 1.0 && mobs.damage > 1.0);
        assert!(mobs.is_monster("minecraft:zombie") && mobs.is_monster("minecraft:skeleton"));
        assert!(!mobs.is_monster("minecraft:cow") && !mobs.is_monster("minecraft:player"));
        let off = MobsConfig {
            enabled: false,
            ..mobs
        };
        assert!(!off.is_monster("minecraft:zombie"));
    }

    #[test]
    fn a_wounded_monster_alone_backs_off_and_one_in_company_fights_on() {
        let mobs = Config::shipped().mobs;
        assert!(mobs.retreats("minecraft:zombie", 5.0, 20.0, 0));
        assert!(
            !mobs.retreats("minecraft:zombie", 5.0, 20.0, 2),
            "it has company"
        );
        assert!(
            !mobs.retreats("minecraft:zombie", 15.0, 20.0, 0),
            "it is not hurt enough"
        );
        assert!(
            !mobs.retreats("minecraft:creeper", 2.0, 20.0, 0),
            "a creeper never does"
        );
        assert!(
            !mobs.retreats("minecraft:cow", 1.0, 10.0, 0),
            "nor anything that is no monster"
        );
    }

    #[test]
    fn a_party_hits_harder_and_is_hurt_less() {
        let party = Config::shipped().party;
        assert_eq!(party.damage(0), 1.0);
        assert_eq!(party.taken(0), 1.0);
        assert!((party.damage(2) - 1.2).abs() < 1e-6);
        assert!((party.taken(2) - 0.84).abs() < 1e-6);
        assert_eq!(
            party.damage(3),
            party.damage(8),
            "only so many members count"
        );
        let off = PartyConfig {
            enabled: false,
            ..party
        };
        assert_eq!((off.damage(3), off.taken(3)), (1.0, 1.0));
    }

    #[test]
    fn a_party_outlasts_the_tougher_monsters() {
        // Alone, a player needs 1.4 times the blows; with three friends near, each of
        // them needs barely more than in vanilla, and there are four of them.
        let config = Config::shipped();
        let alone = config.party.damage(0) / config.mobs.toughness;
        let together = config.party.damage(3) / config.mobs.toughness;
        assert!(alone < 0.75);
        assert!(together > 0.9);
    }

    #[test]
    fn party_members_are_counted_by_distance_and_dimension() {
        let me = ("minecraft:overworld", [0.0, 64.0, 0.0]);
        let others = [
            ("minecraft:overworld", [10.0, 64.0, 0.0]),
            ("minecraft:overworld", [0.0, 64.0, 17.0]),
            ("minecraft:the_nether", [1.0, 64.0, 1.0]),
            ("minecraft:overworld", [8.0, 70.0, 8.0]),
        ];
        assert_eq!(near(me, others.iter().copied(), 16.0), 2);
        assert_eq!(near(me, std::iter::empty(), 16.0), 0);
    }

    #[test]
    fn crude_smelting_gives_nuggets_for_every_vanilla_ore_recipe() {
        let refining = Config::shipped().refining;
        let recipes = refining.crude_recipes();
        // Iron and copper have three sources, gold four; each smelts and blasts.
        assert_eq!(recipes.len(), (3 + 4 + 3) * 2);
        let (key, json) = &recipes[0];
        assert_eq!(key, "recipe/minecraft:iron_ingot_from_smelting_raw_iron");
        assert!(json.contains("\"id\":\"minecraft:iron_nugget\",\"count\":3"));
        assert!(json.contains("\"ingredient\":\"minecraft:raw_iron\""));
        assert!(
            recipes
                .iter()
                .any(|(key, _)| key == "recipe/minecraft:gold_ingot_from_blasting_nether_gold_ore")
        );
        // Every recipe is JSON a parser takes: balanced, no stray whitespace runs.
        for (_, json) in &recipes {
            assert_eq!(json.matches('{').count(), json.matches('}').count());
            assert!(!json.contains("  "));
        }
        let vanilla = RefiningConfig {
            crude_nuggets: 0,
            ..refining
        };
        assert!(vanilla.crude_recipes().is_empty());
    }

    #[test]
    fn only_crushed_ore_washes() {
        assert_eq!(
            washed("hz_content:crushed_iron"),
            Some("hz_content:washed_iron")
        );
        assert_eq!(
            washed("hz_content:crushed_gold"),
            Some("hz_content:washed_gold")
        );
        assert_eq!(washed("minecraft:raw_iron"), None);
        assert!(is_ingot("minecraft:iron_ingot") && !is_ingot("minecraft:iron_nugget"));
    }

    #[test]
    fn flanking_goes_round_the_side_and_fleeing_goes_away() {
        let (me, target) = ([10.0, 64.0, 0.0], [0.0, 64.0, 0.0]);
        let left = flank_point(me, target, true, 4.0);
        let right = flank_point(me, target, false, 4.0);
        assert!((left[0]).abs() < 1e-9 && (left[2] - 4.0).abs() < 1e-9);
        assert!((right[0]).abs() < 1e-9 && (right[2] + 4.0).abs() < 1e-9);
        let away = flee_point(me, target, 8.0);
        assert!((away[0] - 18.0).abs() < 1e-9 && away[2].abs() < 1e-9);
        // Standing on the target gives a point all the same, not a NaN.
        assert!(
            flank_point(target, target, true, 4.0)
                .iter()
                .all(|v| v.is_finite())
        );
        assert!(
            flee_point(target, target, 8.0)
                .iter()
                .all(|v| v.is_finite())
        );
    }
}
