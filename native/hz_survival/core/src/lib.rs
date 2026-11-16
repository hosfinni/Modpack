//! hz_survival's rules, with no game types. The trait mod in `../api` feeds these
//! from Horizon's hooks.

use serde::Deserialize;

pub mod skills;
pub mod thirst;
pub mod world;

/// The config the mod writes on its first run.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub thirst: thirst::ThirstConfig,
    pub skills: skills::SkillsConfig,
    pub party: world::PartyConfig,
    pub mobs: world::MobsConfig,
    pub refining: world::RefiningConfig,
}

impl Config {
    /// Reads a config file. Missing keys take their defaults; an unknown key is an
    /// error, so a typo doesn't silently leave a rule off.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        config.thirst.check()?;
        config.skills.check()?;
        config.party.check()?;
        config.mobs.check()?;
        config.refining.check()?;
        Ok(config)
    }

    /// The config a fresh install runs with.
    pub fn shipped() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the shipped config parses")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_parses() {
        let config = Config::shipped();
        assert!(config.thirst.enabled);
        assert!(!config.thirst.drinks.is_empty());
    }

    #[test]
    fn a_typo_or_a_bad_value_is_refused() {
        assert!(Config::parse("[thirst]\nminutes_per_bar = 0\n").is_err());
        assert!(Config::parse("[thirst]\nenabeld = true\n").is_err());
        assert!(Config::parse("[thirst]\nuntreated_sickness_chance = 2.0\n").is_err());
        assert!(Config::parse("[thirst]\nparched_at = 1.0\ndehydrated_at = 5.0\n").is_err());
    }
}
