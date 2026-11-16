//! hz_travel's rules, with no game types: what the config says and what each rule
//! does to a move. The trait mod in `../api` feeds these from Horizon's hooks.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The config the mod writes on its first run.
pub const DEFAULT_CONFIG: &str = include_str!("../default-config.toml");

/// The component that lets an item be flown with (vanilla's `minecraft:glider`).
pub const GLIDER_COMPONENT: &str = "minecraft:glider";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub elytra: Elytra,
    pub riptide: Toggle,
    pub ender_pearl: EnderPearl,
    pub chorus_fruit: Toggle,
    pub end_gateway: Toggle,
    pub vehicles: Vehicles,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Elytra {
    pub disable_gliding: bool,
}

impl Default for Elytra {
    fn default() -> Self {
        Self {
            disable_gliding: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Toggle {
    pub enabled: bool,
}

impl Default for Toggle {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EnderPearl {
    pub enabled: bool,
    pub max_distance: f64,
}

impl Default for EnderPearl {
    fn default() -> Self {
        Self {
            enabled: true,
            max_distance: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Vehicles {
    pub boat: f64,
    pub minecart: f64,
    pub other: f64,
    pub kinds: BTreeMap<String, f64>,
}

/// What happens to a teleport a rule looked at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Teleport {
    /// Left as the game drew it.
    Allow,
    /// Goes to this spot instead.
    MoveTo([f64; 3]),
    /// Does not happen.
    Deny,
}

impl Config {
    /// Reads a config file. Missing keys take their defaults; an unknown key is an
    /// error, so a typo doesn't silently leave a rule off.
    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        config.check()?;
        Ok(config)
    }

    /// The config a fresh install runs with.
    pub fn shipped() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("the shipped config parses")
    }

    fn check(&self) -> Result<(), String> {
        let limit = |name: &str, value: f64| {
            if value.is_finite() && value >= 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be 0 or more, not {value}"))
            }
        };
        limit("ender_pearl.max_distance", self.ender_pearl.max_distance)?;
        limit("vehicles.boat", self.vehicles.boat)?;
        limit("vehicles.minecart", self.vehicles.minecart)?;
        limit("vehicles.other", self.vehicles.other)?;
        for (kind, value) in &self.vehicles.kinds {
            limit(&format!("vehicles.kinds.\"{kind}\""), *value)?;
        }
        Ok(())
    }

    /// Whether `component` is taken off every item's defaults.
    pub fn strips_component(&self, component: &str) -> bool {
        self.elytra.disable_gliding && component == GLIDER_COMPONENT
    }

    /// Whether a trident with Riptide may be charged.
    pub fn riptide_allowed(&self) -> bool {
        self.riptide.enabled
    }

    /// An ender pearl about to carry its thrower from `from` to `target`.
    pub fn ender_pearl(&self, from: [f64; 3], target: [f64; 3]) -> Teleport {
        if !self.ender_pearl.enabled {
            return Teleport::Deny;
        }
        let max = self.ender_pearl.max_distance;
        if max <= 0.0 {
            return Teleport::Allow;
        }
        let delta = [
            target[0] - from[0],
            target[1] - from[1],
            target[2] - from[2],
        ];
        let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        if !distance.is_finite() || distance <= max {
            return Teleport::Allow;
        }
        let scale = max / distance;
        Teleport::MoveTo([
            from[0] + delta[0] * scale,
            from[1] + delta[1] * scale,
            from[2] + delta[2] * scale,
        ])
    }

    /// The speed limit of vehicle `kind` (`minecraft:oak_boat`), in blocks per tick
    /// over the ground. None means no limit.
    pub fn vehicle_limit(&self, kind: &str) -> Option<f64> {
        let limit = match self.vehicles.kinds.get(kind) {
            Some(limit) => *limit,
            None => match vehicle_group(kind) {
                VehicleGroup::Boat => self.vehicles.boat,
                VehicleGroup::Minecart => self.vehicles.minecart,
                VehicleGroup::Other => self.vehicles.other,
            },
        };
        (limit > 0.0).then_some(limit)
    }

    /// A vehicle of `kind` about to move from `from` to `to` in one tick: where it
    /// goes instead when that is faster than its limit, None when the move stands.
    /// Only the speed over the ground is limited, so falling is left alone.
    pub fn vehicle_move(&self, kind: &str, from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
        let limit = self.vehicle_limit(kind)?;
        let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
        let speed = (dx * dx + dz * dz).sqrt();
        // A jump of many blocks in a tick is a teleport (a portal, a command), not travel.
        if !speed.is_finite() || speed <= limit || speed > TELEPORT_STEP {
            return None;
        }
        let scale = limit / speed;
        Some([from[0] + dx * scale, to[1], from[2] + dz * scale])
    }
}

/// A single tick's move longer than this is a teleport rather than travel.
const TELEPORT_STEP: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleGroup {
    Boat,
    Minecart,
    Other,
}

/// Which group of limits an entity type belongs to, from its id alone: vanilla names
/// every boat `<wood>_boat`, `<wood>_chest_boat` or `bamboo_raft`, and every minecart
/// `minecart` or `<kind>_minecart`, and a mod's own usually follow.
pub fn vehicle_group(kind: &str) -> VehicleGroup {
    let name = kind.rsplit(':').next().unwrap_or(kind);
    if name.ends_with("boat") || name.ends_with("raft") {
        VehicleGroup::Boat
    } else if name.ends_with("minecart") {
        VehicleGroup::Minecart
    } else {
        VehicleGroup::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_parses_and_limits_travel() {
        let config = Config::shipped();
        assert!(config.strips_component("minecraft:glider"));
        assert!(!config.strips_component("minecraft:food"));
        assert!(!config.riptide_allowed());
        assert_eq!(config.ender_pearl.max_distance, 12.0);
        assert_eq!(config.vehicle_limit("minecraft:oak_boat"), Some(0.45));
        assert_eq!(config.vehicle_limit("minecraft:happy_ghast"), Some(0.3));
        assert_eq!(config.vehicle_limit("minecraft:horse"), None);
    }

    #[test]
    fn an_empty_file_is_the_code_defaults() {
        let config = Config::parse("").unwrap();
        assert_eq!(config, Config::default());
        assert!(config.elytra.disable_gliding);
        assert_eq!(config.vehicle_limit("minecraft:oak_boat"), None);
    }

    #[test]
    fn a_typo_or_a_negative_limit_is_refused() {
        assert!(Config::parse("[elytra]\ndisable_glidng = true\n").is_err());
        assert!(Config::parse("[vehicles]\nboat = -1.0\n").is_err());
        assert!(Config::parse("[ender_pearl]\nmax_distance = nan\n").is_err());
    }

    #[test]
    fn a_short_pearl_throw_is_left_alone_and_a_long_one_is_cut() {
        let config = Config::shipped();
        assert_eq!(
            config.ender_pearl([0.0, 64.0, 0.0], [3.0, 64.0, 4.0]),
            Teleport::Allow
        );
        let Teleport::MoveTo(spot) = config.ender_pearl([0.0, 64.0, 0.0], [30.0, 64.0, 40.0])
        else {
            panic!("a 50 block throw is cut");
        };
        assert!((spot[0] - 7.2).abs() < 1e-9 && (spot[2] - 9.6).abs() < 1e-9);
        assert_eq!(spot[1], 64.0);
    }

    #[test]
    fn pearls_can_be_turned_off() {
        let config = Config::parse("[ender_pearl]\nenabled = false\n").unwrap();
        assert_eq!(
            config.ender_pearl([0.0; 3], [1.0, 0.0, 0.0]),
            Teleport::Deny
        );
    }

    #[test]
    fn vehicles_are_grouped_by_their_id() {
        assert_eq!(vehicle_group("minecraft:oak_boat"), VehicleGroup::Boat);
        assert_eq!(
            vehicle_group("minecraft:cherry_chest_boat"),
            VehicleGroup::Boat
        );
        assert_eq!(vehicle_group("minecraft:bamboo_raft"), VehicleGroup::Boat);
        assert_eq!(
            vehicle_group("minecraft:tnt_minecart"),
            VehicleGroup::Minecart
        );
        assert_eq!(vehicle_group("minecraft:minecart"), VehicleGroup::Minecart);
        assert_eq!(vehicle_group("minecraft:strider"), VehicleGroup::Other);
    }

    #[test]
    fn a_boat_on_ice_is_held_to_its_limit_but_may_still_fall() {
        let config = Config::shipped();
        // 3 blocks in a tick over ice, falling 0.5.
        let to = config
            .vehicle_move("minecraft:oak_boat", [0.0, 64.0, 0.0], [3.0, 63.5, 0.0])
            .expect("too fast");
        assert!((to[0] - 0.45).abs() < 1e-9);
        assert_eq!(to[1], 63.5);
        // A boat on water is under the limit.
        assert_eq!(
            config.vehicle_move("minecraft:oak_boat", [0.0, 64.0, 0.0], [0.4, 64.0, 0.0]),
            None
        );
        // A portal's jump is not travel.
        assert_eq!(
            config.vehicle_move("minecraft:oak_boat", [0.0, 64.0, 0.0], [800.0, 64.0, 0.0]),
            None
        );
    }
}
