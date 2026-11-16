# Hook inventory

Every part of Horizon's native mod API the pack uses, and what for. Horizon's
`docs/native-mods.md` and `native-hooks.json` are the reference; the numbers there
(the hook API a helper arrived in) are only a guide: the pack pins the 2026-11-13
Nightly (hook API 415), which has every hook and helper below, as
`tools/horizon.py check` shows by compiling the mods against it.

All five mods are *trait mods*: each `native/<id>/api` crate exports
`pub fn hooks() -> Box<dyn Hooks>`, and the launcher links it into the game. None of
them patches the game's source, so there are no mixin injections to break when Horizon
moves a function. `hz_survival` is the one exception to "API only": its monster goals
use the game's own mob types (`horizon_client::mobs`), which Horizon documents for mob
goals but does not version.

## Shared by every mod

| Hook or helper | Use |
| --- | --- |
| `register_configs`, `config_loading`, `config_reloading` | Each mod's `config/<id>.toml`, written with its defaults when missing and read again when edited |
| `server_about_to_start` | Forget what was kept for the last world |
| `Host::log` | Say when a config file does not parse |

## hz_survival

| Hook or helper | Side | Use |
| --- | --- | --- |
| `player_tick` | server | Thirst drains once a second; a player's place is noted for party range |
| `living_entity_use_item_finish` | server | Eating or drinking changes thirst |
| `living_heal` | server | Slow healing stops while dehydrated |
| `player_respawned`, `player_logged_out` | server | Thirst resets on respawn and is saved on leaving |
| `Host::biome`, `entity_fire_ticks`, `entity_is_in_water`, `level_difficulty` | server | What drains thirst faster or slower |
| `Host::add_effect`, `hurt_entity` | server | Thirst's debuffs and damage |
| `AttachmentType` on `Holder::Player` | server | Thirst and skills saved with the player |
| `net_send_to_player`, `net_from_server` | both | The thirst bar and the player's own skill levels reach their client |
| `health_hud`, `hud`, `Host::sprite`, `Host::fill` | client | The bottle row above the food bar (drawn pixel by pixel when the textures are missing) |
| `tooltip` | client | What an item does to thirst |
| `destroy_block`, `mob_killed`, `furnace_extract` | server | Mining, Combat and Smithing experience |
| `break_speed` | both | Ore is dug slower or faster by Mining level |
| `living_incoming_damage_v3` | server | Combat level, party bonuses, monster toughness and damage |
| `register_commands`, `perform_command` | server | `/skills` |
| `use_item_on`, `Host::take_items`, `give_stack`, `block_property`, `set_block_property`, `edit_blocks` | server | Washing crushed ore in a cauldron |
| `data_pack_files` | server | Vanilla's ore smelting recipes give nuggets |
| `register_goals`, `Goal`, `Mob`, `Cx` | server | Monsters join a fight, flank and retreat |

## hz_seasons

| Hook or helper | Side | Use |
| --- | --- | --- |
| `sleep_finished_time` | server | A night slept through: the calendar moves on, and maybe the climate |
| `level_tick_post` | server | Convert the next few waiting chunks |
| `chunk_load` | server | A chunk that comes into the world may be an epoch behind |
| `Host::surface_heights`, `biome` | server | The biomes a chunk was generated with |
| `Host::set_biomes` | server | Move a chunk's biomes, and send them to its players |
| `Host::read_columns`, `edit_blocks` | server | Read a chunk's ground and change it to suit |
| `Host::chunk_is_loaded`, `world_height`, `world_seed`, `player_view_distance` | server | Which chunks, how deep, which pattern |
| `AttachmentType` on `Holder::Level` and `Holder::Chunk` | server | The calendar and anchors with the world; each chunk's generated biomes and how far they have moved |
| `block_place`, `destroy_block`, `Host::level_block` | server | Climate anchors placed, broken and checked |
| `player_logged_in`, `net_send_to_player`, `Host::title` | server | Tell clients the epoch; the game's own title for a client without the mod |
| `net_from_server`, `gui_render_post`, `Host::text`, `Host::fill` | client | The black screen and the banner |
| `biome_colors`, `Host::refresh_biome_colors` | client | The season's tint on grass and leaves |
| `shader_params` | client | The season handed to the shader pack as `frame.mods[0]` |

