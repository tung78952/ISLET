//! The character's eyes. Every expression is one closed outline with the same number of points,
//! and each point runs on a spring towards the next expression, so any shape melts into any other
//! with no in-between drawings. Coordinates are in the design sheet's pixels around the bar's
//! centre, for the left eye; the right eye mirrors it.

use crate::Spring;

const EDGE: usize = 24;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Look {
    Closed,
    Neutral,
    Happy,
    Surprised,
    Sleepy,
    Squint,
    /// The left eye stays open, the right one winks.
    Wink,
    Angry,
    Sad,
    Cry,
    Love,
    /// The lenses give way to two spinning spirals.
    Dizzy,
    Up,
    Down,
    /// The left eye narrows, the right one arches.
    Smug,
}

type P = (f32, f32);
/// Outer tip, inner tip, and the two control points of the upper and the lower edge between them.
type Lens = (P, P, [P; 2], [P; 2]);

const NEUTRAL: Lens = ((-291.0, -71.0), (-47.0, 41.0), [(-227.0, -45.0), (-117.0, -27.0)], [(-268.0, 76.0), (-137.0, 96.0)]);
/// The lens on the side being looked at opens; the other one narrows.
const GLANCE_NEAR: Lens = ((-306.0, -71.0), (-41.0, 41.0), [(-227.0, -69.0), (-131.0, -34.0)], [(-285.0, 65.0), (-158.0, 89.0)]);
const GLANCE_FAR: Lens = ((-291.0, -43.0), (-57.0, 41.0), [(-213.0, -14.0), (-131.0, 17.0)], [(-264.0, 48.0), (-172.0, 79.0)]);
const HAPPY: Lens = ((-314.0, 35.0), (-49.0, 65.0), [(-280.0, -75.0), (-110.0, -115.0)], [(-240.0, -15.0), (-130.0, 0.0)]);
const SURPRISED: Lens = ((-309.0, -72.0), (-74.0, 45.0), [(-270.0, -55.0), (-140.0, -30.0)], [(-325.0, 80.0), (-170.0, 105.0)]);
const SLEEPY: Lens = ((-316.0, -53.0), (-51.0, -11.0), [(-230.0, -38.0), (-140.0, -25.0)], [(-280.0, 60.0), (-140.0, 110.0)]);
const SQUINT: Lens = ((-304.0, -65.0), (-64.0, 18.0), [(-225.0, -38.0), (-140.0, -10.0)], [(-285.0, 60.0), (-150.0, 100.0)]);
const SLIT: Lens = ((-304.0, -42.0), (-47.0, 46.0), [(-230.0, -45.0), (-130.0, -15.0)], [(-225.0, -20.0), (-125.0, 12.0)]);

const ANGRY: Lens = ((-301.7, -68.9), (-45.2, 47.5), [(-214.7, -29.4), (-129.9, 9.0)], [(-271.2, 67.8), (-169.5, 101.7)]);
const SAD: Lens = ((-320.9, 32.8), (-115.3, -68.9), [(-248.6, 5.6), (-169.5, -33.9)], [(-254.2, 96.0), (-79.1, 73.4)]);
const UP: Lens = ((-337.9, 70.1), (-61.0, 58.8), [(-288.1, -96.0), (-141.2, -107.3)], [(-265.5, 33.9), (-146.9, 22.6)]);
const DOWN: Lens = ((-332.2, -59.9), (-70.1, -7.9), [(-248.6, -28.2), (-152.5, -9.0)], [(-299.4, 79.1), (-146.9, 113.0)]);
const SMUG_FLAT: Lens = ((-304.0, -35.0), (-53.1, 13.6), [(-220.3, -16.9), (-129.9, 0.0)], [(-271.2, 56.5), (-135.6, 73.4)]);
const SMUG_ARCH: Lens = ((-325.4, 64.4), (-20.3, 53.1), [(-288.1, -96.0), (-96.0, -107.3)], [(-237.3, 0.0), (-124.3, -5.6)]);

fn cubic(a: P, b: P, c: P, d: P, t: f32) -> P {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    (w[0] * a.0 + w[1] * b.0 + w[2] * c.0 + w[3] * d.0, w[0] * a.1 + w[1] * b.1 + w[2] * c.1 + w[3] * d.1)
}

fn trace((from, to, top, low): Lens, right: bool) -> Vec<P> {
    let at = |j: usize| j as f32 / EDGE as f32;
    let upper = (0..=EDGE).map(|j| cubic(from, top[0], top[1], to, at(j)));
    let lower = (1..EDGE).rev().map(|j| cubic(from, low[0], low[1], to, at(j)));
    let side = if right { -1.0 } else { 1.0 };
    upper.chain(lower).map(|p| (p.0 * side, p.1)).collect()
}

fn outline(look: Look, right: bool, gaze: f32, lift: f32) -> Vec<P> {
    let lens = match look {
        Look::Closed | Look::Dizzy => SLIT,
        Look::Love => {
            // The classic heart curve, starting from its outer side like every other outline.
            let side = if right { -1.0 } else { 1.0 };
            return (0..2 * EDGE)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / EDGE as f32;
                    let x = 16.0 * a.sin().powi(3);
                    let y = 13.0 * a.cos() - 5.0 * (2.0 * a).cos() - 2.0 * (3.0 * a).cos() - (4.0 * a).cos();
                    (side * (-165.0 + 5.6 * x), 5.0 - 5.6 * y)
                })
                .collect();
        }
        Look::Neutral => {
            let side = if (gaze > 0.0) == right { GLANCE_NEAR } else { GLANCE_FAR };
            let k = gaze.abs().min(1.0);
            let mix = |a: Vec<P>, b: Vec<P>, k: f32| -> Vec<P> { a.iter().zip(b).map(|(a, b)| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k)).collect() };
            return mix(mix(trace(NEUTRAL, right), trace(side, right), k), trace(DOWN, right), lift);
        }
        Look::Happy => HAPPY,
        Look::Surprised => SURPRISED,
        Look::Sleepy => SLEEPY,
        Look::Squint => SQUINT,
        Look::Wink if right => SLIT,
        Look::Wink => NEUTRAL,
        Look::Angry => ANGRY,
        Look::Sad | Look::Cry => SAD,
        Look::Up => UP,
        Look::Down => DOWN,
        Look::Smug if right => SMUG_ARCH,
        Look::Smug => SMUG_FLAT,
    };
    trace(lens, right)
}

