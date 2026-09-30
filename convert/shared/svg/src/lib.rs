#![forbid(unsafe_code)]

mod compile;
pub mod css;
pub mod path;
pub mod style;
pub mod values;
pub mod xml;

use std::fmt::Write as _;

use convert_pdf_canvas::{ImageInfo, Page, Rgb};

use crate::compile::{Brush, Compiler, Op, StrokeStyle};
use crate::path::{Path, Seg};
use crate::values::{Aspect, Matrix, aspect, then, view_box, view_box_matrix};

pub const PX: f32 = 0.75;

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub pieces: Vec<Piece>,
    pub rtl: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub colour: [u8; 3],
    pub letter_spacing: f32,
    pub underline: bool,
    pub strike: bool,
}

pub trait TextPainter {
    fn width(&self, index: usize) -> f32;
    fn draw(&self, page: &mut Page, index: usize);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgError(pub &'static str);

impl std::fmt::Display for SvgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SVG not readable: {}", self.0)
    }
}

impl std::error::Error for SvgError {}

#[must_use]
pub fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with('<') && text.to_ascii_lowercase().contains("<svg")
}

fn root_of(bytes: &[u8]) -> Option<xml::Element> {
    let text = String::from_utf8_lossy(bytes);
    let root = xml::parse(&text)?;
    (root.name == "svg").then_some(root)
}

#[must_use]
pub fn size(bytes: &[u8]) -> Option<(f32, f32)> {
    Some(size_of(&root_of(bytes)?))
}

fn absolute(value: Option<&str>) -> Option<f32> {
    match values::length(value?)? {
        values::Length::User(v) if v > 0.0 => Some(v),
        values::Length::Em(v) if v > 0.0 => Some(v * 16.0),
        values::Length::Ex(v) if v > 0.0 => Some(v * 8.0),
        _ => None,
    }
}

fn size_of(root: &xml::Element) -> (f32, f32) {
    let w = absolute(root.attr("width"));
    let h = absolute(root.attr("height"));
    let vb = view_box(root.attr("viewbox"));
    match (w, h, vb) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(vb)) => (w, w * vb[3] / vb[2]),
        (None, Some(h), Some(vb)) => (h * vb[2] / vb[3], h),
        (Some(w), None, None) => (w, 150.0),
        (None, Some(h), None) => (300.0, h),
        (None, None, Some(vb)) => (300.0, 300.0 * vb[3] / vb[2]),
        (None, None, None) => (300.0, 150.0),
    }
}

#[must_use]
pub fn data_uri(href: &str) -> Option<Vec<u8>> {
    let rest = href.trim().strip_prefix("data:")?;
    let (head, payload) = rest.split_once(',')?;
    if head.to_ascii_lowercase().ends_with(";base64") {
        base64(payload)
    } else {
        Some(percent_decode(payload))
    }
}

