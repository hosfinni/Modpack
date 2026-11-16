//! Seeded 2D simplex noise from integer hashing. Only `+ - * /` and `floor` are
//! used, which IEEE 754 defines exactly, so the same seed gives the same climate on
//! every machine.

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

/// A sine built from straight lines and a cubic, so it is the same on every
/// machine: 0 at 0, 1 a quarter of the way through `period`, 0 half way, -1 at three
/// quarters.
pub fn sine(x: f64, period: f64) -> f64 {
    let phase = (x / period - 0.25).rem_euclid(1.0);
    let ramp = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    let s = ramp * ramp * (3.0 - 2.0 * ramp);
    1.0 - 2.0 * s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sine_hits_its_quarter_points() {
        assert!(sine(0.0, 8.0).abs() < 1e-12);
        assert_eq!(sine(2.0, 8.0), 1.0);
        assert!(sine(4.0, 8.0).abs() < 1e-12);
        assert_eq!(sine(6.0, 8.0), -1.0);
        assert!(sine(8.0, 8.0).abs() < 1e-12);
        assert!(sine(1.0, 8.0) > 0.4 && sine(1.0, 8.0) < 1.0);
    }

    #[test]
    fn simplex_is_seeded_and_in_range() {
        assert_eq!(simplex(1, 0.3, 0.7), simplex(1, 0.3, 0.7));
        assert_ne!(simplex(1, 0.3, 0.7), simplex(2, 0.3, 0.7));
        for i in 0..5000 {
            let n = simplex(5, i as f64 * 0.137, i as f64 * 0.071);
            assert!((-1.0..=1.0).contains(&n));
        }
    }
}