/// How far the pointer can be from the notch for the eyes to notice it, in notch units.
pub const RANGE: f32 = 170.0;
/// How far it must go before they count it as gone, so its edge does not make them flicker.
pub const RANGE_OUT: f32 = 230.0;
/// A pointer must stay this long before they open: one just passing through is ignored.
const NOTICE: f32 = 0.25;
/// How long they watch a pointer leave before their lids drop, and then shut.
const WATCH_LEAVE: f32 = 0.95;
const DROP_LIDS: f32 = 0.3;
/// Seconds of a resting pointer before they lose interest.
const BORED: f32 = 3.0;
/// The longest they stay out in one go; a pointer still moving around here is someone working.
const LONGEST: f32 = 12.0;
/// Seconds they stay away after leaving: bored, worked around, or told to move.
const REST_BORED: f32 = 3.0;
const DIZZY: f32 = 2.0;
const REST_WORKING: f32 = 30.0;
const REST_ACTED: f32 = 1.0;

/// Seconds an event waits for a free moment before it is dropped.
const PEEK_WAIT: f32 = 10.0;

/// A reason to come out without the pointer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Peek {
    /// Look around and pull one face.
    Random,
    /// A moment's look before a reminder card.
    Card(Look),
    /// React to something that happened.
    Mood(Look),
    /// Butt the nearly flat battery to get attention.
    Battery,
}

impl Peek {
    fn lasts(self) -> f32 {
        match self {
            Peek::Random => 4.0,
            Peek::Card(_) => 0.8,
            Peek::Mood(_) => 2.0,
            Peek::Battery => 4.0,
        }
    }
}

/// Spots beats in a stream of loudness readings: a jump above the recent average.
pub struct Beat {
    average: f32,
    cooling: u32,
}

impl Beat {
    pub fn new() -> Self {
        Beat { average: 0.0, cooling: 0 }
    }

    pub fn feed(&mut self, level: f32) -> bool {
        let hit = self.cooling == 0 && level > 0.02 && level > self.average * 1.3;
        self.average += (level - self.average) * 0.15;
        self.cooling = if hit { 5 } else { self.cooling.saturating_sub(1) };
        hit
    }
}

/// What the app tells the eyes about the moment.
pub struct Sense {
    pub t: f32,
    /// Pointer position in px.
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    /// Where the pointer is along the notch (-1..1), and how far below it (0..1).
    pub follow: f32,
    pub lift: f32,
    pub near: bool,
    /// Still within `RANGE_OUT`.
    pub around: bool,
    /// Something more important has the notch: stepping aside, a card, the open panel.
    pub blocked: bool,
    /// The user clicked or typed: they are working, not playing with the eyes.
    pub acted: bool,
}

/// What the eyes are doing.
#[derive(Clone, Copy)]
pub struct Feel {
    pub show: bool,
    pub look: Look,
    pub blink: bool,
    pub gaze: f32,
    pub lift: f32,
    /// The battery scene is on, and how far the eyes lunge at the battery (-1 back to 1 hit).
    pub battery: bool,
    pub lunge: f32,
}

impl Feel {
    pub const ASLEEP: Feel = Feel { show: false, look: Look::Closed, blink: false, gaze: 0.0, lift: 0.0, battery: false, lunge: 0.0 };
}

/// When the eyes come out and what they do: they answer a pointer arriving, never one that is
/// simply there, and anything the user is doing comes first. Eyes jump between points and hold
/// instead of drifting, as real ones do.
pub struct Mind {
    seed: u32,
    last: Option<(f32, f32, f32)>,
    awake: bool,
    near_since: Option<f32>,
    left_at: Option<f32>,
    since: f32,
    rest_until: f32,
    closing_until: f32,
    moved_at: f32,
    mood: Option<(Look, f32)>,
    blink_at: f32,
    blink_until: f32,
    fixed: f32,
    hold_until: f32,
    glance_at: f32,
    twitch: f32,
    twitch_at: f32,
    /// Quick changes of direction, for spotting a shaken pointer.
    heading: f32,
    shakes: u32,
    shake_at: f32,
    next_random: f32,
    /// An event waiting for a free moment, and when it was asked for.
    pending: Option<(Peek, f32)>,
    /// What they are out for: the reason, when it began, the face it uses.
    active: Option<(Peek, f32, Look)>,
}

impl Mind {
    fn new() -> Self {
        Mind {
            seed: 0x2F6E_2B1D,
            last: None,
            awake: false,
            near_since: None,
            left_at: None,
            since: 0.0,
            rest_until: 0.0,
            closing_until: 0.0,
            moved_at: 0.0,
            mood: None,
            blink_at: 0.0,
            blink_until: 0.0,
            fixed: 0.0,
            hold_until: 0.0,
            glance_at: 0.0,
            twitch: 0.0,
            twitch_at: 0.0,
            heading: 0.0,
            shakes: 0,
            shake_at: 0.0,
            next_random: 0.0,
            pending: None,
            active: None,
        }
    }

    /// Asks the eyes to come out. A newer reason replaces one still waiting.
    pub fn ask(&mut self, peek: Peek, t: f32) {
        self.pending = Some((peek, t));
    }

