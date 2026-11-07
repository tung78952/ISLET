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

// ---------------------------------------------------------------- Luma app

/// Temporary field measurements (ISLET_TIMELOG=<file>): a millisecond clock and a line.
pub fn timelog(line: &str) {
    let Some(path) = std::env::var_os("ISLET_TIMELOG") else { return };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let _ = f.write_all(format!("{t} {line}
").as_bytes());
    }
}

/// A fullscreen game, video or presentation is in front (Windows' own judgement).
pub fn busy() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
    unsafe { SHQueryUserNotificationState() }.is_ok_and(|s| s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE)
}

const LUMA_TASK: &str = "Luma";

fn quiet_command(program: &str, args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The separate Luma app's process ids (pause.exe in Luma's install folder).
fn luma_app_pids() -> Vec<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case("pause.exe") {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, e.th32ProcessID) {
                    let mut buf = [0u16; 520];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                        let path = String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase();
                        if path.contains("\\luma\\") {
                            out.push(e.th32ProcessID);
                        }
                    }
                    let _ = CloseHandle(h);
                }
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// The separate Luma app is running or still starts with Windows.
pub fn luma_app_active() -> bool {
    if !luma_app_pids().is_empty() {
        return true;
    }
    quiet_command("schtasks", &["/Query", "/TN", LUMA_TASK, "/FO", "CSV", "/NH"]).is_some_and(|s| s.contains(LUMA_TASK) && !s.contains("Disabled"))
}

/// Closes the Luma app and stops it starting with Windows. Nothing is uninstalled or deleted:
/// re-enabling its task (or starting it) brings it back.
pub fn turn_off_luma_app() {
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    for pid in luma_app_pids() {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(h, 0);
                let _ = CloseHandle(h);
            }
        }
    }
    let _ = quiet_command("schtasks", &["/Change", "/TN", LUMA_TASK, "/DISABLE"]);
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

// ---------------------------------------------------------------- bringing up an app

/// Brings up a top-level window `pick` accepts (by its title and executable name).
fn focus_window(pick: impl Fn(&str, &str) -> bool) -> bool {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> windows::core::BOOL {
        let out = &mut *(lp.0 as *mut Vec<HWND>);
        let tool = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
        if IsWindowVisible(h).as_bool() && !tool && GetWindowTextLengthW(h) > 0 {
            out.push(h);
        }
        true.into()
    }
    let mut wins: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut wins as *mut _ as isize));
        for h in wins {
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(h, &mut buf) as usize;
            let title = String::from_utf16_lossy(&buf[..n]).to_lowercase();
            if pick(&title, &exe_of(h)) {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                SwitchToThisWindow(h, true);
                return true;
            }
        }
    }
    false
}

/// Lower-case file name of the process owning a window.
unsafe fn exe_of(h: HWND) -> String {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
    let _ = CloseHandle(p);
    if !ok {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
    path.rsplit('\\').next().unwrap_or("").to_string()
}

/// An app's own window if it is open, else starts it.
pub fn open_app(aumid: &str, name: &str) {
    let name: String = name.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    let id = aumid.to_lowercase();
    let found = focus_window(|_, exe| {
        let stem = exe.trim_end_matches(".exe");
        !stem.is_empty() && stem != "applicationframehost" && ((name.len() >= 3 && stem.contains(&name)) || (stem.len() >= 3 && id.contains(stem)))
    });
    if !found && !aumid.is_empty() {
        shell_open(std::ffi::OsStr::new(&format!("shell:AppsFolder\\{aumid}")));
    }
}

/// The browser window showing `site` (its tab is the active one) if there is one, else a new tab.
pub fn open_site(site: &str) {
    let host = site.split("://").last().unwrap_or(site).trim_start_matches("www.").to_lowercase();
    let label = host.split('.').next().unwrap_or("").to_string();
    let browsers = ["chrome.exe", "msedge.exe", "brave.exe", "opera.exe", "firefox.exe", "browser.exe", "vivaldi.exe"];
    if label.is_empty() || !focus_window(|title, exe| browsers.contains(&exe) && title.contains(&label)) {
        let url = if site.contains("://") { site.to_string() } else { format!("https://{site}") };
        shell_open(std::ffi::OsStr::new(&url));
    }
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
    static METER: std::cell::RefCell<Option<windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation>> = const { std::cell::RefCell::new(None) };
}

/// Opens the loudness meter of the default output. It reads levels only, never the audio.
pub fn open_meter() {
    let meter = unsafe {
        CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).ok().and_then(|en| en.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()).and_then(|dev| dev.Activate(CLSCTX_ALL, None).ok())
    };
    METER.with(|m| *m.borrow_mut() = meter);
}

