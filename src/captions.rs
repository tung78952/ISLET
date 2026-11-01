//! Words for what has none: Windows' own Live Captions listens to the PC's sound on this machine;
//! it runs out of sight and its text is read through UI Automation. Only while the user wants it
//! and something without lyrics is playing, and only the copy started here is ever closed.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{w, BSTR, VARIANT};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants, UIA_AutomationIdPropertyId};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsWindowVisible, ShowWindow, SW_HIDE};

const POLL: Duration = Duration::from_millis(300);
/// It takes seconds to start, so a short pause between clips does not close it.
const LINGER: Duration = Duration::from_secs(30);
/// About what fits the lyric bar.
const WIDE: usize = 42;

#[derive(Default)]
pub struct Captions {
    wanted: Arc<AtomicBool>,
    text: Arc<Mutex<String>>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Captions {
    pub fn spawn(&self) {
        let (wanted, text, child) = (self.wanted.clone(), self.text.clone(), self.child.clone());
        std::thread::spawn(move || unsafe {
            crate::sys::name_thread("islet-captions");
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL) else { return };
            let mut block: Option<IUIAutomationElement> = None;
            let mut last_wanted = Instant::now();
            loop {
                std::thread::sleep(POLL);
                if !wanted.load(Ordering::Relaxed) {
                    text.lock().unwrap().clear();
                    if last_wanted.elapsed() > LINGER {
                        close(&child);
                        block = None;
                    }
                    continue;
                }
                last_wanted = Instant::now();
                let window = FindWindowW(w!("LiveCaptionsDesktopWindow"), None).ok();
                if window.is_none() {
                    block = None;
                    let mut c = child.lock().unwrap();
                    if c.is_none() {
                        *c = Command::new("LiveCaptions.exe").spawn().ok();
                    }
                    continue;
                }
                let window = window.unwrap_or(HWND::default());
                // The user's own window stays as they left it.
                if child.lock().unwrap().is_some() && IsWindowVisible(window).as_bool() {
                    let _ = ShowWindow(window, SW_HIDE);
                }
                if block.is_none() {
                    block = find_block(&uia, window);
                }
                let read = block.as_ref().and_then(|b| b.CurrentName().ok());
                match read {
                    Some(name) => *text.lock().unwrap() = name.to_string(),
                    None => block = None,
                }
            }
        });
    }

    pub fn want(&self, on: bool) {
        self.wanted.store(on, Ordering::Relaxed);
    }

    /// The end of what is being said, as much as fits.
    pub fn line(&self) -> Option<String> {
        let text = self.text.lock().unwrap();
        tail(&text, WIDE)
    }

    pub fn stop(&self) {
        self.wanted.store(false, Ordering::Relaxed);
        close(&self.child);
    }
}

fn close(child: &Mutex<Option<Child>>) {
    if let Some(mut c) = child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

unsafe fn find_block(uia: &IUIAutomation, window: HWND) -> Option<IUIAutomationElement> {
    let root = uia.ElementFromHandle(window).ok()?;
    let id = VARIANT::from(BSTR::from("CaptionsTextBlock"));
    let only = uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &id).ok()?;
    root.FindFirst(TreeScope_Descendants, &only).ok()
}

/// The last line of `text` cut to `wide` characters at a word.
pub fn tail(text: &str, wide: usize) -> Option<String> {
    let line = text.lines().rev().find(|l| !l.trim().is_empty())?.trim();
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= wide {
        return Some(line.to_string());
    }
    let cut: String = chars[chars.len() - wide..].iter().collect();
    Some(cut.split_once(' ').map_or(cut.clone(), |(_, rest)| rest.to_string()))
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn the_end_of_what_is_said_is_kept_at_a_word() {
        assert_eq!(tail("", 10), None);
        assert_eq!(tail("first\nsecond line\n", 20).as_deref(), Some("second line"));
        assert_eq!(tail("one two three four", 10).as_deref(), Some("four"));
        assert_eq!(tail("xin chào các bạn", 8).as_deref(), Some("các bạn"));
    }
}
