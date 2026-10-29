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
    use windows::Win32::System::SystemServices::GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS;
    for guid in [GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING, GUID_VIDEO_CURRENT_MONITOR_BRIGHTNESS] {
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

/// A network change, boxed into the LPARAM of WM_NET.
pub struct NetChange {
    /// 0 = disconnected, 1 = no internet, 2 = internet.
    pub status: usize,
    /// Wi-Fi network name, or None for wired connections.
    pub ssid: Option<String>,
    /// Signal strength in bars 0–5, when the adapter reports it.
    pub bars: Option<u8>,
}

fn wifi_details() -> (Option<String>, Option<u8>) {
    let Ok(p) = NetworkInformation::GetInternetConnectionProfile() else { return (None, None) };
    let ssid = if p.IsWlanConnectionProfile().unwrap_or(false) {
        p.WlanConnectionProfileDetails().and_then(|d| d.GetConnectedSsid()).map(|s| s.to_string()).ok().filter(|s| !s.is_empty())
    } else {
        None
    };
    let bars = p.GetSignalBars().and_then(|r| r.Value()).ok();
    (ssid, bars)
}

pub fn spawn_network(hwnd: isize) {
    std::thread::spawn(move || {
        name_thread("islet-network");
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let mut last = net_status();
        let mut last_ssid = wifi_details().0;
        // Woken by Windows on any network change; nothing runs in between.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let handler = windows::Networking::Connectivity::NetworkStatusChangedEventHandler::new(move |_| {
            let _ = tx.send(());
            Ok(())
        });
        if NetworkInformation::NetworkStatusChanged(&handler).is_err() {
            return;
        }
        loop {
            if rx.recv().is_err() {
                return;
            }
            // A change arrives as a burst while the link settles.
            std::thread::sleep(Duration::from_millis(400));
            while rx.try_recv().is_ok() {}
            let now = net_status();
            if now != last {
                last = now;
                let (ssid, bars) = if now > 0 { wifi_details() } else { (last_ssid.clone(), None) };
                if now > 0 {
                    last_ssid = ssid.clone();
                }
                let b = Box::into_raw(Box::new(NetChange { status: now, ssid, bars }));
                unsafe {
                    if PostMessageW(Some(HWND(hwnd as _)), WM_NET, WPARAM(0), LPARAM(b as isize)).is_err() {
                        drop(Box::from_raw(b));
                    }
                }
            }
        }
    });
}

/// Names the calling thread (shown by debuggers and process tools when profiling).
pub fn name_thread(name: &str) {
    unsafe {
        let _ = windows::Win32::System::Threading::SetThreadDescription(windows::Win32::System::Threading::GetCurrentThread(), &HSTRING::from(name));
    }
}

// ---------------------------------------------------------------- screenshots

/// A screenshot file found for a Snipping Tool notification, posted as WM_SHOT.
pub struct Shot {
    pub id: u32,
    pub path: std::path::PathBuf,
    pub img: Option<(u32, u32, Vec<u8>)>,
    pub size: String,
}

/// Opens a file, folder, URL or shell: path with its default handler.
pub fn shell_open(target: &std::ffi::OsStr) {
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
    }
}