/// Peak loudness of what is playing right now (0..1).
pub fn audio_peak() -> f32 {
    METER.with(|m| m.borrow().as_ref().and_then(|m| unsafe { m.GetPeakValue() }.ok()).unwrap_or(0.0))
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
    pub path: String,
}

/// Default output changes re-target the volume watch; endpoint state changes become cards.
#[implement(windows::Win32::Media::Audio::IMMNotificationClient)]
struct AudioDevices {
    hwnd: isize,
    /// Whether each endpoint is active. Windows re-reports every endpoint when one device
    /// connects (a Bluetooth headset makes the jack report "unplugged" again): only a real
    /// change counts.
    known: std::sync::Mutex<std::collections::HashMap<String, bool>>,
}

impl windows::Win32::Media::Audio::IMMNotificationClient_Impl for AudioDevices_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: windows::Win32::Media::Audio::DEVICE_STATE) -> windows::core::Result<()> {
        use windows::Win32::Media::Audio::{DEVICE_STATE_ACTIVE, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED};
        // The high bits carry flags.
        let state = windows::Win32::Media::Audio::DEVICE_STATE(state.0 & 0xF);
        let active = state == DEVICE_STATE_ACTIVE;
        if !active && state != DEVICE_STATE_UNPLUGGED && state != DEVICE_STATE_NOTPRESENT {
            return Ok(());
        }
        let Ok(id) = (unsafe { id.to_string() }) else { return Ok(()) };
        let was = self.known.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id.clone(), active);
        if was.unwrap_or(false) == active {
            return Ok(());
        }
        let hwnd = self.hwnd;
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
                let _ = PostMessageW(Some(HWND(self.hwnd as _)), crate::WM_AUDIODEV, WPARAM(1), LPARAM(0));
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
    Some(AudioChange { name: text(&PKEY_DEVICE_DESC), active: false, form_factor, bluetooth: path.contains("BTHENUM") || path.contains("BTHHFENUM") || path.contains("BTHLE"), path })
}

/// Registers for audio endpoint changes for the life of the process.
pub fn watch_audio_devices(hwnd: HWND) {
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        // What is active now, so the first report of each endpoint can be told from a change.
        let mut known = std::collections::HashMap::new();
        if let Ok(list) = en.EnumAudioEndpoints(eRender, windows::Win32::Media::Audio::DEVICE_STATE_ACTIVE) {
            for i in 0..list.GetCount().unwrap_or(0) {
                if let Some(id) = list.Item(i).ok().and_then(|d| d.GetId().ok()).and_then(|p| p.to_string().ok()) {
                    known.insert(id, true);
                }
            }
        }
        let cb: windows::Win32::Media::Audio::IMMNotificationClient = AudioDevices { hwnd: hwnd.0 as isize, known: known.into() }.into();
        if en.RegisterEndpointNotificationCallback(&cb).is_ok() {
            std::mem::forget((en, cb));
        }
    }
}

// ---------------------------------------------------------------- keyboard

static HOOK_HWND: AtomicIsize = AtomicIsize::new(0);
static CAPS_HELD: AtomicBool = AtomicBool::new(false);
/// Caps Lock state tracked from the key stream itself, so the card shows at once: GetKeyState in
/// the hook lags behind fast presses. It is only a prediction (a press can go unseen, and Windows
/// can be set to turn Caps off with Shift), so `caps_settle` puts it right afterwards.
static CAPS_ON: AtomicBool = AtomicBool::new(false);

pub fn shut_down() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("shutdown").args(["/s", "/t", "0", "/f"]).creation_flags(CREATE_NO_WINDOW).spawn();
}

/// Once the key has been quiet: Windows' word against the prediction. Returns the real state if
/// the prediction was wrong.
pub fn caps_settle() -> Option<bool> {
    let real = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1 != 0;
    (CAPS_ON.swap(real, Ordering::Relaxed) != real).then_some(real)
}

