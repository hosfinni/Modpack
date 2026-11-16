//! Seeded 2D noise from integer hashing and simplex gradients.
//!
//! Only `+ - * /`, `floor` and `sqrt` are used, which IEEE 754 defines exactly, so a
//! seed gives the same terrain on every machine: a client drawing far terrain ahead
//! of its server agrees with the chunks the server then sends.

/// A well mixed 64-bit hash of a lattice point under `seed` (SplitMix64's finalizer).
pub fn hash(seed: u64, x: i64, z: i64) -> u64 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as u64)
            .wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            .rotate_left(31);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

/// A hash as a fraction in 0..1.
pub fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// A new seed for an independent layer of the same world.
pub fn derive(seed: u64, layer: u64) -> u64 {
    hash(seed, layer as i64, 0x5EED)
}

const F2: f64 = 0.366_025_403_784_438_6;
const G2: f64 = 0.211_324_865_405_187_13;
const DIAGONAL: f64 = std::f64::consts::FRAC_1_SQRT_2;
const GRADIENTS: [(f64, f64); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (DIAGONAL, DIAGONAL),
    (-DIAGONAL, DIAGONAL),
    (DIAGONAL, -DIAGONAL),
    (-DIAGONAL, -DIAGONAL),
];

/// Simplex noise in about -1..1, smooth, with features about one unit across.
pub fn simplex(seed: u64, x: f64, z: f64) -> f64 {
    let skew = (x + z) * F2;
    let (i, j) = ((x + skew).floor(), (z + skew).floor());
    let unskew = (i + j) * G2;
    let (x0, z0) = (x - (i - unskew), z - (j - unskew));
    let (i1, j1) = if x0 > z0 { (1.0, 0.0) } else { (0.0, 1.0) };
    let (x1, z1) = (x0 - i1 + G2, z0 - j1 + G2);
    let (x2, z2) = (x0 - 1.0 + 2.0 * G2, z0 - 1.0 + 2.0 * G2);
    let (i, j) = (i as i64, j as i64);
    let corner = |dx: f64, dz: f64, ci: i64, cj: i64| {
        let t = 0.5 - dx * dx - dz * dz;
        if t <= 0.0 {
            return 0.0;
        }
        let (gx, gz) = GRADIENTS[(hash(seed, ci, cj) & 7) as usize];
        let t2 = t * t;
        t2 * t2 * (gx * dx + gz * dz)
    };
    70.0 * (corner(x0, z0, i, j)
        + corner(x1, z1, i + i1 as i64, j + j1 as i64)
        + corner(x2, z2, i + 1, j + 1))
}

/// `octaves` layers of simplex noise, each twice as fine and half as strong, in
/// about -1..1.
pub fn fbm(seed: u64, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut sum, mut amplitude, mut frequency, mut total) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += amplitude
            * simplex(
                derive(seed, u64::from(octave)),
                x * frequency,
                z * frequency,
            );
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    sum / total
}

/// Ridged noise in 0..1: sharp crests where the layers cross zero, for mountain
/// ridges and river courses.
pub fn ridged(seed: u64, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut sum, mut amplitude, mut frequency, mut total) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        let n = simplex(
            derive(seed, 100 + u64::from(octave)),
            x * frequency,
            z * frequency,
        );
        let crest = 1.0 - n.abs();
        sum += amplitude * crest * crest;
        total += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    sum / total
}

/// 0 below `low`, 1 above `high`, an S curve between.
pub fn smoothstep(low: f64, high: f64, x: f64) -> f64 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A smooth bump: 1 at `center`, 0 at `half_width` away and beyond.
pub fn bell(x: f64, center: f64, half_width: f64) -> f64 {
    let d = (x - center) / half_width;
    let inside = (1.0 - d * d).max(0.0);
    inside * inside
}

/// A wave from -1 to 1 and back over `period`, 1 at 0: a cosine built from straight
/// lines and a cubic, so it is the same on every machine.
pub fn wave(x: f64, period: f64) -> f64 {
    let phase = (x / period).rem_euclid(1.0);
    let ramp = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    1.0 - 2.0 * smoothstep(0.0, 1.0, ramp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_and_point_give_the_same_value() {
        assert_eq!(simplex(7, 12.25, -3.5), simplex(7, 12.25, -3.5));
        assert_ne!(simplex(7, 12.25, -3.5), simplex(8, 12.25, -3.5));
        assert_eq!(hash(1, 2, 3), hash(1, 2, 3));
        assert_ne!(hash(1, 2, 3), hash(1, 3, 2));
    }

    #[test]
    fn simplex_stays_in_range_and_is_centred() {
        let (mut low, mut high, mut sum) = (f64::MAX, f64::MIN, 0.0);
        let mut count = 0.0;
        for i in 0..200 {
            for j in 0..200 {
                let n = simplex(42, i as f64 * 0.173, j as f64 * 0.219);
                low = low.min(n);
                high = high.max(n);
                sum += n;
                count += 1.0;
            }
        }
        assert!(low >= -1.0 && high <= 1.0, "{low}..{high}");
        assert!(low < -0.7 && high > 0.7, "uses its range: {low}..{high}");
        assert!((sum / count).abs() < 0.05);
    }

    #[test]
    fn simplex_is_continuous() {
        let step = 1e-3;
        for i in 0..2000 {
            let x = i as f64 * 0.37;
            let delta = (simplex(3, x, x * 0.5) - simplex(3, x + step, x * 0.5)).abs();
            assert!(delta < 0.01, "a jump of {delta} at {x}");
        }
    }

    #[test]
    fn the_shaping_curves_hit_their_ends() {
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
        assert_eq!(bell(5.0, 5.0, 2.0), 1.0);
        assert_eq!(bell(7.0, 5.0, 2.0), 0.0);
        assert_eq!(bell(9.0, 5.0, 2.0), 0.0);
        assert_eq!(wave(0.0, 100.0), 1.0);
        assert_eq!(wave(50.0, 100.0), -1.0);
        assert_eq!(wave(100.0, 100.0), 1.0);
        assert_eq!(wave(-50.0, 100.0), -1.0);
        assert!(wave(25.0, 100.0).abs() < 1e-12);
    }

    #[test]
    fn ridged_noise_is_a_fraction() {
        for i in 0..1000 {
            let r = ridged(9, i as f64 * 0.11, i as f64 * 0.07, 4);
            assert!((0.0..=1.0).contains(&r));
        }
    }
}