/// Looks (on a worker thread) for the image the Snipping Tool just saved in the Screenshots folder
/// and posts it with a thumbnail. Snipping Tool saves right around its notification, so it waits
/// briefly; with saving turned off (clipboard only) nothing is posted.
pub fn find_screenshot(hwnd: HWND, id: u32, px: i32) {
    let target = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        name_thread("islet-screenshot");
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let Ok(dir) = windows::Win32::UI::Shell::SHGetKnownFolderPath(&windows::Win32::UI::Shell::FOLDERID_Screenshots, windows::Win32::UI::Shell::KF_FLAG_DEFAULT, None) else { return };
        let folder = std::path::PathBuf::from(dir.to_string().unwrap_or_default());
        windows::Win32::System::Com::CoTaskMemFree(Some(dir.0 as _));
        for _ in 0..8 {
            let newest = std::fs::read_dir(&folder).ok().and_then(|it| {
                it.flatten()
                    .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                    .filter(|(t, _)| t.elapsed().is_ok_and(|d| d < Duration::from_secs(15)))
                    .max_by_key(|(t, _)| *t)
            });
            if let Some((_, path)) = newest {
                let img = crate::drop::shell_image(&path, px);
                let size = std::fs::metadata(&path).map(|m| crate::size_label(m.len())).unwrap_or_default();
                let b = Box::into_raw(Box::new(Shot { id, path, img, size }));
                if PostMessageW(Some(HWND(target as _)), crate::WM_SHOT, WPARAM(0), LPARAM(b as isize)).is_err() {
                    drop(Box::from_raw(b));
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    });
}

// ---------------------------------------------------------------- browser chat tabs

/// Facebook doesn't send desktop notifications while its tab is open; it retitles the tab
/// instead ("Denis đã nhắn tin cho bạn" / "Denis sent you a message"). Chromium reports tab title
/// changes as accessibility name-change events once an accessibility client has looked at its
/// tab strip, so this watcher touches each browser's tab strip once (never page contents) and
/// then just listens: nothing runs until some title changes. New senders are posted as
/// WM_TABMSG with a boxed (sender, text).
static TAB_TARGET: AtomicIsize = AtomicIsize::new(0);
static TAB_RECENT: std::sync::Mutex<Vec<(String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());

const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;

unsafe extern "system" fn on_name_change(_: HWINEVENTHOOK, _: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    use windows::Win32::UI::Accessibility::*;
    // Cheap filter first: only Chromium browser windows.
    let root = GetAncestor(hwnd, GA_ROOT);
    let mut cls = [0u16; 32];
    let n = GetClassNameW(root, &mut cls) as usize;
    if String::from_utf16_lossy(&cls[..n]) != "Chrome_WidgetWin_1" {
        return;
    }
    // Pages rename their own elements constantly (timestamps, players); only tabs matter. Each
    // element's role is asked once (a cross-process call) and remembered, so page churn costs a
    // lookup instead of two calls per event.
    thread_local! {
        static IS_TAB: std::cell::RefCell<std::collections::HashMap<(isize, i32), bool>> = Default::default();
    }
    let key = (root.0 as isize, id_child);
    if IS_TAB.with(|m| m.borrow().get(&key) == Some(&false)) {
        return;
    }
    let mut acc: Option<IAccessible> = None;
    let mut child = windows::Win32::System::Variant::VARIANT::default();
    if AccessibleObjectFromEvent(hwnd, id_object as u32, id_child as u32, &mut acc, &mut child).is_err() {
        return;
    }
    let Some(acc) = acc else { return };
    let known = IS_TAB.with(|m| m.borrow().get(&key).copied());
    if known.is_none() {
        const ROLE_SYSTEM_PAGETAB: i32 = 0x25;
        let tab = acc.get_accRole(&child).ok().and_then(|v| i32::try_from(&v).ok()) == Some(ROLE_SYSTEM_PAGETAB);
        IS_TAB.with(|m| {
            let mut m = m.borrow_mut();
            if m.len() > 20_000 {
                m.clear();
            }
            m.insert(key, tab);
        });
        if !tab {
            return;
        }
    }
    let Some(name) = acc.get_accName(&child).ok().map(|b| b.to_string()) else { return };
    let Some((sender, text)) = chat_title(&name) else { return };
    let now = std::time::Instant::now();
    let mut recent = TAB_RECENT.lock().unwrap();
    recent.retain(|(_, t)| now.duration_since(*t) < Duration::from_secs(45));
    // Facebook flips the title back and forth while a message is unread: report once.
    let fresh = !recent.iter().any(|(s, _)| *s == sender);
    if let Some(e) = recent.iter_mut().find(|(s, _)| *s == sender) {
        e.1 = now;
    } else {
        recent.push((sender.clone(), now));
    }
    if fresh {
        let b = Box::into_raw(Box::new((sender, text)));
        let own = HWND(TAB_TARGET.load(Ordering::Relaxed) as *mut _);
        if PostMessageW(Some(own), crate::WM_TABMSG, WPARAM(0), LPARAM(b as isize)).is_err() {
            drop(Box::from_raw(b));
        }
    }
}

pub fn spawn_tab_watch(hwnd: isize) {
    TAB_TARGET.store(hwnd, Ordering::Relaxed);
    std::thread::spawn(move || unsafe {
        name_thread("islet-tab-watch");
        use windows::Win32::UI::Accessibility::*;
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let hook = SetWinEventHook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE, None, Some(on_name_change), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        if hook.is_invalid() {
            return;
        }
        let uia = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL).ok();
        let walker = uia.as_ref().and_then(|u| u.ControlViewWalker().ok());
        let mut touched: Vec<isize> = Vec::new();
        // A timer wakes the message loop every 10 s to touch newly opened browser windows.
        let _ = SetTimer(None, 1, 10_000, None);
        let mut msg = MSG::default();
        loop {
            if let (Some(u), Some(w)) = (&uia, &walker) {
                let wins = chromium_windows();
                touched.retain(|h| wins.iter().any(|w| w.0 as isize == *h));
                for win in wins {
                    if !touched.contains(&(win.0 as isize)) {
                        if let Ok(root) = u.ElementFromHandle(win) {
                            let _ = find_tab_strip(w, &root);
                        }
                        touched.push(win.0 as isize);
                    }
                }
            }
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                break;
            }
            DispatchMessageW(&msg);
        }
    });
}