fn held(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

/// Tick count (ms) of the last key press anywhere; lets the notch notice typing beneath it.
static LAST_KEY: AtomicIsize = AtomicIsize::new(0);

/// Milliseconds since the last key press.
/// Whether the window in front belongs to a process that outranks this one (run as administrator,
/// or shielded by anti-cheat). The keyboard hook sees nothing pressed in such a window.
/// The class of a window lying over `area` (screen pixels) above ours, if any: something shown,
/// not cloaked and of real size. Only the few windows above ours are looked at.
pub fn covered_from_above(ours: HWND, area: RECT) -> Option<String> {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    unsafe {
        let mut w = GetWindow(ours, GW_HWNDPREV).ok();
        let mut seen = 0;
        while let Some(h) = w.filter(|h| !h.is_invalid()) {
            seen += 1;
            if seen > 64 {
                break;
            }
            let mut r = RECT::default();
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
            if IsWindowVisible(h).as_bool() && cloaked == 0 && GetWindowRect(h, &mut r).is_ok() && r.right - r.left > 4 && r.bottom - r.top > 4 {
                let over = r.left < area.right && area.left < r.right && r.top < area.bottom && area.top < r.bottom;
                if over {
                    let mut name = [0u16; 64];
                    let n = GetClassNameW(h, &mut name);
                    return Some(String::from_utf16_lossy(&name[..n.max(0) as usize]));
                }
            }
            w = GetWindow(h, GW_HWNDPREV).ok();
        }
        None
    }
}

pub fn front_outranks_us() -> bool {
    use windows::Win32::Security::TOKEN_QUERY;
    use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return true };
        let mut token = HANDLE::default();
        let readable = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok();
        if readable {
            let _ = CloseHandle(token);
        }
        let _ = CloseHandle(process);
        !readable
    }
}

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
    *FOCUS_RECT.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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
            *FOCUS_RECT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = r;
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

/// Windows keeps a setting per phone app, each under its own name.
const PHONE_KEY: &str = "Microsoft.YourPhone_8wekyb3d8bbwe!YourPhoneNotifications_";

/// The phone app an icon of Phone Link's belongs to: `app_icon_<bundle id>1.0.jpg`.
fn bundle_of_icon(file: &str) -> Option<&str> {
    let name = file.strip_prefix("app_icon_")?.strip_suffix(".jpg")?;
    let name = name.strip_suffix("1.0").unwrap_or(name);
    (!name.is_empty()).then_some(name)
}

/// The names of Windows' per-app notification settings.
fn notification_keys() -> Vec<String> {
    let mut names = Vec::new();
    let path = HSTRING::from(NOTIF_KEY);
    let mut key = HKEY::default();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_READ, &mut key) != ERROR_SUCCESS {
            return names;
        }
        let mut i = 0;
        loop {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(key, i, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) != ERROR_SUCCESS {
                break;
            }
            names.push(String::from_utf16_lossy(&buf[..len as usize]));
            i += 1;
        }
        let _ = RegCloseKey(key);
    }
    names
}

