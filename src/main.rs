#![windows_subsystem = "windows"]
//! Islet: a living notch for Windows.

mod anim;
mod bt;
mod devices;
mod drop;
mod eyes;
mod gfx;
mod luma;
mod media;
mod model;
mod notify;
mod sound;
mod pump;
mod storm;
mod sys;
mod ui;
mod wheel;

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
use luma::config::Kind as CareKind;
use luma::scheduler::{Due, Outcome};
use model::{Clock, Phase, Settings, Shelf, Todo};
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
pub const WM_ICON: u32 = WM_APP + 14;
pub const WM_SHOT: u32 = WM_APP + 15;
pub const WM_AUDIODEV: u32 = WM_APP + 16;
pub const WM_NOTICEGONE: u32 = WM_APP + 18;
/// Whether the separate Luma app is still active (WPARAM 1), checked once at start.
pub const WM_LUMAAPP: u32 = WM_APP + 17;

const WM_MOUSELEAVE: u32 = 0x02A3;

const T_TICK: usize = 2;
const T_LANG: usize = 5;
const T_HOVER: usize = 6;
const T_DRAGZONE: usize = 8;
/// Collects a device's several interfaces into one card before showing it.
const T_DEVICE: usize = 9;
/// Demo-only event storm driver (see storm.rs).
const T_STORM: usize = 10;

/// Logical window size (layout units); pixels = units × DPI scale.
pub const WIN_W: f32 = 640.0;
pub const WIN_H: f32 = 190.0;

/// Notch sizes measured from the reference app (layout units).
pub const COMPACT: [f32; 2] = [150.0, 30.0];
pub const PEEK: [f32; 2] = [208.0, 52.0];
pub const EXPANDED: [f32; 2] = [420.0, 140.0];
pub const LYRICS: [f32; 2] = [522.0, 140.0];
pub const LYRIC_PILL: [f32; 2] = [296.0, 34.0];
pub const ACT_TOAST: [f32; 2] = [470.0, 58.0];
/// How far the lyric bar grows for the eyes.
pub const JAM_ROOM: f32 = 90.0;
/// The accent of the character in use.
pub const CHARACTER: u32 = 0xFF2D2D;
/// A pause shorter than this leaves the equalizer running.
const EQ_HOLD_MS: u64 = 500;
pub const TOAST: [f32; 2] = [320.0, 36.0];
/// Two-line toasts (Bluetooth devices, app notifications); the water reminder adds buttons.
pub const TALL_TOAST: [f32; 2] = [320.0, 56.0];
pub const WATER_TOAST: [f32; 2] = [372.0, 56.0];
/// End-of-phase Pomodoro toast, measured from the Nusa screenshot (386×76 px at 125 %).
pub const PHASE_TOAST: [f32; 2] = [310.0, 61.0];
/// Chat message toast: icon, sender and two lines of text.
pub const MESSAGE_TOAST: [f32; 2] = [360.0, 78.0];
/// A notification grown into its detail view (hovered or clicked).
pub const NOTICE_OPEN: [f32; 2] = [420.0, 160.0];
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
    pub fn tune(&mut self, k: f32, c: f32) {
        self.k = k;
        self.c = c;
    }
    /// Returns true while still moving.
    pub fn step(&mut self, target: f32, dt: f32) -> bool {
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// A wired device plugged in (true) or out: USB mouse/keyboard/receiver, phone, drive, audio.
    Device(bool),
    Notice,
    /// A chat message (iPhone lock-screen style): sender in `value`, message in `title`.
    Message,
    /// Screen brightness bar.
    Brightness,
    /// Water goal reached.
    Goal,
    /// Luma: a stretch, eye, breathing or bedtime break (Later / Done).
    Remind(CareKind),
    /// Luma: a reminder coming in a few seconds (Later / Now).
    HeadsUp,
    /// The separate Luma app is still running: offer to turn it off.
    LumaApp,
    /// Someone is calling (Accept / Decline): caller in `value`, app in `title`.
    Call,
    /// The song that has just started: title in `title`, artist in `value`.
    Song,
    /// Preview of an animated icon.
    Sample(u8),
}

impl ToastKind {
    pub fn tall(self) -> bool {
        matches!(
            self,
            ToastKind::Bluetooth(_)
                | ToastKind::Sample(_)
                | ToastKind::Song
                | ToastKind::Device(_)
                | ToastKind::Notice
                | ToastKind::Message
                | ToastKind::Water
                | ToastKind::Focus
                | ToastKind::Break
                | ToastKind::Task
                | ToastKind::Charging(_)
                | ToastKind::Battery
                | ToastKind::Net(_)
                | ToastKind::Goal
                | ToastKind::Remind(_)
                | ToastKind::HeadsUp
                | ToastKind::LumaApp
                | ToastKind::Call
        )
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

/// Where a to-do lives: a Luma reminder (has a time) or an untimed to-do of Islet's.
#[derive(Clone, PartialEq, Debug)]
pub enum TodoKey {
    Timed(String),
    Untimed(usize),
}

/// One row of the To-do page.
pub struct TodoItem {
    pub key: TodoKey,
    pub title: String,
    /// "HH:MM" or "HH:MM-HH:MM"; empty when untimed.
    pub time: String,
    pub done: bool,
}

pub struct Form {
    /// The row being edited; None for a new one.
    pub key: Option<TodoKey>,
    pub title: String,
    /// Up to 8 digits filling the "HH:MM-HH:MM" mask.
    pub digits: String,
    /// 0 = title field, 1 = time field.
    pub field: u8,
}

/// Left, top, right, bottom in layout units.
type Zone = (f32, f32, f32, f32);

fn touches(a: Zone, b: Zone) -> bool {
    a.0 <= b.2 && a.2 >= b.0 && a.1 <= b.3 && a.3 >= b.1
}

/// Fixed state for pixel comparisons against reference captures (ISLET_DEMO).
pub struct Demo {
    pub clock: (u16, u16),
    pub date: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Compact,
    Peek,
    Expanded,
}

/// One Bluetooth or wired-device card, kept whole while it waits for its turn.
pub struct DeviceCard {
    pub kind: ToastKind,
    pub name: String,
    pub sub: String,
    pub level: Option<f32>,
    pub device: devices::DeviceKind,
    pub root: Option<String>,
}

/// Time on screen for a text: `base`, plus 40 ms per character past the first 40, at most 12 s.
fn reading_ms(base: u64, text: &str) -> u64 {
    (base + 40 * text.chars().count().saturating_sub(40) as u64).min(12_000)
}

const HISTORY_MAX: usize = 50;

/// How long each device card shows while others are waiting.
const DEVICE_TURN: Duration = Duration::from_millis(1500);

/// Where a notification came from; decides its badge and what "Open" does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NoticeKind {
    /// A chat message (Facebook/Messenger), from a notification or a browser tab title.
    Chat,
    /// From the phone, through Phone Link.
    Phone,
    /// A website's notification shown by a browser.
    Browser,
    /// Snipping Tool / Win+Shift+S / Print Screen.
    Screenshot,
    App,
}

/// A known app's own colour.
fn brand_colour(name: &str) -> Option<u32> {
    let n = name.to_lowercase();
    const BRANDS: [(&str, u32); 17] = [
        ("messenger", 0x7A5CFF), ("zalo", 0x0068FF), ("facebook", 0x1877F2), ("youtube", 0xFF0033), ("gmail", 0xEA4335),
        ("shopee", 0xEE4D2D), ("chatgpt", 0x10A37F), ("claude", 0xD97757), ("telegram", 0x2AABEE), ("discord", 0x5865F2),
        ("instagram", 0xE1306C), ("tiktok", 0xFE2C55), ("whatsapp", 0x25D366), ("spotify", 0x1DB954), ("github", 0x8957E5),
        ("teams", 0x6264A7), ("outlook", 0x0F6CBD),
    ];
    BRANDS.iter().find(|(k, _)| n.contains(k)).map(|(_, c)| *c)
}

/// The most prominent colour of an icon (premultiplied BGRA): the average of its vivid pixels.
fn icon_colour(px: &[u8]) -> Option<u32> {
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for p in px.chunks_exact(4) {
        let (hi, lo) = (p[0].max(p[1]).max(p[2]) as u32, p[0].min(p[1]).min(p[2]) as u32);
        if p[3] > 200 && hi > 60 && (hi - lo) * 10 > hi * 3 {
            b += p[0] as u64;
            g += p[1] as u64;
            r += p[2] as u64;
            n += 1;
        }
    }
    (n > 0).then(|| ((r / n) << 16 | (g / n) << 8 | (b / n)) as u32)
}

/// The icon Phone Link keeps for an app on the phone, e.g. app_icon_com.vng.zalo1.0.jpg.
fn phone_icon(app: &str) -> Option<PathBuf> {
    let name: String = app.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    // Bundle ids that don't contain the app's name.
    let name = match name.as_str() {
        "zalo" => "zingalo".to_string(),
        "chatgpt" => "openai".to_string(),
        _ => name,
    };
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Packages\\Microsoft.YourPhone_8wekyb3d8bbwe\\TempState\\app_icons");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_lowercase().contains(&name))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

/// The notification behind the current Notice/Message toast, for the hover detail view.
#[derive(Clone)]
pub struct NoticeInfo {
    pub kind: NoticeKind,
    pub app: String,
    pub aumid: String,
    pub site: Option<String>,
    /// Windows notification id (0 when it came from a tab title).
    pub id: u32,
    pub title: String,
    pub body: String,
    pub born: Instant,
    /// Screenshot file and its thumbnail, once found.
    pub file: Option<PathBuf>,
    pub thumb: Option<ID2D1Bitmap>,
    pub file_size: String,
    /// The app on the phone, for a phone notification.
    pub phone_app: Option<String>,
    /// Where its icon comes from.
    pub icon: Option<PathBuf>,
}

impl NoticeInfo {
    fn of(n: &notify::Notice) -> Self {
        let a = n.aumid.to_lowercase();
        let kind = if n.phone_app.is_some() {
            NoticeKind::Phone
        } else if n.source == Some("facebook") {
            NoticeKind::Chat
        } else if a.contains("screensketch") || a.contains("screenclipping") || n.app.to_lowercase().contains("snipping") {
            NoticeKind::Screenshot
        } else if n.site.is_some() {
            NoticeKind::Browser
        } else {
            NoticeKind::App
        };
        NoticeInfo {
            kind,
            app: n.app.clone(),
            aumid: n.aumid.clone(),
            site: n.site.clone(),
            id: n.id,
            title: n.title.clone(),
            body: n.body.clone(),
            born: Instant::now(),
            file: None,
            thumb: None,
            file_size: String::new(),
            phone_app: n.phone_app.clone(),
            icon: match &n.phone_app {
                Some(app) => phone_icon(app),
                None => (!n.aumid.is_empty()).then(|| PathBuf::from(format!("shell:AppsFolder\\{}", n.aumid))),
            },
        }
    }

    /// The name people know it by: the phone app, else the site, else the app.
    pub fn source_name(&self) -> String {
        self.phone_app.clone().or_else(|| self.site.as_ref().map(|s| s.trim_start_matches("www.").to_string())).unwrap_or_else(|| self.app.clone())
    }

    /// Whether "Open" can do something for this notification.
    pub fn openable(&self) -> bool {
        match self.kind {
            NoticeKind::Screenshot => self.file.is_some(),
            NoticeKind::Chat | NoticeKind::Phone => true,
            NoticeKind::Browser => self.site.is_some(),
            NoticeKind::App => !self.aumid.is_empty(),
        }
    }
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
    /// Brightness sun rotation (degrees); each change turns it by the change, as Nusa does.
    pub sun_rot: Spring,
    pub sun_target: f32,
    /// Pending brightness rubber band (+1 right at 100 %, -1 left at 0 %), played on arrival.
    bright_bounce: Option<f32>,
    /// What the current notification toast came from, for the hover detail view.
    pub notice: Option<NoticeInfo>,
    /// The notification toast is grown into its detail view (hovered or clicked).
    pub toast_open: bool,
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
    pub lyrics_on: bool,
    pub page: Page,
    pub water_sub: u8,
    pub water_settings: bool,
    /// The rolling pickers on show, by what they set.
    pub wheels: [wheel::Wheel; 4],
    wheel_held: Option<ui::WheelId>,
    wheels_moving: bool,
    pub todo_edit: bool,
    pub form: Option<Form>,
    pub toast: Option<Toast>,
    queue: VecDeque<Toast>,
    /// A notification that arrived while the widgets were open; shown when they close.
    held_notice: Option<Toast>,
    /// Clock minute last drawn by the idle tick.
    drawn_minute: u32,
    /// Current period of the pointer/yield poll (ms).
    zone_every: u32,
    /// Capture exclusion stays on until this moment after the last message card.
    private_until: Option<Instant>,
    pub eyes: eyes::Eyes,
    eye_feel: eyes::Feel,
    pub mini: eyes::Mini,
    pub jam: eyes::Jam,
    pub jam_x: Spring,
    jam_show: bool,
    /// The props, plain and in the character's colour.
    props: std::collections::HashMap<(&'static str, bool), Option<ID2D1Bitmap>>,
    /// How far the equalizer bars are up (0..1), and when music was last heard of.
    pub eq_mix: Spring,
    last_playing: Instant,
    meter_on: bool,
    mini_busy: bool,
    /// A reminder held back for a moment while the eyes look down at it.
    card_after: Option<(Due, Instant)>,
    /// "Later" answers in a row.
    later_row: u32,
    last_drink: Instant,
    last_song: String,
    /// Preview run (ISLET_DEMO "sample=tour|eyes|jam|battery|0..11"): start and what it shows.
    sample: Option<(Instant, String)>,
    /// What the current Bluetooth/Device card is about (its icon), and a drive it can open.
    pub device_kind: devices::DeviceKind,
    pub device_root: Option<String>,
    devices: Option<devices::Tracker>,
    /// Device cards waiting their turn (see `present_device`).
    device_queue: VecDeque<DeviceCard>,
    /// Last Bluetooth card: audio endpoints of the same device appear with it and are not shown.
    last_bt: Option<Instant>,
    /// Audio endpoint name and when it was last announced (one jack reports several endpoints).
    last_audio: Option<(String, bool, Instant)>,
    pending_audio: Option<(Instant, devices::Card)>,
    /// Demo-only event storm under test.
    storm: Option<Box<storm::Storm>>,
    /// Times the frame pump had to be restarted (reported by the storm test).
    pump_stalls: u32,
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
    /// Luma's reminders, water, focus/Pomodoro and timed tasks.
    pub brain: luma::Brain,
    /// Untimed to-dos (timed ones are Luma reminders).
    pub todo: Todo,
    /// The Pomodoro page's chosen mode while nothing runs.
    pub pomo_mode: Phase,
    /// Ticks (500 ms) since start; the brain runs every fourth.
    brain_ticks: u32,
    pub shelf: Shelf,
    pub drag_over: bool,
    /// Drag position in layout units and the glow that trails it.
    pub drag_pt: (f32, f32),
    pub blob: (Spring, Spring),
    /// Drag light builds up with cursor movement (0.3 → 1) and settles back when still.
    pub drag_energy: f32,
    /// While a mouse button is held near the notch the whole window takes hits, so a file
    /// dragged into its surroundings (not only onto the notch) opens the shelf, like Nusa.
    pub catch_all: bool,
    /// Box of the looping indicator (equalizer / timer) drawn in the last frame.
    pub loop_rect: Option<gfx::Rc>,
    /// Last screen brightness reported, and the last chat sender shown.
    brightness: Option<u32>,
    /// Apps seen coming from the phone (lower case), kept between runs: while the phone is
    /// connected their PC versions stay quiet.
    phone_apps: Vec<String>,
    /// The phone's last connection card: Bluetooth flapping doesn't get one each time.
    phone_card: Option<Instant>,
    /// Tasks whose start-time reminder already fired today: (title, day).
    /// Water goal reached toast shown for this day.
    /// Whether the current button press began outside the window (a drag coming in).
    press_known: bool,
    press_outside: bool,
    /// The notch has stepped aside (minimal sliver) because the user is working right below it.
    pub yielded: bool,
    /// Hovering doesn't open the notch until then (just back from stepping aside).
    no_hover_until: Option<Instant>,
    /// Last moment a reason to stay out of the way was seen, and since when the cursor has been
    /// working in the zone next to the notch.
    yield_seen: Instant,
    near_since: Option<Instant>,
    /// Previous left/right button state, to catch fresh clicks.
    buttons_down: bool,
    /// The last click landed in the zone next to the notch (likely focusing a field there).
    click_in_zone: bool,
    click_covered: bool,
    /// Cursor position when the hover timer was armed (expand only once the cursor rests).
    hover_anchor: (f32, f32),
    /// Screen capture exclusion currently applied (message privacy).
    private: bool,
    /// Bottom light level, eased; rises as a dragged file nears the notch's bottom edge.
    pub aurora_lvl: Spring,
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
    icons_pending: std::collections::HashSet<PathBuf>,
    /// Icon colours, measured when an icon loads.
    icon_tints: std::collections::HashMap<PathBuf, u32>,
    /// Recent notifications, newest first; in memory only, gone when Islet closes.
    pub history: VecDeque<NoticeInfo>,
    pub history_unread: u32,
    pub history_open: bool,
    /// First row shown in the history list.
    pub history_top: usize,
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
            sun_rot: Spring::new(0.0, 900.0, 60.0),
            sun_target: 0.0,
            bright_bounce: None,
            notice: None,
            toast_open: false,
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
            lyrics_on: false,
            page: Page::Music,
            water_sub: 0,
            water_settings: false,
            wheels: [wheel::Wheel::new(model::CUPS.len(), 2), wheel::Wheel::new(model::GOALS.len(), 2), wheel::Wheel::new(24, 22), wheel::Wheel::new(4, 0)],
            wheel_held: None,
            wheels_moving: false,
            todo_edit: false,
            form: None,
            toast: None,
            queue: VecDeque::new(),
            held_notice: None,
            drawn_minute: u32::MAX,
            zone_every: 50,
            private_until: None,
            eyes: eyes::Eyes::new(),
            eye_feel: eyes::Feel::ASLEEP,
            mini: eyes::Mini::new(),
            jam: eyes::Jam::new(),
            jam_x: Spring::new(0.0, 170.0, 21.0),
            jam_show: false,
            props: std::collections::HashMap::new(),
            eq_mix: Spring::new(0.0, 170.0, 24.0),
            last_playing: Instant::now() - Duration::from_secs(60),
            meter_on: false,
            mini_busy: false,
            card_after: None,
            later_row: 0,
            last_drink: Instant::now(),
            last_song: String::new(),
            sample: None,
            device_kind: devices::DeviceKind::Other,
            device_root: None,
            devices: None,
            last_bt: None,
            device_queue: VecDeque::new(),
            last_audio: None,
            pending_audio: None,
            storm: None,
            pump_stalls: 0,
            mouse: (-1.0, -1.0),
            mouse_in: false,
            down: None,
            hover: Default::default(),
            toggle_x: Spring::new(1.0, 300.0, 30.0),
            water_pulse: Spring::new(0.0, 420.0, 18.0),
            dot_pos: Spring::new(0.0, 230.0, 26.0),
            hits: Vec::new(),
            settings: Settings::load(),
            brain: if demo.is_none() { luma::Brain::open(chrono::Local::now()) } else { luma::Brain::demo(chrono::Local::now()) },
            todo: Todo::load(),
            pomo_mode: Phase::Focus,
            brain_ticks: 0,
            shelf: Shelf::load(),
            drag_over: false,
            drag_pt: (0.0, 0.0),
            blob: (Spring::new(0.0, 120.0, 18.0), Spring::new(0.0, 120.0, 18.0)),
            drag_energy: 0.3,
            catch_all: false,
            loop_rect: None,
            brightness: None,
            phone_apps: std::fs::read_to_string(model::data_dir().join("phone_apps.txt")).unwrap_or_default().lines().map(str::to_string).collect(),
            phone_card: None,
            press_known: false,
            press_outside: false,
            yielded: false,
            no_hover_until: None,
            yield_seen: Instant::now(),
            near_since: None,
            buttons_down: false,
            click_in_zone: false,
            click_covered: false,
            hover_anchor: (0.0, 0.0),
            private: false,
            aurora_lvl: Spring::new(0.0, 60.0, 15.0),
            drop_energy: 1.0,
            ripple: None,
            demo_drop_at: None,
            media,
            media_tx,
            art: None,
            shelf_icons: Default::default(),
            icons_pending: Default::default(),
            icon_tints: Default::default(),
            history: VecDeque::new(),
            history_unread: 0,
            history_open: false,
            history_top: 0,
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
        notify::HIDE_BANNERS.store(self.settings.hide_banners, std::sync::atomic::Ordering::Relaxed);
        self.lyr_font.x = self.settings.lyric_size;
        self.lyr_font_target = self.settings.lyric_size;
        if self.demo.is_none() {
            self.lyrics_on = self.settings.lyrics;
        }
        self.place();
        if self.demo.is_none() {
            self.tray(NIM_ADD);
            self._hook = sys::install_keyboard_hook(self.hwnd);
            let _ = drop::register(self.hwnd);
            sys::subscribe_power(self.hwnd);
            sys::watch_volume(self.hwnd);
            sys::hook_foreground(self.hwnd);
            bt::spawn(self.hwnd);
            self.devices = Some(devices::Tracker::start(self.hwnd));
            sys::watch_audio_devices(self.hwnd);
            notify::spawn(self.hwnd);
            sys::spawn_focus_watch();
            if self.settings.hide_banners {
                sys::hide_phone_banners();
            }
            self.poll_power();
            self.take_over_from_luma();
        }
        // This timer owns coarse state (clock, reminder eligibility and toast
        // expiry); visual motion is driven separately by the vsync frame pump.
        // Two ticks a second keeps those states prompt without repainting an
        // otherwise idle notch four times a second.
        SetTimer(Some(self.hwnd), T_TICK, 500, None);
        if self.demo.is_none() {
            SetTimer(Some(self.hwnd), T_DRAGZONE, 50, None);
        }
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
        // Stepping aside: only reminders that need an answer keep their size.
        let urgent = self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Water | ToastKind::Focus | ToastKind::Break | ToastKind::Remind(_) | ToastKind::Task | ToastKind::HeadsUp | ToastKind::Call));
        if self.yielded && self.mode == Mode::Compact && !urgent {
            return MINIMAL;
        }
        self.base_target()
    }