/// The element whose children are the browser's tabs (breadth-first over the frame, skipping
/// web documents).
unsafe fn find_tab_strip(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    root: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::Win32::UI::Accessibility::*;
    let mut queue = std::collections::VecDeque::from([(root.clone(), 0u32)]);
    let mut visited = 0;
    while let Some((el, depth)) = queue.pop_front() {
        visited += 1;
        if visited > 400 {
            return None;
        }
        let ct = el.CurrentControlType().unwrap_or_default();
        if ct == UIA_TabItemControlTypeId {
            return walker.GetParentElement(&el).ok();
        }
        if ct == UIA_DocumentControlTypeId || depth > 9 {
            continue;
        }
        let mut child = walker.GetFirstChildElement(&el).ok();
        while let Some(c) = child {
            child = walker.GetNextSiblingElement(&c).ok();
            queue.push_back((c, depth + 1));
        }
    }
    None
}

/// Sender and message line from a Facebook/Messenger chat tab title.
fn chat_title(name: &str) -> Option<(String, String)> {
    let n = name.trim();
    for (pat, text) in [
        (" đã nhắn tin cho bạn", "đã nhắn tin cho bạn"),
        (" đã gửi tin nhắn cho bạn", "đã gửi tin nhắn cho bạn"),
        (" sent you a message", "sent you a message"),
        (" messaged you", "messaged you"),
    ] {
        if let Some(i) = n.find(pat) {
            let sender = n[..i].trim().trim_start_matches(|c: char| c == '(' || c.is_ascii_digit() || c == ')' || c == ' ');
            if !sender.is_empty() {
                return Some((sender.to_string(), text.to_string()));
            }
        }
    }
    None
}

fn chromium_windows() -> Vec<HWND> {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lp.0 as *mut Vec<HWND>);
        let mut cls = [0u16; 64];
        let n = GetClassNameW(h, &mut cls) as usize;
        if IsWindowVisible(h).as_bool() && String::from_utf16_lossy(&cls[..n]) == "Chrome_WidgetWin_1" && GetWindowTextLengthW(h) > 0 {
            out.push(h);
        }
        true.into()
    }
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut v as *mut _ as isize));
    }
    v.truncate(6);
    v
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

thread_local! {
    /// The volume subscription on the current default output; replaced when the default changes.
    static VOLUME_SUB: std::cell::RefCell<Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>> = const { std::cell::RefCell::new(None) };
}