    /// What they do during a peek, `e` seconds in.
    fn peeking(&mut self, t: f32, peek: Peek, e: f32, face: Look) -> Feel {
        let blink = self.blink(t);
        let (look, gaze) = match peek {
            Peek::Random if e < 0.8 => (Look::Neutral, -0.7),
            Peek::Random if e < 1.5 => (Look::Neutral, 0.7),
            Peek::Random if (2.1..3.4).contains(&e) => (face, 0.0),
            Peek::Random => (Look::Neutral, 0.0),
            Peek::Card(look) | Peek::Mood(look) => (look, 0.0),
            Peek::Battery if e < 0.8 => (Look::Neutral, 0.8),
            Peek::Battery if e < 2.3 => (Look::Squint, 0.8),
            Peek::Battery => (Look::Neutral, -0.2),
        };
        let lunge = match e {
            _ if peek != Peek::Battery => 0.0,
            e if (0.8..1.0).contains(&e) || (1.5..1.7).contains(&e) => -0.3,
            e if (1.0..1.16).contains(&e) || (1.7..1.86).contains(&e) => 1.0,
            _ => 0.0,
        };
        Feel { show: true, look, blink: blink && look == Look::Neutral, gaze, lift: 0.0, battery: peek == Peek::Battery, lunge }
    }

    fn blink(&mut self, t: f32) -> bool {
        if t >= self.blink_at {
            self.blink_until = t + 0.11;
            // One blink in four comes twice.
            self.blink_at = t + if self.roll() < 0.25 { 0.24 } else { 2.0 + 4.0 * self.roll() };
        }
        t < self.blink_until
    }

    /// A number from 0 up to 1.
    fn roll(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 1000) as f32 / 1000.0
    }

    /// The lids shut first; the eyes then leave the notch to the clock.
    fn sleep(&mut self, t: f32, rest: f32) {
        self.awake = false;
        self.mood = None;
        self.closing_until = t + 0.14;
        self.rest_until = self.rest_until.max(t + rest);
    }

    fn asleep(&self, t: f32) -> Feel {
        Feel { show: t < self.closing_until, ..Feel::ASLEEP }
    }

    pub fn tick(&mut self, s: &Sense) -> Feel {
        const DART: f32 = 2600.0;
        let t = s.t;
        let speed = self.last.map_or(0.0, |(px, py, pt)| (s.x - px).hypot(s.y - py) / (t - pt).max(0.001));
        let heading = self.last.map_or(0.0, |(px, ..)| (s.x - px).signum());
        self.last = Some((s.x, s.y, t));
        let moving = speed > 60.0 * s.scale;
        if self.next_random == 0.0 {
            self.next_random = t + 900.0 + 1500.0 * self.roll();
        } else if t >= self.next_random {
            self.next_random = t + 900.0 + 1500.0 * self.roll();
            if self.pending.is_none() {
                self.pending = Some((Peek::Random, t));
            }
        }
        if self.pending.is_some_and(|(_, at)| t - at > PEEK_WAIT) {
            self.pending = None;
        }
        if (s.acted || s.blocked) && self.active.take().is_some() {
            self.closing_until = t + 0.14;
        }
        if s.acted {
            if self.awake {
                self.sleep(t, REST_ACTED);
            }
            self.rest_until = self.rest_until.max(t + REST_ACTED);
            return self.asleep(t);
        }
        if s.blocked {
            if self.awake {
                self.sleep(t, 1.0);
            }
            return self.asleep(t);
        }
        if self.active.is_none() {
            if let Some((peek, _)) = self.pending.take() {
                let face = [Look::Happy, Look::Wink, Look::Smug, Look::Up][(self.roll() * 4.0) as usize % 4];
                self.active = Some((peek, t, face));
                self.awake = false;
                self.mood = None;
            }
        }
        if let Some((peek, began, face)) = self.active {
            if t - began < peek.lasts() {
                return self.peeking(t, peek, t - began, face);
            }
            self.active = None;
            self.closing_until = t + 0.14;
            self.rest_until = self.rest_until.max(t + 1.0);
            return self.asleep(t);
        }
        if !self.awake {
            self.near_since = if s.near { self.near_since.or(Some(t)) } else { None };
            let noticed = self.near_since.is_some_and(|at| t - at >= NOTICE);
            if !(noticed && moving && t >= self.rest_until) {
                return self.asleep(t);
            }
            self.awake = true;
            self.near_since = None;
            self.left_at = None;
            self.since = t;
            self.moved_at = t;
            self.fixed = s.follow;
            self.hold_until = t + 0.2;
            self.shakes = 0;
            self.blink_at = t + 1.5 + 2.0 * self.roll();
            // Waking: a touch too wide before they settle.
            self.mood = Some((Look::Surprised, t + 0.22));
        }
        if moving {
            self.moved_at = t;
        }
        // Dizzy lasts its full time whatever the pointer does: nobody recovers in an instant.
        let dizzy = self.mood.is_some_and(|m| m.0 == Look::Dizzy && t <= m.1);
        if dizzy {
            return Feel { show: true, look: Look::Dizzy, ..Feel::ASLEEP };
        }
        let still = t - self.moved_at;
        // A pointer leaving is watched out of sight, then the lids drop slowly.
        if s.around {
            self.left_at = None;
        } else {
            let gone = t - *self.left_at.get_or_insert(t);
            if gone >= WATCH_LEAVE + DROP_LIDS {
                self.left_at = None;
                self.sleep(t, 1.0);
                return self.asleep(t);
            }
            let look = if gone < WATCH_LEAVE { Look::Neutral } else { Look::Sleepy };
            return Feel { show: true, look, gaze: s.follow, lift: s.lift, ..Feel::ASLEEP };
        }
        let rest = if still > BORED {
            Some(REST_BORED)
        } else if t - self.since > LONGEST {
            Some(REST_WORKING)
        } else {
            None
        };
        if let Some(rest) = rest {
            self.sleep(t, rest);
            return self.asleep(t);
        }
        if speed > DART * s.scale {
            self.mood = Some((Look::Surprised, t + 0.6));
        }
        if speed > 900.0 * s.scale {
            if heading * self.heading < 0.0 {
                self.shakes = if t - self.shake_at < 0.45 { self.shakes + 1 } else { 1 };
                self.shake_at = t;
            }
            self.heading = heading;
        }
        if self.shakes >= 4 {
            self.shakes = 0;
            self.mood = Some((Look::Dizzy, t + DIZZY));
            return Feel { show: true, look: Look::Dizzy, ..Feel::ASLEEP };
        }
        // Coming round from dizziness takes a moment, with heavy lids.
        if let Some((look, end)) = self.mood.filter(|m| t > m.1) {
            self.mood = (look == Look::Dizzy).then_some((Look::Sleepy, end + 0.6));
        }
        if self.mood.is_none() && still > BORED - 0.8 {
            self.mood = Some((Look::Sleepy, t + 1.0));
        }
        // A resting pointer gets looked away from and back to; a moving one is chased in jumps,
        // with a blink on the long ones. Small errors are followed smoothly.
        let mut aim = s.follow;
        if still > 0.9 {
            if t > self.glance_at {
                let (a, b) = (self.roll(), self.roll());
                self.twitch = if a < 0.4 { 0.0 } else { (b * 2.0 - 1.0) * 0.8 };
                self.glance_at = t + 0.5 + 0.7 * a;
            }
            aim = (s.follow + self.twitch).clamp(-1.0, 1.0);
        }
        let error = aim - self.fixed;
        if error.abs() > 0.14 {
            if t >= self.hold_until {
                self.fixed = aim;
                self.hold_until = t + 0.15 + 0.25 * self.roll();
                if error.abs() > 0.5 && self.roll() < 0.5 {
                    self.blink_at = t;
                }
            }
        } else {
            self.fixed += error * 0.4;
        }
        let blink = self.blink(t);
        if t > self.twitch_at {
            self.twitch_at = t + 0.8 + 0.6 * self.roll();
        }
        let tremor = 0.025 * (self.twitch_at * 37.0).sin();
        Feel { show: true, look: self.mood.map_or(Look::Neutral, |m| m.0), blink, gaze: self.fixed + tremor, lift: s.lift, ..Feel::ASLEEP }
    }
}

