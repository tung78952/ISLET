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
    pub text: String,
}

fn describe(n: &UserNotification) -> Option<Notice> {
    let app = n.AppInfo().ok()?.DisplayInfo().ok()?.DisplayName().ok()?.to_string();
    let binding = n.Notification().ok()?.Visual().ok()?.GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?).ok()?;
    let lines: Vec<String> = binding.GetTextElements().ok()?.into_iter().filter_map(|t| t.Text().ok()).map(|t| t.to_string()).filter(|t| !t.is_empty()).collect();
    let text = match lines.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [title, body, ..] => format!("{title}: {body}"),
    };
    Some(Notice { app, text })
}

pub fn spawn(hwnd: HWND) {
    let target = hwnd.0 as isize;
    std::thread::spawn(move || unsafe {
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
        loop {
            if let Ok(list) = listener.GetNotificationsAsync(NotificationKinds::Toast).and_then(|op| op.join()) {
                let current: Vec<UserNotification> = list.into_iter().collect();
                let ids: HashSet<u32> = current.iter().filter_map(|n| n.Id().ok()).collect();
                // Notifications already in the Action Center at startup are not replayed.
                if let Some(old) = &seen {
                    for n in current.iter().filter(|n| n.Id().is_ok_and(|id| !old.contains(&id))) {
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
            std::thread::sleep(Duration::from_millis(1500));
        }
    });
}