/// WM_VOLUME carries the level in thousandths (low 16 bits) and the mute flag (bit 16).
/// Called again when the default output changes (headphones plugged in, Bluetooth connected), so
/// the toast follows the device that is actually playing.
pub fn watch_volume(hwnd: HWND) {
    unsafe {
        VOLUME_SUB.with(|s| {
            if let Some((vol, cb)) = s.borrow_mut().take() {
                let _ = vol.UnregisterControlChangeNotify(&cb);
            }
        });
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return };
        let Ok(vol) = dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else { return };
        if let (Ok(v), Ok(m)) = (vol.GetMasterVolumeLevelScalar(), vol.GetMute()) {
            VOLUME.store(((v.clamp(0.0, 1.0) * 1000.0).round() as isize) | ((m.as_bool() as isize) << 16), Ordering::Relaxed);
        }
        let cb: IAudioEndpointVolumeCallback = VolumeWatch(hwnd.0 as isize).into();
        if vol.RegisterControlChangeNotify(&cb).is_ok() {
            VOLUME_SUB.with(|s| *s.borrow_mut() = Some((vol, cb)));
        }
    }
}

/// An output endpoint that became active (plugged in) or went away, posted as WM_AUDIODEV.
pub struct AudioChange {
    pub name: String,
    pub active: bool,
    /// Endpoint form factor: 1 speakers, 3 headphones, 5 headset (others are ignored).
    pub form_factor: u32,
    pub bluetooth: bool,
}

/// Default output changes re-target the volume watch; endpoint state changes become cards.
#[implement(windows::Win32::Media::Audio::IMMNotificationClient)]
struct AudioDevices(isize);

impl windows::Win32::Media::Audio::IMMNotificationClient_Impl for AudioDevices_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: windows::Win32::Media::Audio::DEVICE_STATE) -> windows::core::Result<()> {
        use windows::Win32::Media::Audio::{DEVICE_STATE_ACTIVE, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED};
        let active = state == DEVICE_STATE_ACTIVE;
        if !active && state != DEVICE_STATE_UNPLUGGED && state != DEVICE_STATE_NOTPRESENT {
            return Ok(());
        }
        let Ok(id) = (unsafe { id.to_string() }) else { return Ok(()) };
        let hwnd = self.0;
        // Property reads go to the audio service: off this callback thread.
        std::thread::spawn(move || unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            if let Some(mut c) = endpoint(&id) {
                c.active = active;
                let b = Box::into_raw(Box::new(c));
                if PostMessageW(Some(HWND(hwnd as _)), crate::WM_AUDIODEV, WPARAM(0), LPARAM(b as isize)).is_err() {
                    drop(Box::from_raw(b));
                }
            }
        });
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: windows::Win32::Media::Audio::EDataFlow, role: windows::Win32::Media::Audio::ERole, _: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eMultimedia {
            unsafe {
                let _ = PostMessageW(Some(HWND(self.0 as _)), crate::WM_AUDIODEV, WPARAM(1), LPARAM(0));
            }
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &windows::Win32::Foundation::PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Name, form factor and bus of a render endpoint (None for capture endpoints).
unsafe fn endpoint(id: &str) -> Option<AudioChange> {
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{IMMEndpoint, PKEY_AudioEndpoint_FormFactor};
    use windows::Win32::System::Com::STGM_READ;
    const PKEY_DEVICE_DESC: PROPERTYKEY = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 2 };
    /// The endpoint's device instance path ("{1}.BTHENUM\...").
    const PKEY_DEVICE_PATH: PROPERTYKEY = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0xb3f8fa53_0004_438e_9003_51a46e139bfc), pid: 2 };
    let en = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
    let dev = en.GetDevice(&HSTRING::from(id)).ok()?;
    if windows::core::Interface::cast::<IMMEndpoint>(&dev).ok()?.GetDataFlow().ok()? != eRender {
        return None;
    }
    let store = dev.OpenPropertyStore(STGM_READ).ok()?;
    let text = |k: &PROPERTYKEY| store.GetValue(k).ok().map(|v| v.to_string()).unwrap_or_default();
    let form_factor = store.GetValue(&PKEY_AudioEndpoint_FormFactor).ok().and_then(|v| u32::try_from(&v).ok()).unwrap_or(0);
    let path = text(&PKEY_DEVICE_PATH).to_ascii_uppercase();
    Some(AudioChange { name: text(&PKEY_DEVICE_DESC), active: false, form_factor, bluetooth: path.contains("BTHENUM") || path.contains("BTHHFENUM") || path.contains("BTHLE") })
}