/// Turns off the banner of every phone app Phone Link knows, those it has only an icon for too,
/// so a new app's first notification stays out of the corner.
pub fn hide_phone_banners() {
    let mut keys: Vec<String> = notification_keys().into_iter().filter(|k| k.starts_with(PHONE_KEY)).collect();
    let icons = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Packages\\Microsoft.YourPhone_8wekyb3d8bbwe\\TempState\\app_icons"));
    if let Some(dir) = icons.and_then(|d| std::fs::read_dir(d).ok()) {
        for file in dir.flatten() {
            if let Some(bundle) = bundle_of_icon(&file.file_name().to_string_lossy()) {
                keys.push(format!("{PHONE_KEY}{bundle}"));
            }
        }
    }
    keys.sort();
    keys.dedup();
    for key in keys.iter().filter(|k| show_banner_value(k) != Some(0)) {
        hide_banner(key);
    }
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

/// Whether the foreground window is an app filling the monitor. `None` while one of Windows' own
/// layers is in front (the capture overlay, a notification, Win+Tab, Start, a flyout): those come
/// and go over whatever was there, so the notch stays as it was.
pub fn foreground_covers(mon: RECT, own: HWND) -> Option<bool> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || fg == own || fg == GetShellWindow() || fg == GetDesktopWindow() {
            return Some(false);
        }
        let cls = window_class(fg);
        if cls == "WorkerW" || cls == "Progman" || cls == "Shell_TrayWnd" {
            return Some(false);
        }
        if shell_overlay(fg, &cls) {
            return None;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return Some(false);
        }
        // Chrome in F11 gives up its bottom row for a moment after another window had the front.
        const SLACK: i32 = 2;
        if !(r.left <= mon.left + SLACK && r.top <= mon.top + SLACK && r.right >= mon.right - SLACK && r.bottom >= mon.bottom - SLACK) {
            return Some(false);
        }
        use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
        if SHQueryUserNotificationState().is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE) {
            return Some(true);
        }
        // Borderless full screen (a video after F11): an ordinary window filling the monitor.
        // A browser put in F11 from maximized still says it is maximized, but has lost its frame.
        let ex = GetWindowLongPtrW(fg, GWL_EXSTYLE) as u32;
        let style = GetWindowLongPtrW(fg, GWL_STYLE) as u32;
        let framed = style & WS_CAPTION.0 == WS_CAPTION.0 || style & WS_THICKFRAME.0 != 0;
        Some(ex & (WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0) == 0 && (!IsZoomed(fg).as_bool() || !framed))
    }
}

/// The foreground window's class and program, for the time log.
pub fn foreground_name() -> String {
    unsafe {
        let fg = GetForegroundWindow();
        let mut r = RECT::default();
        let _ = GetWindowRect(fg, &mut r);
        let style = GetWindowLongPtrW(fg, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(fg, GWL_EXSTYLE) as u32;
        format!(
            "{} {} {:?} rect=({},{},{},{}) style={style:#X} ex={ex:#X} zoomed={}",
            window_class(fg),
            window_exe(fg).unwrap_or_default(),
            fg.0,
            r.left,
            r.top,
            r.right,
            r.bottom,
            IsZoomed(fg).as_bool()
        )
    }
}

unsafe fn window_class(h: HWND) -> String {
    let mut cls = [0u16; 64];
    let n = GetClassNameW(h, &mut cls) as usize;
    String::from_utf16_lossy(&cls[..n])
}

/// The program a window belongs to, lower case (e.g. "chrome.exe").
unsafe fn window_exe(h: HWND) -> Option<String> {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
    let _ = CloseHandle(process);
    if !ok {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]).to_ascii_lowercase();
    path.rsplit('\\').next().map(str::to_string)
}

/// The foreground window belongs to one of the shell's transient layers.
unsafe fn shell_overlay(fg: HWND, class: &str) -> bool {
    // ForegroundStaging stands in front for a moment while Windows hands the foreground over.
    if class == "XamlExplorerHostIslandWindow" || class == "MultitaskingViewFrame" || class == "ForegroundStaging" {
        return true;
    }
    window_exe(fg).is_some_and(|exe| ["screenclippinghost.exe", "snippingtool.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe", "searchhost.exe"].contains(&exe.as_str()))
}

// ---------------------------------------------------------------- icon

/// Islet's logo as an icon `size` px square: on its white tile (`tile`), or the pill alone (tray).
pub fn make_icon(size: i32, tile: bool) -> HICON {
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
        for (dst, [r, g, b, a]) in px.iter_mut().zip(crate::logo::logo(size as u32, tile)) {
            let pre = |c: u8| (c as u32 * a as u32 + 127) / 255;
            *dst = (a as u32) << 24 | pre(r) << 16 | pre(g) << 8 | pre(b);
        }
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: TRUE, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

#[cfg(test)]
mod phone_tests {
    use super::bundle_of_icon;

    #[test]
    fn icon_names_give_the_phone_app() {
        assert_eq!(bundle_of_icon("app_icon_com.ss.iphone.ugc.Ame1.0.jpg"), Some("com.ss.iphone.ugc.Ame"));
        assert_eq!(bundle_of_icon("app_icon_vn.com.vng.zingalo.jpg"), Some("vn.com.vng.zingalo"));
        assert_eq!(bundle_of_icon("other.jpg"), None);
        assert_eq!(bundle_of_icon("app_icon_.jpg"), None);
    }
}
