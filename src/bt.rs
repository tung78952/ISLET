//! Bluetooth connect/disconnect toasts for paired devices, via a WinRT association-endpoint watcher.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{Interface, Ref, HSTRING};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher};
use windows::Foundation::{IReference, TypedEventHandler};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows_collections::IIterable;

use crate::devices::DeviceKind;
use crate::WM_BLUETOOTH;

const CONNECTED: &str = "System.Devices.Aep.IsConnected";
const CONTAINER: &str = "System.Devices.Aep.ContainerId";
/// DEVPKEY_Bluetooth_Battery, reported (in percent) on the hands-free node of the device.
const BATTERY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
/// Classic Bluetooth and Bluetooth LE protocol ids, paired devices only.
const SELECTOR: &str = "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\") AND System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";

/// A phone is connected right now, and when the last one left (tick count).
static PHONE_ON: AtomicBool = AtomicBool::new(false);
static PHONE_LEFT: AtomicU64 = AtomicU64::new(0);
/// A phone that left a moment ago still counts: Bluetooth drops briefly.
const PHONE_GRACE_MS: u64 = 180_000;

/// Phones drop their Bluetooth link every half minute or so while idle and pick it up again, so
/// a phone has only left once it has stayed away.
#[derive(Default)]
pub struct PhoneWatch {
    leaving: Option<String>,
}

impl PhoneWatch {
    pub fn left(&mut self, name: &str) -> u32 {
        self.leaving = Some(name.to_string());
        PHONE_GRACE_MS as u32
    }

    /// It is back; whether that is news, or it had never really gone.
    pub fn back(&mut self) -> bool {
        self.leaving.take().is_none()
    }

    /// The wait ran out: it is really gone.
    pub fn gone(&mut self) -> Option<String> {
        self.leaving.take()
    }
}

/// Whether notifications can be expected from the phone (through Phone Link).
pub fn phone_connected() -> bool {
    let tick = || unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    let left = PHONE_LEFT.load(Ordering::Relaxed);
    PHONE_ON.load(Ordering::Relaxed) || (left != 0 && tick() - left < PHONE_GRACE_MS)
}

/// A connection change, boxed into the LPARAM of WM_BLUETOOTH.
pub struct Change {
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub kind: DeviceKind,
}

/// Category of the device ("Audio.Headphone", "Input.Mouse", ...), the value Settings uses
/// for its icon. The container object itself does not expose it; the device nodes in the
/// container carry it as DEVPKEY_DeviceContainer_Category.
fn kind_of(container: windows::core::GUID) -> DeviceKind {
    const CATEGORY: &str = "{78C34FC8-104A-4ACA-9EA4-524D52996E57} 90";
    let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(CATEGORY)]);
    let Ok(list) = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(&aqs, &props, DeviceInformationKind::Device).and_then(|op| op.join()) else {
        return DeviceKind::Other;
    };
    for d in list {
        let Some(v) = d.Properties().ok().and_then(|p| p.Lookup(&HSTRING::from(CATEGORY)).ok()) else { continue };
        let Ok(pv) = v.cast::<windows::Foundation::IPropertyValue>() else { continue };
        let mut cats = windows::core::Array::<HSTRING>::new();
        if pv.GetStringArray(&mut cats).is_ok() {
            if let Some(k) = cats.iter().find_map(|c| DeviceKind::from_category(&c.to_string())) {
                return k;
            }
        }
    }
    DeviceKind::Other
}

struct Device {
    name: String,
    connected: bool,
    container: Option<windows::core::GUID>,
}

fn container(props: windows_collections::IMapView<HSTRING, windows::core::IInspectable>) -> Option<windows::core::GUID> {
    let v = props.Lookup(&HSTRING::from(CONTAINER)).ok()?;
    v.cast::<IReference<windows::core::GUID>>().ok()?.Value().ok()
}

/// Battery percentage of the device in `container`; it can appear a moment after connecting.
fn battery(container: windows::core::GUID) -> Option<u8> {
    let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(BATTERY)]);
    for _ in 0..4 {
        if let Ok(list) = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(&aqs, &props, DeviceInformationKind::Device).and_then(|op| op.join()) {
            for d in list {
                let v = d.Properties().ok().and_then(|p| p.Lookup(&HSTRING::from(BATTERY)).ok());
                if let Some(b) = v.and_then(|v| v.cast::<IReference<u8>>().ok()).and_then(|r| r.Value().ok()) {
                    return Some(b.min(100));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(350));
    }
    None
}

fn connected(props: windows_collections::IMapView<HSTRING, windows::core::IInspectable>) -> Option<bool> {
    let v = props.Lookup(&HSTRING::from(CONNECTED)).ok()?;
    v.cast::<IReference<bool>>().ok()?.Value().ok()
}

pub fn spawn(hwnd: HWND) {
    let target = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        crate::sys::name_thread("islet-bluetooth");
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
        let Ok(watcher) = watch(target) else { return };
        // The watcher lives as long as the process; this thread only keeps it referenced.
        loop {
            std::thread::park();
            let _ = &watcher;
        }
    });
}