    /// Target size ignoring any yield.
    fn base_target(&self) -> [f32; 2] {
        match self.mode {
            Mode::Expanded => {
                if self.lyrics_view() {
                    LYRICS
                } else {
                    EXPANDED
                }
            }
            Mode::Peek => PEEK,
            Mode::Compact => {
                if self.eye_feel_wide() {
                    LYRIC_PILL
                } else if self.act_now().is_some() {
                    ACT_TOAST
                } else if self.toast_open && self.notice_toast() {
                    NOTICE_OPEN
                } else if let Some(t) = &self.toast {
                    if t.kind == ToastKind::Message {
                        [MESSAGE_TOAST[0], self.notice_card_height()]
                    } else if matches!(t.kind, ToastKind::Focus | ToastKind::Break) {
                        PHASE_TOAST
                    } else if matches!(t.kind, ToastKind::Device(true)) && self.device_root.is_some() {
                        WATER_TOAST
                    } else if matches!(t.kind, ToastKind::Charging(_) | ToastKind::Battery | ToastKind::Net(_) | ToastKind::Goal) {
                        TALL_TOAST
                    } else if matches!(t.kind, ToastKind::Water | ToastKind::LumaApp | ToastKind::Call | ToastKind::Task | ToastKind::Remind(_) | ToastKind::HeadsUp) {
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
            && self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Volume(_) | ToastKind::Brightness))
        {
            let s = self.vol_stretch.x;
            let grow = w * 0.08 * s.abs();
            w += grow;
            cx += s.signum() * grow / 2.0;
        }
        // An act can pull the notch wider, or breathe it in and out.
        if let Some((kind, e)) = self.act_now() {
            let f = eyes::act(kind, e);
            w += 76.0 * f.stretch + 16.0 * f.swell.max(0.0);
        }
        // The lyric bar grows to one side for the eyes; the lyrics stay where they are.
        if self.mode == Mode::Compact {
            let grow = JAM_ROOM * self.jam_x.x.max(0.0);
            w += grow;
            cx += self.jam.side * grow / 2.0;
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
    /// Shell icons are slow to fetch (~100 ms), so they load on a worker thread; until one arrives
    /// the tile shows its placeholder.
    pub fn shelf_icon(&mut self, p: &std::path::Path) -> (Option<ID2D1Bitmap>, String) {
        if let Some(c) = self.shelf_icons.get(p) {
            return c.clone();
        }
        if self.icons_pending.insert(p.to_path_buf()) {
            let px = (40.0 * self.gfx.scale).round() as i32;
            let path = p.to_path_buf();
            let hwnd = self.hwnd.0 as isize;
            std::thread::spawn(move || unsafe {
                crate::sys::name_thread("islet-shelf-icon");
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
                let img = drop::shell_image(&path, px);
                let size = std::fs::metadata(&path).map(|m| size_label(m.len())).unwrap_or_default();
                let b = Box::into_raw(Box::new((path, img, size)));
                if PostMessageW(Some(HWND(hwnd as *mut _)), WM_ICON, WPARAM(0), LPARAM(b as isize)).is_err() {
                    drop(Box::from_raw(b));
                }
            });
        }
        (None, String::new())
    }

    /// The notch widens for the battery scene.
    pub fn eye_feel_wide(&self) -> bool {
        self.eye_feel.battery
    }

    /// Lyrics stay switched on with nothing playing, but there is nothing to show until music is back.
    fn lyrics_view(&self) -> bool {
        self.lyrics_on && self.page == Page::Music && self.has_media()
    }

    /// How full the battery is (0..1).
    pub fn charge_level(&self) -> f32 {
        self.power.map_or(1.0, |p| p.pct as f32 / 100.0)
    }

    /// The reminder being acted out on the card, and how long it has been going.
    pub fn act_now(&self) -> Option<(eyes::ActKind, f32)> {
        use eyes::ActKind as A;
        use luma::config::Kind as K;
        let t = self.toast.as_ref()?;
        let kind = match t.kind {
            ToastKind::Water | ToastKind::Sample(0) => A::Water,
            ToastKind::Sample(1) | ToastKind::Remind(K::Sleep | K::Water) => A::Sleep,
            ToastKind::Sample(2) | ToastKind::Remind(K::Breathe) => A::Breathe,
            ToastKind::Sample(3) | ToastKind::Remind(K::Stretch) => A::Stretch,
            ToastKind::Sample(4) | ToastKind::Remind(K::Eyes) => A::Rest,
            _ => return None,
        };
        Some((kind, t.born.elapsed().as_secs_f32()))
    }

    /// A prop's picture, `tinted` for the part in the character's colour.
    pub fn prop(&mut self, name: &'static str, tinted: bool) -> Option<ID2D1Bitmap> {
        if let Some(b) = self.props.get(&(name, tinted)) {
            return b.clone();
        }
        let bmp = ui::prop_bytes(name).and_then(|bytes| media::decode_art(bytes).ok()).and_then(|art| {
            let mut px = art.px.clone();
            if tinted {
                let (r, g, b) = (CHARACTER >> 16 & 255, CHARACTER >> 8 & 255, CHARACTER & 255);
                for p in px.chunks_exact_mut(4) {
                    p[0] = (p[0] as u32 * b / 255) as u8;
                    p[1] = (p[1] as u32 * g / 255) as u8;
                    p[2] = (p[2] as u32 * r / 255) as u8;
                }
            }
            self.gfx.bitmap(art.w, art.h, &px)
        });
        self.props.insert((name, tinted), bmp.clone());
        bmp
    }

    pub fn sample_running(&self) -> bool {
        self.sample.is_some()
    }

    /// Starts the tour of every effect, or stops it and leaves the notch as it was.
    fn toggle_effect_test(&mut self) {
        if self.sample.take().is_some() {
            self.eye_feel = eyes::Feel::ASLEEP;
            self.mini.pose = eyes::Pose::Right;
            if let Some(t) = self.toast.as_mut().filter(|t| matches!(t.kind, ToastKind::Sample(_))) {
                t.until = Instant::now();
            }
        } else {
            self.sample = Some((Instant::now(), "tour".to_string()));
        }
        self.kick();
    }

    /// Plays the eyes and icons on a fixed timeline for review.
    fn sample_step(&mut self, dt: f32) {
        let Some((start, what)) = self.sample.clone() else { return };
        let t = start.elapsed().as_secs_f32();
        // The tour plays every effect in turn until it is stopped.
        let (what, t) = if what == "tour" {
            match t % 62.0 {
                c if c < 17.5 => ("eyes".to_string(), c),
                c if c < 23.0 => ("battery".to_string(), c - 17.5),
                c if c < 53.0 => ((((c - 23.0) / 6.0) as u8).to_string(), c),
                c => ((5 + ((c - 53.0) / 3.0) as u8).to_string(), c),
            }
        } else {
            (what, t)
        };
        let (eyes_at, card) = match what.as_str() {
            "eyes" => (Some(t % 17.5), None),
            "battery" => {
                let now = self.eye_time();
                if self.eye_feel.battery == false && t % 6.0 < 0.1 {
                    self.eyes.mind.ask(eyes::Peek::Battery, now);
                }
                let sense = eyes::Sense { t: now, x: 0.0, y: 0.0, scale: 1.0, follow: 0.0, lift: 0.0, near: false, around: false, blocked: false, acted: false };
                self.eye_feel = self.eyes.mind.tick(&sense);
                self.eyes.step(self.eye_feel, dt);
                return;
            }
            "jam" => {
                let now = self.eye_time();
                self.jam.force(now);
                self.jam_show = true;
                self.mini.pose = eyes::JAM_POSES[self.jam.style];
                if self.mini.feed(now, if (t * 2.0).fract() < 0.12 { 0.6 } else { 0.1 }) {
                    self.jam.beat(now);
                }
                return;
            }
            n => (None, n.parse::<u8>().ok()),
        };
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let centre = self.pos.0 as f32 + WIN_W * self.gfx.scale / 2.0;
        let cursor = ((pt.x as f32 - centre) / (200.0 * self.gfx.scale)).clamp(-1.0, 1.0);
        use eyes::Look;
        let (look, gaze) = match eyes_at {
            Some(e) if e < 2.4 => (Look::Neutral, cursor),
            Some(e) if e < 3.0 => (Look::Neutral, -0.9),
            Some(e) if e < 3.6 => (Look::Neutral, 0.9),
            Some(e) if e < 4.5 => (Look::Wink, 0.0),
            Some(e) if e < 5.6 => (Look::Happy, 0.0),
            Some(e) if e < 6.5 => (Look::Surprised, 0.0),
            Some(e) if e < 7.5 => (Look::Squint, 0.0),
            Some(e) if e < 8.8 => (Look::Sleepy, 0.0),
            Some(e) if e < 9.7 => (Look::Angry, 0.0),
            Some(e) if e < 10.6 => (Look::Sad, 0.0),
            Some(e) if e < 11.8 => (Look::Cry, 0.0),
            Some(e) if e < 12.7 => (Look::Love, 0.0),
            Some(e) if e < 14.0 => (Look::Dizzy, 0.0),
            Some(e) if e < 14.8 => (Look::Up, 0.0),
            Some(e) if e < 15.6 => (Look::Down, 0.0),
            Some(e) if e < 16.4 => (Look::Smug, 0.0),
            _ => (Look::Closed, 0.0),
        };
        let blink = eyes_at.is_some_and(|e| e < 3.6 && e > 0.8 && e % 1.9 < 0.1);
        self.eyes.step(eyes::Feel { show: eyes_at.is_some_and(|e| e < 16.7), look, blink, gaze, ..eyes::Feel::ASLEEP }, dt);
        let now = self.eye_time();
        if self.mini.feed(now, if card == Some(6) && (t * 2.0).fract() < 0.12 { 0.6 } else { 0.1 }) && card == Some(6) {
            self.jam.beat(now);
        }
        let shown = self.toast.as_ref().and_then(|t| if let ToastKind::Sample(i) = t.kind { Some(i) } else { None });
        if shown != card {
            match card {
                Some(i) => {
                    let (title, sub) = [("Uống nước nhé", "Đã 45 phút rồi"), ("Chúc ngủ ngon", "Đến giờ nghỉ rồi"), ("Hít thở", "Chậm và sâu"), ("Giãn cơ thôi", "Đứng dậy nào"), ("Nghỉ mắt", "Nhìn ra xa 20 giây"), ("Sắp tới giờ họp", "Còn 5 phút nữa"), ("Mắt nhún cạnh lời bài hát", "Thanh lời dài ra 1–2 phút một lần"), ("Bài mới", "Tên ca sĩ")][i as usize % 8];
                    self.toast(ToastKind::Sample(i), title, sub, None);
                    if let Some(t) = self.toast.as_mut() {
                        t.until = Instant::now() + Duration::from_secs(3600);
                    }
                }
                None => {
                    if let Some(t) = self.toast.as_mut() {
                        t.until = Instant::now();
                    }
                }
            }
        }
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
            self.history_open = false;
            self.water_settings = false;
            self.todo_edit = false;
        }
        if m == Mode::Expanded {
            self.toast = None;
            self.queue.clear();
        }
        if self.mode == Mode::Expanded && m != Mode::Expanded {
            // Notifications are never lost to an open notch: the latest one plays now.
            if let Some(mut t) = self.held_notice.take().filter(|t| t.born.elapsed() < Duration::from_secs(120)) {
                let life = t.until.saturating_duration_since(t.born);
                t.born = Instant::now();
                t.until = t.born + life;
                self.toast = Some(t);
            }
        }
        self.mode = m;
        self.kick();
    }

    fn step(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;
        self.sample_step(dt);
        if let Some((kind, e)) = self.act_now() {
            let f = eyes::act(kind, e);
            let feel = eyes::Feel { show: true, look: f.look, blink: f.blink, gaze: f.gaze, lunge: f.lunge, ..eyes::Feel::ASLEEP };
            self.eyes.step(feel, dt);
        } else if self.sample.is_none() {
            self.eyes.step(self.eye_feel, dt);
        }
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
        let mut turned = false;
        let mut was_moving = false;
        for w in self.wheels.iter_mut() {
            was_moving |= w.held() || self.wheels_moving;
            turned |= w.step(dt);
        }
        if was_moving && !turned {
            self.apply_wheels();
        }
        self.wheels_moving = turned;
        // The small eyes are only stepped while they are on show: their outlines are not free.
        if self.jam_show || self.jam_x.x > 0.01 || self.sample.is_some() {
            let clock = self.born.elapsed().as_secs_f32();
            self.mini_busy = (0..3).fold(false, |busy, _| self.mini.step(clock, dt / 3.0) | busy);
        } else {
            self.mini_busy = false;
        }
        let t = self.target();
        let n = ((dt / 0.004).ceil() as usize).max(1);
        let sub = dt / n as f32;
        let mut moving = false;
        let fade_target = if self.expanded() { 1.0 } else { 0.0 };
        let toast_target = if self.toast.is_some() { 1.0 } else { 0.0 };
        let pill_target = if self.toast.is_none() && self.playing() && self.lyric_line().is_some() { 1.0 } else { 0.0 };
        // Moving to the next video pauses for an instant: the bars ride through it.
        if self.playing() {
            self.last_playing = now;
        }
        let eq_target = if now.duration_since(self.last_playing) < Duration::from_millis(EQ_HOLD_MS) { 1.0 } else { 0.0 };
        for _ in 0..n {
            moving |= self.sw.step(t[0], sub);
            moving |= self.sh.step(t[1], sub);
            moving |= self.fade.step(fade_target, sub);
            moving |= self.slide.step(0.0, sub);
            moving |= self.toast_mix.step(toast_target, sub);
            moving |= self.pill_mix.step(pill_target, sub);
            moving |= self.eq_mix.step(eq_target, sub);
            moving |= self.jam_x.step(if self.jam_show { 1.0 } else { 0.0 }, sub);
            moving |= self.intro.step(0.0, sub);
            moving |= self.intro_grow.step(1.0, sub);
            moving |= self.vol_stretch.step(0.0, sub);
            moving |= self.sun_rot.step(self.sun_target, sub);
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
            let vol = self.toast.as_ref().filter(|t| matches!(t.kind, ToastKind::Volume(_) | ToastKind::Brightness)).and_then(|t| t.level);
            if let Some(v) = vol {
                moving |= self.vol_level.step(v, sub);
                let bright = self.toast.as_ref().is_some_and(|t| t.kind == ToastKind::Brightness);
                if let Some(dir) = self.bright_bounce.filter(|_| bright && (self.vol_level.x - v).abs() < 0.02) {
                    self.bright_bounce = None;
                    self.vol_stretch.v += 7.0 * dir;
                    moving = true;
                }
            }
            moving |= self.lyr_mix.step(if self.lyrics_view() { 1.0 } else { 0.0 }, sub);
            moving |= self.toggle_x.step(if self.brain.cfg.reminders.water.enabled { 1.0 } else { 0.0 }, sub);
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
            moving |= self.aurora_lvl.step(self.aurora_target(), sub);
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
        self.effects()
            || self.sample.is_some()
            || self.mini_busy
            || self.wheels_moving
            || self.jam_show
            || !self.hidden && self.eq_mix.x > 0.01
            || self.eye_feel.show
            // A card's eyes stay out while it waits, and need frames only when the act says so.
            || self.eyes.show.x > 0.01 && self.act_now().is_none()
            || !self.hidden && self.act_now().is_some_and(|(_, e)| eyes::act_moving(e))
            || !self.hidden && self.toast.as_ref().is_some_and(|t| t.kind == ToastKind::Task)
            || !self.hidden && self.brain.focus.as_ref().is_some_and(|f| !f.paused())
            || !self.hidden && self.playing() && !(self.minimal() && self.mode == Mode::Compact)
    }

    /// Time-driven animations in progress (as opposed to the idle equalizer loop).
    fn effects(&self) -> bool {
        // A short toast animates while it has time left; a long-lived one (a card waiting for an
        // answer) only while it arrives.
        let fresh = |t: &Toast| t.until.saturating_duration_since(Instant::now()) > Duration::from_millis(1300) && t.born.elapsed() < Duration::from_secs(3);
        let recent = |t: Option<Instant>, ms: u64| t.is_some_and(|t| t.elapsed() < Duration::from_millis(ms));
        let effects = recent(self.lyr_anim.map(|x| x.0), 1100)
            || recent(self.pill_prev.as_ref().map(|x| x.0), 450)
            // Cards animate as they arrive (ripples, count-up, a few shakes); one left waiting for
            // an answer then holds still instead of redrawing 60 times a second. The heads-up
            // counts down to the end.
            || self.toast.as_ref().is_some_and(|t| (t.kind.tall() && (t.born.elapsed() < Duration::from_secs(5) || t.kind == ToastKind::HeadsUp)) || fresh(t))
            || self.drag_over || self.ripple.is_some_and(|r| r.0.elapsed() < Duration::from_millis(3800));
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
            let only_loop = !self.springs_moving && !self.effects() && self.mouse_in == false;
            let area = if self.catch_all {
                gfx::Rc::new(0.0, 0.0, WIN_W, WIN_H)
            } else if let (true, Some(r)) = (only_loop, self.loop_rect) {
                // Idle with only the equalizer/timer moving: redraw just its box.
                r
            } else {
                gfx::Rc::new(cx - half, 0.0, half * 2.0, self.sh.x.max(18.0) + 4.0)
            };
            self.gfx.lift = self.intro.x;
            self.gfx.grow = self.intro_grow.x;
            self.gfx.center_x = WIN_W / 2.0;
            if self.gfx.begin(area).is_ok() {
                ui::draw(self);
                if self.gfx.end(self.hwnd, self.pos.0, self.pos.1).is_err() {
                    self.gfx.invalidate();
                }
            }
        }
    }

    pub fn toast(&mut self, kind: ToastKind, title: &str, value: &str, level: Option<f32>) {
        // Device and reminder toasts linger (Nusa keeps them ~4 s / until answered).
        let life = match kind {
            ToastKind::Water => 12_000,
            // Pomodoro phase notices (Luma moves on by itself); cards waiting for an answer are
            // kept up by Luma instead (present_luma).
            ToastKind::Focus | ToastKind::Break => 6_000,
            ToastKind::Task | ToastKind::Call => 60_000,
            ToastKind::Charging(_) | ToastKind::Battery | ToastKind::Net(_) | ToastKind::Goal => 2_600,
            ToastKind::Bluetooth(_) | ToastKind::Device(_) => 4_000,
            ToastKind::Song => 2_400,
            ToastKind::Notice => reading_ms(6_000, title),
            ToastKind::Message => reading_ms(8_000, title),
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
        if self.expanded() {
            if matches!(kind, ToastKind::Notice | ToastKind::Message) {
                self.held_notice = Some(t);
            }
            return;
        }
        let same = |a: ToastKind, b: ToastKind| std::mem::discriminant(&a) == std::mem::discriminant(&b);
        match self.toast.take() {
            Some(cur) if same(cur.kind, kind) && self.toast_mix.x > 0.5 && !(matches!(kind, ToastKind::Bluetooth(_) | ToastKind::Device(_)) && cur.title != title) => {
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
                if let (ToastKind::Volume(_) | ToastKind::Brightness, Some(v)) = (kind, level) {
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
        if let Some(Act::Wheel(id)) = self.hit(self.mouse.0, self.mouse.1) {
            self.wheels[id as usize].turn(notches);
            self.kick();
            return;
        }
        if self.expanded() && self.history_open {
            let last = self.history.len().saturating_sub(3);
            self.history_top = (self.history_top as i32 - notches.signum() as i32).clamp(0, last as i32) as usize;
            self.render();
            return;
        }
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

    /// Bottom light: half strength anywhere over the shelf, full near its bottom edge; after a
    /// drop it holds, then fades slowly (the notch may already be closing).
    fn aurora_target(&self) -> f32 {
        if self.drag_over {
            let bottom = self.sh.x;
            let near = (1.0 - ((bottom - self.drag_pt.1).abs() / 70.0)).clamp(0.0, 1.0);
            return 1.0 + 0.6 * near;
        }
        match self.ripple {
            Some((start, ..)) => {
                let t = start.elapsed().as_secs_f32();
                1.5 * (1.0 - (t - 1.0).max(0.0) / 2.6).clamp(0.0, 1.0)
            }
            None => 0.0,
        }
    }

    /// Polled every 50 ms: arms the window-wide drop zone while a button is held near the notch,
    /// and closes the shelf once the cursor has left after a drop.
    /// Notch footprint ignoring any yield, with a working margin around it, in layout units.
    fn yield_zone(&self, m: f32) -> Zone {
        let [w, h] = self.base_target();
        let cx = WIN_W / 2.0;
        (cx - w / 2.0 - m, 0.0, cx + w / 2.0 + m, h + m)
    }

    /// Steps the notch aside while the user works right below it (click, pointer work, typing into
    /// a field under it, dragging a window to the top), and brings it back a second after.
    fn poll_yield(&mut self, pt: POINT) {
        if self.expanded() || self.drag_over || self.form.is_some() || self.demo.is_some() {
            self.near_since = None;
            return;
        }
        let s = self.gfx.scale;
        let (wx, wy) = (self.pos.0 as f32, self.pos.1 as f32);
        let to_u = |x: i32, y: i32| ((x as f32 - wx) / s, (y as f32 - wy) / s);
        let (x0, y0, x1, y1) = self.yield_zone(24.0);
        // Typing only counts when the notch really covers it: a wide field that merely passes
        // below the notch is not hidden by it.
        let covered = self.yield_zone(0.0);
        let rect_of = |r: RECT| {
            let ((l, t), (rt, b)) = (to_u(r.left, r.top), to_u(r.right, r.bottom));
            (l, t, rt, b)
        };
        let in_zone = |(x, y): (f32, f32)| x >= x0 && x <= x1 && y >= y0 && y <= y1;
        let cur = to_u(pt.x, pt.y);
        let now = Instant::now();
        let mut reason = false;

        // 1. A fresh click near the notch that went to the app below. The low bit of
        // GetAsyncKeyState catches quick taps that start and end between two polls.
        let (l, r) = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32), GetAsyncKeyState(VK_RBUTTON.0 as i32)) };
        let down = l < 0 || r < 0;
        let clicked = (down && !self.buttons_down) || (l & 1) != 0 || (r & 1) != 0;
        if clicked {
            self.click_in_zone = in_zone(cur) && !self.mouse_in;
            self.click_covered = touches((cur.0, cur.1, cur.0, cur.1), covered) && !self.mouse_in;
            if self.click_in_zone {
                reason = true;
            }
        }
        self.buttons_down = down;
        // 2. The pointer working next to the notch (not on it) for a moment.
        if in_zone(cur) && !self.mouse_in {
            let since = *self.near_since.get_or_insert(now);
            if now.duration_since(since) > Duration::from_millis(600) {
                reason = true;
            }
        } else {
            self.near_since = None;
        }
        // 3. Typing into a field that sits under the notch.
        if sys::key_idle_ms() < 1000 {
            match sys::caret_rect() {
                Some(r) => reason |= touches(rect_of(r), covered),
                // Chromium/Electron apps report no Win32 caret: ask UI Automation for the focused
                // field, else assume it is where the last click landed.
                None => match sys::focused_rect() {
                    Some(r) => reason |= touches(rect_of(r), covered),
                    None => reason |= self.click_covered,
                },
            }
        }
        // 4. A window being dragged or resized up into the notch.
        if let Some(r) = sys::moving_rect() {
            let (l, t) = to_u(r.left, r.top);
            let (rt, _) = to_u(r.right, r.bottom);
            if t <= y1 && rt >= x0 && l <= x1 {
                reason = true;
            }
        }

        if reason {
            self.yield_seen = now;
            if !self.yielded {
                self.yielded = true;
                if self.mode != Mode::Compact {
                    self.set_mode(Mode::Compact);
                }
                self.kick();
            }
        } else if self.yielded && now.duration_since(self.yield_seen) > Duration::from_millis(1000) && !in_zone(cur) {
            self.leave_yield();
        }
    }

    /// The notch comes back; a small overshoot right after must not open it.
    fn leave_yield(&mut self) {
        self.yielded = false;
        self.no_hover_until = Some(Instant::now() + Duration::from_secs(2));
        self.kick();
        self.render();
    }

    /// Tells the eyes what is happening. Whatever the user is doing outranks them, so they never
    /// touch the step-aside rule: they only watch it.
    fn sense_eyes(&mut self, pt: POINT, down: bool) {
        let s = self.gfx.scale;
        let [w, h] = self.base_target();
        let (ux, uy) = ((pt.x as f32 - self.pos.0 as f32) / s, (pt.y as f32 - self.pos.1 as f32) / s);
        let along = ux - WIN_W / 2.0;
        let (dx, dy) = ((along.abs() - w / 2.0).max(0.0), (uy - h).max(0.0));
        let t = self.eye_time();
        let (song, artist, playing) = {
            let m = self.media.lock().unwrap();
            (m.title.clone(), m.artist.clone(), m.has && m.playing && !m.title.is_empty())
        };
        // A new song passes by once, unless something else has the notch.
        if playing && song != self.last_song {
            let quiet = self.toast.is_none() && !self.expanded() && !self.yielded && !self.hidden && t > 5.0;
            if quiet {
                self.toast(ToastKind::Song, &song, &artist, None);
            }
            self.last_song = song;
            self.meter_on = false;
        }
        // The bar grows for the eyes now and then, and they listen only while it is out.
        let free = playing && self.pill_mix.x > 0.5 && self.toast.is_none() && self.mode == Mode::Compact && !self.yielded && !self.hidden && !self.minimal() && sys::key_idle_ms() >= 1000;
        self.jam_show = self.jam.tick(t, free);
        if self.jam_show {
            self.mini.pose = eyes::JAM_POSES[self.jam.style];
            if !self.meter_on {
                sys::open_meter();
                self.meter_on = true;
            }
            if self.mini.feed(t, sys::audio_peak()) {
                self.jam.beat(t);
                self.kick();
            }
        } else {
            self.meter_on = false;
        }
        // Plugging in ends the battery scene at once.
        let plugged = self.eye_feel.battery && self.power.is_some_and(|p| p.ac);
        let blocked = self.eyes_blocked() || plugged;
        let sense = eyes::Sense {
            t,
            x: pt.x as f32,
            y: pt.y as f32,
            scale: s,
            follow: (along / (w / 2.0 + 90.0)).clamp(-1.0, 1.0),
            lift: (dy / eyes::RANGE).clamp(0.0, 1.0) * 0.5,
            near: dx.hypot(dy) < eyes::RANGE,
            around: dx.hypot(dy) < eyes::RANGE_OUT,
            blocked,
            acted: down || sys::key_idle_ms() < 700,
        };
        // A card waiting for an answer wakes up for its nudges and blinks.
        if self.act_now().is_some_and(|(_, e)| eyes::act_moving(e)) {
            self.kick();
        }
        // The effect test drives the eyes itself.
        if self.sample.is_none() {
            self.eye_feel = self.eyes.mind.tick(&sense);
        }
        if self.eye_feel.show {
            self.kick();
        }
        if self.card_after.as_ref().is_some_and(|(_, at)| Instant::now() >= *at) {
            if let Some((due, _)) = self.card_after.take() {
                self.present_luma(&due);
            }
        }
    }

    /// Something more important has the notch: the eyes stay in.
    fn eyes_blocked(&self) -> bool {
        !self.settings.pet
            || self.minimal()
            || self.yielded
            || self.no_hover_until.is_some_and(|t| Instant::now() < t)
            || self.hidden
            || self.mouse_in
            || self.mode != Mode::Compact
            || self.toast.is_some()
            || self.drag_over
    }

    fn poll_drag_zone(&mut self) {
        let mut pt = POINT::default();
        let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        let _ = unsafe { GetCursorPos(&mut pt) };
        self.poll_yield(pt);
        self.sense_eyes(pt, down);
        // Fast polling only matters with the pointer near the notch, a button held or typing;
        // otherwise four checks a second are enough.
        let s = self.gfx.scale;
        let near = (pt.y as f32) < self.pos.1 as f32 + (WIN_H + 160.0) * s
            && (pt.x as f32 - (self.pos.0 as f32 + WIN_W * s / 2.0)).abs() < (WIN_W / 2.0 + 160.0) * s;
        let busy = near || down || self.yielded || self.drag_over || sys::key_idle_ms() < 1500 || self.eye_feel.show || self.card_after.is_some() || self.meter_on;
        let every = if busy { 50 } else { 250 };
        if every != self.zone_every {
            self.zone_every = every;
            unsafe { SetTimer(Some(self.hwnd), T_DRAGZONE, every, None) };
        }
        self.update_privacy();
        let s = self.gfx.scale;
        let (wx, wy) = (self.pos.0, self.pos.1);
        let inside = pt.x >= wx && pt.x < wx + (WIN_W * s) as i32 && pt.y >= wy && pt.y < wy + (WIN_H * s) as i32;
        // Only a drag that started outside the notch's window arms the drop zone; a plain click
        // near the notch (pressed inside it) must reach the app below untouched.
        if down && !self.press_known {
            self.press_known = true;
            self.press_outside = !inside;
        } else if !down {
            self.press_known = false;
        }
        let want = down && inside && self.press_outside;
        if want != self.catch_all {
            self.catch_all = want;
            self.render();
        }
        if !down && !self.drag_over && !self.mouse_in && self.expanded() && self.form.is_none() && self.ripple.is_some() {
            let (cx, w) = self.notch_geometry();
            let ux = (pt.x - wx) as f32 / s;
            let uy = (pt.y - wy) as f32 / s;
            let over = ux >= cx - w / 2.0 && ux <= cx + w / 2.0 && uy >= 0.0 && uy <= self.sh.x;
            if !over {
                self.set_mode(Mode::Compact);
            }
        }
    }

    /// Keeps chat messages out of screen shares and recordings while one is on screen.
    /// Only message cards are hidden from captures (other notifications, such as the screenshot
    /// one, stay visible). Each switch makes DWM recompose the window, so it is switched off only
    /// once the card has been gone for a moment, never between cards of a burst.
    fn update_privacy(&mut self) {
        let message = [self.toast.as_ref(), self.last_toast.as_ref()]
            .iter()
            .flatten()
            .any(|t| t.kind == ToastKind::Message)
            && self.toast_mix.x > 0.01;
        if message {
            self.private_until = Some(Instant::now() + Duration::from_millis(300));
        }
        let showing = message || self.private_until.is_some_and(|t| Instant::now() < t);
        if showing != self.private && (self.demo.is_none() || self.storm.is_some()) {
            self.private = showing;
            let mode = if showing { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
            unsafe {
                let _ = SetWindowDisplayAffinity(self.hwnd, mode);
            }
            self.gfx.invalidate();
        }
    }

    /// Brightness toast; the first report after start-up is the current value, not a change.
    fn brightness_changed(&mut self, v: u32) {
        let first = self.brightness.is_none();
        let old = self.brightness;
        let changed = self.brightness != Some(v);
        self.brightness = Some(v);
        if !first && changed {
            // Further presses at an end are never reported (the laptop handles the Fn keys), so
            // the rubber band plays once when the bar arrives at 0 or 100.
            if v == 0 || v == 100 {
                self.bright_bounce = Some(if v == 100 { 1.0 } else { -1.0 });
            }
            // Nusa turns the sun about 1.15° per percent: clockwise brighter, back when dimmer.
            if let Some(o) = old {
                self.sun_target += (v as f32 - o as f32) * 1.15;
            }
            let fresh = !matches!(self.toast.as_ref().map(|t| t.kind), Some(ToastKind::Brightness));
            self.toast(ToastKind::Brightness, "Brightness", &format!("{v}%"), Some(v as f32 / 100.0));
            if fresh {
                // Slide from where it was, like the volume bar does.
                if let Some(o) = old {
                    self.vol_level.x = o as f32 / 100.0;
                }
            }
        }
    }

    fn learn_phone_app(&mut self, app: &str) {
        let app = app.trim().to_lowercase();
        if !self.phone_apps.contains(&app) {
            self.phone_apps.push(app);
            let _ = std::fs::write(model::data_dir().join("phone_apps.txt"), self.phone_apps.join("\n"));
        }
    }

    /// The notification behind the current toast; it also goes into the history.
    fn keep_notice(&mut self, info: NoticeInfo) {
        self.history.push_front(info.clone());
        self.history.truncate(HISTORY_MAX);
        self.history_unread += 1;
        self.notice = Some(info);
    }

    /// Highlight colour of a notification: the app's own, else its icon's.
    pub fn notice_accent(&self, n: &NoticeInfo) -> u32 {
        brand_colour(&n.source_name()).or_else(|| brand_colour(&n.app)).or_else(|| n.icon.as_ref().and_then(|p| self.icon_tints.get(p).copied())).unwrap_or(0x8E8E93)
    }

    fn show_device(&mut self, card: devices::Card) {
        let sub = if !card.detail.is_empty() {
            card.detail
        } else if card.connected {
            "Connected".into()
        } else {
            "Disconnected".into()
        };
        self.present_device(DeviceCard {
            kind: ToastKind::Device(card.connected),
            name: card.name,
            sub,
            level: None,
            device: card.kind,
            root: card.root,
        });
    }

    /// Device cards never replace each other: one arriving while another is still fresh (or
    /// while the widgets are open) waits its turn, and waiting cards play 1.5 s each.
    fn present_device(&mut self, c: DeviceCard) {
        // Only another device card makes it wait; anything else (a reminder card comes back by
        // itself) gives way.
        let fresh = self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Bluetooth(_) | ToastKind::Device(_)) && t.born.elapsed() < DEVICE_TURN);
        if self.mode == Mode::Expanded || fresh || !self.device_queue.is_empty() {
            // The same device changing again: only its latest state is worth showing.
            self.device_queue.retain(|q| q.name != c.name);
            self.device_queue.push_back(c);
            // A flood (a hub or dock with many devices) keeps only the latest few.
            while self.device_queue.len() > 4 {
                self.device_queue.pop_front();
            }
            if let Some(t) = self.toast.as_mut().filter(|t| matches!(t.kind, ToastKind::Bluetooth(_) | ToastKind::Device(_))) {
                t.until = t.until.min(t.born + DEVICE_TURN);
            }
            return;
        }
        self.show_device_now(c);
    }

    fn show_device_now(&mut self, c: DeviceCard) {
        self.device_kind = c.device;
        self.device_root = c.root;
        self.toast(c.kind, &c.name, &c.sub, c.level);
        if !self.device_queue.is_empty() {
            if let Some(t) = self.toast.as_mut() {
                t.until = t.until.min(t.born + DEVICE_TURN);
            }
        }
    }

    /// A headphone jack, USB audio device or speaker endpoint became available or went away.
    fn audio_changed(&mut self, c: sys::AudioChange) {
        sys::timelog(&format!("audio name={:?} active={} form={} bluetooth={} path={:?}", c.name, c.active, c.form_factor, c.bluetooth, c.path));
        use devices::DeviceKind;
        let kind = match c.form_factor {
            3 => DeviceKind::Headphones,
            5 => DeviceKind::Headset,
            1 => DeviceKind::Speaker,
            _ => return,
        };
        // Bluetooth audio has its own card; its endpoints come and go with it.
        if c.bluetooth || self.last_bt.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        if self.last_audio.as_ref().is_some_and(|(n, on, t)| *n == c.name && *on == c.active && t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.last_audio = Some((c.name.clone(), c.active, Instant::now()));
        // Held a moment: a Bluetooth device's own event may still be on its way.
        self.pending_audio = Some((Instant::now(), devices::Card { connected: c.active, kind, name: c.name, root: None, detail: String::new() }));
    }

    // ------------------------------------------------------------ Luma

    /// Islet's older data joins Luma's (once), and until the separate Luma app is off its
    /// reminders wait, so nothing comes twice.
    fn take_over_from_luma(&mut self) {
        let dir = model::data_dir();
        // Timed to-dos become Luma reminders; untimed ones stay to-dos.
        let timed: Vec<(String, String)> = self.todo.tasks.iter().filter(|t| !t.time.is_empty() && t.done_day == 0).map(|t| (t.title.clone(), t.time.clone())).collect();
        if !timed.is_empty() {
            let _ = std::fs::copy(dir.join("todo.txt"), dir.join("todo.txt.bak"));
            self.todo.tasks.retain(|t| t.time.is_empty());
            self.todo.save();
            for (title, time) in timed {
                self.add_todo(&title, &time);
            }
        }
        // Islet's own water counter is replaced by Luma's cups.
        if dir.join("water.txt").exists() {
            let _ = std::fs::rename(dir.join("water.txt"), dir.join("water.txt.bak"));
        }
        if self.settings.luma_app_off {
            return;
        }
        self.brain.enabled = false;
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || unsafe {
            sys::name_thread("islet-luma-check");
            let active = sys::luma_app_active();
            let _ = PostMessageW(Some(HWND(hwnd as _)), WM_LUMAAPP, WPARAM(active as usize), LPARAM(0));
        });
    }

    /// Runs Luma's ticker (every 2 s) and puts what it wants into the notch.
    fn brain_tick(&mut self) {
        let now = chrono::Local::now();
        let idle = idle_seconds() as u64;
        let window = unsafe { GetForegroundWindow() }.0 as isize;
        let busy = self.demo.is_none() && (sys::busy() || sys::foreground_covers(self.monitor, self.hwnd));
        let goal_was = self.brain.stats.water >= self.brain.cfg.water_goal;
        let events = self.brain.tick(now, idle, window, busy);
        if !events.is_empty() {
            self.kick();
        }
        for ev in events {
            match ev {
                luma::Event::HeadsUp(due, secs) => self.present_heads_up(&due, secs),
                luma::Event::Show(due) => self.present_after_look(due),
                luma::Event::Phase { resting, round, rounds, minutes } => {
                    sound::play(sound::Chime::Soft);
                    if resting {
                        self.feel(eyes::Look::Love);
                        let sub = if minutes >= luma::focus::LONG_REST_MINUTES { "15 min, you earned it".to_string() } else { format!("{minutes} minute short break") };
                        self.toast(ToastKind::Break, if minutes >= luma::focus::LONG_REST_MINUTES { "Long Break!" } else { "Break Time!" }, &sub, None);
                    } else {
                        self.toast(ToastKind::Focus, "Focus Time!", &format!("Back to work · round {round}/{rounds}"), None);
                    }
                }
            }
        }
        if self.brain.timed_out(now) {
            self.answer_luma(Outcome::Snooze, true);
        }
        // A reminder card covered by another toast (or the open widgets) comes back afterwards.
        if self.toast.is_none() && self.mode != Mode::Expanded && self.card_after.is_none() {
            match self.brain.showing.as_ref().map(|s| (s.due.clone(), s.heads_up_until)) {
                Some((due, Some(until))) => self.present_heads_up(&due, (until - now).num_seconds().max(1) as u32),
                Some((due, None)) => self.present_luma(&due),
                None => {}
            }
        }
        if !goal_was && self.brain.stats.water >= self.brain.cfg.water_goal {
            self.goal_toast();
        }
    }

    fn eye_time(&self) -> f32 {
        self.born.elapsed().as_secs_f32()
    }

    fn feel(&mut self, look: eyes::Look) {
        let t = self.eye_time();
        self.eyes.mind.ask(eyes::Peek::Mood(look), t);
    }

    /// The eyes look down at a reminder before it appears; if they can't, it appears at once.
    fn present_after_look(&mut self, due: Due) {
        if self.demo.is_some() || self.sample.is_some() || self.eyes_blocked() || sys::key_idle_ms() < 700 {
            self.present_luma(&due);
            return;
        }
        let thirsty = matches!(due, Due::Builtin(CareKind::Water)) && self.last_drink.elapsed() > Duration::from_secs(2 * 3600);
        let look = if thirsty { eyes::Look::Angry } else { eyes::Look::Down };
        let t = self.eye_time();
        self.eyes.mind.ask(eyes::Peek::Card(look), t);
        self.card_after = Some((due, Instant::now() + Duration::from_millis(800)));
        self.kick();
    }

    fn goal_toast(&mut self) {
        self.feel(eyes::Look::Love);
        let l = self.brain.stats.water as f32 * self.brain.cfg.cup_ml as f32 / 1000.0;
        self.toast(ToastKind::Goal, "Goal reached", &format!("{l:.1} L today"), Some(1.0));
    }

    /// The card for a reminder Luma wants answered. It stays until answered (or Luma's own
    /// timeout), so its toast does not run out on its own.
    pub fn present_luma(&mut self, due: &Due) {
        let cfg = &self.brain.cfg;
        let (kind, title, value) = match due {
            Due::Builtin(CareKind::Water) => {
                let sub = if self.brain.stats.water >= cfg.water_goal { "Only if you're thirsty".to_string() } else { format!("Stay hydrated! · {} ml", cfg.cup_ml) };
                (ToastKind::Water, "Time to Drink".to_string(), sub)
            }
            Due::Builtin(CareKind::Stretch) => (ToastKind::Remind(CareKind::Stretch), "Stretch Time".into(), format!("3 moves · {} s", cfg.reminders.stretch.seconds * 3)),
            Due::Builtin(CareKind::Eyes) => (ToastKind::Remind(CareKind::Eyes), "Rest Your Eyes".into(), format!("Look far away · {} s", cfg.reminders.eyes.seconds)),
            Due::Builtin(CareKind::Breathe) => (ToastKind::Remind(CareKind::Breathe), "Breathe".into(), "4-4-4-4 · 1 min".into()),
            Due::Builtin(CareKind::Sleep) => (ToastKind::Remind(CareKind::Sleep), "Bedtime".into(), format!("It's {} · time to rest", cfg.reminders.sleep.time)),
            Due::Custom(id) => (ToastKind::Task, "Task Time!".into(), self.brain.task(id).map(|c| c.title.clone()).unwrap_or_default()),
            Due::Focus(min) => (ToastKind::Focus, "Focus time's up!".into(), format!("{min} min · great work")),
        };
        if self.mode == Mode::Expanded {
            return;
        }
        sound::play(sound::Chime::Soft);
        self.toast(kind, &title, &value, None);
        if let Some(t) = self.toast.as_mut().filter(|t| t.kind == kind) {
            t.until = Instant::now() + Duration::from_secs(3600);
            self.brain.card_shown(chrono::Local::now());
        }
    }

    pub fn present_heads_up(&mut self, due: &Due, secs: u32) {
        let what = match due {
            Due::Builtin(CareKind::Water) => "Water".to_string(),
            Due::Builtin(CareKind::Stretch) => "Stretch".to_string(),
            Due::Builtin(CareKind::Eyes) => "Eye break".to_string(),
            Due::Builtin(CareKind::Breathe) => "Breathing".to_string(),
            Due::Builtin(CareKind::Sleep) => "Bedtime".to_string(),
            Due::Custom(id) => self.brain.task(id).map(|c| c.title.clone()).unwrap_or_default(),
            Due::Focus(_) => "Focus".to_string(),
        };
        if self.mode == Mode::Expanded {
            return;
        }
        self.toast(ToastKind::HeadsUp, &what, "Coming up", None);
        if let Some(t) = self.toast.as_mut().filter(|t| t.kind == ToastKind::HeadsUp) {
            t.until = Instant::now() + Duration::from_secs(secs as u64 + 2);
        }
    }

    /// The reminder card was answered. "Start" on a timed task begins its focus session.
    fn answer_luma(&mut self, outcome: Outcome, timed_out: bool) {
        let now = chrono::Local::now();
        let goal_was = self.brain.stats.water >= self.brain.cfg.water_goal;
        let water = matches!(self.brain.showing.as_ref().map(|s| &s.due), Some(Due::Builtin(CareKind::Water)));
        if outcome == Outcome::Done && self.focus_end_card() {
            self.feel(eyes::Look::Love);
        }
        // Being put off again and again, or ignored, hurts a little.
        self.later_row = if outcome == Outcome::Snooze && !timed_out { self.later_row + 1 } else { 0 };
        if timed_out || self.later_row >= 3 {
            self.later_row = 0;
            self.feel(eyes::Look::Sad);
        }
        if let Some((minutes, label)) = self.brain.answer(outcome, timed_out, now) {
            self.brain.start_focus(minutes, &label, now);
        }
        if self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Water | ToastKind::Remind(_) | ToastKind::Task | ToastKind::HeadsUp | ToastKind::Focus)) {
            self.dismiss_toast();
        }
        if water && outcome == Outcome::Done {
            self.last_drink = Instant::now();
            self.water_pulse.v += 7.5;
            if !goal_was && self.brain.stats.water >= self.brain.cfg.water_goal {
                self.goal_toast();
            }
        }
        self.kick();
    }

    /// The Focus-styled card is the time's-up card of a session (not a Pomodoro phase notice).
    pub fn focus_end_card(&self) -> bool {
        matches!(self.brain.showing.as_ref().map(|s| &s.due), Some(Due::Focus(_)))
    }

    /// The Pomodoro page's view: (seconds left, seconds in this stretch, running, label).
    pub fn pomo_view(&self) -> (f64, f64, bool, String) {
        let now = chrono::Local::now();
        match &self.brain.focus {
            Some(f) => {
                let left = f.left(now).num_milliseconds() as f64 / 1000.0;
                let total = luma::scheduler::minutes(f.minutes).num_seconds() as f64;
                let label = match f.pomodoro {
                    Some(p) if p.resting && f.minutes >= luma::focus::LONG_REST_MINUTES => "Long break".to_string(),
                    Some(p) if p.resting => "Short break".to_string(),
                    Some(p) if p.rounds > 1 => format!("Focus · {}/{}", p.round, p.rounds),
                    _ if !f.label.is_empty() => f.label.clone(),
                    _ => "Focus time".to_string(),
                };
                (left, total.max(1.0), !f.paused(), label)
            }
            None => {
                let secs = self.pomo_mode.secs();
                (secs, secs, false, self.pomo_mode.label().to_string())
            }
        }
    }

    /// The To-do page's list: today's timed tasks (Luma reminders) and untimed to-dos, open
    /// ones first.
    pub fn todo_items(&self) -> Vec<TodoItem> {
        let now = chrono::Local::now();
        let today = model::now().ymd();
        let mut v: Vec<TodoItem> = self
            .brain
            .tasks_today(now)
            .into_iter()
            .map(|(c, done)| {
                let time = if c.start && c.minutes > 0 {
                    let t = luma::scheduler::parse_hm(&c.time).map(|t| t + chrono::TimeDelta::minutes(c.minutes as i64));
                    format!("{}-{}", c.time, t.map(|t| t.format("%H:%M").to_string()).unwrap_or_default())
                } else {
                    c.time.clone()
                };
                TodoItem { key: TodoKey::Timed(c.id.clone()), title: c.title.clone(), time, done }
            })
            .collect();
        v.extend(self.todo.tasks.iter().enumerate().map(|(i, t)| TodoItem { key: TodoKey::Untimed(i), title: t.title.clone(), time: String::new(), done: t.done_day == today }));
        v.sort_by_key(|t| t.done);
        v
    }

    /// Height of the current notification's small card.
    pub fn notice_card_height(&self) -> f32 {
        match self.toast.as_ref().map(|t| t.kind) {
            // A phone notification has one more line: the app's name.
            Some(ToastKind::Message) if self.notice.as_ref().is_some_and(|n| n.phone_app.is_some()) => MESSAGE_TOAST[1] + 12.0,
            Some(ToastKind::Message) => MESSAGE_TOAST[1],
            _ => TALL_TOAST[1],
        }
    }

    /// The current toast is a notification that can grow into its detail view.
    pub fn notice_toast(&self) -> bool {
        self.notice.is_some() && self.toast.as_ref().is_some_and(|t| matches!(t.kind, ToastKind::Notice | ToastKind::Message))
    }

    fn open_notice(&mut self) {
        let Some(n) = &self.notice else { return };
        Self::open_target(n);
        self.close_notice();
    }

    fn open_target(n: &NoticeInfo) {
        match n.kind {
            NoticeKind::Phone => notify::open_phone(n.id),
            NoticeKind::Screenshot => {
                if let Some(f) = &n.file {
                    sys::shell_open(f.as_os_str());
                }
            }
            NoticeKind::Chat => sys::open_site("https://www.facebook.com/messages/"),
            NoticeKind::Browser => {
                if let Some(site) = &n.site {
                    sys::open_site(site);
                }
            }
            NoticeKind::App => sys::open_app(&n.aumid, &n.app),
        }
    }

    fn close_notice(&mut self) {
        if let Some(n) = &self.notice {
            notify::remove(n.id);
        }
        self.toast_open = false;
        self.dismiss_toast();
    }

    fn dismiss_toast(&mut self) {
        self.last_toast = self.toast.take();
        self.kick();
        self.render();
    }

    /// A toast with its own buttons stays put under the cursor instead of opening the notch.
    fn interactive_toast(&self) -> bool {
        self.toast.as_ref().is_some_and(|t| {
            matches!(t.kind, ToastKind::Water | ToastKind::Focus | ToastKind::Break | ToastKind::Task | ToastKind::Remind(_) | ToastKind::HeadsUp | ToastKind::LumaApp | ToastKind::Call)
        })
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
        let item = index.and_then(|i| self.todo_items().into_iter().nth(i));
        let (title, digits) = item.as_ref().map(|t| (t.title.clone(), t.time.chars().filter(char::is_ascii_digit).collect())).unwrap_or_default();
        self.form = Some(Form { key: item.map(|t| t.key), title, digits, field: 0 });
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
                // An edited row is replaced: it may move between timed and untimed.
                match &f.key {
                    Some(TodoKey::Timed(id)) => self.brain.delete_task(id),
                    Some(TodoKey::Untimed(i)) if *i < self.todo.tasks.len() => {
                        self.todo.tasks.remove(*i);
                        self.todo.save();
                    }
                    _ => {}
                }
                self.add_todo(&title, &time);
            }
        }
        self.set_noactivate(true);
    }

