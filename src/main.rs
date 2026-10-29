#![windows_subsystem = "windows"]
//! Islet: a living notch for Windows.

mod bt;
mod drop;
mod gfx;
mod media;
mod model;
mod notify;
mod sound;
mod pump;
mod sys;
mod ui;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{OleInitialize, RevokeDragDrop};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use gfx::Gfx;
use media::{Cmd, Lyrics, MediaInfo};
use model::{Clock, Phase, Pomo, Settings, Shelf, Todo, Water, WaterAlert};
use ui::{Act, Page};

pub const WM_MEDIA: u32 = WM_APP + 1;
pub const WM_VOLUME: u32 = WM_APP + 2;
pub const WM_CAPS: u32 = WM_APP + 3;
pub const WM_LANG: u32 = WM_APP + 4;
pub const WM_TRAY: u32 = WM_APP + 5;
pub const WM_NET: u32 = WM_APP + 6;
pub const WM_LYRICS: u32 = WM_APP + 7;
pub const WM_FRAME: u32 = WM_APP + 8;
pub const WM_FOREGROUND: u32 = WM_APP + 9;
pub const WM_BLUETOOTH: u32 = WM_APP + 10;
pub const WM_NOTICE: u32 = WM_APP + 11;
pub const WM_VOLKEY: u32 = WM_APP + 12;

const WM_MOUSELEAVE: u32 = 0x02A3;

const T_TICK: usize = 2;
const T_LANG: usize = 5;
const T_HOVER: usize = 6;

/// Logical window size (layout units); pixels = units × DPI scale.
pub const WIN_W: f32 = 640.0;
pub const WIN_H: f32 = 190.0;

/// Notch sizes measured from the reference app (layout units).
pub const COMPACT: [f32; 2] = [150.0, 30.0];
pub const PEEK: [f32; 2] = [208.0, 52.0];
pub const EXPANDED: [f32; 2] = [420.0, 140.0];
pub const LYRICS: [f32; 2] = [522.0, 140.0];
pub const LYRIC_PILL: [f32; 2] = [296.0, 34.0];
pub const TOAST: [f32; 2] = [320.0, 36.0];
/// Two-line toasts (Bluetooth devices, app notifications); the water reminder adds buttons.
pub const TALL_TOAST: [f32; 2] = [320.0, 56.0];
pub const WATER_TOAST: [f32; 2] = [372.0, 56.0];
const MINIMAL: [f32; 2] = [44.8, 4.0];

/// Damped spring stepped with semi-implicit Euler, as in the reference app.
#[derive(Clone, Copy)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub k: f32,
    pub c: f32,
}

impl Spring {
    pub const fn new(x: f32, k: f32, c: f32) -> Self {
        Spring { x, v: 0.0, k, c }
    }
    fn tune(&mut self, k: f32, c: f32) {
        self.k = k;
        self.c = c;
    }
    /// Returns true while still moving.
    fn step(&mut self, target: f32, dt: f32) -> bool {
        self.v += (-(self.x - target) * self.k - self.c * self.v) * dt;
        self.x += self.v * dt;
        if self.v.abs() < 0.05 && (self.x - target).abs() < 0.002 * target.abs().max(1.0) {
            self.x = target;
            self.v = 0.0;
            return false;
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Volume(bool),
    Caps(bool),
    Lang,
    Charging(bool),
    Battery,
    Net(bool),
    Water,
    Focus,
    Break,
    Task,
    Bluetooth(bool),
    Notice,
}

impl ToastKind {
    pub fn tall(self) -> bool {
        matches!(self, ToastKind::Bluetooth(_) | ToastKind::Notice | ToastKind::Water)
    }
}

#[derive(Clone)]
pub struct Toast {
    pub born: Instant,
    pub kind: ToastKind,
    pub title: String,
    pub value: String,
    pub level: Option<f32>,
    pub until: Instant,
}

pub struct Form {
    pub index: Option<usize>,
    pub title: String,
    /// Up to 8 digits filling the "HH:MM-HH:MM" mask.
    pub digits: String,
    /// 0 = title field, 1 = time field.
    pub field: u8,
}

/// Fixed state for pixel comparisons against reference captures (ISLET_DEMO).
pub struct Demo {
    pub clock: (u16, u16),
    pub date: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Compact,
    Peek,
    Expanded,
}

pub struct App {
    pub hwnd: HWND,
    pub gfx: Gfx,
    pub scale: f32,
    pos: (i32, i32),
    monitor: RECT,
    pub sw: Spring,
    pub sh: Spring,
    /// Expanded-content visibility 0..1.
    pub fade: Spring,
    /// Page transition progress: 1 at the start of a change, 0 when settled.
    pub slide: Spring,
    pub slide_dir: f32,
    pub prev_page: Page,
    /// Crossfade weights for the compact content: toast and lyric pill.
    pub toast_mix: Spring,
    pub pill_mix: Spring,
    /// The toast being faded out after it ended.
    pub last_toast: Option<Toast>,
    /// Crossfade from `last_toast` to a different toast that replaced it (0 → 1).
    pub toast_swap: Spring,
    /// Previous value of the current toast rolling out (text, direction: 1 = new from below).
    pub roll: Option<(String, f32)>,
    pub roll_t: Spring,
    /// Volume bar level, eased toward the reported volume.
    pub vol_level: Spring,
    /// Rubber-band stretch when volume is pushed past either endpoint. The
    /// toast silhouette takes the impulse while the meter keeps its width.
    pub vol_stretch: Spring,
    /// Timeline fill, eased toward the playback (or dragged) position.
    pub tl_frac: Spring,
    /// Caps Lock icon: 0 = outline arrow, 1 = filled arrow with a bright bar.
    pub caps_mix: Spring,
    /// Manual lyric scroll from the mouse wheel and when it was last moved.
    pub lyr_user: Spring,
    pub lyr_user_target: f32,
    /// Wheel scroll limits (layout units) for the lyrics on screen.
    pub lyr_bounds: (f32, f32),
    /// Lyrics font size in the expanded view (Ctrl + wheel), eased.
    pub lyr_font: Spring,
    pub lyr_font_target: f32,
    /// Visual scale easing to 1 after a lyric size step (old size / new size at the step).
    pub lyr_zoom: Spring,
    /// Minimal mode waits for the first hover or click after launch before shrinking.
    minimal_armed: bool,
    lyr_user_at: Option<Instant>,
    /// Music ↔ lyrics layout morph (0 = music page, 1 = lyrics view).
    pub lyr_mix: Spring,
    /// Water reminder snoozed until this moment.
    snooze_until: Option<Instant>,
    /// Timeline scrubbing: the bar being dragged (x, width in layout units) and the fraction.
    pub scrub: Option<(f32, f32, f32)>,
    /// Lyrics view: current line, the cascading scroll toward it (start, from, to, previous line)
    /// and cached line heights for the lyrics text they were measured from.
    pub lyr_idx: Option<usize>,
    pub lyr_anim: Option<(Instant, f32, f32, Option<usize>)>,
    pub lyr_heights: (String, Vec<f32>),
    /// Lyric pill: line shown and the one rolling out (since when).
    pub pill_cur: String,
    pub pill_prev: Option<(Instant, String)>,
    /// Drops the notch in from above when Islet starts.
    intro: Spring,
    intro_grow: Spring,
    /// Page-dot position, unwrapped so a wrap from last to first moves one step.
    pub dot_target: f32,
    pub mode: Mode,
    last_frame: Instant,
    animating: bool,
    /// Whether any spring moved on the last frame (as opposed to only looping visuals).
    springs_moving: bool,
    trimmed: bool,
    pub lyrics_on: bool,
    pub page: Page,
    pub water_sub: u8,
    pub water_settings: bool,
    pub todo_edit: bool,
    pub form: Option<Form>,
    pub toast: Option<Toast>,
    queue: VecDeque<Toast>,
    pub mouse: (f32, f32),
    mouse_in: bool,
    pub down: Option<(f32, f32, Option<Act>)>,
    /// Hover/press feedback per button, keyed by its quantized position: (spring, target).
    pub hover: std::collections::HashMap<(i32, i32), (Spring, f32)>,
    /// Water reminder switch knob (0 = off, 1 = on) and the page-dot indicator position.
    pub toggle_x: Spring,
    /// A short, local splash after recording a drink. It only drives drawing
    /// on the Water page and settles without keeping the app awake.
    pub water_pulse: Spring,
    pub dot_pos: Spring,
    pub hits: Vec<(gfx::Rc, Act)>,
    pub settings: Settings,
    pub water: Water,
    pub todo: Todo,
    pub pomo: Pomo,
    pub shelf: Shelf,
    pub drag_over: bool,
    /// Drag position in layout units and the glow that trails it.
    pub drag_pt: (f32, f32),
    pub blob: (Spring, Spring),
    /// Drag light builds up with cursor movement (0.3 → 1) and settles back when still.
    pub drag_energy: f32,
    /// Light level at the moment of the drop.
    pub drop_energy: f32,
    /// Burst played where files were dropped: start time and position.
    pub ripple: Option<(Instant, f32, f32)>,
    /// Demo: when to play the drop stretch.
    demo_drop_at: Option<Instant>,
    pub media: Arc<Mutex<MediaInfo>>,
    media_tx: Option<Sender<Cmd>>,
    pub art: Option<(u64, ID2D1Bitmap, u32)>,
    /// Shell icons and size labels for shelf items; a None icon caches a failed lookup.
    shelf_icons: std::collections::HashMap<PathBuf, (Option<ID2D1Bitmap>, String)>,
    pub lyrics: Arc<Mutex<Lyrics>>,
    pub power: Option<sys::Power>,
    layout: isize,
    hidden: bool,
    pub born: Instant,
    icon: HICON,
    taskbar_created: u32,
    _hook: Option<HHOOK>,
    pub demo: Option<Demo>,
}

static mut APP: *mut App = std::ptr::null_mut();

pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}

fn size_label(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{} KB", b / 1024),
        b if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / 1048576.0),
        b => format!("{:.1} GB", b as f64 / 1073741824.0),
    }
}

