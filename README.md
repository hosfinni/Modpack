# Horizon Hardship

A modpack for [Horizon](https://github.com/horizon-rust/horizon), the Rust rewrite of
Minecraft: Java Edition 26.3. Survival is harder, the world moves under you, and a
group does better than the same players alone.

It is seven parts: five native mods written in Rust, a content pack (data and
textures, no code) and a shader pack.

| Part | Kind | What it does |
| --- | --- | --- |
| `hz_survival` | native mod | **Thirst** (a second bar that empties far more slowly than hunger; drinks and some foods fill it), **skills** that level faster in company, **party bonuses**, **tougher monsters that fight as a pack**, and **ore that must be refined** |
| `hz_seasons` | native mod | **Biome Shift**: when the players sleep, the climate turns and biomes move; the screen goes black behind a banner while the land around the players is remade |
| `hz_worldgen` | native mod | **Tectonic terrain**: continents on plates, ranges where plates collide, climate by latitude and height |
| `hz_arsenal` | native mod | **Eight weapons, eight movesets**: each has its own combo, openers and weapon art |
| `hz_travel` | native mod | **No easy travel**: no elytra flight, no riptide launches, no ice-boat highways, short ender pearls |
| `hz_content` | content pack | The items, blocks and textures the mods use: drinks, weapons, crushed and washed ore, the climate anchor |
| `hz_softlight` | shader pack | **Better light, vanilla look**: warm lamps, cool sky, coloured light, sun glow and light shafts, in one cheap pass |

## Status

Version 0.1.0 was tried in the game and only its terrain appeared, most likely because
it was built on a newer Horizon than its pin, where the launcher left the other mods
out (below). 0.1.1 is pinned to the
2026-11-13 Nightly and keeps caves and structures out of the tectonic seas. 0.1.1 has
**not been run in the game**: it was built in an environment with no copy of Minecraft
and no GPU. What was checked, on the pinned Nightly (`9f42064`):

| Check | Result |
| --- | --- |
| Rules of every mod (`cargo test`) | 110 tests pass |
| Lints (`cargo clippy -- -D warnings`) on the rule crates | clean |
| Every mod type-checked against Horizon's real game crate at the pinned commit | compiles |
| Every hook the mods implement is dispatched to trait mods there, and every `Host` helper they call is implemented | yes |
| The bundles run through `horizon-mixins apply` at the launcher's own revision (`5a64af9`) | ok |
| Horizon's registry check (`cargo horizon-mod check`) on all seven packed files | pass |
| The shader pack checked by Horizon's `horizon-shader check` (shader API 0.1, which this Nightly draws without a switch) | pass |
| The terrain looked at as rendered maps | see `docs/worldgen.md` |

On Horizon's `main` of 2026-11-16 four of the mods don't compile (the mod API moved out
of `horizon_client`), which is what a build there leaves out.

Expect bugs the first time it runs. The numbers (thirst rates, move damage, monster
toughness, shift sizes) are first guesses, all in config files.

### Pinned to the 2026-11-13 Nightly

Native mods build on one exact Horizon commit. The pack pins the Nightly of 2026-11-13
(`9f42064`, tag `nightly-source/2026-11-13-9f42064`), and the launcher builds every mod
on that commit whatever Horizon version is installed. Build on it, not on a newer
`main`: when a mod doesn't compile on the commit a build uses, the launcher leaves it out
and builds the rest, so a build on another commit can quietly end up with only some of
the pack. The Mods page and `native-mods.json` (`left_out`) say which mods were left
out and why. To move the pack to another commit, run
`python3 tools/repin.py <commit> --hooks-api <N>`, then `python3 tools/horizon.py check`.

### Known limits

- **Villages and temples on tectonic terrain.** Vanilla still places structures where
  its own terrain would have had them. Where vanilla had land and the tectonic terrain
  has sea (or the other way round), the chunk gets no structures, so no village stands
  in the sea and no monument on land; a structure crossing such a coast is cut off
  there, and so can a stronghold under the sea be. On land, surface structures can
  still float or sit buried. Set `structures = false` in `config/hz_worldgen.toml` to
  have none (strongholds included), or `terrain = "vanilla"` to keep vanilla's terrain.
- **Tectonic caves** are vanilla's carved caves only: no noise caves, no aquifers, and
  none in a chunk with sea, river or lake water in it or beside it (vanilla's carvers
  would cut dry holes through the water). `fit_to_sea = false` brings back vanilla's
  caves and structures everywhere.
- **Ores are harder to refine, not rarer.** Smelting raw ore gives nuggets; a whole
  ingot takes crushing, washing and smelting. Ore veins are generated as in vanilla.
- **Weapon moves have no animations of their own.** A move shows as its name over the
  hotbar, particles and sounds. Against another player a move changes damage and
  knockback, but its stagger and area need a mob target.
- **Not built from the plan:** reviving downed players, and the two-person forge.
- **Thirst** is filled from bottles and food only; you can't drink from a river.
- **Art is placeholder**: generated pixel art, good enough to tell items apart.

## Install

1. Install Horizon with its launcher, on the Nightly of 2026-11-13 (see above). Build
   the native mods on it, not with **Rebuild on Horizon** onto a newer version.
2. Build the files: `python3 tools/package.py` writes them to `dist/`.
3. Put `hz_content-*.hmod` in the instance's `horizon-mods/` folder.
4. Put the five `.hnmod` files in `horizon-native-mods/`. The launcher compiles native
   mods into the game: it asks you to trust them first, because native mods are
   unsandboxed code. Read the sources before you do.
5. Put `hz_softlight-*.hshader` in `shaderpacks/` and pick it in the game's shader
   options.
6. For a dedicated server, export one from the launcher with the same mods. Server and
   clients need the same versions of every part. Horizon checks the content pack when
   a player joins; it does not check native mods yet, so keep those in step yourself.

Each mod writes its config to the game's `config/` folder on first run
(`hz_survival.toml`, `hz_seasons.toml`, `hz_worldgen.toml`, `hz_arsenal.toml`,
`hz_travel.toml`). The server's files decide the rules.

## What each part does

### Thirst (`hz_survival`)

A row of ten droplets above the food bar. A full bar lasts 100 minutes standing still,
50 sprinting, about 28 sprinting through a desert; vanilla's food bar lasts about 12
minutes of sprinting. Heat, the Nether and fire drain it faster; cold and water slow it.

- Water bottles, potions, milk, stews, melon, berries and apples fill it. Dried kelp,
  rotten flesh and pufferfish take from it.
- Bottled water is untreated and can bring on Hunger. Boil it (smelt a water bottle)
  for purified water, or make cactus water or juice.
- Low thirst slows you, then weakens you and stops slow healing. An empty bar hurts,
  but never takes the last half heart unless `lethal = true`.

### Skills, parties and refining (`hz_survival`)

- **Skills**: Mining (breaking ore), Smithing (refining), Combat (killing monsters).
  `/skills` shows them. A novice digs ore at 0.7 of vanilla's speed; level 20 at 1.3.
- **Company pays**: experience earned near other players is worth 20% more for each
  one (up to three), and each of them gets half of it too. Four players working
  together earn about three times what four soloists do.
- **Strength in numbers**: each player near you (up to three) adds 10% to your damage
  and takes 8% off what you are dealt.
- **Monsters** take 1.4 times the blows and hit 1.25 times as hard. One without a
  target joins a fight near it. In company they come at you from the sides. A badly
  hurt one alone backs off.
- **Refining**: raw ore, stonecutter, crushed ore, used on a water cauldron, washed
  ore, furnace, ingot. Smelting raw ore directly gives three nuggets.

### Biome Shift (`hz_seasons`)

Every third night slept through, the climate turns. Each biome sits on a ladder from
cold to hot (snowy plains, plains, savanna, desert; snowy taiga, taiga, forest, jungle;
and so on for oceans, rivers, beaches and peaks), and a shift moves it some steps:

- The **season** swings the whole world: warmer toward summer, colder toward winter.
- A **drifting pattern** of warm and cold patches wanders across the map, so no two
  years are alike.
- The **ground follows**: grass turns to sand under a new desert and back, water
  freezes and thaws, snow settles and melts, plants change, saplings take root in new
  forest, and leaves fall from trees a desert overtakes. Only natural blocks are
  touched, so builds stand.
- A place always returns to the biome it was generated with when its climate returns
  to normal: shifting does not wear the world away.
- The work is spread over ticks (four chunks a tick, nearest players first). Chunks
  nobody has loaded are converted when they next load. Nothing runs between shifts.
- A **climate anchor** block keeps the land within 48 blocks of it as it is.
- Grass and leaves take the season's tint. The shader pack reads the season too.

### Tectonic terrain (`hz_worldgen`)

See `docs/worldgen.md`, which also covers running the real Lithosphere data pack
instead.

### Weapons (`hz_arsenal`)

No new ores: every weapon is iron, crafted at a crafting table. What sets them apart
is how they fight.

| Weapon | Combo | Openers | Use (weapon art) |
| --- | --- | --- | --- |
| Greatsword | Cleave, Cleave, Overhead (staggers) | sprinting: Charging Thrust; falling: Plunging Cut (area) | Guard: blows hurt less, then Guard Break |
| Katana | Slash, Slash, Cross Cut, Flourish | sprinting: Dash Cut; sneaking from rest: Iai Draw | Counter Stance: turn a blow, then Counter for double |
| Dagger | Stab, Stab, Twist | from behind: Backstab (2.2x); sneaking: Ambush | Shadow Step: dash, untouchable for a moment |
| Glaive | Thrust, Thrust, Wide Sweep (area) | sprinting: Lancing Charge | Whirl: hits everything around you |
| Warhammer | Smash, Smash, Earthshaker (area), all stagger | falling: Meteor (area) | Ground Slam: throws everything near into the air |
| Rapier | Thrust, stronger with each hit on one target | sprinting: Lunge | Riposte Stance: turn a blow and strike back |
| Halberd | Chop, Hook (pulls), Cleave | sprinting: Impale | Hook and Drag: pull a target from 5 blocks |
| Chain Whip | Lash, Lash, Crack | sneaking: Snare | Entangle: hold a target from 7 blocks |

### No easy travel (`hz_travel`)

- Elytra: the glider component is removed from every item, so nothing flies.
- Riptide tridents cannot be charged.
- Ender pearls carry at most 12 blocks.
- Boats are held to 0.45 blocks a tick, which ends ice roads; minecarts to vanilla's
  top speed; happy ghasts to 0.3.

"Hyperloops" was read as fast travel built from vanilla parts. If you meant a
particular mod, say which and the rules can cover it.

### Light (`hz_softlight`)

One material stage and one full-screen pass, with no shadow map. Every effect has a
switch in the pack's options.

## Development

```bash
cargo test
```

Runs the rules of every mod. Needs nothing but Rust.

```bash
python3 tools/horizon.py check
```

Type-checks every mod against Horizon. Needs a Horizon checkout at the pinned commit
beside this folder (`../horizon`), or `--horizon PATH`.

```bash
cargo run --release -p hz-worldgen-core --example map -- 2026 24 map.ppm
```

Draws a seed's terrain: seed, blocks per pixel, output file.
`python3 tools/ppm_to_png.py map.ppm map.png` converts it.

```bash
python3 tools/make_content.py
```

Rewrites the content pack's data files and textures from the tables in that script.

```bash
python3 tools/package.py --check
```

Packs everything into `dist/` and runs Horizon's own checks on each file. `--check`
needs Horizon's `cargo-horizon-mod`.

Layout: each native mod is `native/<id>/`, with its rules in `core/` (no game types,
unit-tested) and its hooks into Horizon in `api/`. `docs/hook-inventory.md` lists every
Horizon hook the pack uses and what for.
