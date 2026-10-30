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
    /// For a phone notification (Phone Link): the app on the phone.
    pub phone_app: Option<String>,
    /// An incoming call rather than a message.
    pub call: bool,
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
    let (phone_app, title, body) = split_phone(&aumid, &lines, title, body);
    let call = is_call(&title, &body);
    Some(Notice { app, title, body, aumid, source, site: site_line, id, phone_app, call })
}

/// Call notifications have no marker the listener can read; their wording tells them apart
/// (first guess, to be checked against a real Zalo and Messenger call).
fn is_call(title: &str, body: &str) -> bool {
    let t = format!("{title} {body}").to_lowercase();
    ["cuộc gọi đến", "đang gọi", "incoming call", "is calling", "incoming video", "incoming audio", "cuộc gọi video đến", "cuộc gọi thoại đến"].iter().any(|k| t.contains(k))
}

/// Phone Link's toast lines are: app on the phone, sender, content.
fn split_phone(aumid: &str, lines: &[String], title: String, body: String) -> (Option<String>, String, String) {
    if !aumid.starts_with("Microsoft.YourPhone") || lines.len() < 2 {
        return (None, title, body);
    }
    (Some(lines[0].clone()), lines[1].clone(), lines[2..].join(" "))
}

/// The `launch` link of a notification, from a private copy of Windows' notification store (the
/// listener doesn't expose it). The copy holds notification text, so it is deleted at once.
fn launch_link(id: u32) -> Option<String> {
    let src = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft\\Windows\\Notifications");
    let dir = std::env::temp_dir().join(format!("islet-wpn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    for name in ["wpndatabase.db", "wpndatabase.db-wal", "wpndatabase.db-shm"] {
        let _ = std::fs::copy(src.join(name), dir.join(name));
    }
    let read = || -> Option<Vec<u8>> {
        let db = rusqlite::Connection::open(dir.join("wpndatabase.db")).ok()?;
        db.query_row("SELECT Payload FROM Notification WHERE Id = ?1", [id], |r| r.get(0)).ok()
    };
    let payload = read();
    let _ = std::fs::remove_dir_all(&dir);
    launch_of(&String::from_utf8_lossy(&payload?))
}

fn launch_of(payload: &str) -> Option<String> {
    let rest = &payload[payload.find("launch=\"")? + 8..];
    let link = rest[..rest.find('"')?].replace("&amp;", "&");
    link.starts_with("ms-phone:").then_some(link)
}

/// Opens a phone notification in Phone Link: the exact one when its link can be read.
pub fn open_phone(id: u32) {
    std::thread::spawn(move || {
        crate::sys::name_thread("islet-open-phone");
        let target = launch_link(id).unwrap_or_else(|| "shell:AppsFolder\\Microsoft.YourPhone_8wekyb3d8bbwe!App".to_string());
        crate::sys::shell_open(std::ffi::OsStr::new(&target));
    });
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

/// Windows keeps notifications in a SQLite database. Its write-ahead index starts with a change
/// counter, bumped on every commit, and reading those bytes costs next to nothing, unlike asking
/// the listener for the list (which kept a shell worker busy, about 1 % CPU at 500 ms). The
/// listener's own change event needs package identity, and folder-change notifications don't
/// fire for this file: Windows keeps it open, so a notification waited for the safety read.
fn open_store() -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = std::env::var_os("LOCALAPPDATA")?;
    let path = std::path::Path::new(&dir).join(r"Microsoft\Windows\Notifications\wpndatabase.db-shm");
    std::fs::OpenOptions::new().read(true).share_mode(7).open(path).ok()
}

/// Waits until the store's counter moves (15 s at most, in case it never does). Returns whether
/// it moved.
fn wait_for_change(store: &mut Option<std::fs::File>, last: &mut [u8; 4]) -> bool {
    use std::os::windows::fs::FileExt;
    let Some(f) = store else {
        std::thread::sleep(Duration::from_millis(1500));
        *store = open_store();
        return false;
    };
    for _ in 0..75 {
        std::thread::sleep(Duration::from_millis(200));
        let mut now = [0u8; 4];
        match f.seek_read(&mut now, 8) {
            Ok(4) if now != *last => {
                *last = now;
                return true;
            }
            Ok(4) => {}
            _ => {
                *store = None;
                return false;
            }
        }
    }
    false
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
        let mut store = open_store();
        let mut counter = [0u8; 4];
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
                        crate::sys::timelog("read");
                        if let Some(notice) = describe(n) {
                            let b = Box::into_raw(Box::new(notice));
                            if PostMessageW(Some(HWND(target as *mut _)), WM_NOTICE, WPARAM(0), LPARAM(b as isize)).is_err() {
                                drop(Box::from_raw(b));
                            }
                        }
                    }
                }
                // Gone since the last look (a call that was answered, for one).
                if let Some(old) = &seen {
                    for id in old.difference(&ids) {
                        let _ = PostMessageW(Some(HWND(target as *mut _)), crate::WM_NOTICEGONE, WPARAM(*id as usize), LPARAM(0));
                    }
                }
                seen = Some(ids);
            }
            if recheck {
                recheck = false;
                std::thread::sleep(Duration::from_millis(300));
            } else {
                recheck = wait_for_change(&mut store, &mut counter);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_lines_are_app_sender_content() {
        let l: Vec<String> = ["Messenger", "Denis", "Hello", "there"].iter().map(|s| s.to_string()).collect();
        let (app, title, body) = split_phone("Microsoft.YourPhone_8wekyb3d8bbwe!App", &l, l[0].clone(), String::new());
        assert_eq!((app.as_deref(), title.as_str(), body.as_str()), (Some("Messenger"), "Denis", "Hello there"));
        let (app, title, _) = split_phone("Chrome", &l, "x".into(), String::new());
        assert_eq!((app, title.as_str()), (None, "x"));
    }

    #[test]
    fn calls_are_told_from_messages() {
        assert!(is_call("Minh Anh", "Cuộc gọi đến"));
        assert!(is_call("Denis is calling you", ""));
        assert!(!is_call("Minh Anh", "Mai gọi lại nhé"));
    }

    #[test]
    fn launch_link_is_read_and_unescaped() {
        let p = r#"<toast launch="ms-phone:phonenotifications?deviceid=a&amp;notificationId=b" hint-toastId="c">"#;
        assert_eq!(launch_of(p).as_deref(), Some("ms-phone:phonenotifications?deviceid=a&notificationId=b"));
        assert_eq!(launch_of(r#"<toast launch="0|2|Chrome">"#), None);
    }
}
