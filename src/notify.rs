//! Mirrors new Windows toast notifications from other apps onto the notch.
//! Uses UserNotificationListener; if access is not granted the thread simply exits.

use std::collections::HashSet;
use std::time::Duration;

use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::WM_NOTICE;

/// A new notification, boxed into the LPARAM of WM_NOTICE.
pub struct Notice {
    pub app: String,
    /// First text line (usually the sender or title).
    pub title: String,
    /// Remaining text (the message), possibly empty.
    pub body: String,
    /// Sender app's AppUserModelID (used to turn off its Windows banner).
    pub aumid: String,
    /// A recognised web source such as "facebook" (browser notifications carry the site as a line).
    pub source: Option<&'static str>,
    /// The site line of a browser notification, e.g. "www.youtube.com".
    pub site: Option<String>,
    /// Windows notification id, to remove it from the Action Center when dismissed here.
    pub id: u32,
}

fn describe(n: &UserNotification) -> Option<Notice> {
    // Unpackaged senders (browsers showing Facebook, Win32 apps) may not expose AppInfo.
    let info = n.AppInfo().ok();
    let aumid = info.as_ref().and_then(|i| i.AppUserModelId().ok()).map(|h| h.to_string()).unwrap_or_default();
    let app = info
        .as_ref()
        .and_then(|i| i.DisplayInfo().ok())
        .and_then(|d| d.DisplayName().ok())
        .map(|h| h.to_string())
        .unwrap_or_else(|| "Notification".into());
    let visual = n.Notification().ok()?.Visual().ok()?;
    let binding = visual
        .GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?)
        .ok()
        .or_else(|| visual.Bindings().ok().and_then(|b| b.GetAt(0).ok()))?;
    let mut lines: Vec<String> = binding.GetTextElements().ok()?.into_iter().filter_map(|t| t.Text().ok()).map(|t| t.to_string()).filter(|t| !t.trim().is_empty()).collect();
    // Browsers add the site (e.g. "www.facebook.com") as an attribution line: use it, don't show it.
    let site = |l: &str| {
        let l = l.trim().to_lowercase();
        !l.contains(' ') && (l.ends_with(".com") || l.ends_with(".net") || l.contains("://"))
    };
    let mut source = None;
    let mut site_line = None;
    if let Some(i) = lines.iter().position(|l| site(l)) {
        let line = lines.remove(i);
        let l = line.to_lowercase();
        if l.contains("facebook") || l.contains("messenger") {
            source = Some("facebook");
        }
        site_line = Some(line.trim().to_string());
    }
    let (title, body) = match lines.as_slice() {
        [] => return None,
        [one] => (one.clone(), String::new()),
        [title, rest @ ..] => (title.clone(), rest.join(" ")),
    };
    let id = n.Id().unwrap_or(0);
    Some(Notice { app, title, body, aumid, source, site: site_line, id })
}

/// Removes a notification from the Action Center (it was read on the notch).
pub fn remove(id: u32) {
    if id == 0 {
        return;
    }
    std::thread::spawn(move || unsafe {
        crate::sys::name_thread("islet-notify-remove");
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
        if let Ok(l) = UserNotificationListener::Current() {
            let _ = l.RemoveNotification(id);
        }
    });
}

/// Windows writes every notification into its database under this folder, so a change there is
/// the signal to read the list again (the listener's own change event needs package identity).
/// Reading the list on a timer instead kept a shell worker thread busy (~1 % CPU at 500 ms).
fn watch_store() -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::Storage::FileSystem::*;
    let dir = std::env::var_os("LOCALAPPDATA")?;
    let dir = std::path::Path::new(&dir).join("Microsoft\\Windows\\Notifications");
    let h = unsafe { FindFirstChangeNotificationW(&windows::core::HSTRING::from(dir.as_os_str()), false, FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE) }.ok()?;
    (!h.is_invalid()).then_some(h)
}

/// Blocks until the notification store changes (a slow timer covers missed signals). Returns
/// whether it changed. The store can be written a moment before the listener lists the new
/// notification, so after a change the caller reads once more shortly after.
fn wait_for_change(h: &mut Option<windows::Win32::Foundation::HANDLE>) -> bool {
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::Storage::FileSystem::FindNextChangeNotification;
    use windows::Win32::System::Threading::WaitForSingleObject;
    match *h {
        Some(handle) => unsafe {
            let changed = WaitForSingleObject(handle, 15_000) == WAIT_OBJECT_0;
            if FindNextChangeNotification(handle).is_err() {
                *h = None;
            }
            // A notification is written in several steps; read once they settle.
            std::thread::sleep(Duration::from_millis(60));
            changed
        },
        None => {
            std::thread::sleep(Duration::from_millis(1500));
            false
        }
    }
}

/// Set by the UI thread from the "Hide Windows banners" setting.
pub static HIDE_BANNERS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

fn aumid_of(n: &UserNotification) -> String {
    n.AppInfo().and_then(|i| i.AppUserModelId()).map(|h| h.to_string()).unwrap_or_default()
}

pub fn spawn(hwnd: HWND) {
    let target = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
        crate::sys::name_thread("islet-notify");
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
        let Ok(listener) = UserNotificationListener::Current() else { return };
        let mut status = listener.GetAccessStatus().unwrap_or(UserNotificationListenerAccessStatus::Denied);
        if status == UserNotificationListenerAccessStatus::Unspecified {
            status = listener.RequestAccessAsync().and_then(|op| op.join()).unwrap_or(UserNotificationListenerAccessStatus::Denied);
        }
        if status != UserNotificationListenerAccessStatus::Allowed {
            return;
        }
        let mut seen: Option<HashSet<u32>> = None;
        let mut changes = watch_store();
        let mut recheck = false;
        loop {
            if let Ok(list) = listener.GetNotificationsAsync(NotificationKinds::Toast).and_then(|op| op.join()) {
                let current: Vec<UserNotification> = list.into_iter().collect();
                let ids: HashSet<u32> = current.iter().filter_map(|n| n.Id().ok()).collect();
                let hide = HIDE_BANNERS.load(std::sync::atomic::Ordering::Relaxed);
                if seen.is_none() && hide {
                    // Apps that already notified once get their corner banner turned off up front.
                    for n in &current {
                        crate::sys::hide_banner(&aumid_of(n));
                    }
                }
                // Notifications already in the Action Center at startup are not replayed.
                if let Some(old) = &seen {
                    for n in current.iter().filter(|n| n.Id().is_ok_and(|id| !old.contains(&id))) {
                        let aumid = aumid_of(n);
                        if hide && !crate::sys::banner_hidden(&aumid) {
                            // First notification from this app: its banner is already up. Dismiss it
                            // (Islet shows it) and keep later ones from popping a banner at all.
                            if let Ok(id) = n.Id() {
                                let _ = listener.RemoveNotification(id);
                            }
                        }
                        if let Some(notice) = describe(n) {
                            let b = Box::into_raw(Box::new(notice));
                            if PostMessageW(Some(HWND(target as *mut _)), WM_NOTICE, WPARAM(0), LPARAM(b as isize)).is_err() {
                                drop(Box::from_raw(b));
                            }
                        }
                    }
                }
                seen = Some(ids);
            }
            if recheck {
                recheck = false;
                std::thread::sleep(Duration::from_millis(300));
            } else {
                recheck = wait_for_change(&mut changes);
            }
        }
    });
}
