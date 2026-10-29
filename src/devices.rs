//! What kind of device connected, and plug-in cards for wired devices: USB mice, keyboards and
//! receivers, phones on a cable, drives, and headphones or USB audio. Everything here reacts to
//! system notifications (WM_DEVICECHANGE, audio endpoint state); nothing polls.

use std::collections::HashMap;

use windows::core::{GUID, HSTRING, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::*;
use windows::Win32::Devices::Properties::*;
use windows::Win32::Foundation::{DEVPROPKEY, HANDLE, HWND};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The kind of a device, from its container category (Bluetooth and USB alike), its Bluetooth
/// class of device, or the interface it arrived on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeviceKind {
    #[default]
    Other,
    Headphones,
    /// Headphones with a microphone.
    Headset,
    Speaker,
    Mouse,
    Keyboard,
    Gamepad,
    Phone,
    Drive,
}

impl DeviceKind {
    /// Windows' container category, e.g. "Audio.Headphone", "Communication.Headset.Bluetooth",
    /// "Input.Mouse".
    pub fn from_category(c: &str) -> Option<Self> {
        let c = c.to_ascii_lowercase();
        Some(if c.starts_with("audio.headphone") {
            DeviceKind::Headphones
        } else if c.starts_with("communication.headset") || c.starts_with("audio.headset") {
            DeviceKind::Headset
        } else if c.starts_with("audio.speaker") {
            DeviceKind::Speaker
        } else if c.starts_with("input.mouse") || c.starts_with("input.touchpad") {
            DeviceKind::Mouse
        } else if c.starts_with("input.keyboard") {
            DeviceKind::Keyboard
        } else if c.starts_with("input.gaming") || c.starts_with("input.gamepad") || c.starts_with("input.joystick") {
            DeviceKind::Gamepad
        } else if c.starts_with("communication.phone") || c.starts_with("phone") {
            DeviceKind::Phone
        } else {
            return None;
        })
    }

    /// A classic Bluetooth class of device (major/minor fields of the 24-bit value).
    pub fn from_cod(cod: u32) -> Option<Self> {
        let major = (cod >> 8) & 0x1F;
        let minor = (cod >> 2) & 0x3F;
        Some(match major {
            0x02 => DeviceKind::Phone,
            0x04 => match minor {
                0x01 | 0x02 => DeviceKind::Headset,
                0x05 => DeviceKind::Speaker,
                0x06 => DeviceKind::Headphones,
                _ => return None,
            },
            0x05 => match (minor >> 4, minor & 0x0F) {
                (_, 0x01 | 0x02) => DeviceKind::Gamepad,
                (0b11, _) | (0b01, _) => DeviceKind::Keyboard,
                (0b10, _) => DeviceKind::Mouse,
                _ => return None,
            },
            _ => return None,
        })
    }

    /// When a receiver shows up as several devices at once, the card names the most telling one.
    fn rank(self) -> u8 {
        match self {
            DeviceKind::Mouse => 6,
            DeviceKind::Gamepad => 5,
            DeviceKind::Keyboard => 4,
            DeviceKind::Phone => 3,
            DeviceKind::Drive => 2,
            _ => 1,
        }
    }
}

const GUID_DEVINTERFACE_KEYBOARD: GUID = GUID::from_u128(0x884b96c3_56ef_11d1_bc8c_00a0c91405dd);
const GUID_DEVINTERFACE_MOUSE: GUID = GUID::from_u128(0x378de44c_56ef_11d1_bc8c_00a0c91405dd);
/// Portable devices (phones over MTP, iPhones).
const GUID_DEVINTERFACE_WPD: GUID = GUID::from_u128(0x6ac27878_a6fa_4155_ba85_f98f491d4f33);
const WATCHED: [(GUID, DeviceKind); 3] = [
    (GUID_DEVINTERFACE_MOUSE, DeviceKind::Mouse),
    (GUID_DEVINTERFACE_KEYBOARD, DeviceKind::Keyboard),
    (GUID_DEVINTERFACE_WPD, DeviceKind::Phone),
];

