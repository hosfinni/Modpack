//! Tectonic plates: the large shape of the world.
//!
//! The map is tiled with plates (a jittered grid of sites, each point belonging to
//! its nearest site). A plate is continental or oceanic and drifts in a direction of
//! its own. Where two plates meet, how they move against each other decides the
//! land: colliding continents fold into mountain ranges, ocean floor sliding under a
//! continent leaves a trench offshore and a coastal range inland, ocean plates
//! colliding raise island arcs, and plates pulling apart open rifts and ridges.
//!
//! Everything here is continuous in the position: the answer never jumps between
//! neighbouring blocks, so there are no walls along plate edges.

use crate::noise::{bell, hash, simplex, unit};

/// How many of the nearest plates fold the land at a point.
const NEAREST: usize = 5;
/// A plate's site keeps this far (in cells) from its grid cell's edges. With it, and
/// with `PAIR_REACH`, every plate that matters at a point is in the 3x3 cells around
/// it, so the answer does not change as the point crosses a cell edge.
const SITE_MARGIN: f64 = 0.3;
/// How far (in cells) past the two nearest plates a plate may be and still fold the
/// land.
const PAIR_REACH: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    /// Its site, in blocks.
    pub site: [f64; 2],
    pub oceanic: bool,
    /// Its own level: above 0 is land.
    pub base: f64,
    /// The direction it drifts in, a unit vector.
    pub drift: [f64; 2],
}

/// The shape of the land at one point, before detail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tectonics {
    /// The level the plates give: above 0 is land, 1 is a high range.
    pub elevation: f64,
    /// How much of `elevation` is folding at a plate edge (0 inside a plate, up to
    /// about 1 in a range, negative in a trench or rift).
    pub uplift: f64,
    /// 0 on a plate edge, 1 well inside a plate.
    pub interior: f64,
}

pub struct Plates {
    seed: u64,
    size: f64,
    ocean_fraction: f64,
    mountain_scale: f64,
}

impl Plates {
    pub fn new(seed: u64, size: f64, ocean_fraction: f64, mountain_scale: f64) -> Self {
        Self {
            seed,
            size,
            ocean_fraction,
            mountain_scale,
        }
    }

    /// How far row `cz` of the grid is shifted sideways, in cells: every other row
    /// by half a cell, like bricks, so that plates come out six-sided rather than
    /// square.
    fn row_shift(cz: i64) -> f64 {
        if cz.rem_euclid(2) == 1 { 0.5 } else { 0.0 }
    }

    /// The plate of grid cell `(cx, cz)`.
    pub fn plate(&self, cx: i64, cz: i64) -> Plate {
        let draw = |salt: i64| {
            unit(hash(
                self.seed ^ (salt as u64).wrapping_mul(0xA24B_AED4_963E_E407),
                cx,
                cz,
            ))
        };
        let site = [
            (cx as f64 + Self::row_shift(cz) + SITE_MARGIN + (1.0 - 2.0 * SITE_MARGIN) * draw(1))
                * self.size,
            (cz as f64 + SITE_MARGIN + (1.0 - 2.0 * SITE_MARGIN) * draw(2)) * self.size,
        ];
        let oceanic = draw(3) < self.ocean_fraction;
        let base = if oceanic {
            -0.62 + 0.24 * draw(4)
        } else {
            0.15 + 0.2 * draw(4)
        };
        // A direction from two draws and a square root, without trigonometry.
        let (mut a, mut b) = (draw(5) * 2.0 - 1.0, draw(6) * 2.0 - 1.0);
        let mut length = (a * a + b * b).sqrt();
        if length < 1e-6 {
            (a, b, length) = (1.0, 0.0, 1.0);
        }
        Plate {
            site,
            oceanic,
            base,
            drift: [a / length, b / length],
        }
    }

    /// The shape of the land at `(x, z)`.
    pub fn sample(&self, x: f64, z: f64) -> Tectonics {
        let cz = (z / self.size).floor() as i64;
        // The nearest plates, nearest first.
        let mut near = [(f64::MAX, self.plate(0, 0)); NEAREST];
        for dz in -1..=1 {
            let row = cz + dz;
            let cx = (x / self.size - Self::row_shift(row)).floor() as i64;
            for dx in -1..=1 {
                let plate = self.plate(cx + dx, row);
                let (px, pz) = (x - plate.site[0], z - plate.site[1]);
                let mut entry = ((px * px + pz * pz).sqrt(), plate);
                for slot in &mut near {
                    if entry.0 < slot.0 {
                        std::mem::swap(&mut entry, slot);
                    }
                }
            }
        }
        let (nearest, second) = (near[0].0, near[1].0);

        // Each plate's own level, blended where plates meet: a coast is a slope, not
        // a step.
        let blend = self.size * 0.06;
        let (mut level, mut weight) = (0.0, 0.0);
        for (distance, plate) in &near {
            let d = (distance - nearest) / blend;
            let w = 1.0 / ((1.0 + d * d) * (1.0 + d * d));
            level += w * plate.base;
            weight += w;
        }
        let base = level / weight;

        // Each pair of nearby plates folds the land along its shared edge. A pair
        // counts in full where its two plates are the two nearest, and fades out as
        // a third plate comes between: an edge ends where a third plate begins.
        let reach = self.size * 0.2;
        let fade = self.size * PAIR_REACH;
        let mut uplift = 0.0;
        for i in 0..NEAREST {
            for j in i + 1..NEAREST {
                let ((di, a), (dj, b)) = (near[i], near[j]);
                let envelope = (1.0 - (di.max(dj) - second) / fade).max(0.0)
                    * (1.0 - (di.min(dj) - nearest) / fade).max(0.0);
                if envelope > 0.0 {
                    uplift += envelope * self.fold(&a, &b, di, dj, reach, x, z);
                }
            }
        }
        let uplift = uplift * self.mountain_scale;
        let interior = (((second - nearest) * 0.5) / reach).clamp(0.0, 1.0);
        Tectonics {
            elevation: base + uplift,
            uplift,
            interior,
        }
    }