pub struct Eye {
    pub points: Vec<P>,
    speed: Vec<P>,
}

impl Eye {
    /// Springs every point towards an outline; returns the fastest point's speed.
    fn ease(&mut self, target: &[P], k: f32, c: f32, dt: f32) -> f32 {
        let mut fastest = 0.0f32;
        for ((p, v), t) in self.points.iter_mut().zip(self.speed.iter_mut()).zip(target) {
            v.0 += (-(p.0 - t.0) * k - c * v.0) * dt;
            v.1 += (-(p.1 - t.1) * k - c * v.1) * dt;
            p.0 += v.0 * dt;
            p.1 += v.1 * dt;
            fastest = fastest.max(v.0.abs()).max(v.1.abs());
        }
        fastest
    }
}

const MINI_AHEAD: Lens = ((-132.0, -70.0), (12.0, 40.0), [(-85.0, -45.0), (-15.0, -5.0)], [(-160.0, 40.0), (-50.0, 125.0)]);
const MINI_AHEAD_FAR: Lens = ((-132.0, -40.0), (-40.0, 55.0), [(-95.0, -20.0), (-60.0, 15.0)], [(-140.0, 40.0), (-90.0, 85.0)]);
const MINI_BLINK: Lens = ((-145.0, -20.0), (-10.0, 18.0), [(-100.0, -12.0), (-40.0, 2.0)], [(-120.0, 30.0), (-50.0, 40.0)]);
const MINI_HAPPY: Lens = ((-158.0, 45.0), (-15.0, 45.0), [(-135.0, -70.0), (-50.0, -75.0)], [(-115.0, 0.0), (-60.0, -5.0)]);
const MINI_NOD: Lens = ((-140.0, -45.0), (-10.0, 15.0), [(-95.0, -25.0), (-45.0, -10.0)], [(-150.0, 50.0), (-60.0, 60.0)]);

/// One of the main reminders, played out by the eyes. Everything here is expressions, positions
/// and timing, so any character with the same set of looks can perform it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ActKind {
    Water,
    Sleep,
    Breathe,
    Stretch,
    Rest,
}

/// One moment of an act.
#[derive(Clone, Copy, Debug)]
pub struct ActFrame {
    pub look: Look,
    pub gaze: f32,
    pub blink: bool,
    /// The eyes' run at the prop (-1 back to 1 hit).
    pub lunge: f32,
    /// How far the title, the subtitle and the buttons have come out (the first two overshoot).
    pub title: f32,
    pub subtitle: f32,
    pub buttons: f32,
    /// The eyes pulling the notch apart (0..1).
    pub stretch: f32,
    /// In and out of a breath (-1..1).
    pub swell: f32,
    /// How far along the act's own business is (0..1).
    pub play: f32,
}

/// When the blow lands and the card opens up.
const ACT_HIT: f32 = 0.9;
/// When the act is over and the card just waits.
pub const ACT_END: f32 = 3.2;
/// How often a waiting card gives a little nudge.
pub const ACT_NUDGE: f32 = 15.0;

fn overshoot(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0) - 1.0;
    1.0 + 2.7 * k * k * k + 1.7 * k * k
}

