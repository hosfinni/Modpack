//! Skills: what a player gets better at by doing it, and faster in company.
//!
//! Three tracks, each fed by its own work: Mining by breaking ore, Smithing by
//! refining it, Combat by killing monsters. Experience earned near other players is
//! worth more, and they get a share of it for nothing: a party levels faster than
//! its members would alone.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The channel the server tells each client its own levels on.
pub const CHANNEL: &str = "hz_survival:skills";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Skill {
    Mining,
    Smithing,
    Combat,
}

impl Skill {
    pub const ALL: [Skill; 3] = [Skill::Mining, Skill::Smithing, Skill::Combat];

    pub fn name(self) -> &'static str {
        match self {
            Skill::Mining => "Mining",
            Skill::Smithing => "Smithing",
            Skill::Combat => "Combat",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SkillsConfig {
    pub enabled: bool,
    pub max_level: u32,
    /// The experience the first level takes; each level takes this much more than
    /// the one before.
    pub level_step: u32,
    /// Players this near one another are a party, in blocks.
    pub party_range: f64,
    /// How much more the earner gets for each party member near, and the most
    /// members that count.
    pub party_bonus: f32,
    pub party_bonus_members: u32,
    /// The share of the earner's experience each party member near gets too.
    pub party_share: f32,
    /// How fast ore is dug at Mining level 0, as a share of vanilla's speed, and how
    /// much each level adds.
    pub mining_speed_base: f32,
    pub mining_speed_per_level: f32,
    /// The chance of a second washed ore from one crushed ore, for each Smithing
    /// level.
    pub smithing_bonus_per_level: f32,
    /// The extra damage each Combat level deals.
    pub combat_damage_per_level: f32,
    /// Experience for breaking a block, by block id.
    pub mining_xp: BTreeMap<String, u32>,
    /// Experience for killing a mob, by entity id; `default` for the rest.
    pub combat_xp: BTreeMap<String, u32>,
    /// Experience for each ore washed and each ingot taken from a furnace.
    pub washing_xp: u32,
    pub smelting_xp: u32,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_level: 20,
            level_step: 40,
            party_range: 24.0,
            party_bonus: 0.2,
            party_bonus_members: 3,
            party_share: 0.5,
            mining_speed_base: 0.7,
            mining_speed_per_level: 0.03,
            smithing_bonus_per_level: 0.025,
            combat_damage_per_level: 0.015,
            mining_xp: BTreeMap::new(),
            combat_xp: BTreeMap::new(),
            washing_xp: 3,
            smelting_xp: 1,
        }
    }
}

impl SkillsConfig {
    pub fn check(&self) -> Result<(), String> {
        if !(1..=100).contains(&self.max_level) {
            return Err("skills.max_level must be 1 to 100".into());
        }
        if self.level_step == 0 {
            return Err("skills.level_step must be at least 1".into());
        }
        for (name, value) in [
            ("party_range", self.party_range as f32),
            ("party_bonus", self.party_bonus),
            ("party_share", self.party_share),
            ("mining_speed_per_level", self.mining_speed_per_level),
            ("smithing_bonus_per_level", self.smithing_bonus_per_level),
            ("combat_damage_per_level", self.combat_damage_per_level),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(format!("skills.{name} must be 0 or more, not {value}"));
            }
        }
        if !(self.mining_speed_base > 0.0 && self.mining_speed_base <= 4.0) {
            return Err("skills.mining_speed_base must be more than 0 and at most 4".into());
        }
        Ok(())
    }

    /// The total experience level `level` takes: each level costs `level_step` more
    /// than the last, so level 1 is one step, level 2 three, level 3 six.
    pub fn xp_for(&self, level: u32) -> u32 {
        let level = u64::from(level);
        (u64::from(self.level_step) * level * (level + 1) / 2).min(u64::from(u32::MAX)) as u32
    }

    /// The level `xp` experience makes, up to the highest.
    pub fn level_of(&self, xp: u32) -> u32 {
        (1..=self.max_level)
            .take_while(|level| self.xp_for(*level) <= xp)
            .last()
            .unwrap_or(0)
    }

    /// What the earner of `xp` gets with `near` party members beside them, and what
    /// each of those gets.
    pub fn party(&self, xp: u32, near: u32) -> (u32, u32) {
        let counted = near.min(self.party_bonus_members) as f32;
        let earner = (xp as f32 * (1.0 + self.party_bonus * counted)).round() as u32;
        let share = if near == 0 {
            0
        } else {
            (xp as f32 * self.party_share).round() as u32
        };
        (earner, share)
    }

    /// The speed ore is dug at, as a multiple of vanilla's, at a Mining level.
    pub fn mining_speed(&self, level: u32) -> f32 {
        self.mining_speed_base + self.mining_speed_per_level * level as f32
    }

    /// Whether `block` is ore: a block Mining is practised on, and slowed or sped by.
    pub fn is_ore(&self, block: &str) -> bool {
        self.mining_xp.contains_key(block)
    }

    /// The chance that washing one crushed ore gives a second, at a Smithing level.
    pub fn washing_bonus(&self, level: u32) -> f32 {
        (self.smithing_bonus_per_level * level as f32).min(1.0)
    }

    /// What a Combat level multiplies a player's blows by.
    pub fn combat_damage(&self, level: u32) -> f32 {
        1.0 + self.combat_damage_per_level * level as f32
    }

    pub fn kill_xp(&self, kind: &str) -> u32 {
        self.combat_xp
            .get(kind)
            .or_else(|| self.combat_xp.get("default"))
            .copied()
            .unwrap_or(0)
    }
}

/// One player's experience in each skill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skills {
    pub xp: [u32; 3],
}