/// A plugged-in device as resolved on arrival (removal no longer has a device node to ask).
#[derive(Clone, Debug)]
pub struct Plugged {
    pub kind: DeviceKind,
    pub name: String,
    pub container: GUID,
}

/// One card to show: several interfaces of one device arriving together collapse into it.
#[derive(Clone, Debug)]
pub struct Card {
    pub connected: bool,
    pub kind: DeviceKind,
    pub name: String,
    /// Drive root ("E:\") for drives, so the card can open it.
    pub root: Option<String>,
    pub detail: String,
}

/// Interface paths of the watched classes present now, with what they resolve to; arrivals
/// are added and removals looked up here.
#[derive(Default)]
pub struct Tracker {
    known: HashMap<String, Plugged>,
    pending: Vec<(bool, Plugged)>,
    /// The computer's own container: built-in keyboards and touchpads belong to it and are never
    /// announced (they reappear after sleep).
    own: Option<GUID>,
}

impl Tracker {
    /// Registers for arrivals on `hwnd` and records what is already present (silently).
    pub fn start(hwnd: HWND) -> Self {
        let mut t = Tracker { own: own_container(), ..Default::default() };
        for (guid, kind) in WATCHED {
            unsafe {
                let filter = DEV_BROADCAST_DEVICEINTERFACE_W {
                    dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
                    dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
                    dbcc_classguid: guid,
                    ..Default::default()
                };
                // Registered for the life of the window.
                let _ = RegisterDeviceNotificationW(HANDLE(hwnd.0), &filter as *const _ as _, DEVICE_NOTIFY_WINDOW_HANDLE);
            }
            for path in interfaces(&guid) {
                if let Some(p) = resolve(&path, kind) {
                    t.known.insert(path.to_ascii_lowercase(), p);
                }
            }
        }
        t
    }

    /// Handles WM_DEVICECHANGE. Returns true when cards are pending (flush after a short pause
    /// so a receiver's several interfaces become one card).
    pub fn on_change(&mut self, wp: usize, lp: isize) -> Option<Card> {
        let arrived = wp as u32 == DBT_DEVICEARRIVAL;
        if !arrived && wp as u32 != DBT_DEVICEREMOVECOMPLETE || lp == 0 {
            return None;
        }
        let hdr = unsafe { &*(lp as *const DEV_BROADCAST_HDR) };
        if hdr.dbch_devicetype == DBT_DEVTYP_VOLUME {
            let v = unsafe { &*(lp as *const DEV_BROADCAST_VOLUME) };
            // Media inserted into a reader and network shares are not devices being plugged in.
            if v.dbcv_flags.0 & (DBTF_MEDIA.0 | DBTF_NET.0) != 0 {
                return None;
            }
            let letter = (0..26).find(|i| v.dbcv_unitmask & (1 << i) != 0)?;
            return Some(drive_card(char::from(b'A' + letter as u8), arrived));
        }
        if hdr.dbch_devicetype != DBT_DEVTYP_DEVICEINTERFACE {
            return None;
        }
        let di = unsafe { &*(lp as *const DEV_BROADCAST_DEVICEINTERFACE_W) };
        let path = unsafe { PCWSTR(di.dbcc_name.as_ptr()).to_string() }.ok()?;
        let kind = WATCHED.iter().find(|(g, _)| *g == di.dbcc_classguid)?.1;
        let key = path.to_ascii_lowercase();
        let p = if arrived {
            let p = resolve(&path, kind)?;
            self.known.insert(key, p.clone());
            p
        } else {
            self.known.remove(&key)?
        };
        if Some(p.container) == self.own {
            return None;
        }
        self.pending.push((arrived, p));
        None
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The cards for everything that arrived or left since the last flush, one per device.
    pub fn flush(&mut self) -> Vec<Card> {
        let mut cards: Vec<(GUID, Card)> = Vec::new();
        for (connected, p) in self.pending.drain(..) {
            match cards.iter_mut().find(|(c, card)| *c == p.container && card.connected == connected) {
                Some((_, card)) => {
                    if p.kind.rank() > card.kind.rank() {
                        card.kind = p.kind;
                    }
                }
                None => cards.push((
                    p.container,
                    Card { connected, kind: p.kind, name: p.name, root: None, detail: String::new() },
                )),
            }
        }
        cards.into_iter().map(|(_, c)| c).collect()
    }
}

fn drive_card(letter: char, connected: bool) -> Card {
    let root = format!("{letter}:\\");
    let (mut name, mut detail) = (format!("USB drive ({letter}:)"), String::new());
    if connected {
        unsafe {
            let mut label = [0u16; 64];
            if windows::Win32::Storage::FileSystem::GetVolumeInformationW(&HSTRING::from(&root), Some(&mut label), None, None, None, None).is_ok() {
                let l = String::from_utf16_lossy(&label[..label.iter().position(|&c| c == 0).unwrap_or(0)]);
                if !l.is_empty() {
                    name = format!("{l} ({letter}:)");
                }
            }
            let (mut free, mut total) = (0u64, 0u64);
            if windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&HSTRING::from(&root), Some(&mut free), Some(&mut total), None).is_ok() && total > 0 {
                detail = format!("{} free of {}", crate::size_label(free), crate::size_label(total));
            }
        }
    }
    Card { connected, kind: DeviceKind::Drive, name, root: connected.then_some(root), detail }
}

