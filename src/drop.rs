//! OLE drag-and-drop: files and links dropped onto the notch, files dragged back out.

use std::path::{Path, PathBuf};

use windows::core::{implement, Ref, Result, HSTRING};
use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL, TYMED_ISTREAM};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;

use crate::app;

#[implement(IDropTarget)]
pub struct DropTarget;

fn fmt(cf: u16) -> FORMATETC {
    FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 }
}

const CF_HDROP: u16 = 15;
const CF_UNICODETEXT: u16 = 13;

fn url_format() -> u16 {
    unsafe { RegisterClipboardFormatW(windows::core::w!("UniformResourceLocatorW")) as u16 }
}

fn descriptor_format() -> u16 {
    unsafe { RegisterClipboardFormatW(windows::core::w!("FileGroupDescriptorW")) as u16 }
}

fn contents_format() -> u16 {
    unsafe { RegisterClipboardFormatW(windows::core::w!("FileContents")) as u16 }
}

fn accepts(obj: &IDataObject) -> bool {
    unsafe {
        obj.QueryGetData(&fmt(CF_HDROP)).is_ok()
            || obj.QueryGetData(&fmt(descriptor_format())).is_ok()
            || obj.QueryGetData(&fmt(url_format())).is_ok()
            || obj.QueryGetData(&fmt(CF_UNICODETEXT)).is_ok()
    }
}

fn read_text(obj: &IDataObject, cf: u16) -> Option<String> {
    unsafe {
        let mut m = obj.GetData(&fmt(cf)).ok()?;
        let h = m.u.hGlobal;
        let p = GlobalLock(h) as *const u16;
        let mut s = None;
        if !p.is_null() {
            let mut n = 0;
            while *p.add(n) != 0 && n < 8192 {
                n += 1;
            }
            s = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
            let _ = GlobalUnlock(h);
        }
        ReleaseStgMedium(&mut m);
        s
    }
}

fn read_files(obj: &IDataObject) -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let Ok(mut m) = obj.GetData(&fmt(CF_HDROP)) else { return out };
        let hdrop = HDROP(m.u.hGlobal.0);
        let n = DragQueryFileW(hdrop, u32::MAX, None);
        for i in 0..n {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            out.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
        }
        ReleaseStgMedium(&mut m);
    }
    out
}

/// A path in the shelf folder for `name`, made safe and unique.
fn shelf_path(name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let safe = if safe.trim().is_empty() { "file".to_string() } else { safe };
    let dir = crate::model::shelf_dir();
    let p = Path::new(&safe);
    let (stem, ext) = (
        p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default(),
    );
    let mut path = dir.join(&safe);
    let mut i = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({i}){ext}"));
        i += 1;
    }
    path
}

/// Reads one FileContents entry, delivered either as an HGLOBAL or an IStream.
fn read_contents(obj: &IDataObject, index: i32) -> Option<Vec<u8>> {
    unsafe {
        let f = FORMATETC { cfFormat: contents_format(), ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: index, tymed: (TYMED_HGLOBAL.0 | TYMED_ISTREAM.0) as u32 };
        let mut m = obj.GetData(&f).ok()?;
        let mut out = Vec::new();
        if m.tymed == TYMED_HGLOBAL.0 as u32 {
            let h = m.u.hGlobal;
            let p = GlobalLock(h) as *const u8;
            if !p.is_null() {
                out.extend_from_slice(std::slice::from_raw_parts(p, GlobalSize(h)));
                let _ = GlobalUnlock(h);
            }
        } else if m.tymed == TYMED_ISTREAM.0 as u32 {
            if let Some(stream) = m.u.pstm.as_ref() {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let mut n = 0u32;
                    if stream.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut n)).is_err() || n == 0 {
                        break;
                    }
                    out.extend_from_slice(&buf[..n as usize]);
                }
            }
        }
        ReleaseStgMedium(&mut m);
        Some(out)
    }
}

