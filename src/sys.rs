//! System signals: power, network, volume, keyboard indicators, autostart and tray icon art.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::Duration;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation};
use windows::Win32::Foundation::*;
use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName};
use windows::Win32::Graphics::Gdi::*;
use windows::core::implement;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl};
use windows::Win32::Media::Audio::AUDIO_VOLUME_NOTIFICATION_DATA;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Power::{GetSystemPowerStatus, RegisterPowerSettingNotification, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::System::Registry::*;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::{WM_CAPS, WM_FOREGROUND, WM_LANG, WM_NET, WM_VOLKEY, WM_VOLUME};

// ---------------------------------------------------------------- power

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Power {
    pub ac: bool,
    pub pct: u8,
    pub battery: bool,
}

pub fn power() -> Option<Power> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    Some(Power {
        ac: s.ACLineStatus == 1,
        pct: s.BatteryLifePercent.min(100),
        battery: s.BatteryFlag != 128 && s.BatteryLifePercent != 255,
    })
}

/// Asks Windows to send WM_POWERBROADCAST when the power source or battery level changes.
pub fn subscribe_power(hwnd: HWND) {
    for guid in [GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING] {
        unsafe {
            let _ = RegisterPowerSettingNotification(HANDLE(hwnd.0), &guid, DEVICE_NOTIFY_WINDOW_HANDLE);
        }
    }
}

// ---------------------------------------------------------------- network

/// 0 = disconnected, 1 = connected without internet, 2 = internet.
fn net_status() -> usize {
    match NetworkInformation::GetInternetConnectionProfile() {
        Ok(p) => match p.GetNetworkConnectivityLevel() {
            Ok(NetworkConnectivityLevel::InternetAccess) => 2,
            Ok(_) => 1,
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

pub fn spawn_network(hwnd: isize) {
    std::thread::spawn(move || {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let mut last = net_status();
        loop {
            std::thread::sleep(Duration::from_secs(4));
            let now = net_status();
            if now != last {
                last = now;
                unsafe {
                    let _ = PostMessageW(Some(HWND(hwnd as _)), WM_NET, WPARAM(now), LPARAM(0));
                }
            }
        }
    });
}

// ---------------------------------------------------------------- volume


/// Last reported volume in thousandths, with bit 16 set when muted.
static VOLUME: AtomicIsize = AtomicIsize::new(-1);

/// (level 0..1, muted) as last reported by the endpoint, if known.
pub fn volume_state() -> Option<(f32, bool)> {
    let v = VOLUME.load(Ordering::Relaxed);
    (v >= 0).then(|| ((v & 0xFFFF) as f32 / 1000.0, v >> 16 != 0))
}

/// Volume changes from any source (keys, mixer, apps) arrive here as they happen, so the toast
/// tracks a held volume key smoothly instead of waiting for the key to be released.
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeWatch(isize);

impl IAudioEndpointVolumeCallback_Impl for VolumeWatch_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            let level = (d.fMasterVolume.clamp(0.0, 1.0) * 1000.0).round() as usize;
            let packed = level | ((d.bMuted.as_bool() as usize) << 16);
            VOLUME.store(packed as isize, Ordering::Relaxed);
            unsafe {
                let _ = PostMessageW(Some(HWND(self.0 as *mut _)), WM_VOLUME, WPARAM(packed), LPARAM(0));
            }
        }
        Ok(())
    }
}

/// WM_VOLUME carries the level in thousandths (low 16 bits) and the mute flag (bit 16).
pub fn watch_volume(hwnd: HWND) {
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return };
        let Ok(vol) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else { return };
        if let (Ok(v), Ok(m)) = (vol.GetMasterVolumeLevelScalar(), vol.GetMute()) {
            VOLUME.store(((v.clamp(0.0, 1.0) * 1000.0).round() as isize) | ((m.as_bool() as isize) << 16), Ordering::Relaxed);
        }
        let cb: IAudioEndpointVolumeCallback = VolumeWatch(hwnd.0 as isize).into();
        if vol.RegisterControlChangeNotify(&cb).is_ok() {
            // Registered for the life of the process.
            std::mem::forget((vol, cb));
        }
    }
}

// ---------------------------------------------------------------- keyboard

static HOOK_HWND: AtomicIsize = AtomicIsize::new(0);
static CAPS_HELD: AtomicBool = AtomicBool::new(false);
/// Caps Lock state tracked from the key stream itself; GetKeyState in the hook lags behind
/// fast presses because it reflects this thread's message queue, not the hardware.
static CAPS_ON: AtomicBool = AtomicBool::new(false);

