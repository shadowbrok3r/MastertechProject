//! Headless rendering of egui UI to RGBA images for tests: CPU rasteriser over `ctx.tessellate`.

use std::collections::HashMap;

use eframe::egui::{
    CentralPanel, Color32, ColorImage, Context, Event, ImageData, Pos2, RawInput, Rect, TextureId,
    TexturesDelta, Ui, Vec2,
    epaint::{ClippedPrimitive, ClippedShape, Primitive, Vertex},
};

use super::{
    metrics::{self, Density},
    theme_config::{PresetStyles, apply_preset},
};

/// A context with the app's fonts and image loaders whose frames can be rasterised.
pub(crate) struct Headless {
    pub ctx: Context,
    textures: HashMap<TextureId, ColorImage>,
    freed: Vec<TextureId>,
    size: Vec2,
    ppp: f32,
}

impl Headless {
    /// A `size`-point screen rendered at `ppp` pixels per point.
    pub fn new(size: Vec2, ppp: f32) -> Self {
        let ctx = Context::default();
        ctx.set_fonts(crate::app_state::font_definitions());
        egui_extras::install_image_loaders(&ctx);
        ctx.set_pixels_per_point(ppp);
        Self {
            ctx,
            textures: HashMap::new(),
            freed: Vec::new(),
            size,
            ppp,
        }
    }

    /// Sets the screen size of later frames.
    pub fn resize(&mut self, size: Vec2) {
        self.size = size;
    }

    /// Installs `preset` at `density` the way the theme editor does.
    pub fn theme(&self, preset: PresetStyles, density: Density) {
        metrics::set_density(&self.ctx, density);
        apply_preset(&self.ctx, preset);
    }

    fn input(&self, events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            events,
            ..Default::default()
        }
    }

    /// One pass of `f` inside a central panel; returns its result and the primitives drawn.
    pub fn frame<R>(
        &mut self,
        events: Vec<Event>,
        mut f: impl FnMut(&mut Ui) -> R,
    ) -> (R, Vec<ClippedPrimitive>) {
        self.frame_root(events, |ui| {
            CentralPanel::default().show(ui, |ui| f(ui)).inner
        })
    }

    /// One pass of `f` on the root Ui; returns its result and the primitives drawn.
    pub fn frame_root<R>(
        &mut self,
        events: Vec<Event>,
        f: impl FnMut(&mut Ui) -> R,
    ) -> (R, Vec<ClippedPrimitive>) {
        let (out, shapes) = self.frame_shapes(events, f);
        (out, self.ctx.tessellate(shapes, self.ppp))
    }

    /// One pass of `f` on the root Ui; returns its result and the shapes drawn, untessellated.
    pub fn frame_shapes<R>(
        &mut self,
        events: Vec<Event>,
        mut f: impl FnMut(&mut Ui) -> R,
    ) -> (R, Vec<ClippedShape>) {
        let mut out = None;
        let mut full = self.ctx.run_ui(self.input(events), |ui| out = Some(f(ui)));
        self.fold(&mut full.textures_delta);
        (
            out.expect("the ui closure ran"),
            std::mem::take(&mut full.shapes),
        )
    }

    /// Runs `passes` frames of `f` and rasterises the last; returns its result and the image.
    pub fn render<R>(
        &mut self,
        passes: usize,
        mut f: impl FnMut(&mut Ui) -> R,
    ) -> (R, image::RgbaImage) {
        let mut last = None;
        for _ in 0..passes.max(1) {
            last = Some(self.frame(Vec::new(), &mut f));
        }
        let (out, prims) = last.expect("at least one pass");
        (out, self.rasterize(&prims))
    }

    /// Rasterises primitives from [`Self::frame`] or [`Self::frame_root`].
    pub fn image(&self, prims: &[ClippedPrimitive]) -> image::RgbaImage {
        self.rasterize(prims)
    }

    /// Applies a frame's texture uploads and the previous frame's frees, emptying the delta.
    fn fold(&mut self, delta: &mut TexturesDelta) {
        for id in self.freed.drain(..) {
            self.textures.remove(&id);
        }
        for (id, deltas) in delta.set.drain() {
            for set in deltas {
                let ImageData::Color(image) = set.image;
                match set.pos {
                    None => {
                        self.textures.insert(id, (*image).clone());
                    }
                    Some([x0, y0]) => {
                        if let Some(tex) = self.textures.get_mut(&id) {
                            let w = tex.size[0];
                            for y in 0..image.size[1] {
                                for x in 0..image.size[0] {
                                    tex.pixels[(y0 + y) * w + x0 + x] =
                                        image.pixels[y * image.size[0] + x];
                                }
                            }
                        }
                    }
                }
            }
        }
        self.freed.extend(delta.free.drain());
    }

    /// Premultiplied gamma-space blend of every triangle.
    fn rasterize(&self, prims: &[ClippedPrimitive]) -> image::RgbaImage {
        let w = (self.size.x * self.ppp).round() as usize;
        let h = (self.size.y * self.ppp).round() as usize;
        let mut buf = vec![[0.0_f32; 4]; w * h];
        for prim in prims {
            let Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            let tex = self.textures.get(&mesh.texture_id);
            let clip = Rect::from_min_max(
                (prim.clip_rect.min.to_vec2() * self.ppp).to_pos2(),
                (prim.clip_rect.max.to_vec2() * self.ppp).to_pos2(),
            );
            for tri in mesh.indices.as_chunks::<3>().0 {
                let v = tri.map(|i| mesh.vertices[i as usize]);
                fill_triangle(&mut buf, w, h, clip, self.ppp, v, tex);
            }
        }
        let mut img = image::RgbaImage::new(w as u32, h as u32);
        for (i, p) in buf.iter().enumerate() {
            let px = p.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
            img.put_pixel(
                (i % w) as u32,
                (i / w) as u32,
                image::Rgba([px[0], px[1], px[2], 255]),
            );
        }
        img
    }
}

