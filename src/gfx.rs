//! Direct2D/DirectWrite drawing into a per-pixel-alpha layered window.

use std::collections::HashMap;

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindowIndirect, ULW_ALPHA, UPDATELAYEREDWINDOWINFO};
use windows_numerics::{Matrix3x2, Vector2};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    L,
    C,
    R,
    /// Left aligned, wrapping, anchored to the top of the box.
    LW,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Font {
    Ui,
    Icon,
    Sym,
}

pub fn rgba(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 255) as f32 / 255.0,
        g: ((hex >> 8) & 255) as f32 / 255.0,
        b: (hex & 255) as f32 / 255.0,
        a,
    }
}

pub fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rc {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rc {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rc { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

pub struct Gfx {
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    formats: HashMap<(u32, u32, Align, Font), IDWriteTextFormat>,
    icon_family: HSTRING,
    mem_dc: HDC,
    dib: HBITMAP,
    pub w: i32,
    pub h: i32,
    pub scale: f32,
    /// Global opacity multiplier applied to every draw call.
    pub alpha: f32,
    /// Translation set by `offset` (page slides), kept so `zoom` can compose with it.
    pub off: (f32, f32),
    /// Vertical offset applied to the whole frame (the notch drops in from above on start).
    pub lift: f32,
    /// Scale about the top center applied to the whole frame (startup grow-in).
    pub grow: f32,
    pub center_x: f32,
    /// Pixel rect being drawn this frame and the one drawn last frame (None = whole window).
    dirty: RECT,
    last_dirty: Option<RECT>,
}

impl Gfx {
    pub unsafe fn new(w: i32, h: i32, scale: f32) -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = factory.CreateDCRenderTarget(&props)?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let brush = rt.CreateSolidColorBrush(&rgba(0xffffff, 1.0), None)?;
        let round = factory.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            },
            None,
        )?;

        let mut coll: Option<IDWriteFontCollection> = None;
        dw.GetSystemFontCollection(&mut coll, false)?;
        let mut icon_family = HSTRING::from("Segoe MDL2 Assets");
        if let Some(coll) = coll {
            let mut idx = 0u32;
            let mut exists = windows::core::BOOL(0);
            if coll.FindFamilyName(w!("Segoe Fluent Icons"), &mut idx, &mut exists).is_ok() && exists.as_bool() {
                icon_family = HSTRING::from("Segoe Fluent Icons");
            }
        }

        let mem_dc = CreateCompatibleDC(None);
        let mut g = Gfx {
            factory,
            dw,
            rt,
            brush,
            round,
            formats: HashMap::new(),
            icon_family,
            mem_dc,
            dib: HBITMAP::default(),
            w: 0,
            h: 0,
            scale,
            alpha: 1.0,
            off: (0.0, 0.0),
            lift: 0.0,
            grow: 1.0,
            center_x: 0.0,
            dirty: RECT::default(),
            last_dirty: None,
        };
        g.resize(w, h)?;
        Ok(g)
    }

    pub unsafe fn resize(&mut self, w: i32, h: i32) -> Result<()> {
        if !self.dib.is_invalid() {
            let _ = DeleteObject(self.dib.into());
        }
        let bmi = BITMAPINFO {
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
        let mut bits = std::ptr::null_mut();
        self.dib = CreateDIBSection(Some(self.mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        SelectObject(self.mem_dc, self.dib.into());
        self.w = w;
        self.h = h;
        self.last_dirty = None;
        Ok(())
    }

    /// Starts a frame that redraws only `area` (layout units) plus whatever the previous
    /// frame covered, so small notch states don't repaint and recomposite the whole window.
    pub unsafe fn begin(&mut self, area: Rc) -> Result<()> {
        let s = self.scale;
        let cur = RECT {
            left: ((area.x * s).floor() as i32 - 2).max(0),
            top: 0,
            right: (((area.x + area.w) * s).ceil() as i32 + 2).min(self.w),
            bottom: (((area.y + area.h) * s).ceil() as i32 + 2).min(self.h),
        };
        let full = RECT { left: 0, top: 0, right: self.w, bottom: self.h };
        let rc = match self.last_dirty {
            Some(p) => RECT { left: cur.left.min(p.left), top: 0, right: cur.right.max(p.right), bottom: cur.bottom.max(p.bottom) },
            None => full,
        };
        self.last_dirty = Some(cur);
        self.dirty = rc;
        self.rt.BindDC(self.mem_dc, &rc)?;
        self.rt.BeginDraw();
        self.rt.Clear(Some(&rgba(0, 0.0)));
        self.off = (0.0, 0.0);
        self.rt.SetTransform(&self.base());
        self.alpha = 1.0;
        Ok(())
    }

    /// Layout units → pixels of the bound sub-rectangle.
    fn base(&self) -> Matrix3x2 {
        let c = self.center_x;
        Matrix3x2::translation(-c, 0.0)
            * Matrix3x2::scale(self.grow, self.grow)
            * Matrix3x2::translation(c, self.lift)
            * Matrix3x2::scale(self.scale, self.scale) * Matrix3x2::translation(-self.dirty.left as f32, -self.dirty.top as f32)
    }

    pub unsafe fn end(&mut self, hwnd: HWND, x: i32, y: i32) -> Result<()> {
        self.rt.EndDraw(None, None)?;
        let pos = POINT { x, y };
        let size = SIZE { cx: self.w, cy: self.h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: std::mem::size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: HDC::default(),
            pptDst: &pos,
            psize: &size,
            hdcSrc: self.mem_dc,
            pptSrc: &src,
            crKey: COLORREF(0),
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: &self.dirty,
        };
        UpdateLayeredWindowIndirect(hwnd, &info).ok()
    }

    fn set(&self, hex: u32, a: f32) {
        unsafe { self.brush.SetColor(&rgba(hex, (a * self.alpha).clamp(0.0, 1.0))) };
    }

    pub fn fill_rrect(&self, r: Rc, rad: f32, hex: u32, a: f32) {
        self.set(hex, a);
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
        unsafe { self.rt.FillRoundedRectangle(&rr, &self.brush) };
    }

    pub fn stroke_rrect(&self, r: Rc, rad: f32, width: f32, hex: u32, a: f32) {
        self.set(hex, a);
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
        unsafe { self.rt.DrawRoundedRectangle(&rr, &self.brush, width, None) };
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, rx: f32, ry: f32, hex: u32, a: f32) {
        self.set(hex, a);
        let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: rx, radiusY: ry };
        unsafe { self.rt.FillEllipse(&e, &self.brush) };
    }

    pub fn stroke_circle(&self, cx: f32, cy: f32, r: f32, width: f32, hex: u32, a: f32) {
        self.set(hex, a);
        let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&e, &self.brush, width, None) };
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, hex: u32, a: f32) {
        self.set(hex, a);
        unsafe { self.rt.DrawLine(v2(x1, y1), v2(x2, y2), &self.brush, width, &self.round) };
    }

    fn format(&mut self, size: f32, weight: u32, align: Align, font: Font) -> Option<IDWriteTextFormat> {
        let key = ((size * 10.0) as u32, weight, align, font);
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        unsafe {
            let family = match font {
                Font::Ui => HSTRING::from("Segoe UI"),
                Font::Icon => self.icon_family.clone(),
                Font::Sym => HSTRING::from("Segoe UI Symbol"),
            };
            let f = self
                .dw
                .CreateTextFormat(
                    &family,
                    None,
                    DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()?;
            let _ = f.SetTextAlignment(match align {
                Align::L | Align::LW => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::C => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::R => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            if align == Align::LW {
                let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
                let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            } else {
                let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            }
            if let Ok(sign) = self.dw.CreateEllipsisTrimmingSign(&f) {
                let trim = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let _ = f.SetTrimming(&trim, &sign);
            }
            self.formats.insert(key, f.clone());
            Some(f)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, s: &str, r: Rc, size: f32, weight: u32, hex: u32, a: f32, align: Align, font: Font) {
        let Some(f) = self.format(size, weight, align, font) else { return };
        let wide: Vec<u16> = s.encode_utf16().collect();
        self.set(hex, a);
        unsafe {
            self.rt.DrawText(&wide, &f, &r.d2d(), &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    pub fn ui(&mut self, s: &str, r: Rc, size: f32, weight: u32, hex: u32, a: f32, align: Align) {
        self.text(s, r, size, weight, hex, a, align, Font::Ui);
    }

    pub fn icon(&mut self, glyph: char, cx: f32, cy: f32, size: f32, hex: u32, a: f32) {
        let r = Rc::new(cx - size, cy - size, size * 2.0, size * 2.0);
        self.text(&glyph.to_string(), r, size, 400, hex, a, Align::C, Font::Icon);
    }

    pub fn measure(&mut self, s: &str, size: f32, weight: u32) -> f32 {
        let Some(f) = self.format(size, weight, Align::L, Font::Ui) else { return 0.0 };
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            if let Ok(layout) = self.dw.CreateTextLayout(&wide, &f, 4000.0, 100.0) {
                let mut m = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut m).is_ok() {
                    return m.widthIncludingTrailingWhitespace;
                }
            }
        }
        0.0
    }

    /// Height of `s` wrapped to `maxw`.
    pub fn measure_wrapped(&mut self, s: &str, size: f32, weight: u32, maxw: f32) -> f32 {
        let Some(f) = self.format(size, weight, Align::LW, Font::Ui) else { return 0.0 };
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            if let Ok(layout) = self.dw.CreateTextLayout(&wide, &f, maxw, 1000.0) {
                let mut m = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut m).is_ok() {
                    return m.height;
                }
            }
        }
        0.0
    }

    fn path(&self, build: impl FnOnce(&ID2D1GeometrySink)) -> Option<ID2D1PathGeometry> {
        unsafe {
            let g = self.factory.CreatePathGeometry().ok()?;
            let sink = g.Open().ok()?;
            build(&sink);
            sink.Close().ok()?;
            Some(g)
        }
    }

    fn fill_geom(&self, g: &ID2D1PathGeometry, hex: u32, a: f32) {
        self.set(hex, a);
        unsafe { self.rt.FillGeometry(g, &self.brush, None) };
    }

    /// The notch silhouette: flush with the screen top, flared "ears" into the
    /// top edge and rounded bottom corners.
    /// The notch silhouette: concave ears at the top, rounded bottom corners.
    fn notch_geom(&self, cx: f32, w: f32, h: f32, r: f32, ear: f32) -> Option<ID2D1PathGeometry> {
        let l = cx - w / 2.0;
        let rt = cx + w / 2.0;
        let r = r.min(h / 2.0).min(w / 2.0);
        let ear = ear.min(h - r).max(0.0);
        self.path(|s| unsafe {
            s.BeginFigure(v2(l - ear, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: v2(l, ear),
                size: D2D_SIZE_F { width: ear, height: ear },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            s.AddLine(v2(l, h - r));
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: v2(l + r, h),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            s.AddLine(v2(rt - r, h));
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: v2(rt, h - r),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            s.AddLine(v2(rt, ear));
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: v2(rt + ear, 0.0),
                size: D2D_SIZE_F { width: ear, height: ear },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        })
    }

    pub fn notch(&self, cx: f32, w: f32, h: f32, r: f32, ear: f32, hex: u32, a: f32) {
        if let Some(g) = self.notch_geom(cx, w, h, r, ear) {
            self.fill_geom(&g, hex, a);
        }
    }

    /// Clips subsequent drawing to the notch shape (so light effects never spill past its rim).
    pub fn push_notch_clip(&self, cx: f32, w: f32, h: f32, r: f32, ear: f32) -> bool {
        let Some(g) = self.notch_geom(cx, w, h, r, ear) else { return false };
        let Ok(mask) = windows::core::Interface::cast::<ID2D1Geometry>(&g) else { return false };
        unsafe {
            let Ok(layer) = self.rt.CreateLayer(None) else { return false };
            let mut params = D2D1_LAYER_PARAMETERS {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                geometricMask: std::mem::ManuallyDrop::new(Some(mask)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            self.rt.PushLayer(&params, &layer);
            // The layer holds its own reference; release ours (ManuallyDrop never would).
            std::mem::ManuallyDrop::drop(&mut params.geometricMask);
        }
        true
    }

    pub fn pop_layer(&self) {
        unsafe { self.rt.PopLayer() };
    }


    /// Progress arc starting at 12 o'clock, clockwise.
    pub fn arc(&self, cx: f32, cy: f32, r: f32, frac: f32, width: f32, hex: u32, a: f32) {
        let frac = frac.clamp(0.0, 1.0);
        if frac <= 0.001 {
            return;
        }
        if frac >= 0.999 {
            self.stroke_circle(cx, cy, r, width, hex, a);
            return;
        }
        let ang = frac * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let end = v2(cx + r * ang.cos(), cy + r * ang.sin());
        let g = self.path(|s| unsafe {
            s.BeginFigure(v2(cx, cy - r), D2D1_FIGURE_BEGIN_HOLLOW);
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: end,
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            s.EndFigure(D2D1_FIGURE_END_OPEN);
        });
        if let Some(g) = g {
            self.set(hex, a);
            unsafe { self.rt.DrawGeometry(&g, &self.brush, width, &self.round) };
        }
    }

    pub fn droplet(&self, cx: f32, cy: f32, s: f32, hex: u32, a: f32) {
        let g = self.path(|p| unsafe {
            p.BeginFigure(v2(cx, cy - s), D2D1_FIGURE_BEGIN_FILLED);
            p.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v2(cx + s * 0.25, cy - s * 0.45),
                point2: v2(cx + s * 0.6, cy - s * 0.05),
                point3: v2(cx + s * 0.6, cy + s * 0.32),
            });
            p.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v2(cx + s * 0.6, cy + s * 0.78),
                point2: v2(cx - s * 0.6, cy + s * 0.78),
                point3: v2(cx - s * 0.6, cy + s * 0.32),
            });
            p.AddBezier(&D2D1_BEZIER_SEGMENT {
                point1: v2(cx - s * 0.6, cy - s * 0.05),
                point2: v2(cx - s * 0.25, cy - s * 0.45),
                point3: v2(cx, cy - s),
            });
            p.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        if let Some(g) = g {
            self.fill_geom(&g, hex, a);
        }
    }

    pub fn bolt(&self, cx: f32, cy: f32, s: f32, hex: u32, a: f32) {
        let pts = [(0.15, -1.0), (-0.55, 0.12), (-0.02, 0.12), (-0.18, 1.0), (0.55, -0.15), (0.02, -0.15)];
        let g = self.path(|p| unsafe {
            p.BeginFigure(v2(cx + pts[0].0 * s, cy + pts[0].1 * s), D2D1_FIGURE_BEGIN_FILLED);
            for &(x, y) in &pts[1..] {
                p.AddLine(v2(cx + x * s, cy + y * s));
            }
            p.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        if let Some(g) = g {
            self.fill_geom(&g, hex, a);
        }
    }

    pub fn triangle(&self, pts: [(f32, f32); 3], hex: u32, a: f32) {
        let g = self.path(|p| unsafe {
            p.BeginFigure(v2(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_FILLED);
            p.AddLine(v2(pts[1].0, pts[1].1));
            p.AddLine(v2(pts[2].0, pts[2].1));
            p.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        if let Some(g) = g {
            self.fill_geom(&g, hex, a);
        }
    }

    pub fn bitmap(&self, w: u32, h: u32, px: &[u8]) -> Option<ID2D1Bitmap> {
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            self.rt
                .CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(px.as_ptr() as _), w * 4, &props)
                .ok()
        }
    }

    pub fn image(&self, bmp: &ID2D1Bitmap, r: Rc, rad: f32, a: f32) {
        unsafe {
            let size = bmp.GetSize();
            let Ok(brush) = self.rt.CreateBitmapBrush(bmp, None, None) else { return };
            let m = Matrix3x2::scale(r.w / size.width, r.h / size.height) * Matrix3x2::translation(r.x, r.y);
            brush.SetTransform(&m);
            brush.SetOpacity((a * self.alpha).clamp(0.0, 1.0));
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
            self.rt.FillRoundedRectangle(&rr, &brush);
        }
    }

    /// Vertical gradient fill of a rounded rectangle.
    pub fn fill_rrect_vgrad(&self, r: Rc, rad: f32, top: (u32, f32), bottom: (u32, f32)) {
        self.fill_rrect_grad(r, rad, (r.x, r.y), (r.x, r.y + r.h), top, bottom);
    }

    /// Horizontal gradient fill of a rounded rectangle.
    pub fn fill_rrect_hgrad(&self, r: Rc, rad: f32, left: (u32, f32), right: (u32, f32)) {
        self.fill_rrect_grad(r, rad, (r.x, r.y), (r.x + r.w, r.y), left, right);
    }

    fn linear(&self, p0: (f32, f32), p1: (f32, f32), c0: (u32, f32), c1: (u32, f32)) -> Option<ID2D1LinearGradientBrush> {
        unsafe {
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c0.0, c0.1 * self.alpha) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c1.0, c1.1 * self.alpha) },
            ];
            let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(p0.0, p0.1), endPoint: v2(p1.0, p1.1) };
            self.rt.CreateLinearGradientBrush(&props, None, &coll).ok()
        }
    }

    fn fill_rrect_grad(&self, r: Rc, rad: f32, p0: (f32, f32), p1: (f32, f32), top: (u32, f32), bottom: (u32, f32)) {
        let Some(brush) = self.linear(p0, p1, top, bottom) else { return };
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
        unsafe { self.rt.FillRoundedRectangle(&rr, &brush) };
    }

    /// Soft round glow: `hex` at alpha `a` in the center fading to transparent at radius `r`.
    pub fn glow(&self, cx: f32, cy: f32, r: f32, hex: u32, a: f32) {
        self.glow_ellipse(cx, cy, r, r, hex, a);
    }

    /// Elliptical soft light: `hex` at alpha `a` in the centre, fading out at the radii.
    pub fn glow_ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32, hex: u32, a: f32) {
        unsafe {
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(hex, a * self.alpha) },
                D2D1_GRADIENT_STOP { position: 0.55, color: rgba(hex, a * 0.45 * self.alpha) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(hex, 0.0) },
            ];
            let Ok(coll) = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: v2(cx, cy), gradientOriginOffset: v2(0.0, 0.0), radiusX: rx, radiusY: ry };
            let Ok(brush) = self.rt.CreateRadialGradientBrush(&props, None, &coll) else { return };
            let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: rx, radiusY: ry };
            self.rt.FillEllipse(&e, &brush);
        }
    }

    /// Circle stroke with a linear gradient running from the top-left to the bottom-right of its bounds.
    pub fn stroke_circle_diag(&self, cx: f32, cy: f32, r: f32, width: f32, tl: (u32, f32), br: (u32, f32)) {
        let Some(brush) = self.linear((cx - r, cy - r), (cx + r, cy + r), tl, br) else { return };
        let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&e, &brush, width, None) };
    }

    pub fn stroke_rrect_dashed(&self, r: Rc, rad: f32, width: f32, dash: f32, gap: f32, hex: u32, a: f32) {
        unsafe {
            let dashes = [dash / width, gap / width];
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_FLAT,
                endCap: D2D1_CAP_STYLE_FLAT,
                dashCap: D2D1_CAP_STYLE_FLAT,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_CUSTOM,
                dashOffset: 0.0,
            };
            let Ok(style) = self.factory.CreateStrokeStyle(&props, Some(&dashes)) else { return };
            self.set(hex, a);
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
            self.rt.DrawRoundedRectangle(&rr, &self.brush, width, &style);
        }
    }

    /// Filled pie slice starting at 12 o'clock, clockwise.
    pub fn pie(&self, cx: f32, cy: f32, r: f32, frac: f32, hex: u32, a: f32) {
        let frac = frac.clamp(0.0, 1.0);
        if frac >= 0.999 {
            self.fill_circle(cx, cy, r, r, hex, a);
            return;
        }
        if frac <= 0.001 {
            return;
        }
        let ang = frac * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let end = v2(cx + r * ang.cos(), cy + r * ang.sin());
        let g = self.path(|s| unsafe {
            s.BeginFigure(v2(cx, cy), D2D1_FIGURE_BEGIN_FILLED);
            s.AddLine(v2(cx, cy - r));
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: end,
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        if let Some(g) = g {
            self.fill_geom(&g, hex, a);
        }
    }

    /// Arc between two angles (radians, 0 = 3 o'clock, clockwise positive).
    pub fn arc_between(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32, width: f32, hex: u32, a: f32) {
        if to - from <= 0.001 {
            return;
        }
        let p = |t: f32| v2(cx + r * t.cos(), cy + r * t.sin());
        let g = self.path(|s| unsafe {
            s.BeginFigure(p(from), D2D1_FIGURE_BEGIN_HOLLOW);
            s.AddArc(&D2D1_ARC_SEGMENT {
                point: p(to),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if to - from > std::f32::consts::PI { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            s.EndFigure(D2D1_FIGURE_END_OPEN);
        });
        if let Some(g) = g {
            self.set(hex, a);
            unsafe { self.rt.DrawGeometry(&g, &self.brush, width, &self.round) };
        }
    }

    /// Polyline with round caps and joins (icons drawn from strokes).
    pub fn polyline(&self, pts: &[(f32, f32)], width: f32, hex: u32, a: f32) {
        if pts.len() < 2 {
            return;
        }
        let g = self.path(|s| unsafe {
            s.BeginFigure(v2(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
            for &(x, y) in &pts[1..] {
                s.AddLine(v2(x, y));
            }
            s.EndFigure(D2D1_FIGURE_END_OPEN);
        });
        if let Some(g) = g {
            self.set(hex, a);
            unsafe { self.rt.DrawGeometry(&g, &self.brush, width, &self.round) };
        }
    }

    pub fn polygon(&self, pts: &[(f32, f32)], hex: u32, a: f32) {
        if pts.len() < 3 {
            return;
        }
        let g = self.path(|s| unsafe {
            s.BeginFigure(v2(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_FILLED);
            for &(x, y) in &pts[1..] {
                s.AddLine(v2(x, y));
            }
            s.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        if let Some(g) = g {
            self.fill_geom(&g, hex, a);
        }
    }

    /// Translates subsequent drawing by (dx, dy) layout units.
    pub fn offset(&mut self, dx: f32, dy: f32) {
        self.off = (dx, dy);
        unsafe { self.rt.SetTransform(&(Matrix3x2::translation(dx, dy) * self.base())) };
    }

    /// Scales subsequent drawing by `s` about (cx, cy), on top of the current offset.
    pub fn zoom(&self, cx: f32, cy: f32, s: f32) {
        let about = Matrix3x2::translation(-cx, -cy) * Matrix3x2::scale(s, s) * Matrix3x2::translation(cx, cy);
        unsafe { self.rt.SetTransform(&(about * Matrix3x2::translation(self.off.0, self.off.1) * self.base())) };
    }

    pub fn unzoom(&self) {
        unsafe { self.rt.SetTransform(&(Matrix3x2::translation(self.off.0, self.off.1) * self.base())) };
    }

    pub fn push_clip(&self, r: Rc) {
        unsafe { self.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    pub fn pop_clip(&self) {
        unsafe { self.rt.PopAxisAlignedClip() };
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            if !self.dib.is_invalid() {
                let _ = DeleteObject(self.dib.into());
            }
            let _ = DeleteDC(self.mem_dc);
        }
    }
}
