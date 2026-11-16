//! hz_travel: takes away the easy ways across the world, so that distance matters.
//! The rules themselves are in `hz-travel-core`; this crate hooks them into Horizon.
//!
//! - Elytra: the glider component is taken off every item, so nothing flies.
//! - Riptide: a trident with it cannot be charged (when turned off).
//! - Ender pearls: a long throw lands at most `max_distance` away.
//! - Chorus fruit and End gateways: a switch each.
//! - Boats, minecarts and mounts: a top speed over the ground, which ends ice roads.

use std::sync::RwLock;

use horizon_client::mod_api::{
    ChorusFruitTeleportEvent, ConfigLoadingEvent, ConfigReloadingEvent, EnderPearlTeleportEvent,
    Flow, Hooks, Host, LivingEntityUseItemStartEvent, ModifyDefaultComponentsEvent, NativeConfig,
    NativeConfigKind, PlayerTeleportEndGatewayEvent, RegisterConfigsEvent, Text, TooltipEvent,
    VehicleMovedV2Event,
};
use hz_travel_core::{Config, DEFAULT_CONFIG, GLIDER_COMPONENT, Teleport};

const MOD_ID: &str = "hz_travel";
const CONFIG_FILE: &str = "hz_travel.toml";
const RIPTIDE: &str = "minecraft:riptide";
/// The grey of a tooltip's description lines.
const TOOLTIP_GREY: u32 = 0xAAAAAAFF;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Travel {
        config: RwLock::new(Config::shipped()),
    })
}

struct Travel {
    /// The shipped rules until the game hands over `config/hz_travel.toml`.
    config: RwLock<Config>,
}

impl Travel {
    fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn load(&self, host: &dyn Host, mod_id: &str, file: &str, text: &str) {
        if mod_id != MOD_ID || file != CONFIG_FILE {
            return;
        }
        match Config::parse(text) {
            Ok(config) => *self.config.write().unwrap_or_else(|e| e.into_inner()) = config,
            // A broken file keeps the rules in use, and says so.
            Err(error) => host.log(
                "ERROR",
                &format!("[hz_travel] config/{CONFIG_FILE}: {error}"),
            ),
        }
    }
}

impl Hooks for Travel {
    fn register_configs(&self, _host: &mut dyn Host, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(
            NativeConfig::new(MOD_ID, NativeConfigKind::Common, DEFAULT_CONFIG).file(CONFIG_FILE),
        );
    }

    fn config_loading(&self, host: &mut dyn Host, event: &mut ConfigLoadingEvent<'_>) {
        let config = event.config;
        self.load(host, &config.mod_id, &config.file, &config.text);
    }

    fn config_reloading(&self, host: &mut dyn Host, event: &mut ConfigReloadingEvent<'_>) {
        let config = event.config;
        self.load(host, &config.mod_id, &config.file, &config.text);
    }

    /// Asked for every item, on both sides and from any thread: the answer depends
    /// on the config alone, so client and server agree when they share it.
    fn modify_default_components(&self, event: &mut ModifyDefaultComponentsEvent<'_>) {
        if self.config().strips_component(GLIDER_COMPONENT) {
            event.components.remove(GLIDER_COMPONENT);
        }
    }

    /// Says why an elytra no longer flies, where the player looks for it.
    fn tooltip(&self, _host: &dyn Host, event: &mut TooltipEvent<'_>) {
        let config = self.config();
        if config.elytra.disable_gliding && &*event.stack.item == "minecraft:elytra" {
            event
                .lines
                .push((Text::literal("Too brittle to fly with"), TOOLTIP_GREY));
        }
        if !config.riptide_allowed() && event.stack.enchantment_level(RIPTIDE) > 0 {
            event.lines.push((
                Text::literal("Riptide has no pull in this world"),
                TOOLTIP_GREY,
            ));
        }
    }

    /// Both sides ask this, so the client doesn't predict a launch the server refuses.
    fn living_entity_use_item_start(
        &self,
        _host: &mut dyn Host,
        event: &mut LivingEntityUseItemStartEvent<'_>,
    ) -> Flow {
        if !self.config().riptide_allowed() && event.stack.enchantment_level(RIPTIDE) > 0 {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn ender_pearl_teleport(
        &self,
        _host: &mut dyn Host,
        event: &mut EnderPearlTeleportEvent<'_>,
    ) -> Flow {
        match self.config().ender_pearl(event.from, *event.target) {
            Teleport::Allow => Flow::Continue,
            Teleport::MoveTo(spot) => {
                *event.target = spot;
                Flow::Continue
            }
            Teleport::Deny => Flow::Cancel,
        }
    }

    fn chorus_fruit_teleport(
        &self,
        _host: &mut dyn Host,
        _event: &mut ChorusFruitTeleportEvent<'_>,
    ) -> Flow {
        if self.config().chorus_fruit.enabled {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn player_teleport_end_gateway(
        &self,
        _host: &mut dyn Host,
        _event: &mut PlayerTeleportEndGatewayEvent<'_>,
    ) -> Flow {
        if self.config().end_gateway.enabled {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// A ridden boat, minecart or mount moved: one over its limit is put where the
    /// limit lets it reach. The game tells the riders' clients.
    fn vehicle_moved_v2(&self, _host: &mut dyn Host, event: &mut VehicleMovedV2Event<'_>) -> Flow {
        if let Some(to) = self
            .config()
            .vehicle_move(event.kind, event.from, *event.to)
        {
            *event.to = to;
        }
        Flow::Continue
    }
}