/// Present interface paths of one class.
fn interfaces(guid: &GUID) -> Vec<String> {
    unsafe {
        let mut len = 0u32;
        if CM_Get_Device_Interface_List_SizeW(&mut len, guid, PCWSTR::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT) != CR_SUCCESS || len <= 1 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_Interface_ListW(guid, PCWSTR::null(), &mut buf, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) != CR_SUCCESS {
            return Vec::new();
        }
        buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

/// Name and container of the device behind an interface; None for Bluetooth devices (their own
/// card covers them) and anything that cannot be resolved.
fn resolve(path: &str, kind: DeviceKind) -> Option<Plugged> {
    unsafe {
        let id = interface_string(path, &DEVPKEY_Device_InstanceId)?;
        let mut node = 0u32;
        if CM_Locate_DevNodeW(&mut node, &HSTRING::from(&id), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
            return None;
        }
        let container = node_guid(node, &DEVPKEY_Device_ContainerId)?;
        // Walk up to the bus device: its bus-reported name ("USB Receiver") beats the generic
        // "HID-compliant mouse" of the interface's own node.
        let mut name = None;
        let mut n = node;
        for _ in 0..6 {
            let nid = node_string(n, &DEVPKEY_Device_InstanceId).unwrap_or_default().to_ascii_uppercase();
            if nid.starts_with("BTH") || nid.contains("{00001812-") || nid.contains("{00001124-") {
                return None;
            }
            if name.is_none() {
                name = node_string(n, &DEVPKEY_Device_BusReportedDeviceDesc).filter(|s| !s.trim().is_empty());
            }
            let mut parent = 0u32;
            if CM_Get_Parent(&mut parent, n, 0) != CR_SUCCESS {
                break;
            }
            n = parent;
        }
        let name = name
            .or_else(|| node_string(node, &DEVPKEY_Device_FriendlyName))
            .or_else(|| node_string(node, &DEVPKEY_Device_DeviceDesc))
            .unwrap_or_else(|| "USB device".into());
        Some(Plugged { kind, name: name.trim().to_string(), container })
    }
}

/// The container id of the computer itself (the root device node's).
fn own_container() -> Option<GUID> {
    unsafe {
        let mut root = 0u32;
        if CM_Locate_DevNodeW(&mut root, PCWSTR::null(), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
            return None;
        }
        node_guid(root, &DEVPKEY_Device_ContainerId)
    }
}

unsafe fn interface_string(path: &str, key: &DEVPROPKEY) -> Option<String> {
    let mut ty = DEVPROPTYPE::default();
    let mut buf = [0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let r = CM_Get_Device_Interface_PropertyW(&HSTRING::from(path), key, &mut ty, Some(buf.as_mut_ptr() as *mut u8), &mut size, 0);
    (r == CR_SUCCESS && ty == DEVPROP_TYPE_STRING).then(|| wide_str(&buf, size))
}

unsafe fn node_string(node: u32, key: &DEVPROPKEY) -> Option<String> {
    let mut ty = DEVPROPTYPE::default();
    let mut buf = [0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let r = CM_Get_DevNode_PropertyW(node, key, &mut ty, Some(buf.as_mut_ptr() as *mut u8), &mut size, 0);
    (r == CR_SUCCESS && ty == DEVPROP_TYPE_STRING).then(|| wide_str(&buf, size))
}

unsafe fn node_guid(node: u32, key: &DEVPROPKEY) -> Option<GUID> {
    let mut ty = DEVPROPTYPE::default();
    let mut g = GUID::zeroed();
    let mut size = std::mem::size_of::<GUID>() as u32;
    let r = CM_Get_DevNode_PropertyW(node, key, &mut ty, Some(&mut g as *mut GUID as *mut u8), &mut size, 0);
    (r == CR_SUCCESS && ty == DEVPROP_TYPE_GUID).then_some(g)
}

fn wide_str(buf: &[u16], bytes: u32) -> String {
    let n = (bytes as usize / 2).min(buf.len());
    let s = &buf[..n];
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

#[cfg(test)]
mod tests {
    use super::DeviceKind;

    /// The categories and classes read from the paired devices on the development machine.
    #[test]
    fn kinds_of_paired_devices() {
        let cases = [
            ("Audio.Headphone", DeviceKind::Headphones),
            ("Communication.Headset.Bluetooth", DeviceKind::Headset),
            ("Audio.Speakers", DeviceKind::Speaker),
            ("Input.Mouse", DeviceKind::Mouse),
            ("Input.Keyboard", DeviceKind::Keyboard),
        ];
        for (cat, kind) in cases {
            assert_eq!(DeviceKind::from_category(cat), Some(kind), "{cat}");
        }
        assert_eq!(DeviceKind::from_category("Display.Monitor"), None);
        // M10 / AirPods, Philips / Edifier, HK Aura Studio 3.
        assert_eq!(DeviceKind::from_cod(0x240418), Some(DeviceKind::Headphones));
        assert_eq!(DeviceKind::from_cod(0x240404), Some(DeviceKind::Headset));
        assert_eq!(DeviceKind::from_cod(0x240414), Some(DeviceKind::Speaker));
        // Smartphone, keyboard, mouse, gamepad.
        assert_eq!(DeviceKind::from_cod(0x5a020c), Some(DeviceKind::Phone));
        assert_eq!(DeviceKind::from_cod(0x002540), Some(DeviceKind::Keyboard));
        assert_eq!(DeviceKind::from_cod(0x002580), Some(DeviceKind::Mouse));
        assert_eq!(DeviceKind::from_cod(0x002508), Some(DeviceKind::Gamepad));
    }

    /// Lists what the present mice, keyboards and phones resolve to: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn present_devices() {
        let own = super::own_container();
        for (guid, kind) in super::WATCHED {
            for path in super::interfaces(&guid) {
                let p = super::resolve(&path, kind);
                let note = match &p {
                    Some(p) if Some(p.container) == own => "built in, never announced",
                    Some(_) => "announced",
                    None => "Bluetooth or unresolved, skipped",
                };
                println!("{kind:?}: {:?} ({note})", p.map(|p| p.name));
            }
        }
    }
}
