//! Demo-only event storm: drives the app's real message handlers with a random but repeatable
//! stream of events (volume, brightness, Caps Lock, Wi-Fi, charger, Bluetooth and wired devices,
//! notifications, messages, reminders, hover, clicks, opening the widgets, stepping aside) and
//! checks the state after each step. `ISLET_DEMO="storm=120;seed=7;stormlog=<file>"` runs it and
//! writes a report, then exits.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, PostMessageW, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};

use crate::devices::{Card, DeviceKind};
use crate::{App, Mode, ToastKind};

pub struct Storm {
    rng: u64,
    seed: u64,
    started: Instant,
    end: Instant,
    next: Instant,
    log: PathBuf,
    counts: BTreeMap<&'static str, u32>,
    /// Invariant name -> when it first failed (it must hold again within a second).
    open_failures: BTreeMap<&'static str, (Instant, String)>,
    failures: Vec<String>,
    render_ms: Vec<f32>,
    gaps_ms: Vec<f32>,
    last_frame: Option<Instant>,
    settle_from: Option<Instant>,
}

impl Storm {
    pub fn new(secs: u64, seed: u64, log: PathBuf) -> Self {
        let now = Instant::now();
        // A panic inside a window procedure aborts the process; leave its message in the report.
        let path = log.clone();
        std::panic::set_hook(Box::new(move |info| {
            let _ = std::fs::write(&path, format!("PANIC: {info}\n"));
        }));
        Storm {
            rng: seed.max(1),
            seed,
            started: now,
            end: now + Duration::from_secs(secs),
            next: now + Duration::from_millis(500),
            log,
            counts: BTreeMap::new(),
            open_failures: BTreeMap::new(),
            failures: Vec::new(),
            render_ms: Vec::new(),
            gaps_ms: Vec::new(),
            last_frame: None,
            settle_from: None,
        }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn pick(&mut self, n: u64) -> u64 {
        self.rand() % n
    }

    /// A frame was rendered in `took`; gaps are measured between frames while animating.
    pub fn frame(&mut self, took: Duration, animating: bool) {
        let now = Instant::now();
        self.render_ms.push(took.as_secs_f32() * 1000.0);
        if let (Some(last), true) = (self.last_frame, animating) {
            self.gaps_ms.push((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = animating.then_some(now);
    }
}

impl App {
    /// Runs from a 20 ms timer while the storm is on.
    pub fn storm_tick(&mut self) {
        // The pointer poll that normally keeps capture exclusion in step does not run in demos.
        self.update_privacy();
        let Some(mut s) = self.storm.take() else { return };
        let now = Instant::now();
        if now < s.end {
            if now >= s.next {
                let ev = self.storm_event(&mut s);
                *s.counts.entry(ev).or_default() += 1;
                let gap = 80 + s.pick(320);
                s.next = now + Duration::from_millis(gap);
            }
        } else {
            // Events stop; everything must come to rest on its own.
            if self.mouse_in {
                self.on_mouse_leave();
            }
            if self.mode == Mode::Expanded {
                self.set_mode(Mode::Compact);
            }
            self.yielded = false;
            // Luma's cards wait for an answer (or 45 s): answer them like a user would.
            if self.brain.showing.is_some() && s.settle_from.is_none() {
                self.act(crate::ui::Act::LumaLater);
            }
            self.brain.stop_focus(chrono::Local::now());
            let from = *s.settle_from.get_or_insert(now);
            if now - from > Duration::from_secs(16) {
                self.storm_settled(&mut s);
                self.storm_report(&s);
                std::process::exit(0);
            }
        }
        self.storm_check(&mut s);
        self.storm = Some(s);
    }

    fn storm_event(&mut self, s: &mut Storm) -> &'static str {
        let hwnd = self.hwnd;
        let post = |msg: u32, wp: usize, lp: isize| unsafe {
            let _ = PostMessageW(Some(hwnd), msg, WPARAM(wp), LPARAM(lp));
        };
        let kinds = [
            DeviceKind::Headphones,
            DeviceKind::Headset,
            DeviceKind::Speaker,
            DeviceKind::Mouse,
            DeviceKind::Keyboard,
            DeviceKind::Gamepad,
            DeviceKind::Phone,
        ];
        let (cx, w) = self.notch_geometry();
        match s.pick(24) {
            0 => {
                let v = s.pick(1001) as usize;
                let muted = s.pick(10) == 0;
                post(crate::WM_VOLUME, v | ((muted as usize) << 16), 0);
                "volume"
            }
            1 => {
                let v = [0, 10, 50, 90, 100][s.pick(5) as usize];
                self.brightness_changed(v);
                "brightness"
            }
            2 => {
                post(crate::WM_CAPS, s.pick(2) as usize, 0);
                "caps lock"
            }
            3 => {
                let status = [0usize, 2][s.pick(2) as usize];
                let n = Box::into_raw(Box::new(crate::sys::NetChange { status, ssid: Some("Phong 2_5G".into()), bars: Some(3) }));
                post(crate::WM_NET, 0, n as isize);
                "wifi"
            }
            4 => {
                let on = s.pick(2) == 0;
                if on {
                    self.toast(ToastKind::Charging(true), "Charging", "Charger connected", Some(0.94));
                } else {
                    self.toast(ToastKind::Charging(false), "Charger unplugged", "94% remaining", Some(0.94));
                }
                "charger"
            }
            5 => {
                let kind = kinds[s.pick(kinds.len() as u64) as usize];
                let c = Box::into_raw(Box::new(crate::bt::Change {
                    name: "Storm device".into(),
                    connected: s.pick(3) != 0,
                    battery: [None, Some(2), Some(80)][s.pick(3) as usize],
                    kind,
                }));
                post(crate::WM_BLUETOOTH, 0, c as isize);
                "bluetooth"
            }
            6 => {
                let drive = s.pick(4) == 0;
                let card = Card {
                    connected: s.pick(3) != 0,
                    kind: if drive { DeviceKind::Drive } else { kinds[s.pick(kinds.len() as u64) as usize] },
                    name: if drive { "KINGSTON (E:)".into() } else { "USB Receiver".into() },
                    root: drive.then(|| "E:\\".into()),
                    detail: String::new(),
                };
                self.show_device(card);
                "usb device"
            }
            7 | 8 => {
                let (app, aumid, site) = match s.pick(3) {
                    0 => ("Google Chrome", "Chrome", Some("www.youtube.com".to_string())),
                    1 => ("Snipping Tool", "Microsoft.ScreenSketch_8wekyb3d8bbwe!App", None),
                    _ => ("Zalo", "", None),
                };
                let n = Box::into_raw(Box::new(crate::notify::Notice {
                    app: app.into(),
                    title: "Storm notice".into(),
                    body: "A notification body long enough to wrap onto a second line in the detail view.".into(),
                    aumid: aumid.into(),
                    source: None,
                    site,
                    id: 0,
                    phone_app: None,
                    call: false,
                }));
                post(crate::WM_NOTICE, 0, n as isize);
                "notice"
            }
            9 => {
                let n = Box::into_raw(Box::new(crate::notify::Notice {
                    app: "Phone Link".into(),
                    title: format!("Sender {}", s.pick(3)),
                    body: "Hello from the phone".into(),
                    aumid: "Microsoft.YourPhone_8wekyb3d8bbwe!App".into(),
                    source: None,
                    site: None,
                    id: 0,
                    phone_app: Some("Zalo".into()),
                    call: false,
                }));
                post(crate::WM_NOTICE, 0, n as isize);
                "message (phone)"
            }
            10 => {
                let n = Box::into_raw(Box::new(crate::notify::Notice {
                    app: "Google Chrome".into(),
                    title: "Denis".into(),
                    body: "Tối nay đi ăn không?".into(),
                    aumid: "Chrome".into(),
                    source: Some("facebook"),
                    site: Some("www.facebook.com".into()),
                    id: 0,
                    phone_app: None,
                    call: false,
                }));
                post(crate::WM_NOTICE, 0, n as isize);
                "message (notification)"
            }
            11 => {
                match s.pick(3) {
                    0 => self.toast(ToastKind::Focus, "Focus Time!", "Back to work, 25 min", None),
                    1 => self.toast(ToastKind::Break, "Break Time!", "5 minute short break", None),
                    _ => self.toast(ToastKind::Water, "Time to Drink", "Stay hydrated! · 250 ml", None),
                }
                "reminder"
            }
            12 | 13 => {
                // The pointer comes to rest on the notch (hover grows notifications / opens widgets).
                let x = cx - w / 2.0 + 10.0 + (s.pick(1000) as f32 / 1000.0) * (w - 20.0).max(1.0);
                self.on_mouse_move(x, 12.0);
                "pointer on notch"
            }
            14 => {
                if self.mouse_in {
                    self.on_mouse_leave();
                }
                "pointer leaves"
            }
            15 => {
                // Click: the ✕ of an open notification, or the notch itself.
                if self.toast_open {
                    self.act(crate::ui::Act::NoticeClose);
                    "click close"
                } else if self.mode == Mode::Expanded {
                    // Never click inside the widgets: that would change the user's real data.
                    self.on_mouse_leave();
                    "pointer leaves"
                } else {
                    let (x, y) = (cx, 12.0);
                    self.on_down(x, y);
                    self.on_up(x, y);
                    "click notch"
                }
            }
            16 => {
                let m = if self.mode == Mode::Expanded { Mode::Compact } else { Mode::Expanded };
                self.set_mode(m);
                self.render();
                "widgets open/close"
            }
            17 if s.pick(3) == 0 => {
                // Several devices at once (PC waking with headphones, a receiver and a drive).
                for (name, kind) in [("Burst headphones", DeviceKind::Headphones), ("Burst receiver", DeviceKind::Mouse), ("Burst drive", DeviceKind::Drive)] {
                    self.show_device(Card { connected: true, kind, name: name.into(), root: None, detail: String::new() });
                }
                "device burst"
            }
            17 => {
                self.yielded = !self.yielded;
                self.kick();
                "step aside on/off"
            }
            22 => {
                // Hidden by a full-screen app and shown again: the whole notch must come back.
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                }
                self.gfx.invalidate();
                self.render();
                "hide and show"
            }
            23 => {
                self.history_open = !self.history_open && self.mode == Mode::Expanded;
                self.history_top = 0;
                self.render();
                "history"
            }
            19 => {
                // A Luma reminder comes (as a heads-up or straight as its card).
                use crate::luma::config::Kind as K;
                use crate::luma::scheduler::Due;
                if self.brain.showing.is_none() {
                    let due = match s.pick(6) {
                        0 => Due::Builtin(K::Water),
                        1 => Due::Builtin(K::Stretch),
                        2 => Due::Builtin(K::Eyes),
                        3 => Due::Builtin(K::Breathe),
                        4 => Due::Builtin(K::Sleep),
                        _ => Due::Focus(25),
                    };
                    let now = chrono::Local::now();
                    if s.pick(2) == 0 {
                        self.brain.showing = Some(crate::luma::Showing { due: due.clone(), heads_up_until: Some(now + chrono::TimeDelta::seconds(3)), card_at: None });
                        self.present_heads_up(&due, 3);
                    } else {
                        self.brain.showing = Some(crate::luma::Showing { due: due.clone(), heads_up_until: None, card_at: None });
                        self.present_luma(&due);
                    }
                }
                "luma reminder"
            }
            20 => {
                // Answering it: done, later, or "now" on a heads-up.
                let act = [crate::ui::Act::LumaDone, crate::ui::Act::LumaLater, crate::ui::Act::LumaNow][s.pick(3) as usize];
                self.act(act);
                "luma answer"
            }
            21 => {
                let now = chrono::Local::now();
                match s.pick(4) {
                    0 => self.brain.start_pomodoro(25, 5, 4, "storm", now),
                    1 => self.brain.toggle_pause(now),
                    2 => self.brain.skip_phase(now),
                    _ => self.brain.stop_focus(now),
                }
                "focus"
            }
            _ => {
                self.toast(ToastKind::Caps(true), "Caps Lock", "On", None);
                self.toast(ToastKind::Volume(false), "Volume", "40%", Some(0.4));
                "two toasts at once"
            }
        }
    }

    /// Invariants that must hold (or recover within a second) at every moment.
    fn storm_check(&mut self, s: &mut Storm) {
        let now = Instant::now();
        let mut fail = |name: &'static str, bad: bool, state: String| {
            if bad {
                s.open_failures.entry(name).or_insert((now, state));
            } else {
                s.open_failures.remove(name);
            }
        };
        let state = self.storm_state();
        fail("open detail without a notification", self.toast_open && !self.notice_toast(), state.clone());
        fail("toast while the widgets are open", self.mode == Mode::Expanded && self.toast.is_some(), state.clone());
        fail("held notification not shown after closing", self.mode != Mode::Expanded && self.held_notice.is_some(), state.clone());
        let stuck = self.toast.as_ref().is_some_and(|t| now > t.until + Duration::from_millis(1500)) && !self.toast_open && !self.yielded && !self.hidden;
        fail("toast past its time", stuck, state.clone());
        let message = [self.toast.as_ref(), self.last_toast.as_ref()].iter().flatten().any(|t| t.kind == ToastKind::Message) && self.toast_mix.x > 0.01;
        let grace = self.private_until.is_some_and(|t| now < t);
        fail("capture exclusion out of step", self.private != (message || grace), state.clone());
        let luma_card = self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Water | ToastKind::Remind(_) | ToastKind::HeadsUp | ToastKind::Task | ToastKind::Focus));
        fail("Luma reminder waiting unseen", self.brain.showing.is_some() && self.toast.is_none() && self.mode != Mode::Expanded && !luma_card, state.clone());
        let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
        fail("window hidden", !visible && !self.hidden, state);
        // Luma's ticker runs every 2 s, so its card may take that long to come back.
        let grace = |name: &str| Duration::from_millis(if name.starts_with("Luma") { 3000 } else { 1000 });
        let expired: Vec<_> = s.open_failures.iter().filter(|(k, (t, _))| now - *t > grace(k)).map(|(k, v)| (*k, v.1.clone())).collect();
        for (name, st) in expired {
            s.failures.push(format!("{:>6.1}s  {name}  [{st}]", (now - s.started).as_secs_f32()));
            s.open_failures.remove(name);
        }
    }

    /// After the storm: the notch must be at rest at its target size with nothing left over.
    fn storm_settled(&mut self, s: &mut Storm) {
        let t = self.target();
        let state = self.storm_state();
        let at = (Instant::now() - s.started).as_secs_f32();
        if (self.sw.x - t[0]).abs() > 1.0 || (self.sh.x - t[1]).abs() > 1.0 {
            s.failures.push(format!("{at:>6.1}s  size not at target {:?} (is {:.1}x{:.1})  [{state}]", t, self.sw.x, self.sh.x));
        }
        if self.animating && !self.continuous() {
            let springs = [
                ("sw", &self.sw), ("sh", &self.sh), ("fade", &self.fade), ("slide", &self.slide), ("toast_mix", &self.toast_mix),
                ("pill_mix", &self.pill_mix), ("intro", &self.intro), ("intro_grow", &self.intro_grow), ("vol_stretch", &self.vol_stretch),
                ("sun_rot", &self.sun_rot), ("lyr_user", &self.lyr_user), ("lyr_zoom", &self.lyr_zoom), ("caps_mix", &self.caps_mix),
                ("tl_frac", &self.tl_frac), ("toast_swap", &self.toast_swap), ("roll_t", &self.roll_t), ("vol_level", &self.vol_level),
                ("lyr_mix", &self.lyr_mix), ("water_pulse", &self.water_pulse), ("dot_pos", &self.dot_pos),
                ("aurora_lvl", &self.aurora_lvl),
            ];
            let moving: Vec<String> = springs.iter().filter(|(_, sp)| sp.v.abs() >= 0.05 || !sp.x.is_finite()).map(|(n, sp)| format!("{n} x={:.3} v={:.3}", sp.x, sp.v)).collect();
            s.failures.push(format!("{at:>6.1}s  still animating at rest: {}  hover={}  [{state}]", moving.join(", "), self.hover.len()));
        }
        if !self.device_queue.is_empty() {
            s.failures.push(format!("{at:>6.1}s  device cards never shown: {}  [{state}]", self.device_queue.len()));
        }
        if self.toast.is_some() || self.toast_open || self.held_notice.is_some() {
            s.failures.push(format!("{at:>6.1}s  leftover toast state  [{state}]"));
        }
    }

    fn storm_state(&self) -> String {
        format!(
            "mode={:?} toast={:?} open={} yielded={} held={} private={} size={:.0}x{:.0} animating={} moving={} pump={:?} since_frame={}ms hidden={} toast_age={:?} toast_left={:?} device_queue={} luma={:?}",
            self.mode,
            self.toast.as_ref().map(|t| t.kind),
            self.toast_open,
            self.yielded,
            self.held_notice.is_some(),
            self.private,
            self.sw.x,
            self.sh.x,
            self.animating,
            self.springs_moving,
            crate::pump::state(),
            self.last_frame.elapsed().as_millis(),
            self.hidden,
            self.toast.as_ref().map(|t| t.born.elapsed().as_millis()),
            self.toast.as_ref().map(|t| t.until.saturating_duration_since(Instant::now()).as_millis()),
            self.device_queue.len(),
            self.brain.showing.as_ref().map(|s| s.due.clone())
        )
    }

    fn storm_report(&self, s: &Storm) {
        let mut out = String::new();
        let _ = writeln!(out, "Islet event storm, seed {}, {:.0} s of events", s.seed, (s.end - s.started).as_secs_f32());
        let total: u32 = s.counts.values().sum();
        let _ = writeln!(out, "\nEvents ({total}):");
        for (k, n) in &s.counts {
            let _ = writeln!(out, "  {k:<24} {n}");
        }
        let pct = |v: &[f32], p: f32| -> f32 {
            if v.is_empty() {
                return 0.0;
            }
            let mut v = v.to_vec();
            v.sort_by(|a, b| a.total_cmp(b));
            v[((v.len() - 1) as f32 * p).round() as usize]
        };
        let _ = writeln!(
            out,
            "\nFrames: {} rendered; render ms p50 {:.2} p99 {:.2} max {:.2}; gap while animating p99 {:.1} max {:.1} ms",
            s.render_ms.len(),
            pct(&s.render_ms, 0.5),
            pct(&s.render_ms, 0.99),
            pct(&s.render_ms, 1.0),
            pct(&s.gaps_ms, 0.99),
            pct(&s.gaps_ms, 1.0)
        );
        let _ = writeln!(out, "Frame pump restarts after a stall: {}", self.pump_stalls);
        let _ = writeln!(out, "\nFailures ({}):", s.failures.len());
        for f in &s.failures {
            let _ = writeln!(out, "  {f}");
        }
        let _ = std::fs::write(&s.log, out);
    }
}

/// Starts the storm's 20 ms driver on the demo window.
pub fn timer(hwnd: HWND) {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), crate::T_STORM, 20, None);
    }
}