fn percent_decode(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

#[must_use]
pub fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0_u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(T[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn mime_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if is_svg(bytes) {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

#[must_use]
pub fn embed_files(bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> Vec<u8> {
    let Some(mut root) = root_of(bytes) else {
        return bytes.to_vec();
    };
    let mut changed = false;
    embed_in(&mut root, resolve, &mut changed, 0);
    if changed {
        root.write().into_bytes()
    } else {
        bytes.to_vec()
    }
}

fn embed_in(
    e: &mut xml::Element,
    resolve: &dyn Fn(&str) -> Option<Vec<u8>>,
    changed: &mut bool,
    depth: usize,
) {
    if depth > xml::MOST_DEPTH {
        return;
    }
    if e.name == "image"
        && let Some(href) = e.attr("href").map(str::to_owned)
        && !href.starts_with("data:")
        && !href.starts_with("http:")
        && !href.starts_with("https:")
        && !href.starts_with("//")
        && let Some(file) = resolve(&href)
    {
        let uri = format!("data:{};base64,{}", mime_of(&file), base64_encode(&file));
        e.set("href", &uri);
        *changed = true;
    }
    for child in &mut e.children {
        if let xml::Child::Element(c) = child {
            embed_in(c, resolve, changed, depth + 1);
        }
    }
}

pub struct Drawing {
    ops: Vec<Op>,
    segments: Vec<Segment>,
    images: Vec<Vec<u8>>,
    notes: Vec<String>,
    pub width: f32,
    pub height: f32,
    view_box: Option<[f32; 4]>,
    aspect: Aspect,
}

impl Drawing {
    pub fn parse(bytes: &[u8]) -> Result<Self, SvgError> {
        let root = root_of(bytes).ok_or(SvgError("no <svg> element"))?;
        let (width, height) = size_of(&root);
        let view_box = view_box(root.attr("viewbox"));
        let viewport = view_box.map_or((width, height), |vb| (vb[2], vb[3]));
        let mut compiler = Compiler::new(&root, "");
        compiler.root(&root, viewport, &style::Style::default());
        Ok(Self {
            ops: compiler.ops,
            segments: compiler.segments,
            images: compiler.images,
            notes: compiler.notes.iter().map(|n| (*n).to_owned()).collect(),
            width,
            height,
            view_box,
            aspect: aspect(root.attr("preserveaspectratio")),
        })
    }

    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    #[must_use]
    pub fn images(&self) -> &[Vec<u8>] {
        &self.images
    }

    #[must_use]
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn draw(
        &self,
        page: &mut Page,
        rect: [f32; 4],
        text: &dyn TextPainter,
        images: &[Option<ImageInfo>],
    ) {
        let [x, y, w, h] = rect;
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        page.save();
        page.rect(x, y, w, h).clip();
        let placement: Matrix = [PX, 0.0, 0.0, -PX, x, y + h];
        let root = self.view_box.map_or(values::IDENTITY, |vb| {
            view_box_matrix(vb, [0.0, 0.0, w / PX, h / PX], self.aspect)
        });
        let start = then(root, placement);
        if !values::usable(start) {
            page.restore();
            return;
        }
        page.transform(start);
        let mut replay = Replay {
            page,
            ctm: start,
            stack: Vec::new(),
            pen: (0.0, 0.0),
        };
        for op in &self.ops {
            replay.op(op, text, images);
        }
        for _ in 0..replay.stack.len() {
            replay.page.restore();
        }
        replay.page.restore();
    }
}

fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let mut t = format!("{v:.4}");
    while t.ends_with('0') {
        t.pop();
    }
    if t.ends_with('.') {
        t.pop();
    }
    if t == "-0" {
        t = "0".into();
    }
    t
}

fn path_text(path: &Path) -> String {
    let mut out = String::with_capacity(path.len() * 16);
    for s in path {
        let _ = match *s {
            Seg::Move(x, y) => write!(out, "{} {} m ", num(x), num(y)),
            Seg::Line(x, y) => write!(out, "{} {} l ", num(x), num(y)),
            Seg::Cubic(a, b, c, d, e, f) => write!(
                out,
                "{} {} {} {} {} {} c ",
                num(a),
                num(b),
                num(c),
                num(d),
                num(e),
                num(f)
            ),
            Seg::Close => write!(out, "h "),
        };
    }
    out
}

struct Replay<'p> {
    page: &'p mut Page,
    ctm: Matrix,
    stack: Vec<Matrix>,
    pen: (f32, f32),
}

impl Replay<'_> {
    fn op(&mut self, op: &Op, text: &dyn TextPainter, images: &[Option<ImageInfo>]) {
        match op {
            Op::Save => {
                self.stack.push(self.ctm);
                self.page.save();
            }
            Op::Restore => {
                if let Some(m) = self.stack.pop() {
                    self.ctm = m;
                    self.page.restore();
                }
            }
            Op::Transform(m) => {
                self.ctm = then(*m, self.ctm);
                self.page.transform(*m);
            }
            Op::Clip { path, even_odd } => {
                let ops = path_text(path);
                self.page
                    .raw(&format!("{ops}{}", if *even_odd { "W* n" } else { "W n" }));
            }
            Op::Shape {
                path,
                bbox,
                fill,
                stroke,
            } => self.shape(path, *bbox, fill.as_ref(), stroke.as_ref()),
            Op::TextStart => self.pen = (0.0, 0.0),
            Op::Text {
                x,
                y,
                anchor,
                parts,
            } => {
                let widths: Vec<f32> = parts.iter().map(|p| text.width(p.segment)).collect();
                let total: f32 = parts.iter().zip(&widths).map(|(p, w)| p.dx + w).sum();
                let shift = match anchor {
                    1 => total / 2.0,
                    2 => total,
                    _ => 0.0,
                };
                let mut pos = (x.unwrap_or(self.pen.0) - shift, y.unwrap_or(self.pen.1));
                for (part, w) in parts.iter().zip(&widths) {
                    pos.0 += part.dx;
                    pos.1 += part.dy;
                    if part.visible {
                        self.page.save();
                        if part.alpha < 0.999 {
                            self.page.set_alpha(part.alpha, part.alpha);
                        }
                        self.page.transform([1.0, 0.0, 0.0, -1.0, pos.0, pos.1]);
                        text.draw(self.page, part.segment);
                        self.page.restore();
                    }
                    pos.0 += w;
                }
                self.pen = pos;
            }
            Op::Image {
                index,
                viewport,
                aspect,
            } => {
                let Some(Some(info)) = images.get(*index) else {
                    return;
                };
                let (iw, ih) = (info.width as f32, info.height as f32);
                let [vx, vy, vw, vh] = *viewport;
                if !(vw > 0.0 && vh > 0.0 && iw > 0.0 && ih > 0.0) {
                    return;
                }
                let fit = view_box_matrix([0.0, 0.0, iw, ih], *viewport, *aspect);
                let m = then([iw, 0.0, 0.0, -ih, 0.0, ih], fit);
                if !values::usable(m) {
                    return;
                }
                self.page.save();
                if aspect.slice {
                    self.page.rect(vx, vy, vw, vh).clip();
                }
                self.page.transform(m);
                self.page.image(info.id, 0.0, 0.0, 1.0, 1.0);
                self.page.restore();
            }
        }
    }

    fn brush(&mut self, brush: &Brush, fill: bool, bbox: [f32; 4]) -> f32 {
        match brush {
            Brush::Solid(c) => {
                let rgb = Rgb(c.r, c.g, c.b);
                if fill {
                    self.page.set_fill(rgb);
                } else {
                    self.page.set_stroke(rgb);
                }
                c.a
            }
            Brush::Gradient {
                colours,
                alpha,
                matrix,
                opacity,
            } => {
                if let Some(alpha) = alpha {
                    self.page.set_soft_mask(alpha, *matrix, bbox);
                }
                let page_matrix = then(*matrix, self.ctm);
                if fill {
                    self.page.set_fill_gradient(colours, page_matrix);
                } else {
                    self.page.set_stroke_gradient(colours, page_matrix);
                }
                *opacity
            }
        }
    }

    fn stroke_style(&mut self, s: &StrokeStyle) {
        self.page.set_line_width(s.width);
        if s.cap != 0 {
            self.page.raw(&format!("{} J", s.cap));
        }
        if s.join != 0 {
            self.page.set_join(s.join);
        }
        if (s.miter - 10.0).abs() > 1e-3 {
            self.page.set_miter_limit(s.miter);
        }
        if let Some((dash, offset)) = &s.dash {
            self.page.set_dash(dash, *offset);
        }
    }

    fn shape(
        &mut self,
        path: &Path,
        bbox: [f32; 4],
        fill: Option<&(Brush, bool)>,
        stroke: Option<&(Brush, StrokeStyle)>,
    ) {
        let ops = path_text(path);
        if let (Some((Brush::Solid(f), even_odd)), Some((Brush::Solid(s), style))) = (fill, stroke)
        {
            self.page.save();
            self.brush(&Brush::Solid(*f), true, bbox);
            self.brush(&Brush::Solid(*s), false, bbox);
            self.stroke_style(style);
            if f.a < 0.999 || s.a < 0.999 {
                self.page.set_alpha(f.a, s.a);
            }
            self.page
                .raw(&format!("{ops}{}", if *even_odd { "B*" } else { "B" }));
            self.page.restore();
            return;
        }
        if let Some((brush, even_odd)) = fill {
            self.page.save();
            let a = self.brush(brush, true, bbox);
            if a < 0.999 {
                self.page.set_alpha(a, 1.0);
            }
            self.page
                .raw(&format!("{ops}{}", if *even_odd { "f*" } else { "f" }));
            self.page.restore();
        }
        if let Some((brush, style)) = stroke {
            let half = style.width / 2.0 * style.miter.max(1.0);
            let wide = [
                bbox[0] - half,
                bbox[1] - half,
                bbox[2] + half,
                bbox[3] + half,
            ];
            self.page.save();
            self.stroke_style(style);
            let a = self.brush(brush, false, wide);
            if a < 0.999 {
                self.page.set_alpha(1.0, a);
            }
            self.page.raw(&format!("{ops}S"));
            self.page.restore();
        }
    }
}

#[cfg(test)]
mod tests;