fn fill_triangle(
    buf: &mut [[f32; 4]],
    w: usize,
    h: usize,
    clip: Rect,
    ppp: f32,
    v: [Vertex; 3],
    tex: Option<&ColorImage>,
) {
    let mut p = v.map(|v| v.pos.to_vec2() * ppp);
    let mut v = v;
    let mut area = cross(p[0], p[1], p[2]);
    if area.abs() < 1e-9 {
        return;
    }
    if area < 0.0 {
        p.swap(1, 2);
        v.swap(1, 2);
        area = -area;
    }
    let lo_x = p
        .iter()
        .map(|q| q.x)
        .fold(f32::INFINITY, f32::min)
        .max(clip.min.x)
        .max(0.0);
    let hi_x = p
        .iter()
        .map(|q| q.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .min(clip.max.x)
        .min(w as f32);
    let lo_y = p
        .iter()
        .map(|q| q.y)
        .fold(f32::INFINITY, f32::min)
        .max(clip.min.y)
        .max(0.0);
    let hi_y = p
        .iter()
        .map(|q| q.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .min(clip.max.y)
        .min(h as f32);
    if lo_x >= hi_x || lo_y >= hi_y {
        return;
    }
    let edges = [(p[1], p[2]), (p[2], p[0]), (p[0], p[1])];
    for py in lo_y.floor() as usize..(hi_y.ceil() as usize).min(h) {
        for px in lo_x.floor() as usize..(hi_x.ceil() as usize).min(w) {
            let q = Vec2::new(px as f32 + 0.5, py as f32 + 0.5);
            if q.x < clip.min.x || q.x >= clip.max.x || q.y < clip.min.y || q.y >= clip.max.y {
                continue;
            }
            let e = edges.map(|(a, b)| cross(a, b, q));
            let inside = e
                .iter()
                .zip(edges)
                .all(|(&e, (a, b))| e > 0.0 || (e == 0.0 && top_left(a, b)));
            if !inside {
                continue;
            }
            let [w0, w1, w2] = e.map(|e| e / area);
            let lerp = |f: fn(Color32) -> u8| {
                (f(v[0].color) as f32 * w0 + f(v[1].color) as f32 * w1 + f(v[2].color) as f32 * w2)
                    / 255.0
            };
            let mut src = [
                lerp(|c| c.r()),
                lerp(|c| c.g()),
                lerp(|c| c.b()),
                lerp(|c| c.a()),
            ];
            if let Some(t) = tex {
                let u = v[0].uv.x * w0 + v[1].uv.x * w1 + v[2].uv.x * w2;
                let vv = v[0].uv.y * w0 + v[1].uv.y * w1 + v[2].uv.y * w2;
                let texel = sample(t, u, vv);
                for (s, t) in src.iter_mut().zip(texel) {
                    *s *= t;
                }
            }
            let d = &mut buf[py * w + px];
            for k in 0..4 {
                d[k] = src[k] + d[k] * (1.0 - src[3]);
            }
        }
    }
}

/// Twice the signed area of `a b c`; positive when `c` is clockwise of `a → b` on screen.
fn cross(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Whether edge `a → b` of a clockwise triangle is a top or left edge.
fn top_left(a: Vec2, b: Vec2) -> bool {
    (a.y == b.y && b.x < a.x) || b.y < a.y
}

/// Bilinear sample of a premultiplied texture at normalised `(u, v)`.
fn sample(t: &ColorImage, u: f32, v: f32) -> [f32; 4] {
    let (tw, th) = (t.size[0], t.size[1]);
    let x = (u * tw as f32 - 0.5).clamp(0.0, tw as f32 - 1.0);
    let y = (v * th as f32 - 0.5).clamp(0.0, th as f32 - 1.0);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| {
        let c = t.pixels[y * tw + x];
        [c.r(), c.g(), c.b(), c.a()].map(|v| v as f32 / 255.0)
    };
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bottom = c[k] + (d[k] - c[k]) * fx;
        top + (bottom - top) * fy
    })
}
