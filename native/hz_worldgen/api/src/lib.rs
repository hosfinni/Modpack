//! hz_worldgen: tectonic terrain for the overworld. The terrain function is
//! `hz-worldgen-core`; this crate writes it into the chunks Horizon generates, and
//! into the far terrain a client draws before those chunks exist.
//!
//! Both hooks run on worker threads, several at once, and can be asked for the same
//! chunk twice, so they answer from the seed, the config and the position alone.

use std::sync::{Arc, RwLock};

use horizon_client::mod_api::{
    BlockStateId, ChunkGenerateEvent, ConfigLoadingEvent, ConfigReloadingEvent, Hooks, Host,
    LodColumnEvent, NativeConfig, NativeConfigKind, NativeGenerate, RegisterConfigsEvent,
};
use hz_worldgen_core::{Biome, Block, Column, Config, DEFAULT_CONFIG, Terrain, World};

const MOD_ID: &str = "hz_worldgen";
const CONFIG_FILE: &str = "hz_worldgen.toml";
const OVERWORLD: &str = "minecraft:overworld";
/// How far below the ground this terrain's biomes reach. Deeper down the game's own
/// cave biomes stay, so lush and dripstone caves still decorate the caves there.
const BIOME_DEPTH: i32 = 16;
/// The overworld's build range, for far terrain, which isn't told the level's.
const FAR_RANGE: (i32, i32) = (-64, 320);

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Worldgen {
        config: RwLock::new(Arc::new(Config::shipped())),
        world: RwLock::new(None),
        states: RwLock::new(None),
    })
}

struct Worldgen {
    /// The shipped shape until the game hands over `config/hz_worldgen.toml`.
    config: RwLock<Arc<Config>>,
    /// The terrain of the seed last asked for.
    world: RwLock<Option<(i64, Arc<World>)>>,
    /// The game's states of the terrain's blocks, for the registry last seen.
    states: RwLock<Option<(usize, States)>>,
}

type States = [BlockStateId; Block::ALL.len()];

impl Worldgen {
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
                // The next chunk builds its world from the new config.
                *self.world.write().unwrap_or_else(|e| e.into_inner()) = None;
            }
            Err(error) => {
                host.log(
                    "ERROR",
                    &format!("[hz_worldgen] config/{CONFIG_FILE}: {error}"),
                );
            }
        }
    }

    /// The terrain of `seed`, when this mod builds `dimension`.
    fn world(&self, dimension: &str, seed: i64, flat: bool) -> Option<Arc<World>> {
        if dimension != OVERWORLD || flat {
            return None;
        }
        if let Some((cached, world)) = &*self.world.read().unwrap_or_else(|e| e.into_inner())
            && *cached == seed
        {
            return Some(world.clone());
        }
        let config = self.config();
        if config.terrain != Terrain::Tectonic {
            return None;
        }
        let world = Arc::new(World::new(seed, (*config).clone()));
        *self.world.write().unwrap_or_else(|e| e.into_inner()) = Some((seed, world.clone()));
        Some(world)
    }

    /// The game's state for each of the terrain's blocks, found with `lookup` once
    /// for each block registry (`registry` is its address). None when the game lacks
    /// a block, which leaves the chunk to vanilla rather than full of holes.
    fn states(
        &self,
        registry: usize,
        lookup: impl Fn(&str) -> Option<BlockStateId>,
    ) -> Option<States> {
        if let Some((cached, states)) = &*self.states.read().unwrap_or_else(|e| e.into_inner())
            && *cached == registry
        {
            return Some(*states);
        }
        let mut states = [lookup(Block::Stone.id())?; Block::ALL.len()];
        for block in Block::ALL {
            states[block.index()] = lookup(block.id())?;
        }
        *self.states.write().unwrap_or_else(|e| e.into_inner()) = Some((registry, states));
        Some(states)
    }
}

impl Hooks for Worldgen {
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

    /// A chunk is about to get its terrain: build it, column by column.
    fn chunk_generate(&self, event: &mut ChunkGenerateEvent<'_, '_>) {
        let Some(world) = self.world(event.dimension, event.seed, event.flat) else {
            return;
        };
        let out = &mut *event.chunk_out;
        let registry = out.registry();
        let Some(states) = self.states(std::ptr::from_ref(registry) as usize, |id| {
            registry.default_state(id)
        }) else {
            return;
        };
        let (min_y, height) = out.level();
        let max_y = min_y + height;
        let [chunk_x, chunk_z] = event.chunk;
        let (origin_x, origin_z) = (chunk_x * 16, chunk_z * 16);

        let mut columns: Vec<Column> = Vec::with_capacity(256);
        let mut spans = Vec::with_capacity(8);
        for dz in 0..16 {
            for dx in 0..16 {
                let (x, z) = (origin_x + dx, origin_z + dz);
                columns.push(world.spans(x, z, min_y, max_y, &mut spans));
                for &(low, high, block) in &spans {
                    let state = states[block.index()];
                    for y in low..high {
                        out.set_block(x, y, z, state);
                    }
                }
            }
        }

        // The biomes decoration and the players see. A biome the world lacks (a data
        // pack took it out) keeps vanilla's there.
        let ids = Biome::ALL.map(|biome| out.biome_id(biome.id()));
        out.set_biomes(|[quart_x, quart_y, quart_z]| {
            // A quart is four blocks; take the column at its middle.
            let dx = (quart_x * 4 + 2 - origin_x).clamp(0, 15) as usize;
            let dz = (quart_z * 4 + 2 - origin_z).clamp(0, 15) as usize;
            let column = &columns[dz * 16 + dx];
            if quart_y * 4 + 3 < column.height - BIOME_DEPTH {
                return None;
            }
            ids[column.biome.index()]
        });

        let config = world.config();
        *event.generate = NativeGenerate::Replaced {
            // The game's surface rule reads vanilla's biome map, not this terrain's:
            // the terrain paints its own ground.
            surface: false,
            carvers: config.carvers,
            structures: config.structures,
            decorations: config.decorations,
        };
    }

    /// Far terrain samples a column no chunk exists for yet: answer what the chunk
    /// will hold, so distant land doesn't change shape as the player reaches it.
    fn lod_column(&self, event: &mut LodColumnEvent<'_, '_>) {
        let Some(world) = self.world(event.dimension, event.seed, event.flat) else {
            return;
        };
        let out = &mut *event.column_out;
        let registry = out.registry();
        let Some(states) = self.states(std::ptr::from_ref(registry) as usize, |id| {
            registry.default_state(id)
        }) else {
            return;
        };
        let [x, z] = event.column;
        let mut spans = Vec::with_capacity(8);
        let column = world.spans(x, z, FAR_RANGE.0, FAR_RANGE.1, &mut spans);
        let spans = spans
            .into_iter()
            .map(|(low, high, block)| (low, high, states[block.index()]));
        out.replace(spans.collect(), out.biome_id(column.biome.id()));
    }
}
