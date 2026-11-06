//! What is on the screen under the open notch, for Liquid glass: a Windows.Graphics.Capture
//! session on the notch's monitor, from which the strip under the notch is copied into a small
//! texture Direct2D can draw. It runs only while the glass is open; the notch is kept out of the
//! capture (display affinity) so it never sees itself.

use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap1;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

pub struct Backdrop {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    device: ID3D11Device,
    /// The monitor's top-left on the desktop, to turn screen pixels into frame pixels.
    origin: (i32, i32),
    frame_size: (i32, i32),
    /// The strip under the notch, and Direct2D's view of it.
    strip: Option<(ID3D11Texture2D, ID2D1Bitmap1, (u32, u32))>,
}

impl Backdrop {
    /// Starts watching the monitor that holds `hwnd`, on `device` (Direct2D's own).
    pub fn start(hwnd: HWND, device: &ID3D11Device) -> windows::core::Result<Self> {
        unsafe {
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut info);
            let RECT { left, top, right, bottom } = info.rcMonitor;
            let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = interop.CreateForMonitor(mon)?;
            let dxgi: IDXGIDevice = device.cast()?;
            let rt_device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
            let size = item.Size()?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&rt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
            let session = pool.CreateCaptureSession(&item)?;
            let _ = session.SetIsCursorCaptureEnabled(false);
            let _ = session.SetIsBorderRequired(false);
            session.StartCapture()?;
            Ok(Backdrop { pool, session, device: device.clone(), origin: (left, top), frame_size: (right - left, bottom - top), strip: None })
        }
    }

    /// The screen under `area` (desktop pixels: left, top, width, height), from the newest frame;
    /// the last one is kept when nothing new came. None until a first frame has arrived.
    pub fn latest(&mut self, gfx: &crate::gfx::Gfx, area: (i32, i32, u32, u32)) -> Option<ID2D1Bitmap1> {
        let (w, h) = (area.2.max(1), area.3.max(1));
        if self.strip.as_ref().is_none_or(|s| s.2 != (w, h)) {
            self.strip = make_strip(&self.device, gfx, w, h);
        }
        let (strip, bmp, _) = self.strip.as_ref()?;
        if let Ok(frame) = self.pool.TryGetNextFrame() {
            unsafe {
                let tex: Option<ID3D11Texture2D> = frame.Surface().ok().and_then(|s| s.cast::<IDirect3DDxgiInterfaceAccess>().ok()).and_then(|a| a.GetInterface::<ID3D11Texture2D>().ok());
                if let (Some(tex), Ok(ctx)) = (tex, self.device.GetImmediateContext()) {
                    let x0 = (area.0 - self.origin.0).clamp(0, self.frame_size.0 - 1);
                    let y0 = (area.1 - self.origin.1).clamp(0, self.frame_size.1 - 1);
                    let x1 = (x0 + w as i32).min(self.frame_size.0);
                    let y1 = (y0 + h as i32).min(self.frame_size.1);
                    let src = D3D11_BOX { left: x0 as u32, top: y0 as u32, front: 0, right: x1 as u32, bottom: y1 as u32, back: 1 };
                    ctx.CopySubresourceRegion(strip, 0, 0, 0, 0, &tex, 0, Some(&src));
                }
                let _ = frame.Close();
            }
        }
        Some(bmp.clone())
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

fn make_strip(device: &ID3D11Device, gfx: &crate::gfx::Gfx, w: u32, h: u32) -> Option<(ID3D11Texture2D, ID2D1Bitmap1, (u32, u32))> {
    unsafe {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut tex = None;
        device.CreateTexture2D(&desc, None, Some(&mut tex)).ok()?;
        let tex = tex?;
        let bmp = gfx.bitmap_from_texture(&tex)?;
        Some((tex, bmp, (w, h)))
    }
}
