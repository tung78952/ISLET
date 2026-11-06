//! Direct2D/DirectWrite drawing into a per-pixel-alpha layered window.

use std::collections::HashMap;

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
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
    inter: Option<IDWriteFontCollection>,
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    /// What every draw goes to: the device context, which draws on the GPU into the swap chain
    /// that DirectComposition shows as the window's content.
    rt: ID2D1RenderTarget,
    dc: ID2D1DeviceContext,
    d3d: ID3D11Device,
    swap: Option<IDXGISwapChain1>,
    comp: Option<(IDCompositionDevice, IDCompositionTarget, IDCompositionVisual)>,
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    formats: HashMap<(u32, u32, Align, Font), IDWriteTextFormat>,
    /// Radial glow brushes per colour (re-centred and re-scaled per use) and measured text
    /// widths: creating these every frame caused render spikes.
    glows: std::cell::RefCell<HashMap<u32, ID2D1RadialGradientBrush>>,
    /// Two-colour gradient brushes by colours and the ratio of their alphas; each use only moves
    /// and fades the brush. Layers are kept one per nesting depth for the same reason.
    linears: std::cell::RefCell<HashMap<(u32, u32, u8, u8), ID2D1LinearGradientBrush>>,
    layers: std::cell::RefCell<Vec<ID2D1Layer>>,
    depth: std::cell::Cell<usize>,
    widths: HashMap<(String, u32, u32), f32>,
    /// Liquid glass: three displacements (one per colour, a little apart), their colour picks,
    /// and the sum. Made once, on first use.
    glass_fx: std::cell::RefCell<Option<([ID2D1Effect; 3], [ID2D1Effect; 3], ID2D1Effect)>>,
    icon_family: HSTRING,
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
    /// The pixel rect being drawn: always the whole window, as each frame is drawn anew.
    dirty: RECT,
}

/// A GPU device for drawing, or the software one where there is none (WARP).
unsafe fn d3d_device() -> Result<ID3D11Device> {
    let mut last = Err(windows::core::Error::empty());
    for kind in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        last = D3D11CreateDevice(None, D3D_DRIVER_TYPE(kind.0), Default::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, None);
        if let (Ok(()), Some(d)) = (&last, device) {
            return Ok(d);
        }
    }
    last.map(|_| unreachable!())
}

/// The app's own typeface, from memory: nothing is installed on the machine.
unsafe fn load_inter() -> Option<IDWriteFontCollection> {
    static BYTES: &[u8] = include_bytes!("../assets/fonts/Inter.ttf");
    let dw: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
    let loader = dw.CreateInMemoryFontFileLoader().ok()?;
    dw.RegisterFontFileLoader(&loader).ok()?;
    let file = loader.CreateInMemoryFontFileReference(&dw, BYTES.as_ptr() as *const _, BYTES.len() as u32, None).ok()?;
    let builder = dw.CreateFontSetBuilder().ok()?;
    builder.AddFontFile(&file).ok()?;
    let set = builder.CreateFontSet().ok()?;
    let coll = dw.CreateFontCollectionFromFontSet(&set).ok()?;
    windows::core::Interface::cast(&coll).ok()
}

impl Gfx {
    pub unsafe fn new(w: i32, h: i32, scale: f32) -> Result<Self> {
        let factory1: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let factory: ID2D1Factory = windows::core::Interface::cast(&factory1)?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let d3d = d3d_device()?;
        let dxgi: IDXGIDevice = windows::core::Interface::cast(&d3d)?;
        let dc = factory1.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let rt: ID2D1RenderTarget = windows::core::Interface::cast(&dc)?;
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        // White on black at small sizes needs more contrast than the system's default.
        if let Ok(params) = dw.CreateCustomRenderingParams(1.8, 1.0, 0.0, DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC) {
            rt.SetTextRenderingParams(&params);
        }
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

        let inter = load_inter();
        let mut g = Gfx {
            factory,
            inter,
            dw,
            rt,
            dc,
            d3d,
            swap: None,
            comp: None,
            brush,
            round,
            formats: HashMap::new(),
            glows: Default::default(),
            linears: Default::default(),
            layers: Default::default(),
            depth: Default::default(),
            widths: HashMap::new(),
            glass_fx: std::cell::RefCell::new(None),
            icon_family,
            w: 0,
            h: 0,
            scale,
            alpha: 1.0,
            off: (0.0, 0.0),
            lift: 0.0,
            grow: 1.0,
            center_x: 0.0,
            dirty: RECT::default(),
        };
        g.resize(w, h)?;
        Ok(g)
    }