fn smooth(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// The act at `e` seconds: the wind-up and the blow are shared, then each reminder does its own.
pub fn act(kind: ActKind, e: f32) -> ActFrame {
    let waiting = (e - ACT_END).max(0.0) % ACT_NUDGE;
    let nudge = e >= ACT_END + ACT_NUDGE;
    let lunge = match e {
        e if (0.5..ACT_HIT).contains(&e) => -0.35 * smooth((e - 0.5) / 0.2),
        e if (ACT_HIT..ACT_HIT + 0.15).contains(&e) => 1.0,
        _ if nudge && waiting < 0.2 => -0.2,
        _ if nudge && waiting < 0.34 => 0.6,
        _ => 0.0,
    };
    let play = ((e - 1.2) / (ACT_END - 1.2)).clamp(0.0, 1.0);
    let mut look = if (0.5..ACT_HIT + 0.15).contains(&e) { Look::Squint } else { Look::Neutral };
    let mut gaze = if e < 0.5 { 0.0 } else { 0.7 };
    let (mut stretch, mut swell) = (0.0, 0.0);
    if e >= 1.2 && e < ACT_END {
        match kind {
            ActKind::Water => gaze = if play < 0.7 { 0.8 } else { -0.25 },
            ActKind::Sleep => {
                gaze = 0.0;
                look = match play {
                    p if p < 0.45 => Look::Sleepy,
                    p if p < 0.65 => Look::Closed,
                    p if p < 0.8 => Look::Sleepy,
                    _ => Look::Closed,
                };
            }
            ActKind::Breathe => {
                gaze = 0.0;
                swell = (play * std::f32::consts::TAU).sin();
                look = if swell > 0.0 { Look::Surprised } else { Look::Sleepy };
            }
            ActKind::Stretch => {
                gaze = 0.0;
                // Two pulls: strain to the limit, trembling at the top, then let go.
                let q = (play * 2.0).fract();
                let pulling = q < 0.72;
                stretch = if pulling { smooth(q / 0.5) * (1.0 + 0.035 * (e * 46.0).sin() * smooth((q - 0.4) / 0.2)) } else { 0.0 };
                look = if pulling { Look::Squint } else { Look::Happy };
            }
            ActKind::Rest => {
                look = if play < 0.55 { Look::Closed } else { Look::Neutral };
                gaze = if play < 0.55 { 0.0 } else if play < 0.8 { -0.9 } else { 0.9 };
            }
        }
    } else if e >= ACT_END {
        look = match kind {
            ActKind::Sleep => Look::Sleepy,
            _ => Look::Neutral,
        };
        gaze = if kind == ActKind::Sleep { 0.0 } else { 0.5 };
    }
    ActFrame {
        look,
        gaze,
        blink: e > 1.2 && (e % 4.3) < 0.11 && look == Look::Neutral,
        lunge,
        title: overshoot((e - ACT_HIT) / 0.36),
        subtitle: overshoot((e - ACT_HIT - 0.14) / 0.34),
        buttons: smooth((e - ACT_HIT - 0.4) / 0.25),
        stretch,
        swell,
        play,
    }
}

/// Whether the act needs frames at `e`: not while a card simply waits.
pub fn act_moving(e: f32) -> bool {
    e < ACT_END + 0.8 || (e - ACT_END) % ACT_NUDGE < 0.9 || e % 4.3 < 0.4
}

/// How the small eyes hold themselves when nothing else is going on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pose {
    Right,
    Left,
    Smile,
    Dreamy,
    Wink,
}

/// The lyric bar now and then grows to one side for a moment, and the eyes come and dance there.
pub struct Jam {
    next: f32,
    until: f32,
    pub side: f32,
    pub style: usize,
    seed: u32,
    /// Notes in the air: when each left and whether it is the beamed one.
    pub notes: Vec<(f32, f32, bool)>,
}

/// The dances, used in turn at random, never the same twice running.
pub const JAM_POSES: [Pose; 3] = [Pose::Smile, Pose::Dreamy, Pose::Wink];
pub const NOTE_LIFE: f32 = 1.3;
const JAM_LENGTH: f32 = 6.0;

impl Jam {
    pub fn new() -> Self {
        Jam { next: 0.0, until: 0.0, side: 1.0, style: 0, seed: 0x7A3C_11D3, notes: Vec::new() }
    }

    fn roll(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 1000) as f32 / 1000.0
    }

    /// Time passes; returns whether the bar is out. Not `allowed`: it goes back at once.
    pub fn tick(&mut self, t: f32, allowed: bool) -> bool {
        if self.next == 0.0 {
            self.next = t + 60.0 + 60.0 * self.roll();
        }
        if !allowed {
            self.until = self.until.min(t);
            return false;
        }
        if t >= self.next && t >= self.until {
            self.begin(t);
        }
        t < self.until
    }

    fn begin(&mut self, t: f32) {
        self.side = if self.roll() < 0.5 { -1.0 } else { 1.0 };
        self.style = (self.style + 1 + (self.roll() * 2.0) as usize) % JAM_POSES.len();
        self.until = t + JAM_LENGTH;
        self.next = self.until + 60.0 + 60.0 * self.roll();
        self.notes.clear();
    }

    /// The effect test wants it out whatever the time.
    pub fn force(&mut self, t: f32) {
        if t >= self.until {
            self.begin(t);
            self.side = -self.side;
        }
    }

    pub fn beat(&mut self, t: f32) {
        self.notes.retain(|n| t - n.0 < NOTE_LIFE);
        if self.notes.len() < 4 {
            let pair = self.roll() < 0.5;
            self.notes.push((t, if self.notes.len() % 2 == 0 { -1.0 } else { 1.0 }, pair));
        }
    }
}

/// Small eyes beside the things they react to: props on the cards, the music in the lyric bar.
pub struct Mini {
    pub pair: [Eye; 2],
    pub hop: Spring,
    beat: Beat,
    face: Option<(Lens, f32)>,
    pub pose: Pose,
    blink_at: f32,
    blink_until: f32,
    seed: u32,
}

