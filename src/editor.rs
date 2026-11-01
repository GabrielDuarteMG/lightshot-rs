use crate::config::{self, Config};
use crate::raster::{self, Anno, Drawn, PixelRect, MARKER_ALPHA, TEXT_PX};
use eframe::egui::{
    self, Align2, Color32, FontId, Id, Key, Modifiers, Order, Painter, PointerButton, Pos2, Rect,
    Sense, Stroke, StrokeKind, Vec2, ViewportCommand,
};
use image::RgbaImage;
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const SWATCHES: [[u8; 3]; 8] = [
    [0xe7, 0x4c, 0x3c],
    [0xe6, 0x7e, 0x22],
    [0xf1, 0xc4, 0x0f],
    [0x2e, 0xcc, 0x71],
    [0x34, 0x98, 0xdb],
    [0x9b, 0x59, 0xb6],
    [0xec, 0xf0, 0xf1],
    [0x2c, 0x3e, 0x50],
];

const LINE_WIDTH: f32 = 2.0;
const MARKER_WIDTH: f32 = 14.0;
const HANDLE_SIZE: f32 = 8.0;
const MIN_SELECTION: i32 = 4;
const MIN_STROKE: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pen,
    Line,
    Arrow,
    Rect,
    Marker,
    Text,
}

