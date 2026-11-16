//! hz_seasons: Biome Shift. When the players sleep the climate turns: biomes move
//! along their climate ladders, the ground follows, and every screen goes black
//! behind a banner while the land around the players is remade. The rules are in
//! `hz-seasons-core`; this crate hooks them into Horizon.

use std::sync::{Arc, RwLock};

use horizon_client::mod_api::{
    BiomeColorsEvent, BlockPlaceEvent, ChunkLoadEvent, ClientDisconnectingEvent,
    ConfigLoadingEvent, ConfigReloadingEvent, DestroyBlockEvent, Flow, GuiRenderPostEvent, Hooks,
    Host, LevelTickPostEvent, NativeConfig, NativeConfigKind, NetFromServerEvent,
    PlayerLoggedInEvent, RegisterConfigsEvent, ServerAboutToStartEvent, ShaderParamsEvent,
    SleepFinishedTimeEvent,
};
use hz_seasons_core::{CHANNEL, Config, DEFAULT_CONFIG, Message};

mod client;
mod server;

use server::{ANCHOR, OVERWORLD};

const MOD_ID: &str = "hz_seasons";
const CONFIG_FILE: &str = "hz_seasons.toml";

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Seasons {
        config: RwLock::new(Arc::new(Config::shipped())),
        server: server::Server::default(),
        client: client::Client::default(),
    })
}

/// One instance serves the window's game and the server thread's, so everything it
/// keeps is behind a lock or an atomic.
struct Seasons {
    /// The shipped rules until the game hands over `config/hz_seasons.toml`.
    config: RwLock<Arc<Config>>,
    server: server::Server,
    client: client::Client,
}

impl Seasons {
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
            Err(error) => host.log(
                "ERROR",
                &format!("[hz_seasons] config/{CONFIG_FILE}: {error}"),
            ),
        }
    }
}

impl Hooks for Seasons {
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

    // The server's side.

    fn server_about_to_start(&self, _host: &mut dyn Host, _event: &mut ServerAboutToStartEvent) {
        self.server.reset();
    }

    /// The night was slept through: one night nearer the next shift, or the shift.
    fn sleep_finished_time(&self, host: &mut dyn Host, event: &mut SleepFinishedTimeEvent<'_>) {
        if event.dimension == OVERWORLD {
            self.server.slept(host, &self.config());
        }
    }

    fn level_tick_post(&self, host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {
        if event.dimension == OVERWORLD {
            self.server.tick(host, &self.config());
        }
    }

    fn chunk_load(&self, _host: &mut dyn Host, event: &mut ChunkLoadEvent<'_>) {
        if event.dimension == OVERWORLD {
            self.server.chunk_loaded(event.chunk);
        }
    }

    fn player_logged_in(&self, host: &mut dyn Host, event: &mut PlayerLoggedInEvent<'_>) {
        self.server.joined(host, &self.config(), event.player);
    }

    fn block_place(&self, host: &mut dyn Host, event: &mut BlockPlaceEvent<'_>) -> Flow {
        if &*event.stack.item == ANCHOR && host.dimension() == OVERWORLD {
            self.server.anchor_placed(host, event.pos);
        }
        Flow::Continue
    }

    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        if host.dimension() == OVERWORLD {
            self.server.block_broken(host, event.pos);
        }
        Flow::Continue
    }

    // The window's side.

    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        if event.channel != CHANNEL {
            return;
        }
        if let Some(message) = Message::decode(event.data) {
            self.client.message(host, &self.config(), message);
        }
    }

    fn client_disconnecting(&self, _host: &dyn Host, _event: &mut ClientDisconnectingEvent) {
        self.client.left();
    }

    fn biome_colors(&self, _host: &dyn Host, event: &mut BiomeColorsEvent<'_>) {
        self.client.biome_colors(&self.config(), event);
    }

    fn shader_params(&self, _host: &dyn Host, event: &mut ShaderParamsEvent<'_>) {
        self.client.shader_params(event);
    }

    fn gui_render_post(&self, host: &dyn Host, event: &mut GuiRenderPostEvent<'_>) {
        self.client.draw(host, event);
    }
}