impl Mini {
    pub fn new() -> Self {
        let eye = |lens| {
            let points = trace(lens, true);
            Eye { speed: vec![(0.0, 0.0); points.len()], points }
        };
        let left = trace(MINI_AHEAD, false);
        Mini {
            pair: [Eye { speed: vec![(0.0, 0.0); left.len()], points: left }, eye(MINI_AHEAD_FAR)],
            hop: Spring::new(0.0, 420.0, 16.0),
            beat: Beat::new(),
            face: None,
            pose: Pose::Right,
            blink_at: 2.0,
            blink_until: 0.0,
            seed: 0x51F1_5EED,
        }
    }

    /// A reading of the music's loudness; true when something needs drawing.
    pub fn feed(&mut self, t: f32, level: f32) -> bool {
        let mut moved = false;
        if self.beat.feed(level) {
            self.hop.v = -15.0;
            if self.face.is_none_or(|f| t >= f.1) && matches!(self.pose, Pose::Right | Pose::Left) {
                self.face = Some((MINI_NOD, t + 0.12));
            }
            moved = true;
        }
        if t >= self.blink_at {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 17;
            self.seed ^= self.seed << 5;
            self.blink_until = t + 0.11;
            self.blink_at = t + 2.0 + (self.seed % 1000) as f32 / 250.0;
            moved = true;
        }
        moved
    }

    /// Returns whether anything is still moving.
    pub fn step(&mut self, t: f32, dt: f32) -> bool {
        let face = self.face.filter(|f| t < f.1).map(|f| f.0);
        let pose = self.pose;
        let lens = |right: bool| match face {
            _ if t < self.blink_until => MINI_BLINK,
            Some(f) => f,
            None => match pose {
                Pose::Smile => MINI_HAPPY,
                Pose::Dreamy => MINI_BLINK,
                Pose::Wink if right => MINI_BLINK,
                Pose::Wink => MINI_AHEAD,
                Pose::Right if right => MINI_AHEAD_FAR,
                Pose::Right => MINI_AHEAD,
                Pose::Left if right => MINI_AHEAD,
                Pose::Left => MINI_AHEAD_FAR,
            },
        };
        let (a, b) = (trace(lens(false), false), trace(lens(true), true));
        let fastest = self.pair[0].ease(&a, 700.0, 36.0, dt).max(self.pair[1].ease(&b, 700.0, 36.0, dt));
        let hopping = self.hop.step(0.0, dt);
        fastest > 2.0 || hopping || t < self.blink_until || face.is_some()
    }
}

pub struct Eyes {
    pub show: Spring,
    /// Where the lenses look: quick, like the eyes themselves.
    pub gaze: Spring,
    /// Where the pair sits: slower and with a little swing, like the head behind them.
    pub lean: Spring,
    /// A little hop whenever the expression changes.
    pub hop: Spring,
    /// The battery scene: how far it has come in, the lunge, and the blow's effect on the battery.
    pub scene: Spring,
    pub lunge: Spring,
    pub shove: Spring,
    pub rock: Spring,
    pub dent: Spring,
    /// How far the tear and the spirals have come in (0..1).
    pub tear: Spring,
    pub dizzy: Spring,
    pub pair: [Eye; 2],
    pub mind: Mind,
    last: Look,
    /// Seconds of slow reopening left after a blink.
    reopening: f32,
    last_lunge: f32,
}

impl Eyes {
    pub fn new() -> Self {
        let eye = |right| {
            let points = outline(Look::Closed, right, 0.0, 0.0);
            Eye { speed: vec![(0.0, 0.0); points.len()], points }
        };
        Eyes {
            show: Spring::new(0.0, 170.0, 24.0),
            gaze: Spring::new(0.0, 900.0, 46.0),
            lean: Spring::new(0.0, 130.0, 13.0),
            hop: Spring::new(0.0, 320.0, 15.0),
            scene: Spring::new(0.0, 170.0, 24.0),
            lunge: Spring::new(0.0, 700.0, 30.0),
            shove: Spring::new(0.0, 160.0, 10.0),
            rock: Spring::new(0.0, 220.0, 7.0),
            dent: Spring::new(0.0, 420.0, 20.0),
            tear: Spring::new(0.0, 170.0, 24.0),
            dizzy: Spring::new(0.0, 170.0, 24.0),
            pair: [eye(false), eye(true)],
            mind: Mind::new(),
            last: Look::Closed,
            reopening: 0.0,
            last_lunge: 0.0,
        }
    }