## hz_worldgen

| Hook or helper | Side | Use |
| --- | --- | --- |
| `chunk_generate`, `NativeProtoChunk::set_block`, `set_biomes` | generation workers | Build a chunk's terrain and biomes |
| `lod_column`, `NativeLodColumn::replace` | far terrain workers | What a column will hold, before its chunk exists |

## hz_arsenal

| Hook or helper | Side | Use |
| --- | --- | --- |
| `attack_entity` | server | A swing: choose the move |
| `living_incoming_damage_v3` | server | The move's damage; a stance answers a blow |
| `living_knockback_v2` | server | The move's knockback, or a pull |
| `hurt_enemy` | server | The hit landed: stagger, area, dash |
| `critical_hit` | server | A rapier's critical hits |
| `use_item` | server | The weapon art |
| `server_tick` | server | The tick count; staggered mobs held still; things put off by hooks without a host |
| `Host::held_stack`, `attribute`, `entity_head_yaw`, `entity_view_vector`, `entity_position`, `entity_bounding_box`, `entities_in`, `living` | server | Who holds what, who faces where, who stands near |
| `Host::hurt_entity`, `push_mob`, `mob_stop`, `entity_set_velocity`, `send_particles`, `play_sound`, `action_bar` | server | What a move does |
| `net_send_to_player`, `net_from_server`, `Host::push_player` | both | A dash: the server asks the client to move its own player |
| `tooltip` | client | A weapon's moves |

## hz_travel

| Hook or helper | Side | Use |
| --- | --- | --- |
| `modify_default_components` | both | Take the glider component off every item |
| `living_entity_use_item_start` | both | Refuse to charge a Riptide trident |
| `ender_pearl_teleport`, `chorus_fruit_teleport`, `player_teleport_end_gateway` | server | Shorten or refuse teleports |
| `vehicle_moved_v2` | server | Hold boats, minecarts and mounts to a top speed |
| `tooltip` | client | Say why an elytra or a Riptide trident no longer works |

## hz_vfx

| Hook or helper | Side | Use |
| --- | --- | --- |
| `block_place`, `destroy_block`, `Host::send_particles` | server | Dust where a block is placed, debris where one breaks |
| `hurt_enemy`, `Host::entity`, `entity_position` | server | Sparks of the weapon's kind where a blow lands |
| `explosion_detonate`, `Host::server_players`, `net_send_to_player` | server | Smoke and ash; a shake for the players near |
| `client_tick_post`, `Host::player`, `client_state`, `block`, `biome`, `clock`, `add_particle` | client | Landing and sprint dust, splash, breath, fireflies, embers, cave dust |
| `client_attack`, `Host::client_entities` | client | The swing's trail |
| `client_hurt`, `net_from_server`, `camera_shake` | client | The camera's shake |
| `client_level_load` | client | Forget the player's motion in a new world |

## What Horizon does not offer yet

These would have been used had they existed, and are why some things are as they are:

| Missing | Effect on the pack |
| --- | --- |
| A hook on structure placement, or on the height structures are placed at (`Pipeline::base_height` reads vanilla's noise, in a generic impl horizon-mixins can't inject into) | Structures can't be moved onto the tectonic terrain, or dropped by kind: they float or sit buried |
| A hook on feature placement, or worldgen files from a mod | Ores can't be made rarer from a mod; the pack makes them harder to refine instead |
| A server helper to move a player | Dashes go through a message to the player's own client |
| `hurt_enemy` for a player target | A move's stagger and area need a mob target |
| Attribute modifiers on a content pack's mob effects | The Parched effect is only an icon; vanilla's Slowness and Weakness do the work |
| Native mods in the server's mod-list check | Players and server must keep native mod versions in step by hand |
