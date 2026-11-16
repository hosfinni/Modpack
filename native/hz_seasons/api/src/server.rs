//! Biome Shift on the server: the calendar, the announcement, and the conversion of
//! chunks to the new climate, a few each tick.
//!
//! Everything here runs in the overworld's own hooks, with the overworld in place,
//! so the level attachments and the block helpers reach it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, Holder, Host, NativeBlockEdit,
    NativeBlockRun, NativeHeightmap, Tag, Text,
};
use hz_seasons_core::biomes::{profile, shifted};
use hz_seasons_core::ground::{self, ABOVE, BELOW, Edit};
use hz_seasons_core::{
    CHANNEL, Calendar, Config, Message, Queue, chunks_around, noise, season, year,
};

pub const OVERWORLD: &str = "minecraft:overworld";
pub const ANCHOR: &str = "hz_content:climate_anchor";
/// How far below the lowest ground of a chunk its biomes are changed. Deeper down
/// the cave biomes stay as they are.
const BIOME_DEPTH: i32 = 8;
/// The biome a place is taken to have when the game can't say.
const UNKNOWN_BIOME: &str = "minecraft:plains";
/// A chunk is four quarts across, and a biome fills a quart.
const QUARTS: usize = 16;

/// The epoch and the nights slept toward the next one, saved with the overworld.
static CALENDAR: AttachmentType<Calendar> = AttachmentType {
    namespace: "hz_seasons",
    name: "calendar",
    default: Calendar::default,
    codec: Some(AttachmentCodec {
        encode: |calendar| {
            Ok([
                ("epoch".into(), Tag::Long(calendar.epoch)),
                ("sleeps".into(), Tag::Int(calendar.sleeps as i32)),
            ]
            .into())
        },
        decode: |data: &Compound| match (data.get(&"epoch".into()), data.get(&"sleeps".into())) {
            (Some(Tag::Long(epoch)), Some(Tag::Int(sleeps))) => Ok(Calendar {
                epoch: *epoch,
                sleeps: (*sleeps).max(0) as u32,
            }),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

/// Where climate anchors stand, saved with the overworld: x, y, z of each.
static ANCHORS: AttachmentType<Vec<[i32; 3]>> = AttachmentType {
    namespace: "hz_seasons",
    name: "anchors",
    default: Vec::new,
    codec: Some(AttachmentCodec {
        encode: |anchors| {
            Ok([(
                "blocks".into(),
                Tag::IntArray(anchors.iter().flatten().copied().collect()),
            )]
            .into())
        },
        decode: |data: &Compound| match data.get(&"blocks".into()) {
            Some(Tag::IntArray(flat)) => Ok(flat
                .chunks_exact(3)
                .map(|block| [block[0], block[1], block[2]])
                .collect()),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

/// What a chunk remembers: the biomes it was generated with, and how far each has
/// been moved.
#[derive(Clone, Debug, PartialEq)]
struct ChunkState {
    /// The epoch the chunk was last converted to.
    epoch: i64,
    /// The generated biome of each quart column, x within z.
    base: Vec<String>,
    /// The steps each quart column's biome stands from its generated one.
    applied: Vec<i8>,
}

static CHUNK: AttachmentType<Option<ChunkState>> = AttachmentType {
    namespace: "hz_seasons",
    name: "chunk",
    default: || None,
    codec: Some(AttachmentCodec {
        encode: |state| {
            let Some(state) = state else {
                return Ok(Compound::new());
            };
            // The few biomes of a chunk once each, and an index for each column.
            let mut names: Vec<&str> = Vec::new();
            let mut cells = Vec::with_capacity(QUARTS);
            for biome in &state.base {
                let index = names
                    .iter()
                    .position(|name| name == biome)
                    .unwrap_or_else(|| {
                        names.push(biome);
                        names.len() - 1
                    });
                cells.push(index as u8);
            }
            Ok([
                ("epoch".into(), Tag::Long(state.epoch)),
                (
                    "biomes".into(),
                    Tag::List(
                        names
                            .into_iter()
                            .map(|name| Tag::String(name.into()))
                            .collect(),
                    ),
                ),
                ("cells".into(), Tag::ByteArray(cells)),
                (
                    "applied".into(),
                    Tag::ByteArray(state.applied.iter().map(|steps| *steps as u8).collect()),
                ),
            ]
            .into())
        },
        decode: |data: &Compound| {
            let Some(Tag::Long(epoch)) = data.get(&"epoch".into()) else {
                return Ok(None);
            };
            let (
                Some(Tag::List(names)),
                Some(Tag::ByteArray(cells)),
                Some(Tag::ByteArray(applied)),
            ) = (
                data.get(&"biomes".into()),
                data.get(&"cells".into()),
                data.get(&"applied".into()),
            )
            else {
                return Err(AttachmentError::Decode);
            };
            if cells.len() != QUARTS || applied.len() != QUARTS {
                return Err(AttachmentError::Decode);
            }
            let mut base = Vec::with_capacity(QUARTS);
            for cell in cells {
                match names.get(usize::from(*cell)) {
                    Some(Tag::String(name)) => base.push(name.to_string_lossy()),
                    _ => return Err(AttachmentError::Decode),
                }
            }
            let applied = applied.iter().map(|steps| *steps as i8).collect();
            Ok(Some(ChunkState {
                epoch: *epoch,
                base,
                applied,
            }))
        },
    }),
    copy_on_death: false,
    sync: false,
};

#[derive(Default)]
pub struct Server {
    /// The calendar and the anchors have been read from this world's save.
    loaded: AtomicBool,
    calendar: Mutex<Calendar>,
    anchors: Mutex<Vec<[i32; 3]>>,
    queue: Mutex<Queue>,
    seed: Mutex<Option<i64>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Server {
    /// A world is about to open: what was read from the last one is forgotten.
    pub fn reset(&self) {
        self.loaded.store(false, Ordering::Relaxed);
        *lock(&self.calendar) = Calendar::default();
        lock(&self.anchors).clear();
        lock(&self.queue).clear();
        *lock(&self.seed) = None;
    }

    /// Reads the calendar and the anchors from the save, once for each world. Called
    /// with the overworld in place.
    fn ensure_loaded(&self, host: &mut dyn Host) {
        if self.loaded.swap(true, Ordering::Relaxed) {
            return;
        }
        if let Ok(Some(calendar)) = CALENDAR.get_existing(&mut *host, Holder::Level) {
            *lock(&self.calendar) = calendar;
        }
        if let Ok(Some(anchors)) = ANCHORS.get_existing(&mut *host, Holder::Level) {
            *lock(&self.anchors) = anchors;
        }
        *lock(&self.seed) = host.world_seed();
    }

    fn epoch(&self) -> i64 {
        lock(&self.calendar).epoch
    }

    /// Enough players slept through the night in the overworld.
    pub fn slept(&self, host: &mut dyn Host, config: &Config) {
        self.ensure_loaded(host);
        let (turned, calendar) = {
            let mut calendar = lock(&self.calendar);
            (calendar.slept(config), *calendar)
        };
        let _ = CALENDAR.set(&mut *host, Holder::Level, calendar);
        if !turned {
            // Not yet: say how many more nights it will take.
            if config.enabled {
                let left = config.sleeps_per_shift.saturating_sub(calendar.sleeps);
                let line = nights_left(left);
                for player in host.server_players() {
                    if player.dimension == OVERWORLD {
                        host.action_bar(player.uuid, Text::colored(line.clone(), 0xE8C36A));
                    }
                }
            }
            return;
        }
        self.prune_anchors(host);
        let message = Message {
            shift: true,
            epoch: calendar.epoch,
            shifts_per_year: config.shifts_per_year,
        };
        let mut centres = Vec::new();
        let mut view = 2;
        for player in host.server_players() {
            if host
                .net_send_to_player(player.uuid, CHANNEL, &message.encode())
                .is_err()
            {
                // A client without the mod gets the game's own title.
                let title = Text::colored("Biome Shift", 0xE8C36A);
                let subtitle = Text::literal(subtitle(calendar.epoch, config.shifts_per_year));
                host.title(player.uuid, &title, &subtitle, 15, 60, 25);
            }
            if player.dimension == OVERWORLD {
                let [x, _, z] = player.position;
                centres.push([(x.floor() as i32) >> 4, (z.floor() as i32) >> 4]);
                view = view.max(
                    host.player_view_distance(player.uuid)
                        .unwrap_or(10)
                        .clamp(2, 32),
                );
            }
        }
        // The land around the players first, while their screens are dark; then
        // everything else they can see.
        let mut queue = lock(&self.queue);
        for chunk in chunks_around(&centres, view) {
            queue.later(chunk);
        }
        queue.first(&chunks_around(&centres, config.priority_radius));
    }

    /// The overworld's tick ended: convert the next few chunks that wait.
    pub fn tick(&self, host: &mut dyn Host, config: &Config) {
        self.ensure_loaded(host);
        if !config.enabled || lock(&self.queue).is_empty() {
            return;
        }
        let Some(seed) = *lock(&self.seed) else {
            return;
        };
        let epoch = self.epoch();
        let anchors = lock(&self.anchors).clone();
        for _ in 0..config.chunks_per_tick {
            // The lock is not held while the chunk is converted: an edit can fire
            // other hooks, this mod's `chunk_load` among them.
            let Some(chunk) = lock(&self.queue).pop() else {
                break;
            };
            convert(host, config, chunk, epoch, seed, &anchors);
        }
    }

    /// A chunk came into the world: it may be an epoch behind.
    pub fn chunk_loaded(&self, chunk: [i32; 2]) {
        // Before the first shift every chunk is as it should be.
        if self.loaded.load(Ordering::Relaxed) && self.epoch() != 0 {
            lock(&self.queue).later(chunk);
        }
    }

    /// A player joined: tell their client the season.
    pub fn joined(&self, host: &mut dyn Host, config: &Config, player: u128) {
        let message = Message {
            shift: false,
            epoch: self.epoch(),
            shifts_per_year: config.shifts_per_year,
        };
        let _ = host.net_send_to_player(player, CHANNEL, &message.encode());
    }

    /// A climate anchor was placed at `pos` in the overworld.
    pub fn anchor_placed(&self, host: &mut dyn Host, pos: [i32; 3]) {
        self.ensure_loaded(host);
        let anchors = {
            let mut anchors = lock(&self.anchors);
            if anchors.contains(&pos) {
                return;
            }
            anchors.push(pos);
            anchors.clone()
        };
        let _ = ANCHORS.set(&mut *host, Holder::Level, anchors);
    }

    /// The block at `pos` in the overworld was broken: an anchor there lets go.
    pub fn block_broken(&self, host: &mut dyn Host, pos: [i32; 3]) {
        let anchors = {
            let mut anchors = lock(&self.anchors);
            let before = anchors.len();
            anchors.retain(|anchor| *anchor != pos);
            if anchors.len() == before {
                return;
            }
            anchors.clone()
        };
        let _ = ANCHORS.set(&mut *host, Holder::Level, anchors);
    }

    /// Forgets anchors that are no longer there (blown up, pushed by a piston). One
    /// in a chunk that isn't loaded can't be checked, and is kept.
    fn prune_anchors(&self, host: &mut dyn Host) {
        let Some(level) = host.level(OVERWORLD) else {
            return;
        };
        let known = lock(&self.anchors).clone();
        let standing: Vec<[i32; 3]> = known
            .iter()
            .filter(|pos| {
                host.level_block(level, **pos)
                    .is_none_or(|block| block == ANCHOR)
            })
            .copied()
            .collect();
        if standing.len() != known.len() {
            *lock(&self.anchors) = standing.clone();
            let _ = ANCHORS.set(&mut *host, Holder::Level, standing);
        }
    }
}

/// The line under "Biome Shift": the season the climate turned to.
/// What a player reads on waking when the climate hasn't turned yet.
pub fn nights_left(left: u32) -> String {
    match left {
        1 => "The climate turns after one more night".to_owned(),
        n => format!("The climate turns after {n} more nights"),
    }
}

pub fn subtitle(epoch: i64, shifts_per_year: u32) -> String {
    format!(
        "{}, year {}",
        season(epoch, shifts_per_year).name(),
        year(epoch, shifts_per_year)
    )
}

/// Brings one chunk of the overworld to `epoch`: its biomes, then its ground.
fn convert(
    host: &mut dyn Host,
    config: &Config,
    chunk: [i32; 2],
    epoch: i64,
    seed: i64,
    anchors: &[[i32; 3]],
) {
    if !host.chunk_is_loaded(OVERWORLD, chunk) {
        // It is converted when it next loads.
        return;
    }
    let saved = CHUNK
        .get_existing(&mut *host, Holder::Chunk(chunk))
        .ok()
        .flatten()
        .flatten();
    if saved.as_ref().is_some_and(|state| state.epoch == epoch) {
        return;
    }
    let (origin_x, origin_z) = (chunk[0] * 16, chunk[1] * 16);
    // The middle of each quart column.
    let centres: Vec<(i32, i32)> = (0..QUARTS as i32)
        .map(|quart| {
            (
                origin_x + (quart % 4) * 4 + 2,
                origin_z + (quart / 4) * 4 + 2,
            )
        })
        .collect();
    let Ok(heights) =
        host.surface_heights(OVERWORLD, &centres, NativeHeightmap::MotionBlockingNoLeaves)
    else {
        return;
    };
    let Some(heights) = heights.into_iter().collect::<Option<Vec<i32>>>() else {
        return;
    };
    let mut state = saved.unwrap_or_else(|| ChunkState {
        epoch: 0,
        base: centres
            .iter()
            .zip(&heights)
            .map(|(&(x, z), height)| {
                host.biome([x, *height, z])
                    .unwrap_or_else(|| UNKNOWN_BIOME.into())
            })
            .collect(),
        applied: vec![0; QUARTS],
    });

    // Where each quart column's biome goes.
    let (min_y, level_height) = host.world_height();
    let top_quart = (min_y + level_height - 1) >> 2;
    let low_quart = (heights.iter().copied().min().unwrap_or(min_y) - BIOME_DEPTH).max(min_y) >> 2;
    let mut biomes = Vec::new();
    let mut profiles = Vec::with_capacity(QUARTS);
    for (quart, &(x, z)) in centres.iter().enumerate() {
        let base = state.base[quart].as_str();
        let was = i32::from(state.applied[quart]);
        // An anchor keeps the land as it is, whatever the climate does.
        let now = if config.anchored(anchors, x, z) {
            was
        } else {
            config.steps_at(seed, x, z, epoch)
        };
        let (old, new) = (shifted(base, was), shifted(base, now));
        profiles.push((profile(old), profile(new)));
        if old != new {
            for quart_y in low_quart..=top_quart {
                biomes.push(([x >> 2, quart_y, z >> 2], new.to_owned()));
            }
        }
        state.applied[quart] = now as i8;
    }
    if !biomes.is_empty()
        && let Err(error) = host.set_biomes(OVERWORLD, &biomes)
    {
        host.log(
            "WARN",
            &format!("[hz_seasons] chunk {} {}: {error}", chunk[0], chunk[1]),
        );
        return;
    }

    // The ground follows, where a biome's profile changed.
    if profiles.iter().any(|(old, new)| old != new) {
        follow(host, config, chunk, epoch, seed, &profiles);
    }
    state.epoch = epoch;
    let _ = CHUNK.set(&mut *host, Holder::Chunk(chunk), Some(state));
}

/// Changes the ground of `chunk` to suit its new biomes: `profiles` are each quart
/// column's old and new profile.
fn follow(
    host: &mut dyn Host,
    config: &Config,
    chunk: [i32; 2],
    epoch: i64,
    seed: i64,
    profiles: &[(
        hz_seasons_core::biomes::Profile,
        hz_seasons_core::biomes::Profile,
    )],
) {
    let (origin_x, origin_z) = (chunk[0] * 16, chunk[1] * 16);
    let columns: Vec<(i32, i32)> = (0..256)
        .map(|column| (origin_x + column % 16, origin_z + column / 16))
        .collect();
    let range = (BELOW as u32, ABOVE as u32);
    let Ok(read) = host.read_columns(
        OVERWORLD,
        &columns,
        NativeHeightmap::MotionBlockingNoLeaves,
        range,
    ) else {
        return;
    };
    let per_column = BELOW + ABOVE;
    let mut edit = NativeBlockEdit::default();
    let index_of = |list: &mut Vec<String>, entry: &str| -> u32 {
        list.iter()
            .position(|known| known == entry)
            .unwrap_or_else(|| {
                list.push(entry.to_owned());
                list.len() - 1
            }) as u32
    };
    let mut states: Vec<&str> = Vec::with_capacity(per_column);
    let mut edits: Vec<Edit> = Vec::new();
    for (column, &(x, z)) in columns.iter().enumerate() {
        let Some(height) = read.bases[column] else {
            continue;
        };
        let (old, new) = &profiles[((z - origin_z) / 4 * 4 + (x - origin_x) / 4) as usize];
        if old == new {
            continue;
        }
        states.clear();
        for block in &read.blocks[column * per_column..(column + 1) * per_column] {
            states.push(
                read.palette
                    .get(usize::from(*block))
                    .map_or("minecraft:air", String::as_str),
            );
        }
        // The same column draws the same lot at an epoch, however often it is asked.
        let roll = noise::hash(
            seed as u64 ^ (epoch as u64).wrapping_mul(0x9E37_79B9),
            i64::from(x),
            i64::from(z),
        );
        ground::plan(old, new, &states, roll, &config.features, &mut edits);
        for change in &edits {
            edit.runs.push(NativeBlockRun {
                x,
                z,
                y: height + change.dy,
                height: 1,
                surface: None,
                block: index_of(&mut edit.palette, change.block),
                only: change
                    .only
                    .map(|guard| index_of(&mut edit.filters, guard.predicate())),
            });
        }
    }
    if edit.runs.is_empty() {
        return;
    }
    if let Err(error) = host.edit_blocks(OVERWORLD, &edit) {
        host.log(
            "WARN",
            &format!("[hz_seasons] chunk {} {}: {error}", chunk[0], chunk[1]),
        );
    }
}