    pub fn step(&mut self, feel: Feel, dt: f32) {
        let Feel { show, look, blink, gaze, lift, battery, lunge } = feel;
        // A blink shuts fast and reopens more slowly; expressions change between the two.
        self.reopening = if blink { 0.2 } else { (self.reopening - dt).max(0.0) };
        let (k, c) = if blink {
            (1000.0, 46.0)
        } else if self.reopening > 0.0 {
            (420.0, 30.0)
        } else {
            (300.0, 23.0)
        };
        let shape = if blink { Look::Closed } else { look };
        self.gaze.step(gaze, dt);
        self.lean.step(gaze, dt);
        if look != self.last {
            self.last = look;
            self.hop.v = -38.0;
        }
        self.hop.step(0.0, dt);
        // Contact: the battery is shoved, rocks and dents, then springs back.
        if lunge > 0.5 && self.last_lunge <= 0.5 {
            self.shove.v += 150.0;
            self.rock.v += 5.0;
            self.dent.v += 16.0;
        }
        self.last_lunge = lunge;
        self.lunge.step(lunge, dt);
        self.shove.step(0.0, dt);
        self.rock.step(0.0, dt);
        self.dent.step(0.0, dt);
        self.scene.step(if battery { 1.0 } else { 0.0 }, dt);
        let leaning = self.gaze.x;
        for (i, eye) in self.pair.iter_mut().enumerate() {
            let target = outline(shape, i == 1, leaning, lift);
            eye.ease(&target, k, c, dt);
        }
        self.show.step(if show { 1.0 } else { 0.0 }, dt);
        self.tear.step(if look == Look::Cry { 1.0 } else { 0.0 }, dt);
        self.dizzy.step(if look == Look::Dizzy { 1.0 } else { 0.0 }, dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_expression_has_the_same_points_so_any_can_morph_into_any() {
        for look in [Look::Closed, Look::Neutral, Look::Happy, Look::Surprised, Look::Sleepy, Look::Squint, Look::Wink, Look::Angry, Look::Sad, Look::Cry, Look::Love, Look::Dizzy, Look::Up, Look::Down, Look::Smug] {
            for right in [false, true] {
                let o = outline(look, right, 0.5, 0.2);
                assert_eq!(o.len(), 2 * EDGE);
                assert!(o.iter().all(|p| p.0.is_finite() && p.1.is_finite()));
            }
        }
    }

    #[test]
    fn a_glance_opens_the_near_lens_and_narrows_the_far_one() {
        let height = |o: Vec<P>| o.iter().map(|p| p.1).fold(f32::MIN, f32::max) - o.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let (near, far) = (height(outline(Look::Neutral, true, 1.0, 0.0)), height(outline(Look::Neutral, false, 1.0, 0.0)));
        assert!(near > far, "{near} {far}");
    }

    fn sense(t: f32, x: f32) -> Sense {
        Sense { t, x, y: 0.0, scale: 1.0, follow: 0.2, lift: 0.0, near: true, around: true, blocked: false, acted: false }
    }

    /// Runs a moving pointer for `secs` and returns the mind and the last tick's time.
    fn awake(m: &mut Mind, secs: f32) -> f32 {
        let mut t = 0.0;
        while t < secs {
            m.tick(&sense(t, t * 200.0));
            t += 0.05;
        }
        t
    }

    #[test]
    fn they_answer_a_pointer_arriving_not_one_already_there() {
        let mut m = Mind::new();
        assert!(!m.tick(&sense(0.0, 100.0)).show);
        assert!(!m.tick(&sense(0.05, 100.0)).show, "a still pointer wakes nothing");
        assert!(!m.tick(&sense(0.1, 130.0)).show, "a pointer passing through is ignored");
        assert!(m.tick(&sense(0.3, 200.0)).show);
    }

    #[test]
    fn a_resting_pointer_bores_them_and_they_stay_away() {
        let mut m = Mind::new();
        let t = awake(&mut m, 0.5);
        let x = t * 200.0;
        for i in 0..80 {
            m.tick(&sense(t + i as f32 * 0.05, x));
        }
        assert!(!m.tick(&sense(t + 4.0, x)).show);
        m.tick(&sense(t + 4.5, x));
        assert!(!m.tick(&sense(t + 4.55, x + 50.0)).show, "still resting");
        m.tick(&sense(t + 15.0, x));
        assert!(m.tick(&sense(t + 15.05, x + 100.0)).show);
    }

    #[test]
    fn working_near_the_notch_sends_them_away() {
        let mut m = Mind::new();
        let t = awake(&mut m, LONGEST + 1.0);
        assert!(!m.tick(&sense(t + 1.0, 0.0)).show);
    }

    #[test]
    fn clicks_typing_and_stepping_aside_come_first() {
        for (acted, blocked) in [(true, false), (false, true)] {
            let mut m = Mind::new();
            let t = awake(&mut m, 0.5);
            assert!(m.tick(&sense(t, t * 200.0)).show);
            let x = (t + 0.05) * 200.0;
            m.tick(&Sense { acted, blocked, ..sense(t + 0.05, x) });
            assert!(!m.tick(&Sense { acted, blocked, ..sense(t + 0.3, x + 40.0) }).show);
        }
    }

    #[test]
    fn a_pointer_leaving_closes_them() {
        let mut m = Mind::new();
        let t = awake(&mut m, 0.5);
        let gone = |d: f32| Sense { near: false, around: false, ..sense(t + d, t * 200.0) };
        assert!(m.tick(&gone(0.0)).show, "they watch it go");
        assert_eq!(m.tick(&gone(0.6)).look, Look::Neutral);
        assert_eq!(m.tick(&gone(1.1)).look, Look::Sleepy);
        m.tick(&gone(1.3));
        assert!(!m.tick(&gone(1.6)).show);
    }

    #[test]
    fn the_edge_of_the_range_does_not_make_them_flicker() {
        let mut m = Mind::new();
        let t = awake(&mut m, 0.5);
        for i in 0..40 {
            let x = t * 200.0 + i as f32 * 8.0;
            assert!(m.tick(&Sense { near: false, around: true, ..sense(t + i as f32 * 0.05, x) }).show);
        }
    }

    #[test]
    fn a_random_peek_comes_at_its_time_and_makes_a_face() {
        let mut m = Mind::new();
        m.tick(&Sense { near: false, ..sense(1.0, 0.0) });
        assert!(m.next_random >= 900.0 && m.next_random <= 2401.0);
        let at = m.next_random;
        assert!(m.tick(&Sense { near: false, ..sense(at, 0.0) }).show);
        let face = m.tick(&Sense { near: false, ..sense(at + 2.5, 0.0) }).look;
        assert!(matches!(face, Look::Happy | Look::Wink | Look::Smug | Look::Up), "{face:?}");
        m.tick(&Sense { near: false, ..sense(at + 4.2, 0.0) });
        assert!(!m.tick(&Sense { near: false, ..sense(at + 4.5, 0.0) }).show);
    }

    #[test]
    fn an_event_waits_for_a_free_moment_then_gives_up() {
        let mut m = Mind::new();
        m.ask(Peek::Mood(Look::Love), 1.0);
        assert!(!m.tick(&Sense { blocked: true, ..sense(5.0, 0.0) }).show);
        assert_eq!(m.tick(&sense(9.0, 0.0)).look, Look::Love);
        let mut m = Mind::new();
        m.ask(Peek::Mood(Look::Love), 1.0);
        assert!(!m.tick(&sense(12.0, 0.0)).show, "too late");
    }

    #[test]
    fn a_click_cuts_a_peek_short() {
        let mut m = Mind::new();
        m.ask(Peek::Mood(Look::Love), 1.0);
        assert!(m.tick(&sense(1.1, 0.0)).show);
        m.tick(&Sense { acted: true, ..sense(1.2, 0.0) });
        assert!(!m.tick(&Sense { acted: true, ..sense(1.5, 0.0) }).show);
    }

    #[test]
    fn the_battery_scene_lunges_twice_and_ends() {
        let mut m = Mind::new();
        m.ask(Peek::Battery, 0.0);
        let hits = (0..90).map(|i| m.tick(&sense(i as f32 * 0.05, 0.0))).collect::<Vec<_>>();
        assert!(hits[20].battery && hits[20].show);
        let blows = hits.windows(2).filter(|w| w[0].lunge <= 0.5 && w[1].lunge > 0.5).count();
        assert_eq!(blows, 2);
        assert!(!hits[89].show);
    }

    #[test]
    fn an_act_winds_up_hits_and_lets_the_words_out_in_turn() {
        for kind in [ActKind::Water, ActKind::Sleep, ActKind::Breathe, ActKind::Stretch, ActKind::Rest] {
            assert_eq!(act(kind, 0.3).title, 0.0);
            assert!(act(kind, 0.7).lunge < 0.0);
            assert_eq!(act(kind, 1.0).lunge, 1.0);
            assert!(act(kind, 1.05).title > act(kind, 1.05).subtitle);
            assert!(act(kind, 1.05).subtitle > 0.0 || act(kind, 1.1).subtitle > 0.0);
            assert_eq!(act(kind, 2.0).buttons, 1.0);
        }
    }

    #[test]
    fn stretching_pulls_twice_and_lets_go() {
        let peaks = (0..400)
            .map(|i| act(ActKind::Stretch, i as f32 * 0.01).stretch)
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] > 0.3 && w[1] == 0.0)
            .count();
        assert_eq!(peaks, 2);
    }