fn has_app() -> bool {
    unsafe { !APP.is_null() }
}

impl App {
    unsafe fn new(hwnd: HWND, icon: HICON, demo: Option<Demo>) -> App {
        let media = Arc::new(Mutex::new(MediaInfo::default()));
        let lyrics = Arc::new(Mutex::new(Lyrics::default()));
        let media_tx = if demo.is_none() {
            sys::spawn_network(hwnd.0 as isize);
            Some(media::spawn(hwnd.0 as isize, media.clone(), lyrics.clone()))
        } else {
            None
        };
        let gfx = Gfx::new(10, 10, 1.0).expect("Direct2D unavailable");
        App {
            hwnd,
            gfx,
            scale: 1.0,
            pos: (0, 0),
            monitor: RECT::default(),
            sw: Spring::new(COMPACT[0], 230.0, 19.0),
            sh: Spring::new(COMPACT[1], 230.0, 19.0),
            fade: Spring::new(0.0, 900.0, 60.0),
            slide: Spring::new(0.0, 230.0, 19.0),
            slide_dir: 1.0,
            prev_page: Page::Music,
            toast_mix: Spring::new(0.0, 1600.0, 80.0),
            pill_mix: Spring::new(0.0, 1600.0, 80.0),
            last_toast: None,
            toast_swap: Spring::new(1.0, 1100.0, 66.0),
            roll: None,
            roll_t: Spring::new(1.0, 1100.0, 66.0),
            vol_level: Spring::new(0.0, 260.0, 30.0),
            vol_stretch: Spring::new(0.0, 520.0, 9.0),
            tl_frac: Spring::new(0.0, 420.0, 40.0),
            caps_mix: Spring::new(0.0, 700.0, 52.0),
            lyr_user: Spring::new(0.0, 260.0, 32.0),
            lyr_user_target: 0.0,
            lyr_bounds: (0.0, 0.0),
            lyr_font: Spring::new(15.0, 300.0, 34.0),
            lyr_font_target: 15.0,
            lyr_zoom: Spring::new(1.0, 320.0, 30.0),
            minimal_armed: false,
            lyr_user_at: None,
            lyr_mix: Spring::new(0.0, 170.0, 26.0),
            snooze_until: None,
            scrub: None,
            lyr_idx: None,
            lyr_anim: None,
            lyr_heights: (String::new(), Vec::new()),
            pill_cur: String::new(),
            pill_prev: None,
            intro: Spring::new(-44.0, 600.0, 46.0),
            intro_grow: Spring::new(0.82, 420.0, 15.0),
            dot_target: 0.0,
            mode: Mode::Compact,
            last_frame: Instant::now(),
            animating: false,
            springs_moving: false,
            trimmed: false,
            lyrics_on: false,
            page: Page::Music,
            water_sub: 0,
            water_settings: false,
            todo_edit: false,
            form: None,
            toast: None,
            queue: VecDeque::new(),
            mouse: (-1.0, -1.0),
            mouse_in: false,
            down: None,
            hover: Default::default(),
            toggle_x: Spring::new(1.0, 300.0, 30.0),
            water_pulse: Spring::new(0.0, 420.0, 18.0),
            dot_pos: Spring::new(0.0, 230.0, 26.0),
            hits: Vec::new(),
            settings: Settings::load(),
            water: Water::load(),
            todo: Todo::load(),
            pomo: Pomo::new(),
            shelf: Shelf::load(),
            drag_over: false,
            drag_pt: (0.0, 0.0),
            blob: (Spring::new(0.0, 120.0, 18.0), Spring::new(0.0, 120.0, 18.0)),
            drag_energy: 0.3,
            drop_energy: 1.0,
            ripple: None,
            demo_drop_at: None,
            media,
            media_tx,
            art: None,
            shelf_icons: Default::default(),
            lyrics,
            power: sys::power(),
            layout: sys::current_layout().0,
            hidden: false,
            born: Instant::now(),
            icon,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            _hook: None,
            demo,
        }
    }

    unsafe fn init(&mut self) {
        self.lyr_font.x = self.settings.lyric_size;
        self.lyr_font_target = self.settings.lyric_size;
        self.place();
        if self.demo.is_none() {
            self.tray(NIM_ADD);
            self._hook = sys::install_keyboard_hook(self.hwnd);
            let _ = drop::register(self.hwnd);
            sys::subscribe_power(self.hwnd);
            sys::watch_volume(self.hwnd);
            sys::hook_foreground(self.hwnd);
            bt::spawn(self.hwnd);
            notify::spawn(self.hwnd);
            self.poll_power();
        }
        // This timer owns coarse state (clock, reminder eligibility and toast
        // expiry); visual motion is driven separately by the vsync frame pump.
        // Two ticks a second keeps those states prompt without repainting an
        // otherwise idle notch four times a second.
        SetTimer(Some(self.hwnd), T_TICK, 500, None);
        self.snap();
        // Starts the drop-in / grow-in intro.
        self.kick();
        self.render();
    }

