use ab_glyph::{FontRef, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{
    draw_filled_circle_mut, draw_hollow_rect_mut, draw_line_segment_mut, draw_text_mut,
};
use imageproc::rect::Rect;
use std::sync::LazyLock;

const FONT_PATH: &str = "/usr/share/fonts/TTF/DejaVuSans.ttf";
pub const TEXT_PX: f32 = 20.0;
pub const MARKER_ALPHA: u8 = 96;
pub const MARKER_RADIUS: i32 = 7;
pub const ARROW_HEAD_LEN: f32 = 12.0;
pub const ARROW_HEAD_DEG: f32 = 25.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Anno {
    Pen(Vec<[f32; 2]>),
    Line { a: [f32; 2], b: [f32; 2] },
    Arrow { a: [f32; 2], b: [f32; 2] },
    Rect { a: [f32; 2], b: [f32; 2] },
    Marker(Vec<[f32; 2]>),
    Text { at: [f32; 2], s: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub rgba: [u8; 4],
    pub anno: Anno,
}

impl Anno {
    pub fn points(&self) -> Vec<[f32; 2]> {
        match self {
            Anno::Pen(p) | Anno::Marker(p) => p.clone(),
            Anno::Line { a, b } | Anno::Arrow { a, b } | Anno::Rect { a, b } => vec![*a, *b],
            Anno::Text { at, .. } => vec![*at],
        }
    }

    pub fn map_points(&mut self, f: impl Fn([f32; 2]) -> [f32; 2]) {
        match self {
            Anno::Pen(p) | Anno::Marker(p) => p.iter_mut().for_each(|q| *q = f(*q)),
            Anno::Line { a, b } | Anno::Arrow { a, b } | Anno::Rect { a, b } => {
                *a = f(*a);
                *b = f(*b);
            }
            Anno::Text { at, .. } => *at = f(*at),
        }
    }
}

pub fn arrow_heads(a: [f32; 2], b: [f32; 2]) -> Option<[[f32; 2]; 2]> {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = |deg: f32| {
        let (s, c) = deg.to_radians().sin_cos();
        [
            b[0] + ARROW_HEAD_LEN * (ux * c - uy * s),
            b[1] + ARROW_HEAD_LEN * (ux * s + uy * c),
        ]
    };
    Some([head(ARROW_HEAD_DEG), head(-ARROW_HEAD_DEG)])
}

static FONT_BYTES: LazyLock<Option<Vec<u8>>> = LazyLock::new(|| std::fs::read(FONT_PATH).ok());
static FONT: LazyLock<Option<FontRef<'static>>> =
    LazyLock::new(|| FontRef::try_from_slice(FONT_BYTES.as_deref()?).ok());

pub fn font() -> Option<&'static FontRef<'static>> {
    FONT.as_ref()
}

pub fn render(src: &RgbaImage, sel: PixelRect, ann: &[Drawn]) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    let x = sel.x.min(sw);
    let y = sel.y.min(sh);
    let w = sel.w.min(sw - x);
    let h = sel.h.min(sh - y);
    let mut out = image::imageops::crop_imm(src, x, y, w, h).to_image();
    if w == 0 || h == 0 {
        return out;
    }
    for d in ann {
        let c = Rgba(d.rgba);
        match &d.anno {
            Anno::Pen(pts) => polyline(&mut out, pts, c),
            Anno::Line { a, b } => polyline(&mut out, &[*a, *b], c),
            Anno::Arrow { a, b } => {
                segment(&mut out, *a, *b, c);
                if let Some(heads) = arrow_heads(*a, *b) {
                    for e in heads {
                        segment(&mut out, *b, e, c);
                    }
                }
            }
            Anno::Rect { a, b } => rect(&mut out, *a, *b, c),
            Anno::Marker(pts) => marker(&mut out, pts, Rgba([d.rgba[0], d.rgba[1], d.rgba[2], MARKER_ALPHA])),
            Anno::Text { at, s } => {
                if let Some(f) = font() {
                    draw_text_mut(&mut out, c, at[0].round() as i32, at[1].round() as i32, PxScale::from(TEXT_PX), f, s);
                }
            }
        }
    }
    out
}

fn segment(img: &mut RgbaImage, a: [f32; 2], b: [f32; 2], c: Rgba<u8>) {
    draw_line_segment_mut(img, (a[0], a[1]), (b[0], b[1]), c);
}

fn polyline(img: &mut RgbaImage, pts: &[[f32; 2]], c: Rgba<u8>) {
    for w in pts.windows(2) {
        segment(img, w[0], w[1], c);
    }
    for p in pts {
        draw_filled_circle_mut(img, (p[0].round() as i32, p[1].round() as i32), 1, c);
    }
}

fn rect(img: &mut RgbaImage, a: [f32; 2], b: [f32; 2], c: Rgba<u8>) {
    let x0 = a[0].min(b[0]).round() as i32;
    let y0 = a[1].min(b[1]).round() as i32;
    let w = (a[0].max(b[0]).round() as i32 - x0).max(1);
    let h = (a[1].max(b[1]).round() as i32 - y0).max(1);
    for inset in 0..=1 {
        let (iw, ih) = (w - 2 * inset, h - 2 * inset);
        if iw <= 0 || ih <= 0 {
            break;
        }
        draw_hollow_rect_mut(img, Rect::at(x0 + inset, y0 + inset).of_size(iw as u32, ih as u32), c);
    }
}

fn marker(img: &mut RgbaImage, pts: &[[f32; 2]], c: Rgba<u8>) {
    let (w, h) = img.dimensions();
    let mut mask = vec![false; w as usize * h as usize];
    let mut stamp = |cx: f32, cy: f32| {
        let (cx, cy) = (cx.round() as i32, cy.round() as i32);
        for dy in -MARKER_RADIUS..=MARKER_RADIUS {
            for dx in -MARKER_RADIUS..=MARKER_RADIUS {
                let (px, py) = (cx + dx, cy + dy);
                if dx * dx + dy * dy <= MARKER_RADIUS * MARKER_RADIUS
                    && px >= 0
                    && py >= 0
                    && (px as u32) < w
                    && (py as u32) < h
                {
                    mask[py as usize * w as usize + px as usize] = true;
                }
            }
        }
    };
    if let Some(p) = pts.first() {
        stamp(p[0], p[1]);
    }
    for s in pts.windows(2) {
        let (a, b) = (s[0], s[1]);
        let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let steps = len.ceil().max(1.0) as u32;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            stamp(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
        }
    }
    let alpha = f32::from(c[3]) / 255.0;
    for (i, covered) in mask.into_iter().enumerate() {
        if !covered {
            continue;
        }
        let p = img.get_pixel_mut(i as u32 % w, i as u32 / w);
        for k in 0..3 {
            p[k] = (f32::from(c[k]) * alpha + f32::from(p[k]) * (1.0 - alpha)).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_anno_changes_border_keeps_edge() {
        let red = Rgba([255, 0, 0, 255]);
        let src = RgbaImage::from_pixel(4, 4, red);
        let sel = PixelRect { x: 1, y: 1, w: 2, h: 2 };
        let ann = [Drawn {
            rgba: [0x34, 0x98, 0xdb, 255],
            anno: Anno::Rect { a: [1.0, 1.0], b: [2.0, 2.0] },
        }];
        let out = render(&src, sel, &ann);
        assert_eq!(out.dimensions(), (2, 2));
        assert_eq!(*out.get_pixel(1, 1), Rgba([0x34, 0x98, 0xdb, 255]));
        assert_eq!(*out.get_pixel(0, 0), red);
        assert_eq!(*out.get_pixel(1, 0), red);
        assert_eq!(*out.get_pixel(0, 1), red);
    }

    #[test]
    fn crop_is_clamped_and_marker_blends() {
        let src = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        let sel = PixelRect { x: 2, y: 2, w: 10, h: 10 };
        let ann = [Drawn {
            rgba: [255, 255, 255, 255],
            anno: Anno::Marker(vec![[0.0, 0.0], [1.0, 1.0]]),
        }];
        let out = render(&src, sel, &ann);
        assert_eq!(out.dimensions(), (2, 2));
        assert_eq!(out.get_pixel(0, 0).0, [96, 96, 96, 255]);
    }
}