impl Skills {
    pub fn level(&self, config: &SkillsConfig, skill: Skill) -> u32 {
        config.level_of(self.xp[skill.index()])
    }

    /// Adds experience. The new level when that reached one.
    pub fn gain(&mut self, config: &SkillsConfig, skill: Skill, xp: u32) -> Option<u32> {
        let before = self.level(config, skill);
        let cap = config.xp_for(config.max_level);
        let slot = &mut self.xp[skill.index()];
        *slot = slot.saturating_add(xp).min(cap);
        let after = self.level(config, skill);
        (after > before).then_some(after)
    }

    /// The levels as the server sends them to the player's own client.
    pub fn encode(&self, config: &SkillsConfig) -> [u8; 3] {
        Skill::ALL.map(|skill| self.level(config, skill).min(255) as u8)
    }

    /// One line for each skill: its level, and how far the next one is.
    pub fn report(&self, config: &SkillsConfig) -> Vec<String> {
        Skill::ALL
            .iter()
            .map(|skill| {
                let (xp, level) = (self.xp[skill.index()], self.level(config, *skill));
                if level >= config.max_level {
                    format!("{}: level {level} (mastered)", skill.name())
                } else {
                    let (from, to) = (config.xp_for(level), config.xp_for(level + 1));
                    format!(
                        "{}: level {level} ({} / {} to level {})",
                        skill.name(),
                        xp - from,
                        to - from,
                        level + 1
                    )
                }
            })
            .collect()
    }
}

/// Reads [`Skills::encode`]'s bytes: the levels, Mining first.
pub fn decode_levels(data: &[u8]) -> Option<[u8; 3]> {
    data.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn config() -> SkillsConfig {
        Config::shipped().skills
    }

    #[test]
    fn levels_cost_more_each_time() {
        let config = config();
        assert_eq!(config.xp_for(0), 0);
        assert_eq!(config.xp_for(1), 40);
        assert_eq!(config.xp_for(2), 120);
        assert_eq!(config.xp_for(3), 240);
        assert_eq!(config.level_of(0), 0);
        assert_eq!(config.level_of(39), 0);
        assert_eq!(config.level_of(40), 1);
        assert_eq!(config.level_of(239), 2);
        assert_eq!(config.level_of(u32::MAX), config.max_level);
    }

    #[test]
    fn gaining_reports_a_new_level_once_and_stops_at_the_top() {
        let config = config();
        let mut skills = Skills::default();
        assert_eq!(skills.gain(&config, Skill::Mining, 39), None);
        assert_eq!(skills.gain(&config, Skill::Mining, 1), Some(1));
        assert_eq!(skills.gain(&config, Skill::Mining, 1), None);
        assert_eq!(
            skills.level(&config, Skill::Combat),
            0,
            "each skill is its own"
        );
        assert_eq!(
            skills.gain(&config, Skill::Mining, u32::MAX),
            Some(config.max_level)
        );
        assert_eq!(skills.xp[0], config.xp_for(config.max_level));
        assert_eq!(skills.gain(&config, Skill::Mining, 500), None);
    }

    #[test]
    fn a_party_earns_more_and_shares() {
        let config = config();
        assert_eq!(
            config.party(100, 0),
            (100, 0),
            "alone: as earned, nobody to share with"
        );
        assert_eq!(config.party(100, 1), (120, 50));
        assert_eq!(config.party(100, 3), (160, 50));
        assert_eq!(
            config.party(100, 9),
            (160, 50),
            "only so many members count"
        );
        // Four players mining together: the earner gets 160 and three others 50 each,
        // 310 for the party where four soloists doing the same work get 100.
        let (earner, share) = config.party(100, 3);
        assert_eq!(earner + share * 3, 310);
    }

    #[test]
    fn skills_change_what_they_are_for() {
        let config = config();
        assert!(
            config.mining_speed(0) < 1.0,
            "a novice digs ore slower than vanilla"
        );
        assert!(
            (config.mining_speed(10) - 1.0).abs() < 1e-6,
            "level 10 digs at vanilla's speed"
        );
        assert!(config.mining_speed(20) > 1.2);
        assert_eq!(config.washing_bonus(0), 0.0);
        assert!((config.washing_bonus(20) - 0.5).abs() < 1e-6);
        assert_eq!(config.combat_damage(0), 1.0);
        assert!((config.combat_damage(20) - 1.3).abs() < 1e-6);
    }

    #[test]
    fn the_tables_know_ore_and_monsters() {
        let config = config();
        assert!(
            config.is_ore("minecraft:iron_ore") && config.is_ore("minecraft:deepslate_diamond_ore")
        );
        assert!(!config.is_ore("minecraft:stone"));
        assert!(config.mining_xp["minecraft:diamond_ore"] > config.mining_xp["minecraft:coal_ore"]);
        assert!(config.kill_xp("minecraft:enderman") > config.kill_xp("minecraft:zombie"));
        assert_eq!(
            config.kill_xp("othermod:ghoul"),
            config.combat_xp["default"]
        );
    }

    #[test]
    fn levels_survive_the_wire_and_the_report_reads_well() {
        let config = config();
        let mut skills = Skills::default();
        skills.gain(&config, Skill::Mining, 130);
        skills.gain(&config, Skill::Combat, u32::MAX);
        assert_eq!(decode_levels(&skills.encode(&config)), Some([2, 0, 20]));
        assert_eq!(decode_levels(&[1, 2]), None);
        let report = skills.report(&config);
        assert_eq!(report[0], "Mining: level 2 (10 / 120 to level 3)");
        assert_eq!(report[1], "Smithing: level 0 (0 / 40 to level 1)");
        assert_eq!(report[2], "Combat: level 20 (mastered)");
    }
}