    #[test]
    fn a_waiting_card_nudges_now_and_then_and_otherwise_rests() {
        let e = ACT_END + ACT_NUDGE;
        assert!(act(ActKind::Water, e + 0.3).lunge > 0.0);
        assert_eq!(act(ActKind::Water, e + 5.0).lunge, 0.0);
        assert!(act_moving(e + 0.2));
        assert!(!act_moving(ACT_END + 6.0));
    }

    #[test]
    fn the_bar_comes_out_every_minute_or_two_and_never_dances_the_same_twice() {
        let mut j = Jam::new();
        j.tick(1.0, true);
        assert!(j.next >= 61.0 && j.next <= 121.0);
        let mut t = j.next;
        let mut last = j.style;
        for _ in 0..30 {
            assert!(j.tick(t, true));
            assert_ne!(j.style, last);
            last = j.style;
            assert!(!j.tick(t + JAM_LENGTH + 0.1, true));
            assert!((60.0..=120.0).contains(&(j.next - (t + JAM_LENGTH))));
            t = j.next;
        }
    }

    #[test]
    fn the_bar_goes_back_when_something_needs_the_notch() {
        let mut j = Jam::new();
        j.tick(1.0, true);
        let t = j.next;
        assert!(j.tick(t, true));
        assert!(!j.tick(t + 1.0, false));
        assert!(!j.tick(t + 2.0, true), "it does not come back for the rest of its turn");
    }

    #[test]
    fn the_small_eyes_nod_blink_and_settle() {
        let mut m = Mini::new();
        for i in 0..40 {
            m.feed(i as f32 * 0.05, 0.2);
        }
        assert!(m.feed(2.05, 0.6) || m.hop.v < 0.0, "a beat moves them");
        let mut t = 2.05;
        for _ in 0..600 {
            t += 0.005;
            m.step(t, 0.005);
        }
        assert!(!m.step(t, 0.005), "they come to rest");
    }

    #[test]
    fn beats_are_jumps_in_loudness() {
        let mut b = Beat::new();
        let mut levels = vec![0.2f32; 40];
        levels.extend([0.5, 0.2, 0.0, 0.0]);
        let hits: Vec<bool> = levels.iter().map(|l| b.feed(*l)).collect();
        assert!(hits[40], "{hits:?}");
        assert!(!hits[20..40].iter().any(|h| *h), "steady sound has no beats");
        let mut quiet = Beat::new();
        assert!((0..20).all(|_| !quiet.feed(0.0)));
    }

    #[test]
    fn they_startle_when_it_darts_and_get_dizzy_when_it_shakes() {
        let mut m = Mind::new();
        let t = awake(&mut m, 0.5);
        assert_eq!(m.tick(&sense(t, t * 200.0 + 400.0)).look, Look::Surprised);
        let mut m = Mind::new();
        let t = awake(&mut m, 0.5);
        let looks: Vec<Look> = (0..8).map(|i| m.tick(&sense(t + i as f32 * 0.1, if i % 2 == 0 { 0.0 } else { 150.0 })).look).collect();
        assert_eq!(looks.last(), Some(&Look::Dizzy));
        // Moving on or leaving does not cut it short.
        let t = t + 0.8;
        for i in 0..30 {
            let tick = Sense { near: i < 15, around: i < 15, ..sense(t + i as f32 * 0.05, i as f32 * 80.0) };
            assert_eq!(m.tick(&tick).look, Look::Dizzy, "at {i}");
        }
    }
}