    pub unsafe fn resize(&mut self, w: i32, h: i32) -> Result<()> {
        let (w, h) = (w.max(1), h.max(1));
        self.dc.SetTarget(None);
        match &self.swap {
            Some(swap) => swap.ResizeBuffers(2, w as u32, h as u32, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SWAP_CHAIN_FLAG(0))?,
            None => {
                let dxgi: IDXGIDevice = windows::core::Interface::cast(&self.d3d)?;
                let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: w as u32,
                    Height: h as u32,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                    ..Default::default()
                };
                self.swap = Some(factory.CreateSwapChainForComposition(&self.d3d, &desc, None)?);
            }
        }
        let swap = self.swap.as_ref().expect("made above");
        let surface: IDXGISurface = swap.GetBuffer(0)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };
        let target = self.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
        self.dc.SetTarget(&target);
        if let Some((comp, _, visual)) = &self.comp {
            visual.SetContent(swap)?;
            comp.Commit()?;
        }
        self.w = w;
        self.h = h;
        Ok(())
    }

    /// Shows the drawing as `hwnd`'s content (the window is made without a redirection bitmap).
    pub unsafe fn attach(&mut self, hwnd: HWND) -> Result<()> {
        let dxgi: IDXGIDevice = windows::core::Interface::cast(&self.d3d)?;
        let comp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
        let target = comp.CreateTargetForHwnd(hwnd, true)?;
        let visual = comp.CreateVisual()?;
        if let Some(swap) = &self.swap {
            visual.SetContent(swap)?;
        }
        target.SetRoot(&visual)?;
        comp.Commit()?;
        self.comp = Some((comp, target, visual));
        Ok(())
    }

    /// Every frame is drawn whole now; kept for callers that ask for a full redraw.
    pub fn invalidate(&mut self) {}

    pub unsafe fn begin(&mut self, _area: Rc) -> Result<()> {
        self.dirty = RECT { left: 0, top: 0, right: self.w, bottom: self.h };
        self.rt.BeginDraw();
        self.depth.set(0);
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

    pub unsafe fn end(&mut self, _hwnd: HWND, _x: i32, _y: i32) -> Result<()> {
        self.rt.EndDraw(None, None)?;
        match &self.swap {
            Some(swap) => swap.Present(0, DXGI_PRESENT(0)).ok(),
            None => Ok(()),
        }
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
            let inter = matches!(font, Font::Ui) && self.inter.is_some();
            let family = match font {
                Font::Ui if inter => HSTRING::from("Inter Variable"),
                Font::Ui => HSTRING::from("Segoe UI"),
                Font::Icon => self.icon_family.clone(),
                Font::Sym => HSTRING::from("Segoe UI Symbol"),
            };
            let f = self
                .dw
                .CreateTextFormat(
                    &family,
                    self.inter.as_ref().filter(|_| inter),
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
            // Not snapped to whole pixels: text that slides or grows would step a pixel at a time.
            self.rt.DrawText(&wide, &f, &r.d2d(), &self.brush, D2D1_DRAW_TEXT_OPTIONS_NO_SNAP, DWRITE_MEASURING_MODE_NATURAL);
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
        let key = (s.to_string(), (size * 10.0) as u32, weight);
        if let Some(w) = self.widths.get(&key) {
            return *w;
        }
        let w = self.measure_uncached(s, size, weight);
        if self.widths.len() > 2000 {
            self.widths.clear();
        }
        self.widths.insert(key, w);
        w
    }

    fn measure_uncached(&mut self, s: &str, size: f32, weight: u32) -> f32 {
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
        self.push_mask(&g)
    }

    /// Clips drawing to a closed outline until `pop_layer`.
    pub fn push_outline_clip(&self, pts: &[(f32, f32)]) -> bool {
        let Some((first, rest)) = pts.split_first() else { return false };
        let g = self.path(|p| unsafe {
            p.BeginFigure(v2(first.0, first.1), D2D1_FIGURE_BEGIN_FILLED);
            for q in rest {
                p.AddLine(v2(q.0, q.1));
            }
            p.EndFigure(D2D1_FIGURE_END_CLOSED);
        });
        g.is_some_and(|g| self.push_mask(&g))
    }

    /// The layer for the next push, made once for each depth of nesting.
    fn next_layer(&self) -> Option<ID2D1Layer> {
        let depth = self.depth.get();
        let mut pool = self.layers.borrow_mut();
        if pool.len() <= depth {
            pool.push(unsafe { self.rt.CreateLayer(None) }.ok()?);
        }
        self.depth.set(depth + 1);
        Some(pool[depth].clone())
    }

    fn push_mask(&self, g: &ID2D1PathGeometry) -> bool {
        let Ok(mask) = windows::core::Interface::cast::<ID2D1Geometry>(g) else { return false };
        unsafe {
            let Some(layer) = self.next_layer() else { return false };
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

    /// Runs `draw` with drawing clipped to a circle.
    pub fn clip_circle(&self, cx: f32, cy: f32, r: f32, draw: impl FnOnce()) {
        unsafe {
            let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: r, radiusY: r };
            let Ok(g) = self.factory.CreateEllipseGeometry(&e) else { return draw() };
            let Ok(mask) = windows::core::Interface::cast::<ID2D1Geometry>(&g) else { return draw() };
            let Some(layer) = self.next_layer() else { return draw() };
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
            std::mem::ManuallyDrop::drop(&mut params.geometricMask);
            draw();
            self.pop_layer();
        }
    }

    pub fn pop_layer(&self) {
        self.depth.set(self.depth.get().saturating_sub(1));
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

    pub fn fill_poly(&self, pts: &[(f32, f32)], hex: u32, a: f32) {
        let Some((first, rest)) = pts.split_first() else { return };
        let g = self.path(|p| unsafe {
            p.BeginFigure(v2(first.0, first.1), D2D1_FIGURE_BEGIN_FILLED);
            for q in rest {
                p.AddLine(v2(q.0, q.1));
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

    /// Draws `bmp` in the rounded rectangle `r` as a piece of thick glass: near its rim the picture
    /// bends inward by up to `amount` bitmap px, as read from `map` (made by `glass::map` at the
    /// bitmap's own size), with the colours parting very slightly. Returns false if it could not
    /// (the caller then draws it plainly).
    pub fn image_glass(&self, bmp: &ID2D1Bitmap, map: &ID2D1Bitmap, r: Rc, rad: f32, amount: f32, a: f32) -> bool {
        unsafe {
            let mut fx = self.glass_fx.borrow_mut();
            if fx.is_none() {
                let make = |id: &windows::core::GUID| self.dc.CreateEffect(id).ok();
                let (Some(d0), Some(d1), Some(d2), Some(c0), Some(c1), Some(c2), Some(sum)) = (
                    make(&CLSID_D2D1DisplacementMap),
                    make(&CLSID_D2D1DisplacementMap),
                    make(&CLSID_D2D1DisplacementMap),
                    make(&CLSID_D2D1ColorMatrix),
                    make(&CLSID_D2D1ColorMatrix),
                    make(&CLSID_D2D1ColorMatrix),
                    make(&CLSID_D2D1Composite),
                ) else {
                    return false;
                };
                let picks = [&c0, &c1, &c2];
                for (i, c) in picks.iter().enumerate() {
                    // Keep one colour channel (and full alpha); the three are added back together.
                    let mut m = [0.0f32; 20];
                    m[i * 4 + i] = 1.0;
                    m[15] = 1.0;
                    let bytes: Vec<u8> = m.iter().flat_map(|v| v.to_le_bytes()).collect();
                    let _ = c.SetValue(D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32, D2D1_PROPERTY_TYPE_MATRIX_5X4, &bytes);
                }
                for d in [&d0, &d1, &d2] {
                    let _ = d.SetValue(D2D1_DISPLACEMENTMAP_PROP_X_CHANNEL_SELECT.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_CHANNEL_SELECTOR_R.0 as u32).to_le_bytes());
                    let _ = d.SetValue(D2D1_DISPLACEMENTMAP_PROP_Y_CHANNEL_SELECT.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_CHANNEL_SELECTOR_G.0 as u32).to_le_bytes());
                }
                let _ = sum.SetInputCount(3);
                let _ = sum.SetValue(D2D1_COMPOSITE_PROP_MODE.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_COMPOSITE_MODE_PLUS.0 as u32).to_le_bytes());
                *fx = Some(([d0, d1, d2], [c0, c1, c2], sum));
            }
            let Some((disp, pick, sum)) = fx.as_ref() else { return false };
            let (Ok(src), Ok(field)) = (windows::core::Interface::cast::<ID2D1Image>(bmp), windows::core::Interface::cast::<ID2D1Image>(map)) else { return false };
            for i in 0..3 {
                disp[i].SetInput(0, &src, true);
                disp[i].SetInput(1, &field, true);
                // DisplacementMap moves by scale·(value − 0.5): a full map swing is 2·amount.
                let scale = 2.0 * amount * (1.0 + 0.07 * i as f32);
                let _ = disp[i].SetValue(D2D1_DISPLACEMENTMAP_PROP_SCALE.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &scale.to_le_bytes());
                let Ok(out) = disp[i].GetOutput() else { return false };
                pick[i].SetInput(0, &out, true);
                let Ok(one) = pick[i].GetOutput() else { return false };
                sum.SetInput(i as u32, &one, true);
            }
            let Ok(image) = sum.GetOutput() else { return false };
            // Clipped to the rounded rectangle, at the right opacity.
            let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
            let Ok(shape) = self.factory.CreateRoundedRectangleGeometry(&rr) else { return false };
            let Ok(mask) = windows::core::Interface::cast::<ID2D1Geometry>(&shape) else { return false };
            let Some(layer) = self.next_layer() else { return false };
            let mut params = D2D1_LAYER_PARAMETERS {
                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                geometricMask: std::mem::ManuallyDrop::new(Some(mask)),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: (a * self.alpha).clamp(0.0, 1.0),
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS_NONE,
            };
            self.rt.PushLayer(&params, &layer);
            std::mem::ManuallyDrop::drop(&mut params.geometricMask);
            let size = bmp.GetSize();
            let mut was = Matrix3x2::identity();
            self.rt.GetTransform(&mut was);
            self.rt.SetTransform(&(Matrix3x2::scale(r.w / size.width, r.h / size.height) * Matrix3x2::translation(r.x, r.y) * was));
            self.dc.DrawImage(&image, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            self.rt.SetTransform(&was);
            self.pop_layer();
            true
        }
    }

    /// Draws a bitmap keeping its aspect ratio: `cover` fills `r` (cropping), otherwise it fits
    /// inside `r`, centred.
    pub fn image_fit(&self, bmp: &ID2D1Bitmap, r: Rc, rad: f32, a: f32, cover: bool) {
        let size = unsafe { bmp.GetSize() };
        if size.width <= 0.0 || size.height <= 0.0 {
            return;
        }
        let (sx, sy) = (r.w / size.width, r.h / size.height);
        let k = if cover { sx.max(sy) } else { sx.min(sy) };
        let (w, h) = (size.width * k, size.height * k);
        let img = Rc::new(r.cx() - w / 2.0, r.y + (r.h - h) / 2.0, w, h);
        let shape = if cover { r } else { img };
        unsafe {
            let Ok(brush) = self.rt.CreateBitmapBrush(bmp, None, None) else { return };
            brush.SetTransform(&(Matrix3x2::scale(k, k) * Matrix3x2::translation(img.x, img.y)));
            brush.SetOpacity((a * self.alpha).clamp(0.0, 1.0));
            let rr = D2D1_ROUNDED_RECT { rect: shape.d2d(), radiusX: rad, radiusY: rad };
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
        // The stronger end is the brush's opacity; the stops only keep how the two compare.
        let most = c0.1.max(c1.1);
        let op = (most * self.alpha).clamp(0.0, 1.0);
        if op <= 0.002 {
            return None;
        }
        let q = |a: f32| ((a / most).clamp(0.0, 1.0) * 64.0).round() as u8;
        let key = (c0.0, c1.0, q(c0.1), q(c1.1));
        unsafe {
            let mut cache = self.linears.borrow_mut();
            if !cache.contains_key(&key) {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(c0.0, key.2 as f32 / 64.0) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(c1.0, key.3 as f32 / 64.0) },
                ];
                let coll = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP).ok()?;
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(p0.0, p0.1), endPoint: v2(p1.0, p1.1) };
                let brush = self.rt.CreateLinearGradientBrush(&props, None, &coll).ok()?;
                if cache.len() > 256 {
                    cache.clear();
                }
                cache.insert(key, brush);
            }
            let brush = cache[&key].clone();
            brush.SetStartPoint(v2(p0.0, p0.1));
            brush.SetEndPoint(v2(p1.0, p1.1));
            brush.SetOpacity(op);
            Some(brush)
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
        let op = (a * self.alpha).clamp(0.0, 1.0);
        if op <= 0.002 || rx <= 0.0 || ry <= 0.0 {
            return;
        }
        unsafe {
            let mut cache = self.glows.borrow_mut();
            if !cache.contains_key(&hex) {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: rgba(hex, 1.0) },
                    D2D1_GRADIENT_STOP { position: 0.55, color: rgba(hex, 0.45) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: rgba(hex, 0.0) },
                ];
                let Ok(coll) = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
                let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: v2(0.0, 0.0), gradientOriginOffset: v2(0.0, 0.0), radiusX: 1.0, radiusY: 1.0 };
                let Ok(brush) = self.rt.CreateRadialGradientBrush(&props, None, &coll) else { return };
                if cache.len() > 128 {
                    cache.clear();
                }
                cache.insert(hex, brush);
            }
            let brush = &cache[&hex];
            brush.SetCenter(v2(cx, cy));
            brush.SetRadiusX(rx);
            brush.SetRadiusY(ry);
            brush.SetOpacity(op);
            let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: rx, radiusY: ry };
            self.rt.FillEllipse(&e, brush);
        }
    }

    /// Fills `r` with a left-to-right gradient through evenly spaced `colors`.
    pub fn fill_rect_hstops(&self, r: Rc, colors: &[u32], a: f32) {
        if colors.len() < 2 {
            return;
        }
        unsafe {
            let n = (colors.len() - 1) as f32;
            let stops: Vec<D2D1_GRADIENT_STOP> =
                colors.iter().enumerate().map(|(i, c)| D2D1_GRADIENT_STOP { position: i as f32 / n, color: rgba(*c, a * self.alpha) }).collect();
            let Ok(coll) = self.rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: v2(r.x, 0.0), endPoint: v2(r.x + r.w, 0.0) };
            let Ok(brush) = self.rt.CreateLinearGradientBrush(&props, None, &coll) else { return };
            self.rt.FillRectangle(&r.d2d(), &brush);
        }
    }

    /// Circle stroke with a linear gradient running from the top-left to the bottom-right of its bounds.
    pub fn stroke_circle_diag(&self, cx: f32, cy: f32, r: f32, width: f32, tl: (u32, f32), br: (u32, f32)) {
        let Some(brush) = self.linear((cx - r, cy - r), (cx + r, cy + r), tl, br) else { return };
        let e = D2D1_ELLIPSE { point: v2(cx, cy), radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&e, &brush, width, None) };
    }

    /// Rounded-rectangle stroke with a linear gradient from its top-left to its bottom-right.
    pub fn stroke_rrect_diag(&self, r: Rc, rad: f32, width: f32, tl: (u32, f32), br: (u32, f32)) {
        let Some(brush) = self.linear((r.x, r.y), (r.x + r.w, r.y + r.h), tl, br) else { return };
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad };
        unsafe { self.rt.DrawRoundedRectangle(&rr, &brush, width, None) };
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

    /// Round-capped arc (clockwise, radians from 3 o'clock) with a top-left → bottom-right gradient.
    pub fn arc_diag(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32, width: f32, tl: (u32, f32), br: (u32, f32)) {
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
        let Some(brush) = self.linear((cx - r, cy - r), (cx + r, cy + r), tl, br) else { return };
        if let Some(g) = g {
            unsafe { self.rt.DrawGeometry(&g, &brush, width, &self.round) };
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

    /// Rotates subsequent drawing by `rad` about (cx, cy), on top of the current offset.
    pub fn rotate(&self, cx: f32, cy: f32, rad: f32) {
        let about = Matrix3x2::translation(-cx, -cy) * Matrix3x2::rotation_around(rad.to_degrees(), windows_numerics::Vector2 { X: 0.0, Y: 0.0 }) * Matrix3x2::translation(cx, cy);
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