    /// "HH:MM" or "HH:MM-HH:MM" makes a Luma reminder (a range: "Start" focuses that long);
    /// no time keeps it an untimed to-do.
    fn add_todo(&mut self, title: &str, time: &str) {
        let (start, end) = time.split_once('-').unwrap_or((time, ""));
        match luma::scheduler::parse_hm(start) {
            Some(s) => {
                let minutes = luma::scheduler::parse_hm(end).map(|e| (e - s).num_minutes().rem_euclid(24 * 60) as u32).unwrap_or(0);
                self.brain.add_task(title, start, minutes, chrono::Local::now());
            }
            None => {
                self.todo.tasks.push(model::Task { title: title.to_string(), done_day: 0, time: String::new() });
                self.todo.save();
            }
        }
    }

    fn media_cmd(&self, c: Cmd) {
        if let Some(tx) = &self.media_tx {
            let _ = tx.send(c);
        }
    }

    fn act(&mut self, a: Act) {
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
            Act::Lyrics => {
                self.lyrics_on = !self.lyrics_on;
                self.settings.lyrics = self.lyrics_on;
                if self.demo.is_none() {
                    self.settings.save();
                }
            }
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
                let now = chrono::Local::now();
                if self.brain.focus.is_some() {
                    self.brain.toggle_pause(now);
                } else if self.pomo_mode == Phase::Focus {
                    self.brain.start_pomodoro(25, 5, 4, "", now);
                } else {
                    self.brain.start_rest((self.pomo_mode.secs() / 60.0) as u32, now);
                }
            }
            Act::PomoReset => {
                // Giving up on a work stretch makes them cry; ending a break doesn't.
                if self.brain.focus.as_ref().is_some_and(|f| !f.pomodoro.is_some_and(|p| p.resting)) {
                    self.feel(eyes::Look::Cry);
                }
                self.brain.stop_focus(chrono::Local::now());
            }
            Act::PomoPhase(p) => {
                if self.brain.focus.is_none() {
                    self.pomo_mode = p;
                }
            }
            Act::Drank => {
                let goal_was = self.brain.stats.water >= self.brain.cfg.water_goal;
                self.brain.drink(chrono::Local::now());
                if matches!(self.toast.as_ref().map(|t| t.kind), Some(ToastKind::Water)) {
                    self.dismiss_toast();
                }
                if !goal_was && self.brain.stats.water >= self.brain.cfg.water_goal {
                    self.goal_toast();
                }
                self.last_drink = Instant::now();
                self.water_pulse.v += 7.5;
                self.kick();
            }
            // The phase card's button: "+5 min" on a time's-up card, otherwise skip the break or
            // pause the round.
            Act::ToastStartPhase => {
                let now = chrono::Local::now();
                if self.focus_end_card() {
                    self.answer_luma(Outcome::Snooze, false);
                } else {
                    match self.toast.as_ref().map(|t| t.kind) {
                        Some(ToastKind::Break) => self.brain.skip_phase(now),
                        _ => self.brain.toggle_pause(now),
                    }
                    self.dismiss_toast();
                }
            }
            Act::ToastDismiss => {
                if self.focus_end_card() {
                    self.answer_luma(Outcome::Done, false);
                } else {
                    self.dismiss_toast();
                }
            }
            Act::LumaDone => self.answer_luma(Outcome::Done, false),
            Act::LumaLater => self.answer_luma(Outcome::Snooze, false),
            Act::LumaNow => {
                if let Some(due) = self.brain.heads_up_now() {
                    self.dismiss_toast();
                    self.present_luma(&due);
                }
            }
            Act::LumaAppOff => {
                self.dismiss_toast();
                std::thread::spawn(sys::turn_off_luma_app);
                self.settings.luma_app_off = true;
                self.settings.save();
                self.brain.enabled = true;
            }
            Act::NoticeOpen => self.open_notice(),
            // Islet can't take the call itself: the app comes up to answer it there.
            Act::CallAccept => {
                if let Some(n) = &self.notice {
                    Self::open_target(n);
                }
                self.dismiss_toast();
            }
            Act::CallDecline => self.dismiss_toast(),
            Act::History(on) => {
                self.history_open = on;
                self.history_top = 0;
                self.history_unread = 0;
            }
            Act::HistoryClear => {
                self.history.clear();
                self.history_open = false;
            }
            Act::HistoryOpen(i) => {
                if let Some(n) = self.history.get(i) {
                    Self::open_target(n);
                }
                self.set_mode(Mode::Compact);
            }
            Act::DeviceOpen => {
                if let Some(root) = self.device_root.take() {
                    sys::shell_open(std::ffi::OsStr::new(&root));
                }
                self.dismiss_toast();
            }
            Act::NoticeClose => self.close_notice(),
            Act::WaterOn => {
                let on = !self.brain.cfg.reminders.water.enabled;
                self.brain.set_water_enabled(on, chrono::Local::now());
            }
            Act::WaterSub(s) => self.water_sub = s,
            Act::WaterSettings(on) => {
                self.water_settings = on;
                self.sync_wheels();
            }
            Act::Wheel(_) => {}
            Act::WaterReset => self.brain.reset_water(),
            Act::TodoAdd => self.open_form(None),
            Act::TodoEditMode => {
                if !self.todo_items().is_empty() {
                    self.todo_edit = !self.todo_edit;
                }
            }
            Act::TodoToggle(i) => {
                if self.todo_edit {
                    self.open_form(Some(i));
                } else if let Some(item) = self.todo_items().into_iter().nth(i) {
                    match item.key {
                        TodoKey::Timed(id) => self.brain.set_task_done(&id, !item.done),
                        TodoKey::Untimed(i) => self.todo.toggle(i),
                    }
                }
            }
            Act::TodoDelete(i) => {
                if let Some(item) = self.todo_items().into_iter().nth(i) {
                    match item.key {
                        TodoKey::Timed(id) => self.brain.delete_task(&id),
                        TodoKey::Untimed(i) => {
                            self.todo.tasks.remove(i);
                            self.todo.save();
                        }
                    }
                }
                if self.todo_items().is_empty() {
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

    fn tick(&mut self) {
        // An animation must get frames; one stalled mid-way in a storm test (frozen half-size
        // notch), so a missing frame for this long restarts the pump.
        if self.animating && self.last_frame.elapsed() > Duration::from_millis(400) {
            self.pump_stalls += 1;
            pump::unstick();
        }
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
        if self.demo.is_none() {
            self.watch();
        }
        if self.pending_audio.as_ref().is_some_and(|(t, _)| t.elapsed() >= Duration::from_secs(2)) {
            if let Some((_, card)) = self.pending_audio.take().filter(|_| !self.last_bt.is_some_and(|t| t.elapsed() < Duration::from_secs(5))) {
                self.show_device(card);
            }
        }
        let clock = self.clock();
        self.brain_ticks = self.brain_ticks.wrapping_add(1);
        if self.brain_ticks % 4 == 0 {
            self.brain_tick();
        }
        if !self.notice_toast() {
            self.toast_open = false;
        }
        // While the notch steps aside (or a fullscreen app hides it) notifications wait to be seen.
        if self.yielded || self.hidden {
            if let Some(t) = self.toast.as_mut().filter(|t| matches!(t.kind, ToastKind::Notice | ToastKind::Message)) {
                t.until = t.until.max(Instant::now() + Duration::from_secs(2));
            }
        }
        // Waiting device cards play once the current card is done (or the widgets closed).
        let device_done = match &self.toast {
            Some(t) if matches!(t.kind, ToastKind::Bluetooth(_) | ToastKind::Device(_)) => Instant::now() >= t.until,
            _ => true,
        };
        if device_done && self.mode != Mode::Expanded {
            if let Some(c) = self.device_queue.pop_front() {
                self.show_device_now(c);
            }
        }
        if let Some(t) = self.toast.as_ref().filter(|_| !self.toast_open) {
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
        // Anything that moves is drawn by the frame pump; at rest only the clock text changes, once
        // a minute (a full redraw twice a second was most of Islet's idle CPU).
        if clock.minutes() != self.drawn_minute {
            self.drawn_minute = clock.minutes();
            // Also heals the image if Windows lost it some way nothing reported.
            self.gfx.invalidate();
            self.render();
        }
    }

    fn poll_power(&mut self) {
        let Some(p) = sys::power() else { return };
        if let Some(old) = self.power {
            if old.pct != p.pct {
                self.kick();
            }
            if p.battery {
                let lv = Some(p.pct as f32 / 100.0);
                if p.ac != old.ac {
                    if p.ac {
                        self.toast(ToastKind::Charging(true), "Charging", "Charger connected", lv);
                    } else {
                        self.toast(ToastKind::Charging(false), "Charger unplugged", &format!("{}% remaining", p.pct), lv);
                    }
                } else if !p.ac && old.pct > 20 && p.pct <= 20 && p.pct > 10 {
                    sound::play(sound::Chime::Alert);
                    self.toast(ToastKind::Battery, "Low battery", "Plug in soon", lv);
                } else if !p.ac && old.pct > 10 && p.pct <= 10 {
                    sound::play(sound::Chime::Alert);
                    // The eyes make the point themselves when they can; a busy moment gets the card.
                    if self.eyes_blocked() || sys::key_idle_ms() < 700 {
                        self.toast(ToastKind::Battery, "Battery critical", "Plug in now", lv);
                    } else {
                        let t = self.eye_time();
                        self.eyes.mind.ask(eyes::Peek::Battery, t);
                    }
                } else if p.ac && old.pct < 100 && p.pct == 100 {
                    self.toast(ToastKind::Charging(true), "Fully charged", "You can unplug", Some(1.0));
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
                // Windows may have dropped the hidden window's image.
                self.gfx.invalidate();
                self.render();
            }
        }
    }

    fn on_mouse_move(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        if let Some(id) = self.wheel_held {
            let t = self.eye_time();
            self.wheels[id as usize].drag_to(y, t);
            self.kick();
        }
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
            // Brushing the sliver does nothing: the pointer is usually on its way to something
            // just below it. It comes back when the pointer leaves, or on a click.
            let held = self.yielded || self.no_hover_until.is_some_and(|t| Instant::now() < t);
            if held {
            } else if self.mode == Mode::Compact && self.notice_toast() {
                // A resting cursor grows the notification instead of opening the widgets.
                self.hover_anchor = (x, y);
                unsafe { SetTimer(Some(self.hwnd), T_HOVER, 90, None) };
            } else if self.mode == Mode::Compact && !self.interactive_toast() {
                self.set_mode(Mode::Peek);
                self.hover_anchor = (x, y);
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
        if self.toast_open {
            // Shrink back and leave the notification a moment before it goes.
            self.toast_open = false;
            if let Some(t) = self.toast.as_mut() {
                t.until = Instant::now() + Duration::from_secs(3);
            }
        }
        self.kick();
        self.render();
    }

    fn hit(&self, x: f32, y: f32) -> Option<Act> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| *a)
    }

    /// The wheels show what is set.
    fn sync_wheels(&mut self) {
        use chrono::Timelike;
        let cfg = &self.brain.cfg;
        let nearest = |list: &[u32], v: u32| list.iter().enumerate().min_by_key(|(_, x)| x.abs_diff(v)).map_or(0, |(i, _)| i);
        let (cup, goal) = (nearest(&model::CUPS, cfg.cup_ml), nearest(&model::GOALS, cfg.water_goal * cfg.cup_ml));
        let bed = luma::scheduler::parse_hm(&cfg.reminders.sleep.time).unwrap_or_default();
        let (hour, minute) = (bed.hour() as usize, bed.minute() as usize / 15);
        self.wheels[ui::WheelId::Cup as usize].set(cup);
        self.wheels[ui::WheelId::Goal as usize].set(goal);
        self.wheels[ui::WheelId::BedHour as usize].set(hour);
        self.wheels[ui::WheelId::BedMinute as usize].set(minute);
    }

    /// A wheel came to rest: what it shows is now the setting.
    fn apply_wheels(&mut self) {
        let index = |a: &App, id: ui::WheelId| a.wheels[id as usize].index();
        let (cup, goal) = (model::CUPS[index(self, ui::WheelId::Cup)], model::GOALS[index(self, ui::WheelId::Goal)]);
        let bed = format!("{:02}:{:02}", index(self, ui::WheelId::BedHour), index(self, ui::WheelId::BedMinute) * 15);
        let cfg = &mut self.brain.cfg;
        let cups = ((goal as f32 / cup as f32).round() as u32).clamp(1, 20);
        if cfg.cup_ml != cup || cfg.water_goal != cups || cfg.reminders.sleep.time != bed {
            cfg.cup_ml = cup;
            cfg.water_goal = cups;
            cfg.reminders.sleep.time = bed;
            self.brain.save_config();
        }
    }

    fn on_down(&mut self, x: f32, y: f32) {
        let act = self.hit(x, y);
        if let Some(Act::Wheel(id)) = act {
            let t = self.eye_time();
            self.wheels[id as usize].press(y, t);
            self.wheel_held = Some(id);
            self.kick();
        }
        if let Some(Act::Scrub(bx, bw)) = act {
            let (bx, bw) = (bx as f32 / 10.0, bw as f32 / 10.0);
            self.scrub = Some((bx, bw, ((x - bx) / bw).clamp(0.0, 1.0)));
        }
        self.down = Some((x, y, act));
        self.render();
    }

    fn on_up(&mut self, x: f32, y: f32) {
        let pressed_at = self.down.as_ref().map_or(y, |d| d.1);
        if let Some(id) = self.wheel_held.take() {
            let tap = self.hits.iter().find(|(_, a)| *a == Act::Wheel(id)).map_or(0, |(r, _)| ((pressed_at - r.cy()) / wheel::ROW).round() as i32);
            self.wheels[id as usize].release(tap.clamp(-1, 1));
            self.down = None;
            self.kick();
            return;
        }
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
            if self.yielded && !self.interactive_toast() {
                self.leave_yield();
                return;
            }
            if self.notice_toast() {
                if !self.toast_open {
                    self.toast_open = true;
                    self.kick();
                } else if let (Some(a), Some(b)) = (pressed, self.hit(x, y)) {
                    if a == b {
                        self.act(a);
                    }
                }
                self.render();
                return;
            }
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
        let _ = AppendMenuW(m, MF_STRING | check(self.settings.hide_banners), 6, w!("Hide Windows banners"));
        let _ = AppendMenuW(m, MF_STRING | check(self.settings.show_text), 7, w!("Show notification text without hover"));
        if self.sample.is_some() {
            let _ = AppendMenuW(m, MF_STRING, 8, w!("Stop effect test"));
        } else {
            let _ = AppendMenuW(m, MF_STRING, 8, w!("Test all effects"));
        }
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
            8 => self.toggle_effect_test(),
            7 => {
                self.settings.show_text = !self.settings.show_text;
                self.settings.save();
            }
            3 => {
                self.settings.minimal = !self.settings.minimal;
                self.minimal_armed = true;
                self.settings.save();
            }
            6 => {
                self.settings.hide_banners = !self.settings.hide_banners;
                self.settings.save();
                notify::HIDE_BANNERS.store(self.settings.hide_banners, std::sync::atomic::Ordering::Relaxed);
                if !self.settings.hide_banners {
                    sys::restore_banners();
                }
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
                let t0 = Instant::now();
                a.step();
                a.render();
                let animating = a.animating;
                if let Some(s) = a.storm.as_mut() {
                    s.frame(t0.elapsed(), animating);
                }
            }
        }
        WM_TIMER => match wp.0 {
            T_TICK => a.tick(),
            T_DRAGZONE => a.poll_drag_zone(),
            T_STORM => a.storm_tick(),
            T_DEVICE => {
                let _ = KillTimer(Some(hwnd), T_DEVICE);
                let cards = a.devices.as_mut().map(|t| t.flush()).unwrap_or_default();
                for card in cards {
                    a.show_device(card);
                }
            }
            T_HOVER => {
                let _ = KillTimer(Some(hwnd), T_HOVER);
                if a.mouse_in && a.mode == Mode::Compact && a.notice_toast() && !a.toast_open {
                    let (ax, ay) = a.hover_anchor;
                    if ((a.mouse.0 - ax).powi(2) + (a.mouse.1 - ay).powi(2)).sqrt() < 6.0 {
                        a.toast_open = true;
                        a.kick();
                        a.render();
                    } else {
                        a.hover_anchor = a.mouse;
                        SetTimer(Some(hwnd), T_HOVER, 90, None);
                    }
                } else if a.mouse_in && a.mode == Mode::Peek {
                    // Only open once the cursor rests; a quick pass across does nothing.
                    let (ax, ay) = a.hover_anchor;
                    let moved = ((a.mouse.0 - ax).powi(2) + (a.mouse.1 - ay).powi(2)).sqrt();
                    if moved < 6.0 {
                        a.set_mode(Mode::Expanded);
                    } else {
                        a.hover_anchor = a.mouse;
                        SetTimer(Some(hwnd), T_HOVER, 90, None);
                    }
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
        WM_NET => {
            let n = Box::from_raw(lp.0 as *mut sys::NetChange);
            let name = n.ssid.clone().unwrap_or_else(|| "Network".into());
            let state = match n.status {
                2 => "Connected",
                1 => "No internet access",
                _ => "Disconnected",
            };
            a.toast(ToastKind::Net(n.status > 0), &name, state, n.bars.map(|b| b as f32 / 5.0));
        }
        WM_SHOT => {
            let b = Box::from_raw(lp.0 as *mut sys::Shot);
            let sys::Shot { id, path, img, size } = *b;
            if let Some(n) = a.notice.as_mut().filter(|n| n.id == id) {
                n.thumb = img.and_then(|(w, h, px)| a.gfx.bitmap(w, h, &px));
                n.file = Some(path);
                n.file_size = size;
                if let Some(h) = a.history.iter_mut().find(|h| h.id == id) {
                    *h = n.clone();
                }
                a.render();
            }
        }
        // A call's notification left: answered, declined or missed.
        WM_NOTICEGONE => {
            if a.toast.as_ref().is_some_and(|t| t.kind == ToastKind::Call) && a.notice.as_ref().is_some_and(|n| n.id == wp.0 as u32) {
                a.dismiss_toast();
            }
        }
        WM_ICON => {
            let b = Box::from_raw(lp.0 as *mut (PathBuf, Option<(u32, u32, Vec<u8>)>, String));
            let (path, img, size) = *b;
            if let Some(c) = img.as_ref().and_then(|(_, _, px)| icon_colour(px)) {
                a.icon_tints.insert(path.clone(), c);
            }
            let bmp = img.and_then(|(w, h, px)| a.gfx.bitmap(w, h, &px));
            a.icons_pending.remove(&path);
            a.shelf_icons.insert(path, (bmp, size));
            a.render();
        }
        WM_TRAY => match (lp.0 & 0xFFFF) as u32 {
            WM_LBUTTONUP => {
                let m = if a.expanded() { Mode::Compact } else { Mode::Expanded };
                a.set_mode(m);
                a.render();
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => a.menu(),
            _ => {}
        },
        WM_POWERBROADCAST => {
            const PBT_POWERSETTINGCHANGE: usize = 0x8013;
            if wp.0 == PBT_POWERSETTINGCHANGE && lp.0 != 0 {
                let ps = &*(lp.0 as *const windows::Win32::System::Power::POWERBROADCAST_SETTING);
                if ps.PowerSetting == windows::Win32::System::SystemServices::GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS && ps.DataLength >= 4 {
                    let v = std::ptr::read_unaligned(ps.Data.as_ptr() as *const u32).min(100);
                    a.brightness_changed(v);
                }
            }
            a.poll_power();
            a.render();
        }
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
            // Title row: sender (or app); second row: the message, else the app name.
            let body = if n.body.is_empty() { n.app.clone() } else { n.body.clone() };
            if a.settings.hide_banners {
                sys::hide_banner(&n.aumid);
                if n.phone_app.is_some() {
                    sys::hide_phone_banners();
                }
            }
            sys::timelog(&format!("notice phone_connected={}", bt::phone_connected()));
            match &n.phone_app {
                Some(app) => a.learn_phone_app(app),
                None if bt::phone_connected() && notify::phone_covers(&n.aumid, &n.app, n.site.as_deref(), &a.phone_apps) => return LRESULT(0),
                None => {}
            }
            let info = NoticeInfo::of(&n);
            sys::timelog(&format!("shown aumid={} lines_title={} lines_body={}", n.aumid, n.title.chars().count(), n.body.chars().count()));
            if info.kind == NoticeKind::Screenshot {
                sys::find_screenshot(a.hwnd, n.id, (240.0 * a.gfx.scale) as i32);
            }
            if n.call {
                sound::play(sound::Chime::Alert);
                a.toast(ToastKind::Call, &n.phone_app.clone().unwrap_or_else(|| n.app.clone()), &n.title, None);
            } else if info.kind == NoticeKind::Phone || n.source == Some("facebook") {
                sound::play(sound::Chime::Soft);
                a.toast(ToastKind::Message, &body, &n.title, None);
            } else {
                sound::play(sound::Chime::Soft);
                a.toast(ToastKind::Notice, &body, &n.title, None);
            }
            a.keep_notice(info);
        }
        WM_BLUETOOTH => {
            let c = Box::from_raw(lp.0 as *mut bt::Change);
            let state = if c.connected { "Connected" } else { "Disconnected" };
            let level = c.battery.filter(|_| c.connected).map(|b| b as f32 / 100.0);
            a.last_bt = Some(Instant::now());
            sys::timelog(&format!("bluetooth name={:?} connected={} kind={:?} battery={:?}", c.name, c.connected, c.kind, c.battery));
            let flapping = c.kind == devices::DeviceKind::Phone && a.phone_card.is_some_and(|t| t.elapsed() < Duration::from_secs(60));
            if !flapping {
                if c.kind == devices::DeviceKind::Phone {
                    a.phone_card = Some(Instant::now());
                }
                a.present_device(DeviceCard { kind: ToastKind::Bluetooth(c.connected), name: c.name.clone(), sub: state.into(), level, device: c.kind, root: None });
            }
        }
        WM_DEVICECHANGE => {
            if let Some(t) = a.devices.as_mut() {
                if let Some(card) = t.on_change(wp.0, lp.0) {
                    a.show_device(card);
                } else if t.has_pending() {
                    SetTimer(Some(hwnd), T_DEVICE, 700, None);
                }
            }
            return LRESULT(1);
        }
        WM_LUMAAPP => {
            if wp.0 != 0 {
                a.toast(ToastKind::LumaApp, "Luma lives here now", "Turn the old Luma app off?", None);
                if let Some(t) = a.toast.as_mut() {
                    t.until = Instant::now() + Duration::from_secs(60);
                }
            } else {
                a.settings.luma_app_off = true;
                a.settings.save();
                a.brain.enabled = true;
            }
        }
        WM_AUDIODEV => {
            if lp.0 == 0 {
                // The default output changed: follow its volume.
                sys::watch_volume(hwnd);
            } else {
                let b = Box::from_raw(lp.0 as *mut sys::AudioChange);
                a.audio_changed(*b);
            }
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE | WM_DWMCOMPOSITIONCHANGED => {
            a.place();
            a.gfx.invalidate();
            a.render();
            if msg == WM_DPICHANGED {
                return LRESULT(0);
            }
        }
        // Luma's clocks survive a restart or log-off (a short one picks up where it left off).
        WM_ENDSESSION if wp.0 != 0 => a.brain.save_schedule(chrono::Local::now()),
        WM_DESTROY => {
            a.brain.save_schedule(chrono::Local::now());
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
    a.brain.enabled = false;
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
        let pct = kv.get("pct").and_then(|v| v.parse().ok()).unwrap_or(100);
        a.power = Some(sys::Power { ac: true, pct, battery: true });
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
        a.sync_wheels();
    }
    if let Some(s) = kv.get("watersub") {
        a.water_sub = s.parse().unwrap_or(0);
    }
    // "todos=go@16:54-00:00~Read~Gym@18:00-19:30": tasks for screenshots.
    if let Some(list) = kv.get("todos") {
        for t in list.split('~') {
            let (title, time) = t.split_once('@').unwrap_or((t, ""));
            a.add_todo(title, time);
        }
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
    if let Some((who, text)) = kv.get("msg").and_then(|v| v.split_once('|')) {
        a.toast = Some(Toast {
            born: Instant::now(),
            kind: ToastKind::Message,
            title: text.to_string(),
            value: who.to_string(),
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    for (key, kind, title, value) in [
        ("chg_toast", ToastKind::Charging(true), "Charging", "Charger connected"),
        ("unplugged", ToastKind::Charging(false), "Charger unplugged", "95% remaining"),
        ("wifi_on", ToastKind::Net(true), "Phong 2_5G", "Connected"),
        ("wifi_off", ToastKind::Net(false), "Phong 2_5G", "Disconnected"),
        ("bright", ToastKind::Brightness, "Brightness", "70%"),
        ("task", ToastKind::Task, "Task Time!", "Học từ vựng"),
        ("goal", ToastKind::Goal, "Goal reached", "2.0 L today"),
        ("song", ToastKind::Song, "You're Sorry", "Some Artist"),
    ] {
        if let Some(v) = kv.get(key).and_then(|v| v.parse::<f32>().ok()) {
            a.toast = Some(Toast {
                born: Instant::now(),
                kind,
                title: title.into(),
                value: value.into(),
                level: Some(v),
                until: Instant::now() + Duration::from_secs(3600),
            });
            a.vol_level.x = v;
        }
    }
    if kv.get("phase_end") == Some(&"1") {
        a.toast = Some(Toast {
            born: Instant::now() - Duration::from_secs(5),
            kind: ToastKind::Focus,
            title: "Focus Time!".into(),
            value: "Back to work, 25 min".into(),
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    // "phone=Zalo|Minh Anh|Text": a phone notification; "history=1": some past notifications.
    let demo_notice = |a: &mut App, app: &str, title: &str, body: &str| {
        let n = notify::Notice { app: "Phone Link".into(), title: title.into(), body: body.into(), aumid: "Microsoft.YourPhone_8wekyb3d8bbwe!App".into(), source: None, site: None, id: 0, phone_app: Some(app.into()), call: false };
        a.keep_notice(NoticeInfo::of(&n));
    };
    if kv.get("history") == Some(&"1") {
        for (app, title, body) in [("Shopee", "Đơn hàng", "Đơn hàng của bạn đang được giao"), ("YouTube", "Sơn Tùng M-TP", "vừa đăng video mới"), ("Messenger", "Denis", "Tối nay đi ăn không?"), ("Zalo", "Minh Anh", "Mai họp lúc mấy giờ?")] {
            demo_notice(a, app, title, body);
        }
    }
    if let Some(p) = kv.get("phone").map(|v| v.split('|').collect::<Vec<_>>()) {
        demo_notice(a, p[0], p[1], p.get(2).unwrap_or(&""));
        a.toast = Some(Toast {
            born: Instant::now(),
            kind: ToastKind::Message,
            title: p.get(2).unwrap_or(&"").to_string(),
            value: p[1].to_string(),
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    // "call=Zalo|Minh Anh": an incoming call.
    if let Some(p) = kv.get("call").map(|v| v.split('|').collect::<Vec<_>>()) {
        demo_notice(a, p[0], p[1], "");
        a.toast = Some(Toast {
            born: Instant::now(),
            kind: ToastKind::Call,
            title: p[0].to_string(),
            value: p[1].to_string(),
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    if kv.get("history_open") == Some(&"1") {
        a.history_open = true;
    }
    // "luma=water|stretch|eyes|breathe|sleep|task|headsup|focusend|app": Luma's cards.
    if let Some(v) = kv.get("luma") {
        let now = chrono::Local::now();
        let due = match *v {
            "water" => Some(Due::Builtin(CareKind::Water)),
            "stretch" => Some(Due::Builtin(CareKind::Stretch)),
            "eyes" => Some(Due::Builtin(CareKind::Eyes)),
            "breathe" => Some(Due::Builtin(CareKind::Breathe)),
            "sleep" => Some(Due::Builtin(CareKind::Sleep)),
            "focusend" => Some(Due::Focus(100)),
            "task" | "headsup" => {
                a.brain.add_task("IELTS reading", "20:06", 45, now);
                a.brain.cfg.custom.last().map(|c| Due::Custom(c.id.clone()))
            }
            _ => None,
        };
        match (*v, due) {
            ("headsup", Some(due)) => {
                a.brain.showing = Some(luma::Showing { due: due.clone(), heads_up_until: Some(now + chrono::TimeDelta::seconds(8)), card_at: None });
                a.present_heads_up(&due, 3600);
            }
            (_, Some(due)) => {
                a.brain.showing = Some(luma::Showing { due: due.clone(), heads_up_until: None, card_at: None });
                a.present_luma(&due);
            }
            ("app", None) => a.toast(ToastKind::LumaApp, "Luma lives here now", "Turn the old Luma app off?", None),
            _ => {}
        }
        if let Some(t) = a.toast.as_mut() {
            t.until = Instant::now() + Duration::from_secs(3600);
        }
    }
    if kv.get("pomo_run") == Some(&"1") {
        let left: f64 = kv.get("pomo_left").and_then(|v| v.parse().ok()).unwrap_or(1408.0);
        let now = chrono::Local::now();
        a.brain.start_pomodoro(25, 5, 4, "", now);
        if let Some(f) = a.brain.focus.as_mut() {
            f.set_left(chrono::TimeDelta::milliseconds((left * 1000.0) as i64), now);
        }
    }
    // "bt=Name" with "bt_kind=mouse|keyboard|speaker|headset|phone|gamepad" and "bt_batt=0.02".
    let kind_of = |k: &str| match k {
        "mouse" => devices::DeviceKind::Mouse,
        "keyboard" => devices::DeviceKind::Keyboard,
        "speaker" => devices::DeviceKind::Speaker,
        "headset" => devices::DeviceKind::Headset,
        "phone" => devices::DeviceKind::Phone,
        "gamepad" => devices::DeviceKind::Gamepad,
        "drive" => devices::DeviceKind::Drive,
        "headphones" => devices::DeviceKind::Headphones,
        _ => devices::DeviceKind::Other,
    };
    if let Some(name) = kv.get("bt") {
        a.device_kind = kv.get("bt_kind").map(|k| kind_of(k)).unwrap_or_default();
        a.toast = Some(Toast {
            born: Instant::now(),
            kind: ToastKind::Bluetooth(true),
            title: name.to_string(),
            value: "Connected".into(),
            level: Some(kv.get("bt_batt").and_then(|v| v.parse().ok()).unwrap_or(1.0)),
            until: Instant::now() + Duration::from_secs(3600),
        });
    }
    // "device=kind|Name|1|detail|E:" (connected flag, optional detail line and drive root).
    if let Some(v) = kv.get("device") {
        let p: Vec<&str> = v.split('|').collect();
        let card = devices::Card {
            kind: kind_of(p.first().unwrap_or(&"")),
            name: p.get(1).unwrap_or(&"USB device").to_string(),
            connected: p.get(2) != Some(&"0"),
            detail: p.get(3).unwrap_or(&"").to_string(),
            root: p.get(4).filter(|r| !r.is_empty()).map(|r| format!("{r}\\")),
        };
        a.show_device(card);
        if let Some(t) = a.toast.as_mut() {
            t.until = Instant::now() + Duration::from_secs(3600);
        }
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
    // "notice=App|Title|Body|site" (+ "notice_open=1", "shot=<image>"): notification detail view.
    if let Some(v) = kv.get("notice") {
        let p: Vec<&str> = v.split('|').collect();
        let n = notify::Notice {
            app: p.first().unwrap_or(&"App").to_string(),
            title: p.get(1).unwrap_or(&"").to_string(),
            body: p.get(2).unwrap_or(&"").to_string(),
            aumid: if kv.contains_key("shot") { "Microsoft.ScreenSketch_8wekyb3d8bbwe!App".into() } else { String::new() },
            source: None,
            site: p.get(3).filter(|s| !s.is_empty()).map(|s| s.to_string()),
            id: 0,
            phone_app: None,
            call: false,
        };
        a.toast = Some(Toast {
            born: Instant::now() - Duration::from_secs(5),
            kind: ToastKind::Notice,
            title: if n.body.is_empty() { n.app.clone() } else { n.body.clone() },
            value: n.title.clone(),
            level: None,
            until: Instant::now() + Duration::from_secs(3600),
        });
        let mut info = NoticeInfo::of(&n);
        if let Some(path) = kv.get("shot") {
            let p = PathBuf::from(path);
            info.thumb = drop::shell_image(&p, (240.0 * a.gfx.scale) as i32).and_then(|(w, h, px)| a.gfx.bitmap(w, h, &px));
            info.file_size = std::fs::metadata(&p).map(|m| size_label(m.len())).unwrap_or_default();
            info.file = Some(p);
        }
        a.notice = Some(info);
        a.toast_open = kv.get("notice_open") == Some(&"1");
    }
    // "storm=120;seed=7;stormlog=<file>": event-storm test (storm.rs); writes the report and exits.
    if let Some(secs) = kv.get("storm").and_then(|v| v.parse::<u64>().ok()) {
        let seed = kv.get("seed").and_then(|v| v.parse().ok()).unwrap_or(7);
        let log = kv.get("stormlog").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("islet_storm.txt"));
        sound::MUTED.store(true, std::sync::atomic::Ordering::Relaxed);
        a.storm = Some(Box::new(storm::Storm::new(secs, seed, log)));
        storm::timer(a.hwnd);
    }
    if let Some(v) = kv.get("sample") {
        a.sample = Some((Instant::now(), v.to_string()));
        a.kick();
    }
    if let Some(deg) = kv.get("sun").and_then(|v| v.parse::<f32>().ok()) {
        a.sun_rot.x = deg;
        a.sun_target = deg;
    }
    if let Some(stretch) = kv.get("stretch").and_then(|v| v.parse::<f32>().ok()) {
        a.vol_stretch.x = stretch.clamp(-1.0, 1.0);
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let demo_spec = std::env::var("ISLET_DEMO").ok();
        // Fast-time test runs (1 minute = 2 s) stay silent.
        if std::env::var_os("ISLET_FAST").is_some() {
            sound::MUTED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
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

#[cfg(test)]
mod zone_tests {
    use super::touches;

    #[test]
    fn a_wide_field_below_the_notch_is_not_covered_by_it() {
        let notch = (300.0, 0.0, 450.0, 30.0);
        assert!(!touches((0.0, 46.0, 1600.0, 80.0), notch), "address bar under the notch");
        assert!(touches((0.0, 20.0, 1600.0, 60.0), notch), "a field reaching into the notch");
        assert!(!touches((0.0, 0.0, 200.0, 60.0), notch), "a field beside the notch");
    }
}
