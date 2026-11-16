//! hz_survival: thirst, shared progression and smarter monsters. The rules are in
//! `hz-survival-core`; this crate hooks them into Horizon.

use std::sync::{Arc, RwLock};

use horizon_client::mod_api::{
    BreakSpeedEvent, ClientDisconnectingEvent, ClientLevelUnloadEvent, ConfigLoadingEvent,
    ConfigReloadingEvent, DataPackFilesEvent, DestroyBlockEvent, Flow, FurnaceExtractEvent,
    HealthHudEvent, Hooks, Host, HudEvent, LivingEntityUseItemFinishEvent, LivingHealEvent,
    LivingIncomingDamageV3Event, MobKilledEvent, NativeCommand, NativeConfig, NativeConfigKind,
    NetFromServerEvent, PerformCommandEvent, PlayerLoggedOutEvent, PlayerRespawnedEvent,
    PlayerTickEvent, RegisterCommandsEvent, RegisterConfigsEvent, RegisterGoalsEvent,
    ServerAboutToStartEvent, TooltipEvent, UseItemOnEvent,
};
use hz_survival_core::{Config, DEFAULT_CONFIG};

mod mobs;
mod progress;
mod thirst;

const MOD_ID: &str = "hz_survival";
const CONFIG_FILE: &str = "hz_survival.toml";

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Survival {
        config: RwLock::new(Arc::new(Config::shipped())),
        thirst: thirst::State::default(),
        progress: progress::State::default(),
    })
}

/// One instance serves the window's game and the server thread's, so everything it
/// keeps is behind a lock or an atomic.
struct Survival {
    /// The shipped rules until the game hands over `config/hz_survival.toml`.
    config: RwLock<Arc<Config>>,
    thirst: thirst::State,
    progress: progress::State,
}

impl Survival {
    fn config(&self) -> Arc<Config> {
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
            Ok(config) => {
                *self.config.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(config);
            }
            // A broken file keeps the rules in use, and says so.
            Err(error) => {
                host.log(
                    "ERROR",
                    &format!("[hz_survival] config/{CONFIG_FILE}: {error}"),
                );
            }
        }
    }
}

impl Hooks for Survival {
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

    fn server_about_to_start(&self, _host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        self.thirst.reset();
        self.progress.reset();
    }

    fn player_tick(&self, host: &mut dyn Host, _event: &mut PlayerTickEvent) {
        let config = self.config();
        self.thirst.player_tick(host, &config.thirst);
        self.progress.player_tick(host, &config);
    }

    fn living_entity_use_item_finish(
        &self,
        host: &mut dyn Host,
        event: &mut LivingEntityUseItemFinishEvent<'_>,
    ) {
        self.thirst
            .use_item_finish(host, &self.config().thirst, event);
    }

    fn living_heal(&self, event: &mut LivingHealEvent) -> Flow {
        self.thirst.living_heal(&self.config().thirst, event)
    }

    fn player_respawned(&self, host: &mut dyn Host, event: &mut PlayerRespawnedEvent<'_>) {
        self.thirst
            .respawned(host, &self.config().thirst, event.player);
    }

    fn player_logged_out(&self, host: &mut dyn Host, event: &mut PlayerLoggedOutEvent<'_>) {
        self.thirst.logged_out(host, event.player);
        self.progress.logged_out(event.player);
    }

    // Skills, parties and refining.

    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        self.progress.block_broken(host, &self.config(), event.pos);
        Flow::Continue
    }

    fn mob_killed(&self, host: &mut dyn Host, event: &mut MobKilledEvent<'_>) {
        self.progress
            .mob_killed(host, &self.config(), event.player, event.kind);
    }

    fn furnace_extract(&self, host: &mut dyn Host, event: &mut FurnaceExtractEvent<'_>) {
        self.progress
            .furnace_extract(host, &self.config(), event.player, event.item, event.count);
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        self.progress.use_item_on(host, &self.config(), event)
    }

    fn break_speed(&self, host: &dyn Host, event: &mut BreakSpeedEvent) {
        self.progress.break_speed(host, &self.config(), event);
    }

    fn living_incoming_damage_v3(&self, event: &mut LivingIncomingDamageV3Event<'_>) -> Flow {
        self.progress.incoming_damage(&self.config(), event);
        Flow::Continue
    }

    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        event.commands.push(NativeCommand::new("skills"));
    }

    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        if event.command.trim() != "skills" {
            return Flow::Continue;
        }
        self.progress
            .report(host, &self.config(), event.player.uuid);
        Flow::Cancel
    }

    /// The server builds its data tables: smelting ore as it comes out of the ground
    /// gives nuggets, not ingots.
    fn data_pack_files(&self, _host: &dyn Host, event: &mut DataPackFilesEvent<'_>) {
        for (key, json) in self.config().refining.crude_recipes() {
            event.files.retain(|(path, _)| *path != key);
            event.files.push((key, json.into_bytes()));
        }
    }

    /// A new mob's goals are being set: a monster gets the pack's.
    fn register_goals(&self, event: &mut RegisterGoalsEvent<'_>) {
        mobs::register_goals(&self.config(), event);
    }

    // The window's side.

    fn net_from_server(&self, _host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        self.thirst.net_from_server(event);
        self.progress.net_from_server(event);
    }

    fn client_level_unload(&self, _host: &mut dyn Host, _event: &mut ClientLevelUnloadEvent<'_>) {
        self.thirst.client_left();
    }

    fn client_disconnecting(&self, _host: &dyn Host, _event: &mut ClientDisconnectingEvent) {
        self.thirst.client_left();
        self.progress.client_left();
    }

    /// Only watches: answering None keeps Horizon's own health rows.
    fn health_hud(
        &self,
        _host: &mut dyn Host,
        event: &mut HealthHudEvent<'_>,
    ) -> Option<Vec<horizon_client::mod_api::GuiDraw>> {
        self.thirst.health_rows(event);
        None
    }

    fn hud(&self, host: &mut dyn Host, event: &mut HudEvent<'_>) {
        self.thirst.hud(host, event);
    }

    fn tooltip(&self, _host: &dyn Host, event: &mut TooltipEvent<'_>) {
        self.thirst.tooltip(&self.config().thirst, event);
    }
}
