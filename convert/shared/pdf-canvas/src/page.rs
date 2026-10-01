use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::ops::Range;
use std::rc::Rc;

use crate::font::Shaped;
use crate::{Fonts, ImageId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const BLACK: Self = Self(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self(1.0, 1.0, 1.0);

    #[must_use]
    pub fn from_u8(r: u8, g: u8, b: u8) -> Self {
        Self(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
        )
    }

    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(Self::from_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub color: Rgb,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

impl TextStyle {
    #[must_use]
    pub const fn new(size: f32, color: Rgb) -> Self {
        Self {
            size,
            color,
            fake_bold: false,
            fake_italic: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt = 0,
    Round = 1,
    Square = 2,
}

pub(crate) fn num(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let mut text = format!("{value:.4}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    if text == "-0" {
        text = "0".to_owned();
    }
    text
}

pub struct Page {
    pub width: f32,
    pub height: f32,
    pub(crate) content: String,
    pub(crate) fonts_used: BTreeSet<usize>,
    pub(crate) images_used: BTreeSet<usize>,
    pub(crate) alphas: BTreeSet<(u8, u8)>,
    pub(crate) links: Vec<([f32; 4], String)>,
    pub(crate) fonts: Rc<RefCell<Fonts>>,
    spanned: bool,
    pub(crate) patterns: Vec<String>,
    pub(crate) soft_masks: Vec<crate::gradient::SoftMask>,
}

impl Page {
    pub(crate) fn new(width: f32, height: f32, fonts: Rc<RefCell<Fonts>>) -> Self {
        Self {
            width,
            height,
            content: String::new(),
            fonts_used: BTreeSet::new(),
            images_used: BTreeSet::new(),
            alphas: BTreeSet::new(),
            links: Vec::new(),
            fonts,
            spanned: false,
            patterns: Vec::new(),
            soft_masks: Vec::new(),
        }
    }

    fn op(&mut self, text: &str) -> &mut Self {
        self.content.push_str(text);
        self.content.push('\n');
        self
    }

    pub fn raw(&mut self, operators: &str) -> &mut Self {
        self.op(operators)
    }

    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn save(&mut self) -> &mut Self {
        self.op("q")
    }

    pub fn restore(&mut self) -> &mut Self {
        self.op("Q")
    }

    pub fn transform(&mut self, m: [f32; 6]) -> &mut Self {
        let text = format!(
            "{} {} {} {} {} {} cm",
            num(m[0]),
            num(m[1]),
            num(m[2]),
            num(m[3]),
            num(m[4]),
            num(m[5])
        );
        self.op(&text)
    }

    pub fn set_fill(&mut self, c: Rgb) -> &mut Self {
        let text = format!("{} {} {} rg", num(c.0), num(c.1), num(c.2));
        self.op(&text)
    }

    pub fn set_stroke(&mut self, c: Rgb) -> &mut Self {
        let text = format!("{} {} {} RG", num(c.0), num(c.1), num(c.2));
        self.op(&text)
    }

    pub fn set_line_width(&mut self, width: f32) -> &mut Self {
        let text = format!("{} w", num(width));
        self.op(&text)
    }

    pub fn set_cap(&mut self, cap: Cap) -> &mut Self {
        let text = format!("{} J", cap as u8);
        self.op(&text)
    }

    pub fn set_dash(&mut self, pattern: &[f32], phase: f32) -> &mut Self {
        let parts: Vec<String> = pattern.iter().map(|v| num(*v)).collect();
        let text = format!("[{}] {} d", parts.join(" "), num(phase));
        self.op(&text)
    }

    pub fn set_alpha(&mut self, fill: f32, stroke: f32) -> &mut Self {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let key = (q(fill), q(stroke));
        self.alphas.insert(key);
        let text = format!("/A{}_{} gs", key.0, key.1);
        self.op(&text)
    }

    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        let text = format!("{} {} m", num(x), num(y));
        self.op(&text)
    }

    pub fn line_to(&mut self, x: f32, y: f32) -> &mut Self {
        let text = format!("{} {} l", num(x), num(y));
        self.op(&text)
    }

    pub fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) -> &mut Self {
        let text = format!(
            "{} {} {} {} {} {} c",
            num(x1),
            num(y1),
            num(x2),
            num(y2),
            num(x),
            num(y)
        );
        self.op(&text)
    }

    pub fn close(&mut self) -> &mut Self {
        self.op("h")
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        let text = format!("{} {} {} {} re", num(x), num(y), num(w), num(h));
        self.op(&text)
    }

    pub fn ellipse(&mut self, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        const K: f32 = 0.552_284_8;
        let (rx, ry) = (w / 2.0, h / 2.0);
        let (cx, cy) = (x + rx, y + ry);
        self.move_to(cx + rx, cy);
        self.curve_to(cx + rx, cy + K * ry, cx + K * rx, cy + ry, cx, cy + ry);
        self.curve_to(cx - K * rx, cy + ry, cx - rx, cy + K * ry, cx - rx, cy);
        self.curve_to(cx - rx, cy - K * ry, cx - K * rx, cy - ry, cx, cy - ry);
        self.curve_to(cx + K * rx, cy - ry, cx + rx, cy - K * ry, cx + rx, cy);
        self.close()
    }

    pub fn fill(&mut self) -> &mut Self {
        self.op("f")
    }

    pub fn fill_even_odd(&mut self) -> &mut Self {
        self.op("f*")
    }

    pub fn stroke(&mut self) -> &mut Self {
        self.op("S")
    }

    pub fn fill_stroke(&mut self) -> &mut Self {
        self.op("B")
    }

    pub fn clip(&mut self) -> &mut Self {
        self.op("W n")
    }

    pub fn end_path(&mut self) -> &mut Self {
        self.op("n")
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) -> &mut Self {
        self.save()
            .set_fill(color)
            .rect(x, y, w, h)
            .fill()
            .restore()
    }

    pub fn stroke_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        width: f32,
        color: Rgb,
    ) -> &mut Self {
        self.save()
            .set_stroke(color)
            .set_line_width(width)
            .rect(x, y, w, h)
            .stroke()
            .restore()
    }

    pub fn stroke_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: Rgb,
    ) -> &mut Self {
        self.save()
            .set_stroke(color)
            .set_line_width(width)
            .move_to(x1, y1)
            .line_to(x2, y2)
            .stroke()
            .restore()
    }

    pub fn image(&mut self, image: ImageId, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        self.images_used.insert(image.0);
        let text = format!(
            "q {} 0 0 {} {} {} cm /Im{} Do Q",
            num(w),
            num(h),
            num(x),
            num(y),
            image.0
        );
        self.op(&text)
    }

    pub fn link(&mut self, x: f32, y: f32, w: f32, h: f32, uri: &str) -> &mut Self {
        self.links.push(([x, y, x + w, y + h], uri.to_owned()));
        self
    }

    pub fn begin_actual_text(&mut self, text: &str) -> &mut Self {
        let mut hex = String::from("/Span << /ActualText <FEFF");
        for unit in text.encode_utf16() {
            let _ = write!(hex, "{unit:04X}");
        }
        hex.push_str("> >> BDC");
        self.spanned = true;
        self.op(&hex)
    }

    pub fn end_actual_text(&mut self) -> &mut Self {
        self.spanned = false;
        self.op("EMC")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show_as(
        &mut self,
        text: &str,
        shaped: &Shaped,
        clusters: Range<usize>,
        style: &TextStyle,
        x: f32,
        y: f32,
        word_spacing: f32,
    ) -> f32 {
        let was = self.spanned;
        self.spanned = true;
        let start = self.content.len();
        let drawn = self.show(shaped, clusters, style, x, y, word_spacing);
        self.spanned = was;
        let written = &self.content[start..];
        if let (Some(bt), Some(et)) = (written.find(" BT "), written.rfind(" ET Q")) {
            let mut span = String::from(" /Span << /ActualText <FEFF");
            for unit in text.encode_utf16() {
                let _ = write!(span, "{unit:04X}");
            }
            span.push_str("> >> BDC");
            let (bt, et) = (start + bt + 3, start + et);
            self.content.insert_str(et, " EMC");
            self.content.insert_str(bt, &span);
        }
        drawn
    }

    pub fn show(
        &mut self,
        shaped: &Shaped,
        clusters: Range<usize>,
        style: &TextStyle,
        x: f32,
        y: f32,
        word_spacing: f32,
    ) -> f32 {
        let font = shaped.font.0;
        self.fonts_used.insert(font);
        let upem = f64::from(shaped.units_per_em.max(1));
        let size = f64::from(style.size);
        let extra_units = f64::from(word_spacing) * upem / size.max(0.001);
        let mut out = String::with_capacity(64 + clusters.len() * 12);
        let c = style.color;
        let _ = write!(
            out,
            "q BT /F{font} {} Tf {} {} {} rg",
            num(style.size),
            num(c.0),
            num(c.1),
            num(c.2)
        );
        if style.fake_bold {
            let _ = write!(
                out,
                " {} {} {} RG {} w 2 Tr",
                num(c.0),
                num(c.1),
                num(c.2),
                num(style.size * 0.03)
            );
        }
        let skew = if style.fake_italic { 0.21 } else { 0.0 };
        let _ = write!(out, " 1 0 {} 1 {} {} Tm [", num(skew), num(x), num(y));
        let mut fonts = self.fonts.borrow_mut();
        let mut logical = 0.0_f64;
        let mut pdf_pen = 0.0_f64;
        let mut rise = 0_i32;
        for cluster in &shaped.clusters[clusters] {
            let spanned = cluster.glyphs.len() > 1 && !self.spanned;
            if spanned {
                let text = &shaped.text[cluster.range.clone()];
                out.push_str("] TJ /Span << /ActualText <FEFF");
                for unit in text.encode_utf16() {
                    let _ = write!(out, "{unit:04X}");
                }
                out.push_str("> >> BDC [");
            }
            let order = draw_order(cluster);
            for (step, &at) in order.iter().enumerate() {
                let glyph = &cluster.glyphs[at];
                if cluster.space && extra_units == 0.0 && glyph.gid == 0 {
                    continue;
                }
                if glyph.y != rise {
                    rise = glyph.y;
                    let _ = write!(
                        out,
                        "] TJ {} Ts [",
                        num((f64::from(rise) * size / upem) as f32)
                    );
                }
                let desired = logical + f64::from(glyph.x);
                let shift = desired - pdf_pen;
                if shift.abs() > 0.05 {
                    let _ = write!(out, "{} ", num((-shift * 1000.0 / upem) as f32));
                }
                let natural = fonts.advance(font, glyph.gid);
                let width = if spanned && step + 1 == order.len() {
                    (f64::from(cluster.advance) - f64::from(glyph.x))
                        .round()
                        .max(0.0) as i32
                } else {
                    natural
                };
                let (cid, width) = fonts.cid(font, glyph.gid, &glyph.meaning, width);
                let _ = write!(out, "<{cid:04X}>");
                pdf_pen = desired + f64::from(width);
            }
            if spanned {
                out.push_str("] TJ EMC [");
            }
            logical += f64::from(cluster.advance);
            if cluster.space {
                logical += extra_units;
            }
        }
        drop(fonts);
        out.push_str("] TJ ET Q");
        self.op(&out);
        (logical * size / upem) as f32
    }
}

fn draw_order(cluster: &crate::font::ShapedCluster) -> Vec<usize> {
    let n = cluster.glyphs.len();
    if n < 2 {
        return (0..n).collect();
    }
    let glyphs = &cluster.glyphs;
    let start = (0..n)
        .filter(|&k| glyphs[k].y == 0)
        .min_by_key(|&k| glyphs[k].x)
        .unwrap_or(0);
    let mut order = vec![start];
    order.extend((0..n).filter(|&k| k != start));
    order
}
