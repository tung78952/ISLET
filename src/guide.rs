//! The activities the eyes lead: a list of timed steps, and where the eyes are in each of them.
//! This is only the timing; the UI draws it from the moment `at` describes.

use crate::eyes::Look;
use crate::ui::Tile;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Move {
    ReachUp,
    Left,
    Right,
    NeckRoll,
    Close,
    FarAway,
    BreatheIn,
    Hold,
    BreatheOut,
    HoldEmpty,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Step {
    pub label: &'static str,
    pub secs: f32,
    pub moves: Move,
}

/// How the pair of eyes stands: moved, tilted and sized, wearing a look.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pose {
    pub dx: f32,
    pub dy: f32,
    pub tilt: f32,
    pub size: f32,
    pub look: Look,
}

const BREATH: f32 = 4.0;

pub fn steps(tile: Tile, secs: u32) -> Vec<Step> {
    let s = secs as f32;
    let step = |label, secs, moves| Step { label, secs, moves };
    match tile {
        Tile::Stretch => vec![step("Reach up · 1/4", s, Move::ReachUp), step("Lean left · 2/4", s, Move::Left), step("Lean right · 3/4", s, Move::Right), step("Neck roll · 4/4", s, Move::NeckRoll)],
        Tile::Eyes => vec![step("Close your eyes", (s * 0.25).max(2.0), Move::Close), step("Look far away", s, Move::FarAway)],
        Tile::Breathe => {
            let rounds = ((s / (4.0 * BREATH)).round() as usize).max(1);
            let one = [step("Breathe in", BREATH, Move::BreatheIn), step("Hold", BREATH, Move::Hold), step("Breathe out", BREATH, Move::BreatheOut), step("Hold", BREATH, Move::HoldEmpty)];
            one.iter().copied().cycle().take(4 * rounds).collect()
        }
        Tile::Sleep => Vec::new(),
    }
}

pub fn total(steps: &[Step]) -> f32 {
    steps.iter().map(|s| s.secs).sum()
}

/// The step at `t` seconds, how far through it (0..1) and the seconds left in it. None once over.
pub fn at(steps: &[Step], t: f32) -> Option<(usize, f32, f32)> {
    let mut from = 0.0;
    for (i, s) in steps.iter().enumerate() {
        if t < from + s.secs {
            return Some((i, (t - from) / s.secs, from + s.secs - t));
        }
        from += s.secs;
    }
    None
}

/// The pose `u` (0..1) through a step, `t` seconds in: nothing ever stands quite still.
pub fn pose(m: Move, u: f32, t: f32) -> Pose {
    use std::f32::consts::TAU;
    let still = Pose { dx: 0.0, dy: 0.0, tilt: 0.0, size: 1.0, look: Look::Neutral };
    let into = (t / 1.2).clamp(0.0, 1.0);
    let into = into * into * (3.0 - 2.0 * into);
    let sway = (t * 2.2).sin();
    match m {
        Move::ReachUp => Pose { dy: -14.0 * into + 2.5 * sway, size: 1.0 + 0.05 * into, ..still },
        Move::Left => Pose { dx: -24.0 * into + 3.0 * sway, dy: -4.0 * into, tilt: -0.22 * into + 0.04 * sway, ..still },
        Move::Right => Pose { dx: 24.0 * into + 3.0 * sway, dy: -4.0 * into, tilt: 0.22 * into + 0.04 * sway, ..still },
        Move::NeckRoll => {
            let a = -TAU / 4.0 + TAU * 2.0 * u;
            Pose { dx: 22.0 * a.cos(), dy: 13.0 * a.sin(), tilt: 0.28 * a.cos(), ..still }
        }
        Move::Close => Pose { look: Look::Closed, size: 1.0 + 0.06 * (t * 1.4).sin(), ..still },
        Move::FarAway => Pose { dx: 10.0 * (t * 0.8).sin(), dy: 3.0 * (t * 1.1).sin(), size: 0.82, look: if t % 3.4 < 0.14 { Look::Closed } else { Look::Neutral }, ..still },
        Move::BreatheIn => Pose { size: 1.0 + 0.4 * u, ..still },
        Move::Hold => Pose { size: 1.4, ..still },
        Move::BreatheOut => Pose { size: 1.4 - 0.65 * u, ..still },
        Move::HoldEmpty => Pose { size: 0.75, ..still },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stretch_is_three_moves_then_the_neck() {
        let s = steps(Tile::Stretch, 15);
        assert_eq!(s.iter().map(|x| x.moves).collect::<Vec<_>>(), [Move::ReachUp, Move::Left, Move::Right, Move::NeckRoll]);
        assert_eq!(total(&s), 60.0);
    }

    #[test]
    fn breathing_goes_in_fours_for_about_as_long_as_asked() {
        let s = steps(Tile::Breathe, 60);
        assert_eq!(total(&s), 64.0);
        assert_eq!(s[2].moves, Move::BreatheOut);
    }

    #[test]
    fn time_walks_through_the_steps_and_ends() {
        let s = steps(Tile::Stretch, 10);
        assert_eq!(at(&s, 5.0), Some((0, 0.5, 5.0)));
        assert_eq!(at(&s, 12.0).map(|a| a.0), Some(1));
        assert_eq!(at(&s, 40.0), None);
    }
}