fn watch(target: isize) -> windows::core::Result<DeviceWatcher> {
    let props = IIterable::<HSTRING>::from(vec![HSTRING::from(CONNECTED), HSTRING::from(CONTAINER)]);
    let w = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(&HSTRING::from(SELECTOR), &props, DeviceInformationKind::AssociationEndpoint)?;
    let devices: Arc<Mutex<HashMap<String, Device>>> = Arc::default();
    // Devices already connected at startup are recorded silently.
    let ready = Arc::new(AtomicBool::new(false));
    let kinds: Arc<Mutex<HashMap<windows::core::GUID, DeviceKind>>> = Arc::default();
    let cached = {
        let kinds = kinds.clone();
        move |c: windows::core::GUID| {
            if let Some(k) = kinds.lock().unwrap().get(&c) {
                return *k;
            }
            let k = kind_of(c);
            kinds.lock().unwrap().insert(c, k);
            k
        }
    };
    let (d, kind_for) = (devices.clone(), cached.clone());
    let check_phone = move || {
        let on = d.lock().unwrap().values().filter(|v| v.connected).filter_map(|v| v.container).collect::<Vec<_>>();
        let on = on.into_iter().any(|c| kind_for(c) == DeviceKind::Phone);
        if PHONE_ON.swap(on, Ordering::Relaxed) && !on {
            PHONE_LEFT.store(unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }, Ordering::Relaxed);
        }
    };
    let post = move |name: String, connected: bool, container: Option<windows::core::GUID>| {
        let battery = if connected { container.and_then(battery) } else { None };
        let kind = container.map(&cached).unwrap_or_default();
        let change = Box::into_raw(Box::new(Change { name, connected, battery, kind }));
        let hwnd = HWND(target as *mut _);
        if unsafe { PostMessageW(Some(hwnd), WM_BLUETOOTH, WPARAM(0), LPARAM(change as isize)) }.is_err() {
            drop(unsafe { Box::from_raw(change) });
        }
    };

    let post_updated = post.clone();
    let phone_updated = check_phone.clone();
    let phone_added = check_phone.clone();
    let (d, r) = (devices.clone(), ready.clone());
    w.Added(&TypedEventHandler::new(move |_: Ref<DeviceWatcher>, info: Ref<DeviceInformation>| {
        if let Some(info) = info.as_ref() {
            let on = info.Properties().ok().and_then(connected).unwrap_or(false);
            let cont = info.Properties().ok().and_then(container);
            let name = info.Name()?.to_string();
            if on && r.load(Ordering::Acquire) {
                post(name.clone(), true, cont);
            }
            d.lock().unwrap().insert(info.Id()?.to_string(), Device { name, connected: on, container: cont });
            if on {
                phone_added();
            }
        }
        Ok(())
    }))?;
    let d = devices.clone();
    let r = ready.clone();
    w.Updated(&TypedEventHandler::new(move |_: Ref<DeviceWatcher>, upd: Ref<DeviceInformationUpdate>| {
        if let Some(upd) = upd.as_ref() {
            let Some(on) = upd.Properties().ok().and_then(connected) else { return Ok(()) };
            let changed = {
                let mut map = d.lock().unwrap();
                map.get_mut(&upd.Id()?.to_string()).filter(|dev| dev.connected != on).map(|dev| {
                    dev.connected = on;
                    (dev.name.clone(), dev.container)
                })
            };
            // Posted outside the lock: the battery lookup may wait for the device to report.
            let moved = changed.is_some();
            if let (Some((name, cont)), true) = (changed, r.load(Ordering::Acquire)) {
                post_updated(name, on, cont);
            }
            if moved {
                phone_updated();
            }
        }
        Ok(())
    }))?;
    let d = devices.clone();
    w.Removed(&TypedEventHandler::new(move |_: Ref<DeviceWatcher>, upd: Ref<DeviceInformationUpdate>| {
        if let Some(upd) = upd.as_ref() {
            d.lock().unwrap().remove(&upd.Id()?.to_string());
        }
        Ok(())
    }))?;
    let r = ready.clone();
    w.EnumerationCompleted(&TypedEventHandler::new(move |_: Ref<DeviceWatcher>, _: Ref<windows::core::IInspectable>| {
        r.store(true, Ordering::Release);
        check_phone();
        Ok(())
    }))?;
    w.Start()?;
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::PhoneWatch;

    #[test]
    fn a_phone_flapping_every_half_minute_says_nothing() {
        let mut w = PhoneWatch::default();
        assert!(w.back(), "the first connection is news");
        for _ in 0..20 {
            w.left("ip");
            assert!(!w.back());
        }
        assert_eq!(w.gone(), None);
    }

    #[test]
    fn a_phone_that_stays_away_is_reported_once_and_so_is_its_return() {
        let mut w = PhoneWatch::default();
        w.back();
        w.left("ip");
        assert_eq!(w.gone().as_deref(), Some("ip"));
        assert_eq!(w.gone(), None);
        assert!(w.back());
    }

    /// Reads a real device's kind: `ISLET_TEST_CONTAINER={guid} cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn kind_of_real_container() {
        let Ok(g) = std::env::var("ISLET_TEST_CONTAINER") else { return };
        let g = windows::core::GUID::try_from(g.trim_matches(|c| c == '{' || c == '}')).unwrap();
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        println!("kind = {:?}", super::kind_of(g));
    }
}