/// Registers for audio endpoint changes for the life of the process.
pub fn watch_audio_devices(hwnd: HWND) {
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let cb: windows::Win32::Media::Audio::IMMNotificationClient = AudioDevices(hwnd.0 as isize).into();
        if en.RegisterEndpointNotificationCallback(&cb).is_ok() {
            std::mem::forget((en, cb));
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

/// Tick count (ms) of the last key press anywhere; lets the notch notice typing beneath it.
static LAST_KEY: AtomicIsize = AtomicIsize::new(0);

/// Milliseconds since the last key press.
pub fn key_idle_ms() -> u32 {
    let last = LAST_KEY.load(Ordering::Relaxed) as u32;
    unsafe { windows::Win32::System::SystemInformation::GetTickCount() }.wrapping_sub(last)
}

unsafe extern "system" fn kb_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        if down {
            // Injected keys can carry time 0; stamp with the clock instead.
            LAST_KEY.store(windows::Win32::System::SystemInformation::GetTickCount() as isize, Ordering::Relaxed);
        }
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

static FG_TARGET: AtomicIsize = AtomicIsize::new(0);

/// Posts WM_FOREGROUND whenever the foreground window changes or moves/resizes
/// (e.g. a video entering fullscreen), so fullscreen hiding needs no polling.
pub fn hook_foreground(hwnd: HWND) {
    FG_TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    unsafe {
        // Location changes are not hooked: the system sends one for every cursor and caret move,
        // all delivered to the UI thread. A window going fullscreen in place is caught by the tick.
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT);
        SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, None, Some(on_move_size), 0, 0, WINEVENT_OUTOFCONTEXT);
    }
}

const EVENT_SYSTEM_MOVESIZESTART: u32 = 0x000A;
const EVENT_SYSTEM_MOVESIZEEND: u32 = 0x000B;
/// The window being moved or resized by the user, if any.
static MOVING: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn on_move_size(_: HWINEVENTHOOK, event: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    MOVING.store(if event == EVENT_SYSTEM_MOVESIZESTART { hwnd.0 as isize } else { 0 }, Ordering::Relaxed);
}

/// Screen rectangle of the window the user is currently dragging or resizing.
pub fn moving_rect() -> Option<RECT> {
    let h = MOVING.load(Ordering::Relaxed);
    if h == 0 {
        return None;
    }
    let mut r = RECT::default();
    unsafe { GetWindowRect(HWND(h as *mut _), &mut r) }.ok()?;
    Some(r)
}

/// Screen rectangle of the text caret in the foreground window, if that app reports one.
pub fn caret_rect() -> Option<RECT> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return None;
        }
        let tid = GetWindowThreadProcessId(fg, None);
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(tid, &mut gi).ok()?;
        if gi.hwndCaret.is_invalid() {
            return None;
        }
        let r = gi.rcCaret;
        let mut a = POINT { x: r.left, y: r.top };
        let mut b = POINT { x: r.right, y: r.bottom };
        let _ = ClientToScreen(gi.hwndCaret, &mut a);
        let _ = ClientToScreen(gi.hwndCaret, &mut b);
        Some(RECT { left: a.x, top: a.y, right: b.x.max(a.x + 1), bottom: b.y })
    }
}

thread_local! {
    static UIA: Option<windows::Win32::UI::Accessibility::IUIAutomation> = unsafe {
        CoCreateInstance(&windows::Win32::UI::Accessibility::CUIAutomation, None, CLSCTX_ALL).ok()
    };
}

static FOCUS_RECT: std::sync::Mutex<Option<RECT>> = std::sync::Mutex::new(None);

/// Last focused-field rectangle found by the background focus watcher (see `spawn_focus_watch`).
pub fn focused_rect() -> Option<RECT> {
    *FOCUS_RECT.lock().unwrap()
}

