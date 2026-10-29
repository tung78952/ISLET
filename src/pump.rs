//! Vsync-paced frame pump: while an animation runs, posts one WM_FRAME per
//! compositor frame (DwmFlush) instead of relying on coarse, jittery WM_TIMER.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::thread::Thread;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::WM_FRAME;

static RUNNING: AtomicBool = AtomicBool::new(false);
static PENDING: AtomicBool = AtomicBool::new(false);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static WORKER: OnceLock<Thread> = OnceLock::new();

pub fn start(hwnd: HWND) {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    let handle = std::thread::spawn(|| loop {
        if !RUNNING.load(Ordering::Acquire) {
            std::thread::park();
            continue;
        }
        if unsafe { DwmFlush() }.is_err() {
            std::thread::sleep(Duration::from_millis(8));
        }
        // Coalesce: never queue a second frame before the UI thread consumed the first.
        if RUNNING.load(Ordering::Acquire) && !PENDING.swap(true, Ordering::AcqRel) {
            let hwnd = HWND(TARGET.load(Ordering::Relaxed) as *mut _);
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_FRAME, WPARAM(0), LPARAM(0));
            }
        }
    });
    let _ = WORKER.set(handle.thread().clone());
}

pub fn resume() {
    RUNNING.store(true, Ordering::Release);
    if let Some(t) = WORKER.get() {
        t.unpark();
    }
}

pub fn pause() {
    RUNNING.store(false, Ordering::Release);
}

/// Called by the UI thread when it handles WM_FRAME.
pub fn consumed() {
    PENDING.store(false, Ordering::Release);
}