    /// What plates `a` and `b` do to the land at a point `da` and `db` from their
    /// sites.
    #[allow(clippy::too_many_arguments)]
    fn fold(&self, a: &Plate, b: &Plate, da: f64, db: f64, reach: f64, x: f64, z: f64) -> f64 {
        // How fast the two close on each other along the line between their sites:
        // above 0 they collide, below 0 they part.
        let (lx, lz) = (b.site[0] - a.site[0], b.site[1] - a.site[1]);
        let length = (lx * lx + lz * lz).sqrt().max(1.0);
        let closing =
            ((a.drift[0] - b.drift[0]) * lx + (a.drift[1] - b.drift[1]) * lz) / length * 0.5;
        // The distance from the shared edge.
        let edge = (da - db).abs() * 0.5;
        // Ranges have peaks and passes along their length.
        let strike = 0.62 + 0.38 * simplex(self.seed ^ 0x51_7A1C, x / 340.0, z / 340.0);
        match (a.oceanic, b.oceanic) {
            (false, false) if closing > 0.0 => 0.95 * closing * strike * bell(edge, 0.0, reach),
            (false, false) => 0.3 * closing * bell(edge, 0.0, reach * 0.5),
            (true, true) if closing > 0.0 => {
                // An island arc: only its highest parts break the surface.
                let islands = simplex(self.seed ^ 0x15_1A2D, x / 260.0, z / 260.0).max(0.0);
                0.75 * closing * islands * bell(edge, 0.0, reach * 0.35)
            }
            (true, true) => -0.14 * closing * bell(edge, 0.0, reach * 0.5),
            _ if closing > 0.0 => {
                // Positive on the continent's side of the edge, negative offshore.
                let inland = if a.oceanic {
                    (da - db) * 0.5
                } else {
                    (db - da) * 0.5
                };
                0.75 * closing * strike * bell(inland, reach * 0.4, reach * 0.36)
                    - 0.4 * closing * bell(inland, -reach * 0.2, reach * 0.16)
            }
            // A continent and an ocean drifting apart: a quiet shelf.
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plates() -> Plates {
        Plates::new(0xC0FFEE, 2400.0, 0.45, 1.0)
    }

    #[test]
    fn a_plate_is_the_same_every_time_it_is_asked_for() {
        let plates = plates();
        assert_eq!(plates.plate(3, -7), plates.plate(3, -7));
        assert_ne!(plates.plate(3, -7).site, plates.plate(4, -7).site);
        let drift = plates.plate(3, -7).drift;
        assert!(((drift[0] * drift[0] + drift[1] * drift[1]).sqrt() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn about_the_asked_share_of_plates_is_ocean() {
        let plates = plates();
        let oceanic = (0..40)
            .flat_map(|x| (0..40).map(move |z| (x, z)))
            .filter(|(x, z)| plates.plate(*x, *z).oceanic)
            .count();
        assert!(
            (600..840).contains(&oceanic),
            "{oceanic} of 1600 plates are ocean"
        );
    }

    #[test]
    fn the_land_never_jumps_between_neighbouring_blocks() {
        let plates = plates();
        let mut worst: f64 = 0.0;
        // Three long walks across many plate edges and junctions.
        for (dx, dz, start) in [(1.0, 0.0, -9000.0), (0.0, 1.0, 4000.0), (0.7, 0.7, 250.0)] {
            let mut last = plates.sample(start, start * 0.3).elevation;
            for step in 1..30_000 {
                let (x, z) = (start + dx * step as f64, start * 0.3 + dz * step as f64);
                let now = plates.sample(x, z).elevation;
                worst = worst.max((now - last).abs());
                last = now;
            }
        }
        // 0.02 of elevation is about three blocks of height at most.
        assert!(worst < 0.02, "a step of {worst} in one block");
    }

    #[test]
    fn there_are_continents_oceans_and_ranges() {
        let plates = plates();
        let (mut land, mut sea, mut high, mut deep, mut count) = (0, 0, 0, 0, 0);
        for i in 0..160 {
            for j in 0..160 {
                let t = plates.sample(i as f64 * 150.0 - 12_000.0, j as f64 * 150.0 - 12_000.0);
                count += 1;
                if t.elevation > 0.0 {
                    land += 1
                } else {
                    sea += 1
                }
                if t.elevation > 0.7 {
                    high += 1
                }
                if t.elevation < -0.7 {
                    deep += 1
                }
                assert!((-1.5..2.0).contains(&t.elevation));
            }
        }
        assert!(
            land * 100 / count > 30 && sea * 100 / count > 25,
            "{land} land, {sea} sea"
        );
        assert!(high > 20, "{high} samples in high ranges");
        assert!(deep > 5, "{deep} samples in trenches");
    }
}