fn held(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

unsafe extern "system" fn kb_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let vk = VIRTUAL_KEY(k.vkCode as u16);
        let hwnd = Some(HWND(HOOK_HWND.load(Ordering::Relaxed) as _));
        // Volume keys only matter at the ends of the range, where no change notification comes.
        if down && (vk == VK_VOLUME_UP || vk == VK_VOLUME_DOWN) {
            let _ = PostMessageW(hwnd, WM_VOLKEY, WPARAM((vk == VK_VOLUME_UP) as usize), LPARAM(0));
        }
        if vk == VK_CAPITAL {
            if down {
                if !CAPS_HELD.swap(true, Ordering::Relaxed) {
                    let on = !CAPS_ON.load(Ordering::Relaxed);
                    CAPS_ON.store(on, Ordering::Relaxed);
                    let _ = PostMessageW(hwnd, WM_CAPS, WPARAM(on as usize), LPARAM(0));
                }
            } else {
                CAPS_HELD.store(false, Ordering::Relaxed);
            }
        } else if down {
            let shift = matches!(vk, VK_SHIFT | VK_LSHIFT | VK_RSHIFT);
            let alt = matches!(vk, VK_MENU | VK_LMENU | VK_RMENU);
            let ctrl = matches!(vk, VK_CONTROL | VK_LCONTROL | VK_RCONTROL);
            let combo = (vk == VK_SPACE && (held(VK_LWIN) || held(VK_RWIN)))
                || (shift && (held(VK_MENU) || held(VK_CONTROL)))
                || ((alt || ctrl) && held(VK_SHIFT));
            if combo {
                let _ = PostMessageW(hwnd, WM_LANG, WPARAM(0), LPARAM(0));
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

pub fn install_keyboard_hook(hwnd: HWND) -> Option<HHOOK> {
    HOOK_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
    CAPS_ON.store(unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1 != 0, Ordering::Relaxed);
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), None, 0).ok() }
}

/// Keyboard layout of the foreground thread: (layout id, short code, native language name).
pub fn current_layout() -> (isize, String, String) {
    unsafe {
        let fg = GetForegroundWindow();
        let tid = GetWindowThreadProcessId(fg, None);
        let hkl = GetKeyboardLayout(tid);
        let langid = (hkl.0 as usize & 0xFFFF) as u32;
        let mut loc = [0u16; 85];
        let n = LCIDToLocaleName(langid, Some(&mut loc), 0);
        if n <= 1 {
            return (hkl.0 as isize, "??".into(), String::new());
        }
        let name = String::from_utf16_lossy(&loc[..(n - 1) as usize]);
        let mut disp = [0u16; 128];
        // LOCALE_SNATIVELANGUAGENAME
        let m = GetLocaleInfoEx(PCWSTR(loc.as_ptr()), 0x04, Some(&mut disp));
        let native = if m > 1 { String::from_utf16_lossy(&disp[..(m - 1) as usize]) } else { name.clone() };
        let code = name.split('-').next().unwrap_or("").to_uppercase();
        (hkl.0 as isize, code, native)
    }
}

// ---------------------------------------------------------------- autostart

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_NAME: PCWSTR = w!("Islet");

pub fn autostart_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
}

pub fn set_autostart(on: bool) {
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let value = HSTRING::from(format!("\"{}\"", exe.display()));
            let bytes = ((value.len() + 1) * 2) as u32;
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME, REG_SZ.0, Some(value.as_ptr() as _), bytes);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME);
        }
    }
}

// ---------------------------------------------------------------- fullscreen

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
const OBJID_WINDOW: i32 = 0;
static FG_TARGET: AtomicIsize = AtomicIsize::new(0);

/// Posts WM_FOREGROUND whenever the foreground window changes or moves/resizes
/// (e.g. a video entering fullscreen), so fullscreen hiding needs no polling.
pub fn hook_foreground(hwnd: HWND) {
    FG_TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    unsafe {
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT);
        SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT);
    }
}

unsafe extern "system" fn on_win_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _: i32, _: u32, _: u32) {
    if event == EVENT_OBJECT_LOCATIONCHANGE && (id_object != OBJID_WINDOW || hwnd != GetForegroundWindow()) {
        return;
    }
    let own = HWND(FG_TARGET.load(Ordering::Relaxed) as *mut _);
    let _ = PostMessageW(Some(own), WM_FOREGROUND, WPARAM(0), LPARAM(0));
}

pub fn foreground_covers(mon: RECT, own: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == own || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return false;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls) as usize;
        let cls = String::from_utf16_lossy(&cls[..n]);
        if cls == "WorkerW" || cls == "Progman" || cls == "Shell_TrayWnd" {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        r.left <= mon.left && r.top <= mon.top && r.right >= mon.right && r.bottom >= mon.bottom
    }
}

// ---------------------------------------------------------------- icon

/// A small white notch silhouette for the tray and window class.
pub fn make_icon(size: i32) -> HICON {
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            return HICON::default();
        };
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);
        let s = size as f32;
        let (w, h) = (s * 0.86, s * 0.46);
        let (cx, cy) = (s / 2.0, s / 2.0);
        let r = h / 2.0;
        for y in 0..size {
            for x in 0..size {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                // Signed distance to a capsule.
                let dx = ((fx - cx).abs() - (w / 2.0 - r)).max(0.0);
                let dy = fy - cy;
                let d = (dx * dx + dy * dy).sqrt() - r;
                let mut a = (0.5 - d).clamp(0.0, 1.0);
                // Camera dot.
                let dd = ((fx - (cx + w * 0.22)).powi(2) + (fy - cy).powi(2)).sqrt() - h * 0.18;
                let hole = (0.5 - dd).clamp(0.0, 1.0);
                let lum = 1.0 - hole * 0.85;
                a = a.max(0.0);
                let c = (255.0 * lum * a) as u32;
                let al = (255.0 * a) as u32;
                px[(y * size + x) as usize] = (al << 24) | (c << 16) | (c << 8) | c;
            }
        }
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