    pub fn clock(&self) -> Clock {
        let mut c = model::now();
        if let Some(d) = &self.demo {
            c.hour = d.clock.0;
            c.minute = d.clock.1;
        }
        c
    }

    pub fn date_label(&self) -> String {
        match &self.demo {
            Some(d) => d.date.clone(),
            None => model::now().date_label(),
        }
    }

    /// Positions the window at the top center of the primary monitor.
    unsafe fn place(&mut self) {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        self.scale = dx as f32 / 96.0;
        self.monitor = mi.rcMonitor;
        let w = (WIN_W * self.scale).round() as i32;
        let h = (WIN_H * self.scale).round() as i32;
        let x = mi.rcMonitor.left + (mi.rcMonitor.right - mi.rcMonitor.left - w) / 2;
        let y = mi.rcMonitor.top;
        self.pos = (x, y);
        self.gfx.scale = self.scale;
        let _ = self.gfx.resize(w, h);
        let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
    }

    unsafe fn tray(&mut self, op: NOTIFY_ICON_MESSAGE) {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        for (d, s) in nid.szTip.iter_mut().zip("Islet".encode_utf16()) {
            *d = s;
        }
        let _ = Shell_NotifyIconW(op, &nid);
    }

    pub fn playing(&self) -> bool {
        let m = self.media.lock().unwrap();
        m.has && m.playing && !m.title.is_empty()
    }

    pub fn expanded(&self) -> bool {
        self.mode == Mode::Expanded
    }

    pub fn minimal(&self) -> bool {
        self.settings.minimal && self.minimal_armed
    }

    pub fn has_media(&self) -> bool {
        let m = self.media.lock().unwrap();
        m.has && !m.title.is_empty()
    }

    /// Current synced lyric line, if lyrics are on and available.
    pub fn lyric_line(&self) -> Option<String> {
        if !self.lyrics_on || !self.has_media() {
            return None;
        }
        let pos = self.media.lock().unwrap().position();
        let l = self.lyrics.lock().unwrap();
        if l.status == media::LyrStatus::Loading {
            return Some("Loading lyrics...".into());
        }
        l.line_at(pos).map(str::to_string)
    }

    /// Target notch size for the current state.
    pub fn target(&self) -> [f32; 2] {
        match self.mode {
            Mode::Expanded => {
                if self.lyrics_on && self.page == Page::Music {
                    LYRICS
                } else {
                    EXPANDED
                }
            }
            Mode::Peek => PEEK,
            Mode::Compact => {
                if let Some(t) = &self.toast {
                    if t.kind == ToastKind::Water {
                        WATER_TOAST
                    } else if t.kind.tall() {
                        TALL_TOAST
                    } else {
                        TOAST
                    }
                } else if self.minimal() {
                    // Minimal playback still needs to expose the current line
                    // when lyric mode is active; only ordinary playback uses
                    // the four-unit minimal sliver.
                    if self.playing() && self.lyric_line().is_some() {
                        LYRIC_PILL
                    } else {
                        MINIMAL
                    }
                } else if self.playing() && self.lyric_line().is_some() {
                    LYRIC_PILL
                } else {
                    COMPACT
                }
            }
        }
    }

