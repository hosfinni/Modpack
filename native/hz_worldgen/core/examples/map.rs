// Draws a top-down map of a seed's terrain, to look at a config before playing it:
//
//   cargo run --release -p hz-worldgen-core --example map -- <seed> <blocks per pixel> <out.ppm>
//
// The picture is 1024 pixels square and centred on 0 0: biome colours, shaded by
// the slope. PPM opens in most image viewers; `python3 tools/ppm_to_png.py` converts
// it.

use std::io::Write;

use hz_worldgen_core::{Biome, Config, World};

const SIZE: i32 = 1024;

fn colour(biome: Biome) -> [f64; 3] {
    use Biome::*;
    let rgb = match biome {
        DeepFrozenOcean => 0x3A4A7A,
        DeepColdOcean => 0x1F3A7A,
        DeepOcean => 0x1A2F8C,
        DeepLukewarmOcean => 0x1A4F9C,
        FrozenOcean => 0x8CA3D6,
        ColdOcean => 0x2F56B0,
        Ocean => 0x2A48C8,
        LukewarmOcean => 0x2F78D0,
        WarmOcean => 0x35A6D6,
        SnowyBeach => 0xE8E4D0,
        Beach => 0xE6D89A,
        StonyShore => 0x8A8A8A,
        FrozenRiver => 0xA8C0F0,
        River => 0x3F76E4,
        FrozenPeaks => 0xF2F6FF,
        JaggedPeaks => 0xDCE2EC,
        StonyPeaks => 0x9A9A96,
        SnowySlopes => 0xE2EAF2,
        Grove => 0xB8CFC2,
        Meadow => 0x86C06A,
        WindsweptHills => 0x7C8C74,
        SnowyPlains => 0xEEF2F2,
        SnowyTaiga => 0xB4CBC0,
        Taiga => 0x3F7358,
        OldGrowthSpruceTaiga => 0x34604A,
        Plains => 0x8DB360,
        Forest => 0x3F8A32,
        BirchForest => 0x5E9A52,
        DarkForest => 0x2C5A22,
        Swamp => 0x4C6B46,
        Savanna => 0xB5A84E,
        SavannaPlateau => 0xA59848,
        Jungle => 0x2E9A1E,
        SparseJungle => 0x5CB038,
        MangroveSwamp => 0x4E7A3A,
        Desert => 0xE8D28A,
        Badlands => 0xC8703A,
    };
    [
        f64::from((rgb >> 16) & 255),
        f64::from((rgb >> 8) & 255),
        f64::from(rgb & 255),
    ]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: i64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let scale: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(16);
    let out = args.next().unwrap_or_else(|| "map.ppm".into());
    let world = World::new(seed, Config::shipped());
    let sea = world.config().sea_level;
    let at = |px: i32, pz: i32| world.column((px - SIZE / 2) * scale, (pz - SIZE / 2) * scale);
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 3) as usize);
    for pz in 0..SIZE {
        for px in 0..SIZE {
            let column = at(px, pz);
            // Light from the north-west: a column higher than its lit neighbours is bright.
            let (west, north) = (at(px - 1, pz).height, at(px, pz - 1).height);
            let land = column.height >= sea;
            let slope = if land {
                f64::from(2 * column.height - west - north) / f64::from(scale) * 2.2
            } else {
                0.0
            };
            let depth = if land {
                1.0
            } else {
                1.0 - f64::from(sea - column.height).min(50.0) / 140.0
            };
            let light = (1.0 + slope).clamp(0.45, 1.5) * depth;
            for channel in colour(column.biome) {
                pixels.push((channel * light).clamp(0.0, 255.0) as u8);
            }
        }
    }
    let mut file = std::fs::File::create(&out).expect("the output file");
    write!(file, "P6\n{SIZE} {SIZE}\n255\n").unwrap();
    file.write_all(&pixels).unwrap();
    println!("{out}: seed {seed}, {} blocks across", SIZE * scale);
}