const TOOLS: [(Tool, &str); 7] = [
    (Tool::Select, "Select"),
    (Tool::Pen, "Pen"),
    (Tool::Line, "Line"),
    (Tool::Arrow, "Arrow"),
    (Tool::Rect, "Rect"),
    (Tool::Marker, "Marker"),
    (Tool::Text, "Text"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sel {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Sel {
    fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x as f32
            && p[1] >= self.y as f32
            && p[0] <= (self.x + self.w) as f32
            && p[1] <= (self.y + self.h) as f32
    }

    fn rel(&self, p: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.x as f32).clamp(0.0, self.w as f32),
            (p[1] - self.y as f32).clamp(0.0, self.h as f32),
        ]
    }

    fn pixel_rect(&self) -> PixelRect {
        PixelRect {
            x: self.x.max(0) as u32,
            y: self.y.max(0) as u32,
            w: self.w.max(0) as u32,
            h: self.h.max(0) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Handle {
    l: bool,
    t: bool,
    r: bool,
    b: bool,
}

enum Drag {
    None,
    NewSel { start: [f32; 2], cur: [f32; 2] },
    Move { grab: [f32; 2] },
    Resize { handle: Handle, orig: Sel, orig_ann: Vec<Drawn> },
    Draw(Anno),
}

struct TextState {
    at: [f32; 2],
    s: String,
    focus: bool,
}

enum Action {
    Undo,
    Copy,
    Save,
    Close,
}

#[derive(Default)]
pub struct Outcome {
    pub clipboard: Option<RgbaImage>,
}

static PENDING: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

pub struct Editor {
    src: RgbaImage,
    tex: Option<egui::TextureHandle>,
    cfg: Config,
    tool: Tool,
    color: usize,
    sel: Option<Sel>,
    ann: Vec<Drawn>,
    drag: Drag,
    text: Option<TextState>,
    status: String,
    view: Rect,
    zoom: f32,
    clipboard: Option<arboard::Clipboard>,
    outcome: Rc<RefCell<Outcome>>,
}

pub fn run(src: RgbaImage, cfg: Config) -> Result<Outcome, String> {
    let outcome = Rc::new(RefCell::new(Outcome::default()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lightshot")
            .with_fullscreen(true)
            .with_decorations(false)
            .with_active(true),
        ..Default::default()
    };
    let shared = outcome.clone();
    eframe::run_native(
        "Lightshot",
        options,
        Box::new(move |_cc| Ok(Box::new(Editor::new(src, cfg, shared)))),
    )
    .map_err(|e| e.to_string())?;
    Ok(outcome.take())
}

pub fn wait_notifications(max: Duration) {
    let deadline = Instant::now() + max;
    let handles = std::mem::take(&mut *PENDING.lock().unwrap_or_else(|e| e.into_inner()));
    for h in handles {
        while !h.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

pub fn next_save(dir: &Path, index: u32, ext: &str) -> (PathBuf, u32) {
    let mut i = index.max(1);
    loop {
        let path = dir.join(format!("Screenshot_{i}.{ext}"));
        if !path.exists() || i == u32::MAX {
            return (path, i.saturating_add(1));
        }
        i += 1;
    }
}

fn write_image(img: &RgbaImage, path: &Path, jpeg: bool, quality: u8) -> image::ImageResult<()> {
    if jpeg {
        use image::buffer::ConvertBuffer;
        let rgb: image::RgbImage = img.convert();
        let out = std::io::BufWriter::new(std::fs::File::create(path)?);
        image::codecs::jpeg::JpegEncoder::new_with_quality(out, quality).encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
    } else {
        img.save_with_format(path, image::ImageFormat::Png)
    }
}

fn notify_saved(path: &Path) -> Result<(), notify_rust::error::Error> {
    let body = format!(
        "Screenshot saved to {}. Click here to open in folder.",
        path.display()
    );
    let handle = notify_rust::Notification::new()
        .summary("Screenshot saved")
        .body(&body)
        .action("default", "Open folder")
        .action("open", "Open folder")
        .show()?;
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let t = std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            if action == "open" || action == "default" {
                let _ = std::process::Command::new("xdg-open").arg(&dir).status();
            }
        })
    });
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).push(t);
    Ok(())
}

fn handles(r: Rect) -> [(Handle, Pos2); 8] {
    let h = |l, t, r, b| Handle { l, t, r, b };
    let c = r.center();
    [
        (h(true, true, false, false), r.left_top()),
        (h(false, true, false, false), Pos2::new(c.x, r.top())),
        (h(false, true, true, false), r.right_top()),
        (h(false, false, true, false), Pos2::new(r.right(), c.y)),
        (h(false, false, true, true), r.right_bottom()),
        (h(false, false, false, true), Pos2::new(c.x, r.bottom())),
        (h(true, false, false, true), r.left_bottom()),
        (h(true, false, false, false), Pos2::new(r.left(), c.y)),
    ]
}

fn handle_rect(p: Pos2) -> Rect {
    Rect::from_center_size(p, Vec2::splat(HANDLE_SIZE))
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn color32(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
}

impl Editor {
    fn new(src: RgbaImage, cfg: Config, outcome: Rc<RefCell<Outcome>>) -> Self {
        Editor {
            src,
            tex: None,
            cfg,
            tool: Tool::Select,
            color: 0,
            sel: None,
            ann: Vec::new(),
            drag: Drag::None,
            text: None,
            status: String::new(),
            view: Rect::NOTHING,
            zoom: 1.0,
            clipboard: None,
            outcome,
        }
    }

    fn rgba_for(&self, tool: Tool) -> [u8; 4] {
        let [r, g, b] = SWATCHES[self.color];
        let a = if tool == Tool::Marker { MARKER_ALPHA } else { 255 };
        [r, g, b, a]
    }

    fn to_img(&self, p: Pos2) -> [f32; 2] {
        let v = (p - self.view.min) / self.zoom;
        [
            v.x.clamp(0.0, self.src.width() as f32),
            v.y.clamp(0.0, self.src.height() as f32),
        ]
    }

    fn to_screen(&self, p: [f32; 2]) -> Pos2 {
        self.view.min + Vec2::new(p[0], p[1]) * self.zoom
    }

    fn sel_screen(&self, s: Sel) -> Rect {
        Rect::from_min_size(
            self.to_screen([s.x as f32, s.y as f32]),
            Vec2::new(s.w as f32, s.h as f32) * self.zoom,
        )
    }

    fn sel_from(&self, a: [f32; 2], b: [f32; 2]) -> Sel {
        let (iw, ih) = (self.src.width() as i32, self.src.height() as i32);
        let x0 = (a[0].min(b[0]).round() as i32).clamp(0, iw);
        let y0 = (a[1].min(b[1]).round() as i32).clamp(0, ih);
        let x1 = (a[0].max(b[0]).round() as i32).clamp(0, iw);
        let y1 = (a[1].max(b[1]).round() as i32).clamp(0, ih);
        Sel { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.tex.is_some() {
            return;
        }
        let max = ctx.input(|i| i.max_texture_side) as u32;
        let (w, h) = self.src.dimensions();
        let shown: Cow<'_, RgbaImage> = if w > max || h > max {
            let s = max as f32 / w.max(h) as f32;
            Cow::Owned(image::imageops::resize(
                &self.src,
                ((w as f32 * s) as u32).clamp(1, max),
                ((h as f32 * s) as u32).clamp(1, max),
                image::imageops::FilterType::Triangle,
            ))
        } else {
            Cow::Borrowed(&self.src)
        };
        let ci = egui::ColorImage::from_rgba_unmultiplied(
            [shown.width() as usize, shown.height() as usize],
            shown.as_raw(),
        );
        self.tex = Some(ctx.load_texture("screenshot", ci, egui::TextureOptions::LINEAR));
    }

    fn layout(&mut self, screen: Rect) {
        let (iw, ih) = (self.src.width() as f32, self.src.height() as f32);
        let fit = (screen.width() / iw).min(screen.height() / ih);
        self.view = Rect::from_center_size(screen.center(), Vec2::new(iw, ih) * fit);
        self.zoom = self.view.width() / iw;
    }

    fn exit(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn handle_keys(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let editing = self.text.is_some();
        let (esc, enter, undo, copy, save) = ctx.input_mut(|i| {
            if editing {
                return (false, false, false, false, i.consume_key(Modifiers::COMMAND, Key::S));
            }
            let copy_event = i.events.iter().any(|e| matches!(e, egui::Event::Copy));
            (
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                copy_event || i.consume_key(Modifiers::COMMAND, Key::C),
                i.consume_key(Modifiers::COMMAND, Key::S),
            )
        });
        if esc {
            self.exit(ctx);
            return;
        }
        if undo {
            self.ann.pop();
        }
        if copy || enter {
            self.copy(ctx);
        }
        if save {
            self.save(ctx, frame);
        }
    }

    fn handle_pointer(&mut self, ctx: &egui::Context, resp: &egui::Response) {
        if resp.clicked_by(PointerButton::Secondary) && self.text.is_none() {
            self.exit(ctx);
            return;
        }
        let pos = resp
            .interact_pointer_pos()
            .or_else(|| ctx.input(|i| i.pointer.latest_pos()));
        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(p0) = ctx.input(|i| i.pointer.press_origin()).or(pos) {
                self.begin_drag(p0);
            }
        }
        if resp.dragged_by(PointerButton::Primary) || resp.drag_stopped_by(PointerButton::Primary) {
            if let Some(p) = pos {
                self.update_drag(p);
            }
        }
        if resp.drag_stopped_by(PointerButton::Primary) {
            self.end_drag(ctx);
        }
        if resp.clicked_by(PointerButton::Primary) && self.tool == Tool::Text {
            if let (Some(p), Some(sel)) = (pos, self.sel) {
                let ip = self.to_img(p);
                if sel.contains(ip) {
                    self.commit_text();
                    self.text = Some(TextState { at: sel.rel(ip), s: String::new(), focus: true });
                }
            }
        }
    }

    fn begin_drag(&mut self, p0: Pos2) {
        self.commit_text();
        let ip = self.to_img(p0);
        if let Some(sel) = self.sel {
            let sr = self.sel_screen(sel);
            if let Some((handle, _)) = handles(sr)
                .into_iter()
                .find(|(_, hp)| handle_rect(*hp).expand(2.0).contains(p0))
            {
                self.drag = Drag::Resize { handle, orig: sel, orig_ann: self.ann.clone() };
                return;
            }
            if sel.contains(ip) {
                let rel = sel.rel(ip);
                self.drag = match self.tool {
                    Tool::Select => Drag::Move { grab: [ip[0] - sel.x as f32, ip[1] - sel.y as f32] },
                    Tool::Text => Drag::None,
                    Tool::Pen => Drag::Draw(Anno::Pen(vec![rel])),
                    Tool::Marker => Drag::Draw(Anno::Marker(vec![rel])),
                    Tool::Line => Drag::Draw(Anno::Line { a: rel, b: rel }),
                    Tool::Arrow => Drag::Draw(Anno::Arrow { a: rel, b: rel }),
                    Tool::Rect => Drag::Draw(Anno::Rect { a: rel, b: rel }),
                };
                return;
            }
        }
        self.sel = None;
        self.ann.clear();
        self.text = None;
        self.drag = Drag::NewSel { start: ip, cur: ip };
    }

    fn update_drag(&mut self, p: Pos2) {
        let ip = self.to_img(p);
        let (iw, ih) = (self.src.width() as i32, self.src.height() as i32);
        match &mut self.drag {
            Drag::None => {}
            Drag::NewSel { cur, .. } => *cur = ip,
            Drag::Move { grab } => {
                if let Some(sel) = self.sel.as_mut() {
                    sel.x = ((ip[0] - grab[0]).round() as i32).clamp(0, (iw - sel.w).max(0));
                    sel.y = ((ip[1] - grab[1]).round() as i32).clamp(0, (ih - sel.h).max(0));
                }
            }
            Drag::Resize { handle, orig, orig_ann } => {
                let (mut l, mut t) = (orig.x, orig.y);
                let (mut r, mut b) = (orig.x + orig.w, orig.y + orig.h);
                let (px, py) = (ip[0].round() as i32, ip[1].round() as i32);
                if handle.l {
                    l = px;
                }
                if handle.r {
                    r = px;
                }
                if handle.t {
                    t = py;
                }
                if handle.b {
                    b = py;
                }
                let new = Sel { x: l.min(r), y: t.min(b), w: (r - l).abs(), h: (b - t).abs() };
                if new.w < 1 || new.h < 1 {
                    return;
                }
                let (dx, dy) = ((new.x - orig.x) as f32, (new.y - orig.y) as f32);
                let mut ann = orig_ann.clone();
                for d in &mut ann {
                    d.anno.map_points(|q| [q[0] - dx, q[1] - dy]);
                }
                let inside = ann.iter().all(|d| {
                    d.anno.points().iter().all(|q| {
                        q[0] >= 0.0 && q[1] >= 0.0 && q[0] <= new.w as f32 && q[1] <= new.h as f32
                    })
                });
                if inside {
                    self.sel = Some(new);
                    self.ann = ann;
                }
            }
            Drag::Draw(anno) => {
                let Some(sel) = self.sel else { return };
                let rel = sel.rel(ip);
                match anno {
                    Anno::Pen(pts) | Anno::Marker(pts) => {
                        if pts.last().is_none_or(|q| dist(*q, rel) >= 0.5) {
                            pts.push(rel);
                        }
                    }
                    Anno::Line { b, .. } | Anno::Arrow { b, .. } | Anno::Rect { b, .. } => *b = rel,
                    Anno::Text { .. } => {}
                }
            }
        }
    }

    fn end_drag(&mut self, ctx: &egui::Context) {
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::NewSel { start, cur } => {
                let sel = self.sel_from(start, cur);
                if sel.w >= MIN_SELECTION && sel.h >= MIN_SELECTION {
                    self.sel = Some(sel);
                    if self.cfg.auto_copy {
                        let img = raster::render(&self.src, sel.pixel_rect(), &[]);
                        match self.set_clipboard(img) {
                            Ok(()) => {
                                if let Err(e) = notify_rust::Notification::new()
                                    .summary("Screenshot copied to clipboard")
                                    .show()
                                {
                                    self.status = format!("notify: {e}");
                                }
                            }
                            Err(e) => self.status = format!("clipboard: {e}"),
                        }
                    }
                }
                ctx.request_repaint();
            }
            Drag::Draw(anno) => {
                let keep = match &anno {
                    Anno::Pen(p) | Anno::Marker(p) => p.len() >= 2,
                    Anno::Line { a, b } | Anno::Arrow { a, b } | Anno::Rect { a, b } => {
                        dist(*a, *b) >= MIN_STROKE
                    }
                    Anno::Text { .. } => false,
                };
                if keep {
                    self.ann.push(Drawn { rgba: self.rgba_for(self.tool), anno });
                }
            }
            _ => {}
        }
    }

    fn commit_text(&mut self) {
        if let Some(t) = self.text.take() {
            if !t.s.is_empty() {
                self.ann.push(Drawn {
                    rgba: self.rgba_for(Tool::Text),
                    anno: Anno::Text { at: t.at, s: t.s },
                });
            }
        }
    }

    fn text_ui(&mut self, ctx: &egui::Context) {
        let Some(sel) = self.sel else {
            self.text = None;
            return;
        };
        let Some(at) = self.text.as_ref().map(|t| t.at) else { return };
        let pos = self.to_screen([sel.x as f32 + at[0], sel.y as f32 + at[1]]);
        let color = color32(self.rgba_for(Tool::Text));
        let font = FontId::proportional(TEXT_PX * self.zoom);
        let width = (sel.w as f32 - at[0]).max(1.0) * self.zoom;
        let Some(t) = self.text.as_mut() else { return };
        let resp = egui::Area::new(Id::unique("lightshot-text"))
            .fixed_pos(pos)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut t.s)
                        .font(font)
                        .text_color(color)
                        .desired_width(width),
                )
            })
            .inner;
        if t.focus {
            resp.request_focus();
            t.focus = false;
        } else if resp.lost_focus() {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.text = None;
            } else {
                self.commit_text();
            }
        }
    }

    fn render_sel(&self) -> Option<RgbaImage> {
        let sel = self.sel?;
        Some(raster::render(&self.src, sel.pixel_rect(), &self.ann))
    }

    fn set_clipboard(&mut self, img: RgbaImage) -> Result<(), arboard::Error> {
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new()?);
        }
        if let Some(cb) = self.clipboard.as_mut() {
            cb.set_image(arboard::ImageData {
                width: img.width() as usize,
                height: img.height() as usize,
                bytes: Cow::Borrowed(img.as_raw()),
            })?;
        }
        self.outcome.borrow_mut().clipboard = Some(img);
        Ok(())
    }

    fn copy(&mut self, ctx: &egui::Context) {
        self.commit_text();
        let Some(img) = self.render_sel() else {
            self.status = "copy: no selection".into();
            return;
        };
        if let Err(e) = self.set_clipboard(img) {
            self.status = format!("clipboard: {e}");
            return;
        }
        if let Err(e) = notify_rust::Notification::new()
            .summary("Screenshot copied to clipboard")
            .show()
        {
            self.status = format!("notify: {e}");
            return;
        }
        self.status.clear();
        if self.cfg.auto_close {
            self.exit(ctx);
        }
    }

    fn save(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        self.commit_text();
        let Some(img) = self.render_sel() else {
            self.status = "save: no selection".into();
            return;
        };
        let last = Path::new(&self.cfg.last_saved_dir);
        let dir = if !self.cfg.last_saved_dir.is_empty() && last.is_dir() {
            last.to_path_buf()
        } else {
            config::pictures_dir()
        };
        let ext = if self.cfg.is_jpeg() { "jpg" } else { "png" };
        let (suggested, next) = next_save(&dir, self.cfg.save_index, ext);
        let name = suggested
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(path) = rfd::FileDialog::new()
            .set_parent(frame)
            .set_directory(&dir)
            .set_file_name(name)
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .save_file()
        else {
            return;
        };
        let jpeg = match path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .as_deref()
        {
            Some("jpg" | "jpeg") => true,
            Some("png") => false,
            _ => self.cfg.is_jpeg(),
        };
        if let Err(e) = write_image(&img, &path, jpeg, self.cfg.jpeg_quality) {
            self.status = format!("save {}: {e}", path.display());
            return;
        }
        self.status.clear();
        self.cfg.save_index = next;
        if let Some(parent) = path.parent() {
            self.cfg.last_saved_dir = parent.to_string_lossy().into_owned();
        }
        if let Err(e) = config::store(&self.cfg) {
            eprintln!("lightshot: config {}: {e}", config::config_path().display());
            self.status = format!("config: {e}");
        }
        if let Err(e) = notify_saved(&path) {
            self.status = format!("notify: {e}");
        }
        if self.cfg.auto_close {
            self.exit(ctx);
        }
    }

    fn paint_anno(&self, p: &Painter, origin: [f32; 2], d: &Drawn) {
        let z = self.zoom;
        let pt = |q: [f32; 2]| self.to_screen([origin[0] + q[0], origin[1] + q[1]]);
        let c = color32(d.rgba);
        let line = Stroke::new(LINE_WIDTH * z, c);
        match &d.anno {
            Anno::Pen(pts) => {
                p.line(pts.iter().map(|q| pt(*q)).collect(), line);
            }
            Anno::Marker(pts) => {
                p.line(pts.iter().map(|q| pt(*q)).collect(), Stroke::new(MARKER_WIDTH * z, c));
            }
            Anno::Line { a, b } => {
                p.line_segment([pt(*a), pt(*b)], line);
            }
            Anno::Arrow { a, b } => {
                p.line_segment([pt(*a), pt(*b)], line);
                if let Some(heads) = raster::arrow_heads(*a, *b) {
                    for e in heads {
                        p.line_segment([pt(*b), pt(e)], line);
                    }
                }
            }
            Anno::Rect { a, b } => {
                p.rect_stroke(Rect::from_two_pos(pt(*a), pt(*b)), 0.0, line, StrokeKind::Inside);
            }
            Anno::Text { at, s } => {
                p.text(pt(*at), Align2::LEFT_TOP, s, FontId::proportional(TEXT_PX * z), c);
            }
        }
    }

    fn paint(&self, p: &Painter, screen: Rect) {
        p.rect_filled(screen, 0.0, Color32::BLACK);
        if let Some(tex) = &self.tex {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            p.image(tex.id(), self.view, uv, Color32::WHITE);
        }
        let dim = Color32::from_black_alpha(110);
        let shown = match &self.drag {
            Drag::NewSel { start, cur } => Some(self.sel_from(*start, *cur)),
            _ => self.sel,
        };
        let Some(sel) = shown else {
            p.rect_filled(self.view, 0.0, dim);
            return;
        };
        let sr = self.sel_screen(sel);
        let v = self.view;
        for r in [
            Rect::from_min_max(v.min, Pos2::new(v.max.x, sr.min.y)),
            Rect::from_min_max(Pos2::new(v.min.x, sr.max.y), v.max),
            Rect::from_min_max(Pos2::new(v.min.x, sr.min.y), Pos2::new(sr.min.x, sr.max.y)),
            Rect::from_min_max(Pos2::new(sr.max.x, sr.min.y), Pos2::new(v.max.x, sr.max.y)),
        ] {
            if r.is_positive() {
                p.rect_filled(r, 0.0, dim);
            }
        }
        let clip = p.with_clip_rect(sr);
        let origin = [sel.x as f32, sel.y as f32];
        for d in &self.ann {
            self.paint_anno(&clip, origin, d);
        }
        if let Drag::Draw(anno) = &self.drag {
            let d = Drawn { rgba: self.rgba_for(self.tool), anno: anno.clone() };
            self.paint_anno(&clip, origin, &d);
        }
        p.rect_stroke(sr, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Outside);
        if self.sel.is_some() && !matches!(self.drag, Drag::NewSel { .. }) {
            for (_, hp) in handles(sr) {
                let hr = handle_rect(hp);
                p.rect_filled(hr, 0.0, Color32::WHITE);
                p.rect_stroke(hr, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Inside);
            }
        }
        p.text(
            sr.left_top() - Vec2::new(0.0, 4.0),
            Align2::LEFT_BOTTOM,
            format!("{} x {}", sel.w, sel.h),
            FontId::proportional(13.0),
            Color32::WHITE,
        );
    }

    fn toolbar(&mut self, ctx: &egui::Context) -> Option<Action> {
        let mut action = None;
        egui::Area::new(Id::unique("lightshot-toolbar"))
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -16.0))
            .order(Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (tool, name) in TOOLS {
                            if ui.selectable_label(self.tool == tool, name).clicked() {
                                self.commit_text();
                                self.tool = tool;
                            }
                        }
                        ui.separator();
                        for (i, [r, g, b]) in SWATCHES.into_iter().enumerate() {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                            ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(r, g, b));
                            if i == self.color {
                                ui.painter().rect_stroke(
                                    rect,
                                    3.0,
                                    Stroke::new(2.0, Color32::WHITE),
                                    StrokeKind::Outside,
                                );
                            }
                            if resp.clicked() {
                                self.color = i;
                            }
                        }
                        ui.separator();
                        for (label, act) in [
                            ("Undo", Action::Undo),
                            ("Copy", Action::Copy),
                            ("Save", Action::Save),
                            ("Close", Action::Close),
                        ] {
                            if ui.button(label).clicked() {
                                action = Some(act);
                            }
                        }
                    });
                    if !self.status.is_empty() {
                        ui.colored_label(Color32::LIGHT_RED, &self.status);
                    }
                });
            });
        action
    }
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ensure_texture(&ctx);
        let screen = ui.max_rect();
        self.layout(screen);
        self.handle_keys(&ctx, frame);
        let resp = ui.interact(screen, Id::unique("lightshot-canvas"), Sense::click_and_drag());
        self.handle_pointer(&ctx, &resp);
        self.paint(ui.painter(), screen);
        self.text_ui(&ctx);
        match self.toolbar(&ctx) {
            Some(Action::Undo) => {
                self.ann.pop();
            }
            Some(Action::Copy) => self.copy(&ctx),
            Some(Action::Save) => self.save(&ctx, frame),
            Some(Action::Close) => self.exit(&ctx),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_save_skips_existing() {
        let dir = std::env::temp_dir().join(format!("lightshot-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Screenshot_1.png"), b"x").unwrap();
        let (path, next) = next_save(&dir, 1, "png");
        assert_eq!(path, dir.join("Screenshot_2.png"));
        assert_eq!(next, 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