    /// The visual notch bounds for this frame.  At a volume endpoint, Nusa
    /// pulls the outer toast edge as well as the level bar: zero extends left,
    /// full volume extends right.  Keeping the opposite edge fixed makes the
    /// spring feel anchored to the endpoint that was hit.
    pub fn notch_geometry(&self) -> (f32, f32) {
        let mut cx = WIN_W / 2.0;
        let mut w = self.sw.x;
        if self.mode == Mode::Compact
            && self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Volume(_)))
        {
            let s = self.vol_stretch.x;
            let grow = w * 0.08 * s.abs();
            w += grow;
            cx += s.signum() * grow / 2.0;
        }
        (cx, w)
    }

    fn snap(&mut self) {
        let t = self.target();
        self.sw.x = t[0];
        self.sh.x = t[1];
        self.fade.x = if self.expanded() { 1.0 } else { 0.0 };
    }

    /// Shell icon (None if unavailable) and a size label for a shelf item, cached per path.
    pub fn shelf_icon(&mut self, p: &std::path::Path) -> (Option<ID2D1Bitmap>, String) {
        if let Some(c) = self.shelf_icons.get(p) {
            return c.clone();
        }
        let px = (40.0 * self.gfx.scale).round() as i32;
        let b = drop::shell_image(p, px).and_then(|(w, h, buf)| self.gfx.bitmap(w, h, &buf));
        let size = std::fs::metadata(p).map(|m| size_label(m.len())).unwrap_or_default();
        self.shelf_icons.insert(p.to_path_buf(), (b.clone(), size.clone()));
        (b, size)
    }

    pub fn kick(&mut self) {
        self.springs_moving = true;
        if !self.animating {
            self.animating = true;
            self.last_frame = Instant::now();
            pump::resume();
        }
    }

    fn set_mode(&mut self, m: Mode) {
        if self.mode == m {
            return;
        }
        // Spring constants per transition, as measured from the reference app.
        let (k, c) = match (self.mode, m) {
            (Mode::Compact | Mode::Peek, Mode::Expanded) => (330.0, 21.0),
            (Mode::Compact, Mode::Peek) => (200.0, 29.0),
            _ => (420.0, 25.0),
        };
        self.sw.tune(k, c);
        self.sh.tune(k, c);
        if self.mode == Mode::Expanded && m != Mode::Expanded {
            self.water_settings = false;
            self.todo_edit = false;
        }
        if m == Mode::Expanded {
            self.toast = None;
            self.queue.clear();
        }
        self.mode = m;
        self.kick();
    }

    fn step(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;
        // Pages set without go_page (startup, demo) snap the dots instead of sliding.
        let pages = Page::ALL.len() as f32;
        if (self.dot_target.rem_euclid(pages).round() as usize) % Page::ALL.len() != self.page.index() {
            self.dot_target = self.page.index() as f32;
            self.dot_pos.x = self.dot_target;
        }
        // Overscrolled lyrics bounce back once the wheel stops.
        let (lo, hi) = self.lyr_bounds;
        if self.lyr_user_at.is_some_and(|t| t.elapsed() > Duration::from_millis(140)) {
            let clamped = self.lyr_user_target.clamp(lo, hi);
            if clamped != self.lyr_user_target {
                self.lyr_user_target = clamped;
                self.kick();
            }
        }
        let t = self.target();
        let n = ((dt / 0.004).ceil() as usize).max(1);
        let sub = dt / n as f32;
        let mut moving = false;
        let fade_target = if self.expanded() { 1.0 } else { 0.0 };
        let toast_target = if self.toast.is_some() { 1.0 } else { 0.0 };
        let pill_target = if self.toast.is_none() && self.playing() && self.lyric_line().is_some() { 1.0 } else { 0.0 };
        for _ in 0..n {
            moving |= self.sw.step(t[0], sub);
            moving |= self.sh.step(t[1], sub);
            moving |= self.fade.step(fade_target, sub);
            moving |= self.slide.step(0.0, sub);
            moving |= self.toast_mix.step(toast_target, sub);
            moving |= self.pill_mix.step(pill_target, sub);
            moving |= self.intro.step(0.0, sub);
            moving |= self.intro_grow.step(1.0, sub);
            moving |= self.vol_stretch.step(0.0, sub);
            moving |= self.lyr_user.step(self.lyr_user_target, sub);
            moving |= self.lyr_zoom.step(1.0, sub);
            let caps_on = matches!(self.toast.as_ref().map(|t| t.kind), Some(ToastKind::Caps(true)));
            moving |= self.caps_mix.step(if caps_on { 1.0 } else { 0.0 }, sub);
            let tl = {
                let m = self.media.lock().unwrap();
                match self.scrub {
                    Some((_, _, f)) => f,
                    None if m.dur > 0.0 => (m.position() / m.dur).clamp(0.0, 1.0) as f32,
                    None => 0.0,
                }
            };
            moving |= self.tl_frac.step(tl, sub);
            moving |= self.toast_swap.step(1.0, sub);
            moving |= self.roll_t.step(1.0, sub);
            let vol = self.toast.as_ref().filter(|t| matches!(t.kind, ToastKind::Volume(_))).and_then(|t| t.level);
            if let Some(v) = vol {
                moving |= self.vol_level.step(v, sub);
            }
            moving |= self.lyr_mix.step(if self.lyrics_on && self.page == Page::Music && self.has_media() { 1.0 } else { 0.0 }, sub);
            moving |= self.toggle_x.step(if self.water.on { 1.0 } else { 0.0 }, sub);
            moving |= self.water_pulse.step(0.0, sub);
            moving |= self.dot_pos.step(self.dot_target, sub);
            for (sp, target) in self.hover.values_mut() {
                moving |= sp.step(*target, sub);
            }
            if self.drag_over && self.drag_energy > 0.75 && self.demo.is_none() {
                // The shelf stays visibly lit for the full DragOver lifetime;
                // cursor motion enriches the aurora but must not be required
                // to keep it alive.
                self.drag_energy = (self.drag_energy - sub * 0.35).max(0.75);
            }
            moving |= self.blob.0.step(self.drag_pt.0, sub);
            moving |= self.blob.1.step(self.drag_pt.1, sub);
        }
        self.hover.retain(|_, (sp, target)| *target != 0.0 || sp.x != 0.0);
        self.springs_moving = moving;
        if !moving && !self.continuous() {
            self.animating = false;
            pump::pause();
        }
    }

    /// Looping visuals (the equalizer) that need frames even when no spring moves.
    fn continuous(&self) -> bool {
        self.effects() || !self.hidden && self.playing() && !self.pomo.running() && !(self.minimal() && self.mode == Mode::Compact)
    }

    /// Time-driven animations in progress (as opposed to the idle equalizer loop).
    fn effects(&self) -> bool {
        let fresh = |t: &Toast| t.until.saturating_duration_since(Instant::now()) > Duration::from_millis(1300);
        let recent = |t: Option<Instant>, ms: u64| t.is_some_and(|t| t.elapsed() < Duration::from_millis(ms));
        let effects = recent(self.lyr_anim.map(|x| x.0), 1100)
            || recent(self.pill_prev.as_ref().map(|x| x.0), 450)
            || self.toast.as_ref().is_some_and(|t| t.kind.tall() || fresh(t))
            || self.drag_over || self.ripple.is_some_and(|r| r.0.elapsed() < Duration::from_millis(3100));
        effects
    }

    /// Looping visuals are capped near 60 fps; spring motion runs at the display rate.
    fn frame_due(&self) -> bool {
        self.springs_moving || self.effects() || self.last_frame.elapsed() >= Duration::from_micros(15_500)
    }

    pub fn render(&mut self) {
        if self.hidden {
            return;
        }
        unsafe {
            // Everything is drawn inside the notch (plus its ears) or the minimal hover pad.
            let (cx, w) = self.notch_geometry();
            let half = (w / 2.0 + 24.0).max(72.0);
            let area = gfx::Rc::new(cx - half, 0.0, half * 2.0, self.sh.x.max(18.0) + 4.0);
            self.gfx.lift = self.intro.x;
            self.gfx.grow = self.intro_grow.x;
            self.gfx.center_x = WIN_W / 2.0;
            if self.gfx.begin(area).is_ok() {
                ui::draw(self);
                let _ = self.gfx.end(self.hwnd, self.pos.0, self.pos.1);
            }
        }
    }

    pub fn toast(&mut self, kind: ToastKind, title: &str, value: &str, level: Option<f32>) {
        if self.expanded() {
            return;
        }
        // Device and reminder toasts linger (Nusa keeps them ~4 s / until answered).
        let life = match kind {
            ToastKind::Water => 12_000,
            ToastKind::Bluetooth(_) | ToastKind::Notice => 4_000,
            _ => 1_800,
        };
        let now = Instant::now();
        let mut t = Toast {
            born: now,
            kind,
            title: title.to_string(),
            value: value.to_string(),
            level,
            until: now + Duration::from_millis(life),
        };
        let same = |a: ToastKind, b: ToastKind| std::mem::discriminant(&a) == std::mem::discriminant(&b);
        match self.toast.take() {
            Some(cur) if same(cur.kind, kind) && self.toast_mix.x > 0.5 => {
                // Same toast updating: keep its entrance time; roll the value if it changed.
                t.born = cur.born;
                if cur.value != t.value {
                    let dir = match kind {
                        ToastKind::Caps(on) => if on { 1.0 } else { -1.0 },
                        _ => 1.0,
                    };
                    self.roll = Some((cur.value.clone(), dir));
                    self.roll_t.x = 0.0;
                    self.roll_t.v = 0.0;
                }
            }
            Some(cur) => {
                // A different toast replaces the current one at once, crossfading.
                self.last_toast = Some(cur);
                self.toast_swap.x = 0.0;
                self.toast_swap.v = 0.0;
                self.roll = None;
            }
            None => {
                self.roll = None;
                if let (ToastKind::Volume(_), Some(v)) = (kind, level) {
                    self.vol_level.x = v;
                }
            }
        }
        self.toast = Some(t);
        self.queue.clear();
        self.kick();
        self.render();
    }

    pub fn drag_hover(&mut self, on: bool) {
        self.drag_over = on;
        if on {
            self.drag_energy = self.drag_energy.max(0.75);
            self.go_page(Page::Shelf);
            self.set_mode(Mode::Expanded);
        } else if !self.mouse_in && self.ripple.is_none_or(|r| r.0.elapsed() > Duration::from_millis(1200)) {
            // Stay open after a drop so the new items (and the burst) are visible.
            self.set_mode(Mode::Compact);
        }
        self.kick();
        self.render();
    }

    /// Mouse wheel over the lyrics view scrolls them; they glide back to the current line later.
    fn wheel(&mut self, notches: f32, ctrl: bool) {
        if !(self.expanded() && self.page == Page::Music && self.lyr_mix.x > 0.5) {
            return;
        }
        if ctrl {
            // Ctrl + wheel: from ~5 lines (15) up to a single large line (26).
            // The layout jumps straight to the new size (one re-wrap); a scale glides from the
            // old size so the step looks continuous without re-wrapping every frame.
            let old = self.lyr_font_target;
            let new = (old + notches.signum() * 2.0).clamp(15.0, 26.0);
            if new == old {
                return;
            }
            self.lyr_zoom.x *= old / new;
            self.lyr_font_target = new;
            self.lyr_font.x = new;
            self.settings.lyric_size = new;
            if self.demo.is_some() {
                // Demo captures must not change the user's saved settings.
                self.kick();
                return;
            }
            self.settings.save();
            self.kick();
            return;
        }
        let (lo, hi) = self.lyr_bounds;
        let mut t = self.lyr_user_target - notches * 36.0;
        // Past either end the list follows with growing resistance.
        if t < lo {
            t = lo - (lo - t).min(160.0) * 0.35;
        } else if t > hi {
            t = hi + (t - hi).min(160.0) * 0.35;
        }
        self.lyr_user_target = t;
        self.lyr_user_at = Some(Instant::now());
        self.kick();
    }

    fn dismiss_toast(&mut self) {
        self.last_toast = self.toast.take();
        self.kick();
        self.render();
    }

    /// A toast with its own buttons stays put under the cursor instead of opening the notch.
    fn interactive_toast(&self) -> bool {
        self.toast.as_ref().is_some_and(|t| t.kind == ToastKind::Water)
    }

    /// Tracks the cursor during a drag (screen pixels).
    pub fn drag_move(&mut self, x: i32, y: i32, snap: bool) {
        let s = self.gfx.scale;
        let pt = ((x - self.pos.0) as f32 / s, (y - self.pos.1) as f32 / s);
        if snap {
            self.blob.0.x = pt.0;
            self.blob.1.x = pt.1;
            self.drag_energy = 0.75;
        } else {
            let moved = ((pt.0 - self.drag_pt.0).powi(2) + (pt.1 - self.drag_pt.1).powi(2)).sqrt();
            self.drag_energy = (self.drag_energy + moved / 260.0).min(1.0);
        }
        self.drag_pt = pt;
        self.kick();
    }

    pub fn drop_burst(&mut self) {
        self.ripple = Some((Instant::now(), self.drag_pt.0, self.drag_pt.1));
        self.drop_energy = self.drag_energy.max(0.7);
        // The notch takes the drop like a landing: it stretches a little and springs back.
        self.sw.v += 460.0;
        self.sh.v += 170.0;
        self.kick();
    }

    pub fn shelf_add(&mut self, paths: Vec<PathBuf>) {
        if !paths.is_empty() {
            self.shelf.add(paths);
        }
        self.kick();
        self.render();
    }

    fn go_page(&mut self, p: Page) {
        if p == self.page {
            return;
        }
        let from = self.page.index() as i32;
        let to = p.index() as i32;
        let n = Page::ALL.len() as i32;
        let fwd = (to - from).rem_euclid(n) <= n / 2;
        self.slide_dir = if fwd { 1.0 } else { -1.0 };
        let step = if fwd { (to - from).rem_euclid(n) } else { -(from - to).rem_euclid(n) };
        self.dot_target += step as f32;
        self.prev_page = self.page;
        self.page = p;
        self.slide.x = 1.0;
        self.slide.v = 0.0;
        self.water_settings = false;
        self.todo_edit = false;
        self.form = None;
        self.kick();
    }

    fn set_noactivate(&self, on: bool) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, if on { ex | flag } else { ex & !flag });
        }
    }

    fn open_form(&mut self, index: Option<usize>) {
        let (title, digits) = index
            .and_then(|i| self.todo.tasks.get(i))
            .map(|t| (t.title.clone(), t.time.chars().filter(char::is_ascii_digit).collect()))
            .unwrap_or_default();
        self.form = Some(Form { index, title, digits, field: 0 });
        self.set_noactivate(false);
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
    }

    fn close_form(&mut self, save: bool) {
        if let Some(f) = self.form.take() {
            let title = f.title.trim().to_string();
            if save && !title.is_empty() {
                let time = ui::time_mask(&f.digits).trim_end_matches(['_', ':', '-']).to_string();
                let time = if f.digits.len() >= 4 { time } else { String::new() };
                match f.index {
                    Some(i) if i < self.todo.tasks.len() => {
                        self.todo.tasks[i].title = title;
                        self.todo.tasks[i].time = time;
                    }
                    _ => self.todo.tasks.push(model::Task { title, done_day: 0, time }),
                }
                self.todo.save();
            }
        }
        self.set_noactivate(true);
    }

    fn media_cmd(&self, c: Cmd) {
        if let Some(tx) = &self.media_tx {
            let _ = tx.send(c);
        }
    }

    fn act(&mut self, a: Act) {
        let clock = self.clock();
        match a {
            Act::PagePrev => self.go_page(self.page.prev()),
            Act::PageNext => self.go_page(self.page.next()),
            Act::Page(p) => self.go_page(p),
            Act::Prev => self.media_cmd(Cmd::Prev),
            Act::Toggle => {
                // Optimistic UI: flip immediately, the session poll will confirm.
                let mut m = self.media.lock().unwrap();
                m.pos = m.position();
                m.stamp = Some(Instant::now());
                m.playing = !m.playing;
                drop(m);
                self.media_cmd(Cmd::Toggle);
            }
            Act::Next => self.media_cmd(Cmd::Next),
            Act::Lyrics => self.lyrics_on = !self.lyrics_on,
            // Scrubbing is handled by the mouse down/move/up handlers.
            Act::Scrub(..) => {}
            Act::Seek(ms) => {
                let secs = ms as f64 / 1000.0;
                let mut m = self.media.lock().unwrap();
                m.pos = secs;
                m.stamp = Some(Instant::now());
                drop(m);
                self.media_cmd(Cmd::Seek(secs));
            }
            Act::PomoToggle => {
                if self.pomo.running() {
                    self.pomo.pause();
                } else {
                    self.pomo.start();
                }
            }
            Act::PomoReset => {
                self.pomo.reset();
                self.pomo.task = None;
            }
            Act::PomoPhase(p) => self.pomo.set_phase(p),
            Act::Drank => {
                self.water.drank(clock.hour as usize);
                self.water_pulse.v += 7.5;
                self.kick();
            }
            Act::ToastDrank => {
                self.water.drank(clock.hour as usize);
                self.water_pulse.v += 7.5;
                self.kick();
                self.dismiss_toast();
            }
            Act::ToastSnooze => {
                self.snooze_until = Some(Instant::now() + Duration::from_secs(10 * 60));
                self.dismiss_toast();
            }
            Act::WaterOn => {
                self.water.on = !self.water.on;
                self.water.save();
            }
            Act::WaterSub(s) => self.water_sub = s,
            Act::WaterSettings(on) => self.water_settings = on,
            Act::WaterReset => self.water.reset(),
            Act::Bottle(d) => {
                let i = model::BOTTLES.iter().position(|&b| b == self.water.bottle).unwrap_or(2) as i32;
                let i = (i + d).clamp(0, model::BOTTLES.len() as i32 - 1) as usize;
                self.water.bottle = model::BOTTLES[i];
                self.water.inb = self.water.inb.min(self.water.bottle);
                self.water.save();
            }
            Act::Goal(g) => {
                self.water.goal = g;
                self.water.save();
            }
            Act::Start(d) => {
                let s = (self.water.start as i32 + d * 60).clamp(0, self.water.end as i32 - 60);
                self.water.start = s as u32;
                self.water.save();
            }
            Act::End(d) => {
                let e = (self.water.end as i32 + d * 60).clamp(self.water.start as i32 + 60, 24 * 60);
                self.water.end = e as u32;
                self.water.save();
            }
            Act::TodoAdd => self.open_form(None),
            Act::TodoEditMode => {
                if !self.todo.tasks.is_empty() {
                    self.todo_edit = !self.todo_edit;
                }
            }
            Act::TodoToggle(i) => {
                if self.todo_edit {
                    self.open_form(Some(i));
                } else {
                    self.todo.toggle(i);
                }
            }
            Act::TodoDelete(i) => {
                if i < self.todo.tasks.len() {
                    self.todo.tasks.remove(i);
                    self.todo.save();
                }
                if self.todo.tasks.is_empty() {
                    self.todo_edit = false;
                }
            }
            Act::FormField(f) => {
                if let Some(form) = self.form.as_mut() {
                    form.field = f;
                }
            }
            Act::FormClear => {
                if let Some(form) = self.form.as_mut() {
                    form.title.clear();
                }
            }
            Act::FormSave => self.close_form(true),
            Act::FormCancel => self.close_form(false),
            Act::ShelfOpen(i) => {
                if let Some(p) = self.shelf.items.get(i) {
                    unsafe {
                        ShellExecuteW(Some(self.hwnd), w!("open"), &HSTRING::from(p.as_os_str()), None, None, SW_SHOWNORMAL);
                    }
                }
            }
            Act::ShelfRemove(i) => self.shelf.remove(i),
            Act::ShelfClear => self.shelf.clear(),
            Act::None => {}
        }
        self.kick();
        self.render();
    }

    fn announce_phase(&mut self, p: Phase) {
        sound::play(sound::Chime::Soft);
        match p {
            Phase::Focus => self.toast(ToastKind::Focus, "Focus Time!", "25 min", None),
            Phase::Short => self.toast(ToastKind::Break, "Break Time!", "5 min", None),
            Phase::Long => self.toast(ToastKind::Break, "Long Break!", "15 min", None),
        }
    }

    fn tick(&mut self) {
        if self.demo_drop_at.is_some_and(|t| Instant::now() >= t) {
            self.demo_drop_at = None;
            self.sw.v += 460.0;
            self.sh.v += 170.0;
            self.kick();
        }
        if self.lyr_user_at.is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
            self.lyr_user_at = None;
            self.lyr_user_target = 0.0;
            self.kick();
        }
        if self.continuous() {
            self.kick();
        }
        // Startup (WinRT, fonts, Direct2D) touches far more memory than steady state needs;
        // hand those pages back once things have settled.
        if !self.trimmed && self.born.elapsed() > Duration::from_secs(8) {
            self.trimmed = true;
            unsafe {
                let _ = windows::Win32::System::ProcessStatus::K32EmptyWorkingSet(windows::Win32::System::Threading::GetCurrentProcess());
            }
        }
        if let Some(p) = self.pomo.tick() {
            self.announce_phase(p);
        }
        let clock = self.clock();
        self.water.roll_day();
        if self.snooze_until.is_some_and(|t| Instant::now() >= t) {
            self.snooze_until = None;
            let ml = self.water.each();
            sound::play(sound::Chime::Soft);
            self.toast(ToastKind::Water, "Time to Drink", &format!("Stay hydrated! · {ml} ml"), None);
        }
        if self.demo.is_none() && idle_seconds() < 300 {
            match self.water.check_due(clock.minutes()) {
                Some(WaterAlert::Drink(ml)) => {
                    sound::play(sound::Chime::Soft);
                    self.toast(ToastKind::Water, "Time to Drink", &format!("Stay hydrated! · {ml} ml"), None);
                }
                Some(WaterAlert::Refill) => {
                    sound::play(sound::Chime::Soft);
                    self.toast(ToastKind::Water, "Time to Refill", "Your bottle is empty", None);
                }
                None => {}
            }
        }
        if let Some(t) = &self.toast {
            if Instant::now() >= t.until {
                self.last_toast = self.toast.take();
                self.toast = self.queue.pop_front().map(|mut t| {
                    t.until = Instant::now() + Duration::from_millis(1800);
                    t
                });
                self.kick();
            }
        }
        // Album art lives in the media thread; upload it once per track.
        let art = {
            let m = self.media.lock().unwrap();
            m.art.clone().map(|a| (m.art_id, a))
        };
        match art {
            Some((id, a)) if self.art.as_ref().map(|x| x.0) != Some(id) => {
                self.art = self.gfx.bitmap(a.w, a.h, &a.px).map(|b| (id, b, a.avg));
            }
            None => self.art = None,
            _ => {}
        }
        // Size depends on live state (music, lyrics, timers); let the springs follow.
        let t = self.target();
        if (t[0] - self.sw.x).abs() > 0.5 || (t[1] - self.sh.x).abs() > 0.5 {
            self.kick();
        }
        self.render();
    }

    fn poll_power(&mut self) {
        let Some(p) = sys::power() else { return };
        if let Some(old) = self.power {
            if p.battery {
                let lv = Some(p.pct as f32 / 100.0);
                if p.ac != old.ac {
                    if p.ac {
                        self.toast(ToastKind::Charging(true), "Charging", &format!("{}%", p.pct), lv);
                    } else {
                        self.toast(ToastKind::Charging(false), "Charger unplugged", &format!("{}%", p.pct), lv);
                    }
                } else if !p.ac && old.pct > 20 && p.pct <= 20 && p.pct > 10 {
                    sound::play(sound::Chime::Alert);
                    self.toast(ToastKind::Battery, "Low battery", &format!("{}%", p.pct), lv);
                } else if !p.ac && old.pct > 10 && p.pct <= 10 {
                    sound::play(sound::Chime::Alert);
                    self.toast(ToastKind::Battery, "Battery critical", &format!("{}%", p.pct), lv);
                } else if p.ac && old.pct < 100 && p.pct == 100 {
                    self.toast(ToastKind::Charging(true), "Fully charged", "100%", Some(1.0));
                }
            }
        }
        self.power = Some(p);
    }

    fn watch(&mut self) {
        let cover = sys::foreground_covers(self.monitor, self.hwnd) && !self.expanded();
        if cover != self.hidden {
            self.hidden = cover;
            unsafe {
                let _ = ShowWindow(self.hwnd, if cover { SW_HIDE } else { SW_SHOWNOACTIVATE });
            }
            if !cover {
                self.render();
            }
        }
    }

    fn on_mouse_move(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        if !self.mouse_in {
            self.mouse_in = true;
            self.minimal_armed = true;
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            if self.mode == Mode::Compact && !self.interactive_toast() {
                self.set_mode(Mode::Peek);
                unsafe { SetTimer(Some(self.hwnd), T_HOVER, 90, None) };
            }
        }
        if let Some((bx, bw, _)) = self.scrub {
            self.scrub = Some((bx, bw, ((x - bx) / bw).clamp(0.0, 1.0)));
        }
        // Dragging a shelf tile out to another app.
        if let Some((dx, dy, Some(Act::ShelfOpen(i)))) = self.down {
            if (x - dx).abs() + (y - dy).abs() > 8.0 {
                self.down = None;
                if let Some(p) = self.shelf.items.get(i).cloned() {
                    let _ = drop::drag_out(self.hwnd, &p);
                }
            }
        }
        self.render();
    }

    fn on_mouse_leave(&mut self) {
        self.mouse_in = false;
        self.mouse = (-1.0, -1.0);
        self.down = None;
        self.scrub = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), T_HOVER);
        }
        if self.form.is_none() && !self.drag_over {
            self.set_mode(Mode::Compact);
        }
        self.kick();
        self.render();
    }

    fn hit(&self, x: f32, y: f32) -> Option<Act> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| *a)
    }

    fn on_down(&mut self, x: f32, y: f32) {
        let act = self.hit(x, y);
        if let Some(Act::Scrub(bx, bw)) = act {
            let (bx, bw) = (bx as f32 / 10.0, bw as f32 / 10.0);
            self.scrub = Some((bx, bw, ((x - bx) / bw).clamp(0.0, 1.0)));
        }
        self.down = Some((x, y, act));
        self.render();
    }

    fn on_up(&mut self, x: f32, y: f32) {
        let Some((_, _, pressed)) = self.down.take() else { return };
        if let Some((_, _, f)) = self.scrub.take() {
            let dur = self.media.lock().unwrap().dur;
            if dur > 0.0 {
                self.act(Act::Seek((f as f64 * dur * 1000.0) as u32));
            }
            self.render();
            return;
        }
        if !self.expanded() {
            if self.interactive_toast() {
                if let (Some(a), Some(b)) = (pressed, self.hit(x, y)) {
                    if a == b {
                        self.act(a);
                    }
                }
                return;
            }
            self.set_mode(Mode::Expanded);
            self.render();
            return;
        }
        if let (Some(a), Some(b)) = (pressed, self.hit(x, y)) {
            if a == b {
                self.act(a);
            }
        }
        self.render();
    }

    fn on_char(&mut self, c: u32) {
        let Some(f) = self.form.as_mut() else { return };
        match c {
            8 => {
                if f.field == 0 {
                    f.title.pop();
                } else {
                    f.digits.pop();
                }
            }
            9 => f.field ^= 1,
            13 => return self.act(Act::FormSave),
            27 => return self.act(Act::FormCancel),
            c if c >= 32 => {
                if let Some(ch) = char::from_u32(c) {
                    if f.field == 0 {
                        if f.title.chars().count() < 60 {
                            f.title.push(ch);
                        }
                    } else if ch.is_ascii_digit() && f.digits.len() < 8 {
                        f.digits.push(ch);
                    }
                }
            }
            _ => {}
        }
        self.render();
    }

    unsafe fn menu(&mut self) {
        let Ok(m) = CreatePopupMenu() else { return };
        let check = |b: bool| if b { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(m, MF_STRING | check(sys::autostart_enabled()), 1, w!("Start with Windows"));
        let _ = AppendMenuW(m, MF_STRING | check(self.settings.pet), 2, w!("Islet Mascot"));
        let _ = AppendMenuW(m, MF_STRING | check(self.settings.minimal), 3, w!("Minimal notch"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(m, MF_STRING, 4, w!("About Islet"));
        let _ = AppendMenuW(m, MF_STRING, 5, w!("Exit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(self.hwnd);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, None, self.hwnd, None);
        let _ = DestroyMenu(m);
        match cmd.0 {
            1 => sys::set_autostart(!sys::autostart_enabled()),
            2 => {
                self.settings.pet = !self.settings.pet;
                self.settings.save();
            }
            3 => {
                self.settings.minimal = !self.settings.minimal;
                self.minimal_armed = true;
                self.settings.save();
            }
            4 => {
                MessageBoxW(
                    Some(self.hwnd),
                    w!("Islet 0.2.0\nA living notch for Windows.\n\nMusic, lyrics, focus timer, water reminder, to-do and file shelf.\nLyrics provided by LRCLIB (lrclib.net)."),
                    w!("About Islet"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            5 => {
                let _ = DestroyWindow(self.hwnd);
            }
            _ => {}
        }
        self.kick();
        self.render();
    }
}

fn idle_seconds() -> u32 {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if GetLastInputInfo(&mut li).as_bool() {
            return windows::Win32::System::SystemInformation::GetTickCount().wrapping_sub(li.dwTime) / 1000;
        }
    }
    0
}

fn lparam_xy(lp: LPARAM) -> (f32, f32) {
    let s = app().scale;
    let x = (lp.0 & 0xFFFF) as i16 as f32 / s;
    let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / s;
    (x, y)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if !has_app() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    let a = app();
    match msg {
        WM_FRAME => {
            pump::consumed();
            if a.animating && a.frame_due() {
                a.step();
                a.render();
            }
        }
        WM_TIMER => match wp.0 {
            T_TICK => a.tick(),
            T_HOVER => {
                let _ = KillTimer(Some(hwnd), T_HOVER);
                if a.mouse_in && a.mode == Mode::Peek {
                    a.set_mode(Mode::Expanded);
                }
            }
            T_LANG => {
                let _ = KillTimer(Some(hwnd), T_LANG);
                let (id, code, native) = sys::current_layout();
                if id != a.layout {
                    a.layout = id;
                    a.toast(ToastKind::Lang, &native, &code, None);
                }
            }
            _ => {}
        },
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lp);
            a.on_mouse_move(x, y);
        }
        WM_MOUSELEAVE => a.on_mouse_leave(),
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lp);
            a.on_down(x, y);
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_xy(lp);
            a.on_up(x, y);
        }
        WM_MOUSEACTIVATE => {
            return LRESULT(if a.form.is_some() { MA_ACTIVATE } else { MA_NOACTIVATE } as isize);
        }
        WM_CHAR => a.on_char(wp.0 as u32),
        WM_MEDIA | WM_LYRICS => {
            a.kick();
            a.tick();
        }
        WM_VOLUME => {
            let v = (wp.0 & 0xFFFF) as f32 / 1000.0;
            let muted = wp.0 >> 16 != 0;
            let pct = (v * 100.0).round() as u32;
            a.toast(ToastKind::Volume(muted), "Volume", &format!("{pct}%"), Some(if muted { 0.0 } else { v }));
        }
        WM_CAPS => {
            let on = wp.0 != 0;
            a.toast(ToastKind::Caps(on), "Caps Lock", if on { "On" } else { "Off" }, None);
        }
        WM_LANG => {
            SetTimer(Some(hwnd), T_LANG, 180, None);
        }
        WM_NET => match wp.0 {
            2 => a.toast(ToastKind::Net(true), "Connected", "Online", None),
            1 => a.toast(ToastKind::Net(false), "No internet access", "Limited", None),
            _ => a.toast(ToastKind::Net(false), "Disconnected", "Offline", None),
        },
        WM_TRAY => match (lp.0 & 0xFFFF) as u32 {
            WM_LBUTTONUP => {
                let m = if a.expanded() { Mode::Compact } else { Mode::Expanded };
                a.set_mode(m);
                a.render();
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => a.menu(),
            _ => {}
        },
        WM_POWERBROADCAST => a.poll_power(),
        WM_FOREGROUND => a.watch(),
        WM_VOLKEY => {
            let up = wp.0 != 0;
            if let Some((v, muted)) = sys::volume_state() {
                let at_end = if up { v >= 0.999 && !muted } else { v <= 0.001 || muted };
                if at_end {
                    let lv = if muted { 0.0 } else { v };
                    a.toast(ToastKind::Volume(muted), "Volume", &format!("{}%", (lv * 100.0).round()), Some(lv));
                    a.vol_stretch.v += if up { 7.0 } else { -7.0 };
                    a.kick();
                }
            }
        }
        WM_MOUSEWHEEL => {
            let delta = (wp.0 >> 16) as u16 as i16 as f32 / 120.0;
            // The key-state flag can be missing for an unfocused window; ask the keyboard directly.
            let ctrl = wp.0 & 0x0008 != 0 || GetAsyncKeyState(VK_CONTROL.0 as i32) < 0;
            a.wheel(delta, ctrl);
        }
        WM_NOTICE => {
            let n = Box::from_raw(lp.0 as *mut notify::Notice);
            a.toast(ToastKind::Notice, &n.text, &n.app, None);
        }
        WM_BLUETOOTH => {
            let c = Box::from_raw(lp.0 as *mut bt::Change);
            let state = if c.connected { "Connected" } else { "Disconnected" };
            let level = c.battery.filter(|_| c.connected).map(|b| b as f32 / 100.0);
            a.toast(ToastKind::Bluetooth(c.connected), &c.name, state, level);
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
            a.place();
            a.render();
            if msg == WM_DPICHANGED {
                return LRESULT(0);
            }
        }
        WM_DESTROY => {
            a.tray(NIM_DELETE);
            let _ = RevokeDragDrop(hwnd);
            PostQuitMessage(0);
        }
        m if m == a.taskbar_created && m != 0 => a.tray(NIM_ADD),
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(0)
}

/// Parses ISLET_DEMO, e.g.
/// `page=music;clock=20:06;date=Wed 28 Oct 2026;title=…;artist=…;art=C:\a.png;pos=100;dur=290;expanded=1`.
fn demo_setup(a: &mut App, spec: &str) {
    let kv: std::collections::HashMap<&str, &str> = spec.split(';').filter_map(|p| p.split_once('=')).collect();
    // Demo states start from empty personal data so captures are reproducible.
    a.shelf.items = kv.get("shelf").map(|v| v.split('|').map(PathBuf::from).collect()).unwrap_or_default();
    a.todo.tasks.clear();
    a.settings.minimal = kv.get("minimal") == Some(&"1");
    a.minimal_armed = a.settings.minimal;
    a.settings.pet = kv.get("pet") == Some(&"1");
    if let Some((x, y)) = kv.get("dropped").and_then(|v| v.split_once(',')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) {
        // Starts the drop glow a little after launch so a recording catches all of it.
        a.ripple = Some((Instant::now() + Duration::from_millis(2500), x, y));
        a.demo_drop_at = Some(Instant::now() + Duration::from_millis(2500));
    }
    if let Some((x, y)) = kv.get("drag").and_then(|v| v.split_once(',')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) {
        a.drag_over = true;
        a.drag_energy = kv.get("energy").and_then(|v| v.parse().ok()).unwrap_or(1.0);
        a.drag_pt = (x, y);
        a.blob.0.x = x;
        a.blob.1.x = y;
    }
    a.water.taken = 0;
    a.water.hours = [0; 24];
    a.water.goal = 2000;
    a.water.bottle = 500;
    a.water.start = 8 * 60;
    a.water.end = 17 * 60;
    a.water.on = true;
    {
        let mut m = a.media.lock().unwrap();
        if let Some(t) = kv.get("title") {
            m.has = true;
            m.title = t.to_string();
            m.artist = kv.get("artist").unwrap_or(&"").to_string();
            m.playing = kv.get("playing").is_none_or(|v| *v == "1");
            m.pos = kv.get("pos").and_then(|v| v.parse().ok()).unwrap_or(0.0);
            m.dur = kv.get("dur").and_then(|v| v.parse().ok()).unwrap_or(200.0);
            m.stamp = None;
            if let Some(p) = kv.get("art") {
                if let Ok(bytes) = std::fs::read(p) {
                    m.art = media::decode_art(&bytes).ok();
                    m.art_id = 1;
                }
            }
        }
    }
    if let Some(p) = kv.get("page") {
        a.page = match *p {
            "pomo" => Page::Pomo,
            "water" => Page::Water,
            "shelf" => Page::Shelf,
            "todo" => Page::Todo,
            _ => Page::Music,
        };
    }
    if kv.get("charging") == Some(&"1") {
        a.power = Some(sys::Power { ac: true, pct: 100, battery: true });
    }
    if kv.get("expanded") == Some(&"1") {
        a.mode = Mode::Expanded;
    }
    if kv.get("lyrics") == Some(&"1") {
        a.lyrics_on = true;
    }
    // Demo: synced lyrics from a local .lrc file, with the position running.
    if let Some(text) = kv.get("lrc").and_then(|p| std::fs::read_to_string(p).ok()) {
        let mut l = a.lyrics.lock().unwrap();
        l.lines = media::parse_lrc(&text);
        l.status = media::LyrStatus::Ready;
        l.key = "demo".into();
        a.media.lock().unwrap().stamp = Some(Instant::now());
    }
    if kv.get("watersettings") == Some(&"1") {
        a.water_settings = true;
    }
    if let Some(s) = kv.get("watersub") {
        a.water_sub = s.parse().unwrap_or(0);
    }
    if let Some(ml) = kv.get("water") {
        a.toast = Some(Toast {
            born: Instant::now() - Duration::from_secs(5),
            kind: ToastKind::Water,
            title: "Time to Drink".into(),
            value: format!("Stay hydrated! · {ml} ml"),
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    if let Some(name) = kv.get("bt") {
        a.toast = Some(Toast {
            born: Instant::now(),
            kind: ToastKind::Bluetooth(true),
            title: name.to_string(),
            value: "Connected".into(),
            level: Some(1.0),
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    if let Some(t) = kv.get("caps") {
        a.toast = Some(Toast {
            born: Instant::now() - Duration::from_secs(5),
            kind: ToastKind::Caps(*t == "1"),
            title: "Caps Lock".into(),
            value: if *t == "1" { "On".into() } else { "Off".into() },
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    if let Some(v) = kv.get("volume").and_then(|v| v.parse::<f32>().ok()) {
        a.toast = Some(Toast {
            born: Instant::now() - Duration::from_secs(5),
            kind: ToastKind::Volume(false),
            title: "Volume".into(),
            value: String::new(),
            level: Some(v),
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    // Capture-only control for verifying the endpoint rubber-band geometry.
    // It is accepted solely by the existing ISLET_DEMO fixture and does not
    // participate in normal volume input.
    if let Some(stretch) = kv.get("stretch").and_then(|v| v.parse::<f32>().ok()) {
        a.vol_stretch.x = stretch.clamp(-1.0, 1.0);
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let demo_spec = std::env::var("ISLET_DEMO").ok();
        let mutex_name = if demo_spec.is_some() { w!("Local\\Islet.Demo") } else { w!("Local\\Islet.SingleInstance") };
        let _mutex = CreateMutexW(None, true, mutex_name);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let _ = OleInitialize(None);
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let icon = sys::make_icon(32);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            hIcon: icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("IsletNotch"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("IsletNotch"),
            w!("Islet"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(hinst),
            None,
        ) else {
            return;
        };
        let demo = demo_spec.as_ref().map(|spec| {
            let kv: std::collections::HashMap<&str, &str> = spec.split(';').filter_map(|p| p.split_once('=')).collect();
            let clock = kv
                .get("clock")
                .and_then(|c| c.split_once(':'))
                .and_then(|(h, m)| Some((h.parse().ok()?, m.parse().ok()?)))
                .unwrap_or((20, 6));
            Demo { clock, date: kv.get("date").unwrap_or(&"Wed 28 Oct 2026").to_string() }
        });
        pump::start(hwnd);
        APP = Box::into_raw(Box::new(App::new(hwnd, icon, demo)));
        if let Some(spec) = &demo_spec {
            demo_setup(app(), spec);
        }
        app().init();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