/// UI Automation calls go to other processes and can block, so they run here, only while the user
/// is typing, never on the UI thread.
pub fn spawn_focus_watch() {
    std::thread::spawn(|| unsafe {
        name_thread("islet-focus-watch");
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        loop {
            std::thread::sleep(Duration::from_millis(200));
            // Only fields without a Win32 caret, in a window that reaches the top of the screen,
            // can sit under the notch; anything else skips the cross-process query.
            let wanted = key_idle_ms() < 1200 && caret_rect().is_none() && {
                let mut r = RECT::default();
                GetWindowRect(GetForegroundWindow(), &mut r).is_ok() && r.top < 240
            };
            let r = if wanted { focused_rect_now() } else { None };
            *FOCUS_RECT.lock().unwrap() = r;
        }
    });
}

/// Screen rectangle of the keyboard-focused element (via UI Automation), for apps whose text
/// fields have no Win32 caret (browsers, Electron apps such as VS Code). Ignores whole windows.
fn focused_rect_now() -> Option<RECT> {
    UIA.with(|u| unsafe {
        let el = u.as_ref()?.GetFocusedElement().ok()?;
        let r = el.CurrentBoundingRectangle().ok()?;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        // A focused field is small; a focused document or window would match everything.
        (w > 0 && h > 0 && h < 200).then_some(r)
    })
}

// ---------------------------------------------------------------- notification banners

const NOTIF_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Notifications\\Settings";

fn banner_backup_path() -> std::path::PathBuf {
    crate::model::data_dir().join("banners.txt")
}

fn read_banner_backup() -> Vec<(String, Option<u32>)> {
    std::fs::read_to_string(banner_backup_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.rsplit_once('\t'))
        .map(|(id, v)| (id.to_string(), v.parse().ok()))
        .collect()
}

fn show_banner_value(aumid: &str) -> Option<u32> {
    let key = HSTRING::from(format!("{NOTIF_KEY}\\{aumid}"));
    let mut v = 0u32;
    let mut n = 4u32;
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, &key, w!("ShowBanner"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as _), Some(&mut n)) };
    (ok == ERROR_SUCCESS).then_some(v)
}

/// Turns off Windows' corner banner for `aumid` (Islet shows its notifications instead), recording
/// the previous setting so `restore_banners` can put it back. Notifications still reach the
/// notification centre, where Islet reads them.
pub fn hide_banner(aumid: &str) {
    if aumid.is_empty() {
        return;
    }
    let mut backup = read_banner_backup();
    if !backup.iter().any(|(id, _)| id == aumid) {
        backup.push((aumid.to_string(), show_banner_value(aumid)));
        let text: String = backup.iter().map(|(id, v)| format!("{id}\t{}\n", v.map(|v| v.to_string()).unwrap_or_else(|| "-".into()))).collect();
        let _ = std::fs::write(banner_backup_path(), text);
    }
    let key = HSTRING::from(format!("{NOTIF_KEY}\\{aumid}"));
    let zero = 0u32;
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, &key, w!("ShowBanner"), REG_DWORD.0, Some(&zero as *const u32 as _), 4);
    }
}

/// Whether Islet has already turned off `aumid`'s banner.
pub fn banner_hidden(aumid: &str) -> bool {
    show_banner_value(aumid) == Some(0) && read_banner_backup().iter().any(|(id, _)| id == aumid)
}

/// Puts every banner setting Islet changed back the way it was.
pub fn restore_banners() {
    for (aumid, old) in read_banner_backup() {
        let key = HSTRING::from(format!("{NOTIF_KEY}\\{aumid}"));
        unsafe {
            match old {
                Some(v) => {
                    let _ = RegSetKeyValueW(HKEY_CURRENT_USER, &key, w!("ShowBanner"), REG_DWORD.0, Some(&v as *const u32 as _), 4);
                }
                None => {
                    let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, w!("ShowBanner"));
                }
            }
        }
    }
    let _ = std::fs::remove_file(banner_backup_path());
}

unsafe extern "system" fn on_win_event(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
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