/// Saves virtual files (Outlook attachments, browser images) that arrive without a path.
fn read_virtual(obj: &IDataObject) -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let Ok(mut m) = obj.GetData(&fmt(descriptor_format())) else { return out };
        let h = m.u.hGlobal;
        let g = GlobalLock(h) as *const FILEGROUPDESCRIPTORW;
        let mut entries = Vec::new();
        if !g.is_null() {
            let count = (*g).cItems as usize;
            let first = std::ptr::addr_of!((*g).fgd) as *const FILEDESCRIPTORW;
            for i in 0..count.min(crate::model::SHELF_MAX) {
                let d = std::ptr::read_unaligned(first.add(i));
                let (attrs, file_name) = (d.dwFileAttributes, d.cFileName);
                let n = file_name.iter().position(|&c| c == 0).unwrap_or(file_name.len());
                const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
                if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 {
                    entries.push((i as i32, String::from_utf16_lossy(&file_name[..n])));
                }
            }
            let _ = GlobalUnlock(h);
        }
        ReleaseStgMedium(&mut m);
        for (i, name) in entries {
            let Some(bytes) = read_contents(obj, i) else { continue };
            let base = Path::new(&name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(name);
            let path = shelf_path(&base);
            if std::fs::write(&path, bytes).is_ok() {
                out.push(path);
            }
        }
    }
    out
}

/// The shell's icon or thumbnail for `path`, as premultiplied top-down BGRA pixels.
pub fn shell_image(path: &Path, px: i32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let hbmp = factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK).ok()?;
        // Fitted thumbnails keep their aspect ratio, so read the bitmap at its own size.
        let mut bm = BITMAP::default();
        if GetObjectW(hbmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as _)) == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            let _ = DeleteObject(hbmp.into());
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let rows = GetDIBits(dc, hbmp, 0, h as u32, Some(buf.as_mut_ptr() as _), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(hbmp.into());
        if rows == 0 {
            return None;
        }
        // Thumbnails come without alpha; icons come with straight alpha.
        if buf.chunks(4).all(|p| p[3] == 0) {
            buf.chunks_mut(4).for_each(|p| p[3] = 255);
        } else {
            for p in buf.chunks_mut(4) {
                let a = p[3] as u32;
                for c in &mut p[..3] {
                    *c = (*c as u32 * a / 255) as u8;
                }
            }
        }
        Some((w as u32, h as u32, buf))
    }
}

/// Saves a dropped link as an Internet Shortcut so it can live on the shelf like a file.
fn save_link(url: &str) -> Option<PathBuf> {
    let url = url.lines().next()?.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let host = url.split("://").nth(1)?.split('/').next()?.trim_start_matches("www.");
    let safe: String = host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    let dir = crate::model::shelf_dir();
    let mut path = dir.join(format!("{safe}.url"));
    let mut i = 2;
    while path.exists() {
        path = dir.join(format!("{safe} ({i}).url"));
        i += 1;
    }
    std::fs::write(&path, format!("[InternetShortcut]\r\nURL={url}\r\n")).ok()?;
    Some(path)
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, obj: Ref<IDataObject>, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let ok = obj.as_ref().is_some_and(accepts);
        unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if ok {
            app().drag_move(pt.x, pt.y, true);
            app().drag_hover(true);
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        app().drag_move(pt.x, pt.y, false);
        unsafe { *effect = if app().drag_over { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        app().drag_hover(false);
        Ok(())
    }

    fn Drop(&self, obj: Ref<IDataObject>, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        app().drag_move(pt.x, pt.y, false);
        let mut paths = Vec::new();
        if let Some(obj) = obj.as_ref() {
            paths = read_files(obj);
            if paths.is_empty() {
                paths = read_virtual(obj);
            }
            if paths.is_empty() {
                let url = read_text(obj, url_format()).or_else(|| read_text(obj, CF_UNICODETEXT));
                if let Some(p) = url.and_then(|u| save_link(&u)) {
                    paths.push(p);
                }
            }
        }
        unsafe { *effect = if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY } };
        if !paths.is_empty() {
            app().drop_burst();
        }
        app().drag_hover(false);
        app().shelf_add(paths);
        Ok(())
    }
}

pub fn register(hwnd: HWND) -> Result<()> {
    let target: IDropTarget = DropTarget.into();
    unsafe { RegisterDragDrop(hwnd, &target) }
}

/// Starts a shell drag of one shelf file so it can be dropped into Explorer, mail, chat…
pub fn drag_out(hwnd: HWND, path: &Path) -> Result<()> {
    unsafe {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        SHParseDisplayName(&HSTRING::from(path.as_os_str()), None, &mut pidl, 0, None)?;
        let parent = ILClone(pidl);
        let _ = ILRemoveLastID(Some(parent));
        let child = ILFindLastID(pidl) as *const ITEMIDLIST;
        let data: Result<IDataObject> = SHCreateDataObject(Some(parent), Some(&[child]), None);
        let result = data.and_then(|d| SHDoDragDrop(Some(hwnd), &d, None, DROPEFFECT_COPY | DROPEFFECT_LINK).map(|_| ()));
        ILFree(Some(parent));
        ILFree(Some(pidl));
        result
    }
}
