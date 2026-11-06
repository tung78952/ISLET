//! A rolling picker like a phone's timer: dragged, flicked, turned with the mouse wheel or tapped,
//! it coasts and settles on a value. This is only the motion; the UI draws it.

/// How much of its speed a flick keeps per frame at 60 fps.
const FRICTION: f32 = 0.94;
/// Below this many items a second a flick is over and the wheel settles.
const COASTED: f32 = 1.2;
/// How quickly it settles on the nearest value (per second).
const SNAP: f32 = 16.0;
/// Pixels of drag that turn it by one item.
pub const ROW: f32 = 19.0;

pub struct Wheel {
    pub pos: f32,
    /// Height of a row: how far a drag turns it by one value.
    pub row: f32,
    len: usize,
    speed: f32,
    target: Option<f32>,
    drag: Option<Drag>,
    /// Seconds since it last passed from one value to the next: the picker's lines flash then.
    pub since_tick: f32,
    passing: i32,
}

struct Drag {
    y: f32,
    from: f32,
    last: (f32, f32),
    moved: bool,
}

impl Wheel {
    pub fn new(len: usize, index: usize) -> Self {
        Wheel { pos: index as f32, row: ROW, len, speed: 0.0, target: None, drag: None, since_tick: 9.0, passing: index as i32 }
    }

    pub fn tall(mut self, row: f32) -> Self {
        self.row = row;
        self
    }

    /// The value it is on, or passing.
    pub fn index(&self) -> usize {
        (self.pos.round().max(0.0) as usize).min(self.len.saturating_sub(1))
    }

    pub fn set(&mut self, index: usize) {
        self.pos = index.min(self.len.saturating_sub(1)) as f32;
        // Set from outside: no flash for it.
        self.passing = self.pos as i32;
        self.speed = 0.0;
        self.target = None;
    }

    pub fn press(&mut self, y: f32, t: f32) {
        self.drag = Some(Drag { y, from: self.pos, last: (y, t), moved: false });
        self.speed = 0.0;
        self.target = None;
    }

    pub fn drag_to(&mut self, y: f32, t: f32) {
        let Some(d) = self.drag.as_mut() else { return };
        d.moved |= (y - d.y).abs() > 3.0;
        let dt = (t - d.last.1).max(0.001);
        // A touch of smoothing, so one jittery sample doesn't decide the flick.
        self.speed = self.speed * 0.6 + 0.4 * (d.last.0 - y) / self.row / dt;
        d.last = (y, t);
        let pos = d.from + (d.y - y) / self.row;
        let top = (self.len - 1) as f32;
        // Past the ends it gives way reluctantly.
        self.pos = if pos < 0.0 { pos * 0.35 } else if pos > top { top + (pos - top) * 0.35 } else { pos };
    }

    /// Lets go; `tap` is the row tapped (-1 above the middle, 1 below) if it was not dragged.
    pub fn release(&mut self, tap: i32) {
        let Some(d) = self.drag.take() else { return };
        if !d.moved {
            self.speed = 0.0;
            self.target = Some((self.pos.round() + tap as f32).clamp(0.0, (self.len - 1) as f32));
        }
    }

    /// The drag was taken away: it just settles where it is.
    pub fn drop_drag(&mut self) {
        if let Some(d) = self.drag.as_mut() {
            d.moved = true;
        }
        self.release(0);
    }

    pub fn turn(&mut self, notches: f32) {
        let from = self.target.unwrap_or(self.pos.round());
        self.speed = 0.0;
        self.target = Some((from - notches.signum()).clamp(0.0, (self.len - 1) as f32));
    }

    pub fn held(&self) -> bool {
        self.drag.is_some()
    }

    /// Moves on by `dt`; returns whether it is still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        self.since_tick += dt;
        let passing = self.pos.round() as i32;
        if passing != self.passing {
            self.passing = passing;
            self.since_tick = 0.0;
        }
        if self.drag.is_some() {
            return true;
        }
        let top = (self.len - 1) as f32;
        if self.speed.abs() > COASTED && self.target.is_none() {
            self.pos += self.speed * dt;
            self.speed *= FRICTION.powf(dt * 60.0);
            if self.pos < 0.0 || self.pos > top {
                self.speed = 0.0;
            }
            return true;
        }
        self.speed = 0.0;
        let aim = self.target.unwrap_or(self.pos.round()).clamp(0.0, top);
        let gap = aim - self.pos;
        if gap.abs() < 0.002 {
            self.pos = aim;
            self.target = None;
            return false;
        }
        self.pos += gap * (1.0 - (-SNAP * dt).exp());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(w: &mut Wheel) {
        for _ in 0..600 {
            if !w.step(1.0 / 60.0) {
                return;
            }
        }
        panic!("it never settled");
    }

    #[test]
    fn a_flick_coasts_on_and_settles_on_a_value() {
        let mut w = Wheel::new(60, 10);
        w.press(100.0, 0.0);
        for i in 1..=5 {
            w.drag_to(100.0 - i as f32 * 8.0, i as f32 * 0.016);
        }
        let at_release = w.pos;
        w.release(0);
        settle(&mut w);
        assert!(w.pos > at_release + 1.0, "it should coast: {at_release} -> {}", w.pos);
        assert_eq!(w.pos, w.pos.round());
    }

    #[test]
    fn a_tap_rolls_one_row_and_stops_at_the_ends() {
        let mut w = Wheel::new(5, 0);
        w.press(100.0, 0.0);
        w.release(1);
        settle(&mut w);
        assert_eq!(w.index(), 1);
        w.press(100.0, 1.0);
        w.release(-1);
        settle(&mut w);
        w.press(100.0, 2.0);
        w.release(-1);
        settle(&mut w);
        assert_eq!(w.index(), 0);
    }

    #[test]
    fn dragging_past_the_end_comes_back() {
        let mut w = Wheel::new(5, 4);
        w.press(100.0, 0.0);
        w.drag_to(0.0, 0.5);
        assert!(w.pos > 4.0 && w.pos < 9.0);
        w.release(0);
        settle(&mut w);
        assert_eq!(w.index(), 4);
    }

    #[test]
    fn losing_the_mouse_mid_drag_does_not_leave_it_stuck() {
        let mut w = Wheel::new(10, 5);
        w.press(100.0, 0.0);
        w.drag_to(60.0, 0.1);
        w.drop_drag();
        assert!(!w.held());
        settle(&mut w);
        assert_eq!(w.pos, w.pos.round());
    }

    #[test]
    fn passing_a_value_is_noticed() {
        let mut w = Wheel::new(10, 5);
        settle(&mut w);
        assert!(w.since_tick > 1.0);
        w.turn(1.0);
        for _ in 0..60 {
            w.step(1.0 / 60.0);
        }
        assert_eq!(w.index(), 4);
        assert!(w.since_tick < 1.0, "it passed 5 -> 4 within the last second");
    }

    #[test]
    fn the_mouse_wheel_turns_it_step_by_step() {
        let mut w = Wheel::new(10, 5);
        w.turn(1.0);
        w.turn(1.0);
        settle(&mut w);
        assert_eq!(w.index(), 3);
    }
}
