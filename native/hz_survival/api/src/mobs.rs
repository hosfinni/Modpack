//! Monsters that fight as a pack: goals added to each monster as it is made.
//!
//! A goal is that one mob's own state, and runs where the mob's AI runs, on the
//! server. Other mobs are out of reach while a mob thinks, so every goal here only
//! changes the mob it belongs to.

use std::sync::Arc;

use horizon_client::mobs::goals::{self, Cx, Goal};
use horizon_client::mobs::{Mob, finalize};
use horizon_client::mod_api::RegisterGoalsEvent;
use hz_survival_core::Config;
use hz_survival_core::world::{flank_point, flee_point};

/// How near (in blocks) another monster after the same target counts as company.
const COMPANY: f64 = 10.0;
/// Flanking starts between these distances from the target: nearer, the monster
/// just attacks; further, it closes in first.
const FLANK_FROM: f64 = 5.0;
const FLANK_TO: f64 = 14.0;
/// How far to the side of its target a flanking monster heads.
const FLANK_RADIUS: f64 = 4.0;
const FLANK_TICKS: i64 = 50;
const FLANK_REST: i64 = 100;
const FLEE_DISTANCE: f64 = 12.0;
const FLEE_TICKS: i64 = 70;
const FLEE_REST: i64 = 200;

fn place(m: &Mob) -> [f64; 3] {
    [m.position.x, m.position.y, m.position.z]
}

fn distance_sqr(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum()
}

/// The monsters near `m` that are after the same target.
fn company(m: &Mob, cx: &Cx) -> usize {
    let (at, target) = (place(m), m.ai.target);
    cx.w.others
        .iter()
        .filter(|other| other.id != m.id && other.alive && other.hostile && other.target == target)
        .filter(|other| {
            distance_sqr([other.position.x, other.position.y, other.position.z], at)
                <= COMPANY * COMPANY
        })
        .count()
}

/// A monster with nothing to attack takes up the fight of one near it: an attack
/// on one draws the pack.
#[derive(Clone, Debug)]
struct JoinPack {
    config: Arc<Config>,
    next_look: i64,
}

impl Goal for JoinPack {
    fn flags(&self) -> u8 {
        goals::TARGET
    }

    /// Looks once a second or so, and never runs: finding a fight is all it does.
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if m.ai.target.is_some() || cx.w.game_time < self.next_look {
            return false;
        }
        // Not every monster on the same tick.
        self.next_look = cx.w.game_time + 20 + i64::from(m.id % 20);
        let (at, range) = (place(m), self.config.mobs.pack_range);
        let fight = cx
            .w
            .others
            .iter()
            .filter(|other| other.id != m.id && other.alive && other.hostile)
            .filter_map(|other| {
                let target = other.target?;
                let away = distance_sqr([other.position.x, other.position.y, other.position.z], at);
                // Only a fight with a player who can be fought.
                let info = cx.w.target_info(target)?;
                (away <= range * range && info.alive && info.is_player && !info.creative)
                    .then_some((away, target))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, target)) = fight {
            m.ai.target = Some(target);
        }
        false
    }

    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// A melee monster in company circles to the side of its target before closing in,
/// so that a pack comes from several sides at once.
#[derive(Clone, Debug)]
struct Flank {
    until: i64,
    rest_until: i64,
}

impl Goal for Flank {
    fn flags(&self) -> u8 {
        goals::MOVE
    }

    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if cx.w.game_time < self.rest_until {
            return false;
        }
        let Some(target) = m.target_info(cx.w).filter(|target| target.alive) else {
            return false;
        };
        let away = distance_sqr(
            place(m),
            [target.position.x, target.position.y, target.position.z],
        );
        (FLANK_FROM * FLANK_FROM..=FLANK_TO * FLANK_TO).contains(&away) && company(m, cx) >= 1
    }

    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.until = cx.w.game_time + FLANK_TICKS;
        let Some(target) = m.target_info(cx.w) else {
            return;
        };
        // Odd and even monsters take opposite sides.
        let to = flank_point(
            place(m),
            [target.position.x, target.position.y, target.position.z],
            m.id.is_multiple_of(2),
            FLANK_RADIUS,
        );
        m.nav_move_to(cx.w, to, 1.15);
    }

    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        cx.w.game_time < self.until
            && !m.ai.nav.is_done()
            && m.target_info(cx.w).is_some_and(|target| target.alive)
    }

    fn stop(&mut self, _m: &mut Mob, cx: &mut Cx) {
        self.rest_until = cx.w.game_time + FLANK_REST;
    }

    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// A badly hurt monster with no company backs away from its target. It comes back:
/// its target is still its target, and others may have joined by then.
#[derive(Clone, Debug)]
struct Retreat {
    config: Arc<Config>,
    until: i64,
    rest_until: i64,
}

impl Goal for Retreat {
    fn flags(&self) -> u8 {
        goals::MOVE
    }

    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if cx.w.game_time < self.rest_until || m.target_info(cx.w).is_none() {
            return false;
        }
        self.config.mobs.retreats(
            &m.kind.id,
            m.health,
            finalize::max_health(m),
            company(m, cx),
        )
    }

    fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
        self.until = cx.w.game_time + FLEE_TICKS;
        let Some(target) = m.target_info(cx.w) else {
            return;
        };
        let to = flee_point(
            place(m),
            [target.position.x, target.position.y, target.position.z],
            FLEE_DISTANCE,
        );
        m.nav_move_to(cx.w, to, 1.3);
    }

    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        cx.w.game_time < self.until && !m.ai.nav.is_done()
    }

    fn stop(&mut self, _m: &mut Mob, cx: &mut Cx) {
        self.rest_until = cx.w.game_time + FLEE_REST;
    }

    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// A new mob's goals are being set: a monster gets the pack's.
pub fn register_goals(config: &Arc<Config>, event: &mut RegisterGoalsEvent<'_>) {
    let mob = &mut *event.mob;
    if !config.mobs.is_monster(&mob.kind.id) {
        return;
    }
    // Ahead of the species' own attack goal, which takes over when these stop.
    let ai = &mut mob.ai;
    ai.goals.add(
        1,
        Retreat {
            config: config.clone(),
            until: 0,
            rest_until: 0,
        },
    );
    if config.mobs.flank {
        ai.goals.add(
            1,
            Flank {
                until: 0,
                rest_until: 0,
            },
        );
    }
    if config.mobs.pack_range > 0.0 {
        ai.targets.add(
            2,
            JoinPack {
                config: config.clone(),
                next_look: 0,
            },
        );
    }
}
