use convert_office_read::Element;
use convert_pdf_canvas::families::symbol_text;
use convert_pdf_canvas::{Direction, Span, layout_directed};

use crate::geometry::{emu, flag, num};
use crate::scene::{Frame, Inherit, Scene};
use crate::theme::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Spacing {
    Percent(f32),
    Points(f32),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunProps {
    pub(crate) size: Option<f32>,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) underline: Option<bool>,
    pub(crate) strike: Option<bool>,
    pub(crate) color: Option<Color>,
    pub(crate) latin: Option<String>,
    pub(crate) cs: Option<String>,
}

#[derive(Clone, Debug, Default)]
enum Bullet {
    #[default]
    None,
    Char(String),
    Number(String, u32),
}

#[derive(Clone, Debug, Default)]
struct ParaProps {
    align: Option<String>,
    rtl: Option<bool>,
    mar_l: Option<f32>,
    indent: Option<f32>,
    line: Option<Spacing>,
    before: Option<Spacing>,
    after: Option<Spacing>,
    bullet: Option<Bullet>,
    bullet_color: Option<Color>,
    bullet_size: Option<f32>,
    bullet_font: Option<String>,
    run: RunProps,
}

fn spacing(e: Option<&Element>) -> Option<Spacing> {
    let e = e?;
    if let Some(p) = e.child("spcPct") {
        return num(p, "val").map(|v| Spacing::Percent(v / 100_000.0));
    }
    e.child("spcPts")
        .and_then(|p| num(p, "val"))
        .map(|v| Spacing::Points(v / 100.0))
}

impl Scene<'_, '_> {
    pub(crate) fn merge_run(&self, props: &mut RunProps, e: &Element, placeholder: Option<Color>) {
        if let Some(sz) = num(e, "sz") {
            props.size = Some(sz / 100.0);
        }
        if let Some(b) = flag(e, "b") {
            props.bold = Some(b);
        }
        if let Some(i) = flag(e, "i") {
            props.italic = Some(i);
        }
        if let Some(u) = e.attr("u") {
            props.underline = Some(u != "none");
        }
        if let Some(s) = e.attr("strike") {
            props.strike = Some(s != "noStrike");
        }
        if let Some(fill) = e.child("solidFill")
            && let Some(c) = self.palette(placeholder).first_color(fill)
        {
            props.color = Some(c);
        }
        if let Some(latin) = e.child("latin").and_then(|l| l.attr("typeface")) {
            props.latin = Some(self.theme.font(latin));
        }
        if let Some(cs) = e.child("cs").and_then(|l| l.attr("typeface")) {
            props.cs = Some(self.theme.font(cs));
        }
    }

    fn merge_para(&self, props: &mut ParaProps, e: &Element, placeholder: Option<Color>) {
        if let Some(a) = e.attr("algn") {
            props.align = Some(a.to_owned());
        }
        if let Some(r) = flag(e, "rtl") {
            props.rtl = Some(r);
        }
        if let Some(v) = e.attr("marL") {
            props.mar_l = Some(emu(Some(v)));
        }
        if let Some(v) = e.attr("indent") {
            props.indent = Some(emu(Some(v)));
        }
        if let Some(s) = spacing(e.child("lnSpc")) {
            props.line = Some(s);
        }
        if let Some(s) = spacing(e.child("spcBef")) {
            props.before = Some(s);
        }
        if let Some(s) = spacing(e.child("spcAft")) {
            props.after = Some(s);
        }
        if e.child("buNone").is_some() {
            props.bullet = Some(Bullet::None);
        }
        if let Some(c) = e.child("buChar").and_then(|b| b.attr("char")) {
            props.bullet = Some(Bullet::Char(c.to_owned()));
        }
        if let Some(n) = e.child("buAutoNum") {
            props.bullet = Some(Bullet::Number(
                n.attr("type").unwrap_or("arabicPeriod").to_owned(),
                n.attr("startAt").and_then(|v| v.parse().ok()).unwrap_or(1),
            ));
        }
        if let Some(c) = e
            .child("buClr")
            .and_then(|c| self.palette(placeholder).first_color(c))
        {
            props.bullet_color = Some(c);
        }
        if let Some(s) = e.child("buSzPct").and_then(|s| num(s, "val")) {
            props.bullet_size = Some(s / 100_000.0);
        }
        if let Some(f) = e.child("buFont").and_then(|f| f.attr("typeface")) {
            props.bullet_font = Some(f.to_owned());
        }
        if let Some(d) = e.child("defRPr") {
            self.merge_run(&mut props.run, d, placeholder);
        }
    }
}

pub fn script_runs(text: &str) -> Vec<(bool, String)> {
    let complex = |c: char| matches!(c, '\u{0590}'..='\u{08FF}' | '\u{0900}'..='\u{0DFF}' | '\u{0E00}'..='\u{0EFF}' | '\u{1000}'..='\u{109F}' | '\u{1780}'..='\u{17FF}');
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        let kind = if c.is_whitespace() || c.is_ascii_punctuation() {
            out.last().is_some_and(|(k, _)| *k)
        } else {
            complex(c)
        };
        match out.last_mut() {
            Some((k, s)) if *k == kind => s.push(c),
            _ => out.push((kind, c.to_string())),
        }
    }
    out
}

fn roman(mut n: u32) -> String {
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn autonumber(kind: &str, n: u32) -> String {
    let letter = |base: u8| char::from(base + ((n.max(1) - 1) % 26) as u8).to_string();
    let (body, style) = if let Some(style) = kind.strip_prefix("romanUc") {
        (roman(n).to_uppercase(), style)
    } else if let Some(style) = kind.strip_prefix("romanLc") {
        (roman(n), style)
    } else if let Some(style) = kind.strip_prefix("alphaUc") {
        (letter(b'A'), style)
    } else if let Some(style) = kind.strip_prefix("alphaLc") {
        (letter(b'a'), style)
    } else if let Some(style) = kind.strip_prefix("thaiNum") {
        let digits: String = n
            .to_string()
            .chars()
            .map(|d| char::from_u32(0x0E50 + d.to_digit(10).unwrap_or(0)).unwrap_or(d))
            .collect();
        (digits, style)
    } else {
        (
            n.to_string(),
            kind.strip_prefix("arabic").unwrap_or("Period"),
        )
    };
    match style {
        "ParenR" => format!("{body})"),
        "ParenBoth" => format!("({body})"),
        "Plain" => body,
        "Minus" => format!("- {body}"),
        _ => format!("{body}."),
    }
}

fn bullet_char(c: &str, font: Option<&str>) -> String {
    let symbol = font.is_some_and(|f| {
        let f = f.to_ascii_lowercase();
        f.contains("wingdings") || f.contains("symbol")
    });
    if !symbol {
        return c.to_owned();
    }
    match c.chars().next().map(|c| u32::from(c) & 0xFF) {
        Some(0xA7) => "\u{25AA}".to_owned(),
        Some(0xD8) => "\u{27A2}".to_owned(),
        Some(0xFC) => "\u{2713}".to_owned(),
        Some(0x71) => "\u{274F}".to_owned(),
        Some(0x76) => "\u{2756}".to_owned(),
        Some(0x6E) => "\u{25A0}".to_owned(),
        Some(0x6C) => "\u{25CF}".to_owned(),
        _ => "\u{2022}".to_owned(),
    }
}

struct Para {
    props: ParaProps,
    spans: Vec<Span>,
    bullet: Option<Span>,
    size: f32,
}

impl Scene<'_, '_> {
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn text(
        &mut self,
        body: &Element,
        ph: Option<&Element>,
        inherited: &[Option<&Element>; 2],
        frame: Frame,
        font_color: Option<Color>,
        host: &dyn Inherit,
    ) {
        let Frame { m, w, h } = frame;
        let paragraphs: Vec<&Element> = body.children_named("p").collect();
        if paragraphs.iter().all(|p| p.text().trim().is_empty()) {
            return;
        }
        let mut body_chain: Vec<&Element> = inherited
            .iter()
            .flatten()
            .filter_map(|s| s.path(&["txBody", "bodyPr"]))
            .collect();
        if let Some(b) = body.child("bodyPr") {
            body_chain.push(b);
        }
        let body_attr = |key: &str| body_chain.iter().rev().find_map(|b| b.attr(key));
        let inset = |key: &str, default: f32| body_attr(key).map_or(default, |v| emu(Some(v)));
        let (l_ins, r_ins) = (inset("lIns", 7.2), inset("rIns", 7.2));
        let (t_ins, b_ins) = (inset("tIns", 3.6), inset("bIns", 3.6));
        let wrap = body_attr("wrap") != Some("none");
        let anchor = body_attr("anchor").unwrap_or("t");
        let autofit = body_chain.iter().rev().find_map(|b| b.child("normAutofit"));
        let font_scale = autofit
            .and_then(|a| num(a, "fontScale"))
            .map_or(1.0, |v| v / 100_000.0);
        let spacing_cut = autofit
            .and_then(|a| num(a, "lnSpcReduction"))
            .map_or(0.0, |v| v / 100_000.0);
        let kind = ph.map(|p| p.attr("type").unwrap_or("obj"));
        let master_style = host.master_style(kind);
        let mut chain: Vec<&Element> = Vec::new();
        if ph.is_none()
            && let Some(d) = self.default_text
        {
            chain.push(d);
        }
        if let Some(s) = master_style {
            chain.push(s);
        }
        for s in inherited.iter().flatten() {
            if let Some(l) = s.path(&["txBody", "lstStyle"]) {
                chain.push(l);
            }
        }
        if let Some(l) = body.child("lstStyle") {
            chain.push(l);
        }
        let minor = self.theme.minor.clone();
        let major = self.theme.major.clone();
        let title = matches!(kind, Some("title" | "ctrTitle"));
        let base_latin = if title {
            major.latin.clone()
        } else {
            minor.latin.clone()
        };
        let base_cs = if title {
            major.cs.clone()
        } else {
            minor.cs.clone()
        };
        let columns = body_attr("numCol")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 16);
        let column_gap = body_attr("spcCol").map_or(0.0, |v| emu(Some(v)));
        let rtl_columns = body_attr("rtlCol").is_some_and(|v| v == "1" || v == "true");
        let inner_w = if columns > 1 {
            ((w - l_ins - r_ins - column_gap * (columns - 1) as f32) / columns as f32).max(1.0)
        } else {
            (w - l_ins - r_ins).max(1.0)
        };
        let mut counters = [0_u32; 10];
        let mut paras: Vec<Para> = Vec::new();
        for p in &paragraphs {
            let ppr = p.child("pPr");
            let lvl = ppr.and_then(|x| num(x, "lvl")).unwrap_or(0.0) as usize;
            let level_name = format!("lvl{}pPr", lvl + 1);
            let mut props = ParaProps::default();
            for style in &chain {
                if let Some(l) = style.child(&level_name) {
                    self.merge_para(&mut props, l, None);
                }
            }
            if let Some(ppr) = ppr {
                self.merge_para(&mut props, ppr, None);
            }
            let mut spans = Vec::new();
            let mut first_run: Option<RunProps> = None;
            for item in p.elements() {
                let text = match item.local() {
                    "r" | "fld" => item.child("t").map(Element::text).unwrap_or_default(),
                    "br" => "\n".to_owned(),
                    _ => continue,
                };
                let mut run = props.run.clone();
                if let Some(rpr) = item.child("rPr") {
                    self.merge_run(&mut run, rpr, None);
                }
                if first_run.is_none() && item.local() != "br" {
                    first_run = Some(run.clone());
                }
                let size = run.size.unwrap_or(self.default_size) * font_scale;
                let color = run.color.or(font_color).unwrap_or(Color::rgb(0)).pdf();
                let latin = run
                    .latin
                    .clone()
                    .filter(|f| !f.is_empty())
                    .unwrap_or_else(|| base_latin.clone());
                let cs = run
                    .cs
                    .clone()
                    .filter(|f| !f.is_empty())
                    .or_else(|| (!base_cs.is_empty()).then(|| base_cs.clone()));
                for (complex, piece) in script_runs(&text) {
                    let family = if complex {
                        cs.clone().unwrap_or_else(|| latin.clone())
                    } else {
                        latin.clone()
                    };
                    let family = self.theme.font(&family);
                    spans.push(Span {
                        text: symbol_text(&family, &piece).unwrap_or(piece),
                        family,
                        size,
                        bold: run.bold.unwrap_or(false),
                        italic: run.italic.unwrap_or(false),
                        underline: run.underline.unwrap_or(false),
                        strike: run.strike.unwrap_or(false),
                        color,
                    });
                }
            }
            let end_size = p
                .child("endParaRPr")
                .and_then(|e| num(e, "sz"))
                .map(|s| s / 100.0)
                .or(props.run.size)
                .unwrap_or(self.default_size)
                * font_scale;
            let size = spans
                .iter()
                .map(|s| s.size)
                .fold(0.0, f32::max)
                .max(if spans.is_empty() { end_size } else { 0.0 });
            let has_text = spans.iter().any(|s| !s.text.trim().is_empty());
            let bullet_text = match (&props.bullet, has_text) {
                (Some(Bullet::Char(c)), true) => {
                    for deeper in counters.iter_mut().skip(lvl) {
                        *deeper = 0;
                    }
                    Some(bullet_char(c, props.bullet_font.as_deref()))
                }
                (Some(Bullet::Number(kind, start)), true) => {
                    for deeper in counters.iter_mut().skip(lvl + 1) {
                        *deeper = 0;
                    }
                    let n = if counters[lvl] == 0 {
                        *start
                    } else {
                        counters[lvl] + 1
                    };
                    counters[lvl] = n;
                    Some(autonumber(kind, n))
                }
                _ => {
                    if has_text {
                        for deeper in counters.iter_mut().skip(lvl) {
                            *deeper = 0;
                        }
                    }
                    None
                }
            };
            let bullet = bullet_text.map(|text| {
                let first = first_run.clone().unwrap_or_default();
                let run_size = first.size.unwrap_or(self.default_size) * font_scale;
                let color = props
                    .bullet_color
                    .or(first.color)
                    .or(font_color)
                    .unwrap_or(Color::rgb(0))
                    .pdf();
                let family = first.latin.clone().unwrap_or_else(|| base_latin.clone());
                Span {
                    text,
                    family: self.theme.font(&family),
                    size: run_size * props.bullet_size.unwrap_or(1.0),
                    bold: first.bold.unwrap_or(false),
                    italic: false,
                    underline: false,
                    strike: false,
                    color,
                }
            });
            paras.push(Para {
                props,
                spans,
                bullet,
                size,
            });
        }
        struct SetLine {
            line: convert_pdf_canvas::Line,
            baseline: f32,
            x: f32,
            spacing: f32,
            top: f32,
            bottom: f32,
        }
        let mut set: Vec<SetLine> = Vec::new();
        let mut bullets: Vec<(convert_pdf_canvas::Line, f32, f32, usize)> = Vec::new();
        let mut y = 0.0_f32;
        for (k, para) in paras.iter().enumerate() {
            let mar_l = para.props.mar_l.unwrap_or(0.0);
            let indent = para.props.indent.unwrap_or(0.0);
            let spc = |s: Option<Spacing>| match s {
                Some(Spacing::Points(p)) => p,
                Some(Spacing::Percent(p)) => p * para.size * 1.2,
                None => 0.0,
            };
            if k > 0 {
                y += spc(para.props.before);
            }
            let width = wrap.then_some((inner_w - mar_l).max(1.0));
            let rtl = para.props.rtl.unwrap_or(false);
            let direction = if rtl { Direction::Rtl } else { Direction::Ltr };
            let lines = if para.spans.iter().all(|s| s.text.is_empty()) {
                Vec::new()
            } else {
                layout_directed(self.canvas, self.fonts, &para.spans, width, direction)
            };
            let line_height = |line_size: f32| match para.props.line {
                Some(Spacing::Points(p)) => p,
                Some(Spacing::Percent(p)) => (p - spacing_cut).max(0.5) * line_size * 1.2,
                None => (1.0 - spacing_cut).max(0.5) * line_size * 1.2,
            };
            if lines.is_empty() {
                y += line_height(para.size);
            }
            for (n, line) in lines.into_iter().enumerate() {
                let line_size = line
                    .pieces
                    .iter()
                    .map(|p| p.style.size)
                    .fold(0.0, f32::max)
                    .max(if line.pieces.is_empty() {
                        para.size
                    } else {
                        0.0
                    });
                let lh = line_height(line_size);
                let top = y;
                let baseline = top + lh - 0.26 * line_size;
                let avail = inner_w - mar_l;
                let (x, spacing) = if rtl {
                    match para.props.align.as_deref() {
                        Some("ctr") => ((avail - line.width) / 2.0, 0.0),
                        Some("l") => (0.0, 0.0),
                        Some("just" | "dist") if !line.last && line.spaces > 0 && wrap => {
                            (0.0, ((avail - line.width) / line.spaces as f32).max(0.0))
                        }
                        _ => (avail - line.width, 0.0),
                    }
                } else {
                    match para.props.align.as_deref() {
                        Some("ctr") => (mar_l + (avail - line.width) / 2.0, 0.0),
                        Some("r") => (inner_w - line.width, 0.0),
                        Some("just" | "dist") if !line.last && line.spaces > 0 && wrap => {
                            (mar_l, ((avail - line.width) / line.spaces as f32).max(0.0))
                        }
                        _ => (mar_l, 0.0),
                    }
                };
                if n == 0
                    && let Some(b) = &para.bullet
                {
                    let lines = layout_directed(
                        self.canvas,
                        self.fonts,
                        std::slice::from_ref(b),
                        None,
                        direction,
                    );
                    if let Some(bl) = lines.into_iter().next() {
                        let bx = if rtl {
                            if matches!(para.props.align.as_deref(), Some("ctr" | "l")) {
                                x + line.width + 4.0
                            } else {
                                inner_w - (mar_l + indent).max(0.0) - bl.width
                            }
                        } else if matches!(para.props.align.as_deref(), Some("ctr" | "r")) {
                            x - bl.width - 4.0
                        } else {
                            (mar_l + indent).max(0.0)
                        };
                        bullets.push((bl, bx, baseline, set.len()));
                    }
                }
                set.push(SetLine {
                    line,
                    baseline,
                    x,
                    spacing,
                    top,
                    bottom: top + lh,
                });
                y += lh;
            }
            y += spc(para.props.after);
        }
        let mut total = y;
        let avail_h = h - t_ins - b_ins;
        if columns > 1 {
            let mut column = 0;
            let mut start = 0.0_f32;
            let mut tallest = 0.0_f32;
            let mut placed: Vec<(usize, f32)> = Vec::with_capacity(set.len());
            for s in &set {
                if s.bottom - start > avail_h + 0.01 && s.top > start + 0.01 && column + 1 < columns
                {
                    tallest = tallest.max(s.top - start);
                    column += 1;
                    start = s.top;
                }
                placed.push((column, start));
            }
            tallest = tallest.max(y - start);
            let column_x = |c: usize| {
                let at = if rtl_columns { columns - 1 - c } else { c };
                at as f32 * (inner_w + column_gap)
            };
            for (s, &(c, from)) in set.iter_mut().zip(&placed) {
                s.baseline -= from;
                s.x += column_x(c);
            }
            for (_, x, baseline, line) in &mut bullets {
                if let Some(&(c, from)) = placed.get(*line) {
                    *baseline -= from;
                    *x += column_x(c);
                }
            }
            total = tallest;
        }
        let shift = match anchor {
            "ctr" => t_ins + (avail_h - total) / 2.0,
            "b" => h - b_ins - total,
            _ => t_ins,
        };
        self.page().save();
        self.apply(m, h);
        let turned = m[1].abs() > 1e-3 && m[0].abs() > 1e-3;
        for s in &set {
            let x = l_ins + s.x;
            let yb = h - (shift + s.baseline);
            let page = self.canvas.page(self.page);
            if turned {
                page.begin_actual_text(&s.line.text());
            }
            s.line.draw(page, x, yb, s.spacing);
            if turned {
                page.end_actual_text();
            }
        }
        for (b, x, baseline, _) in &bullets {
            b.draw(
                self.canvas.page(self.page),
                l_ins + x,
                h - (shift + baseline),
                0.0,
            );
        }
        self.page().restore();
    }

    pub(crate) fn cell_spans(&self, tc: &Element) -> Vec<Span> {
        let slide = self;
        let Some(body) = tc.child("txBody") else {
            return Vec::new();
        };
        let minor = &slide.theme.minor;
        let mut spans = Vec::new();
        for (k, p) in body.children_named("p").enumerate() {
            if k > 0
                && let Some(last) = spans.last_mut()
            {
                let last: &mut Span = last;
                last.text.push('\n');
            }
            for r in p.elements().filter(|e| matches!(e.local(), "r" | "fld")) {
                let mut run = RunProps::default();
                if let Some(d) = slide
                    .default_text
                    .and_then(|d| d.child("lvl1pPr"))
                    .and_then(|l| l.child("defRPr"))
                {
                    slide.merge_run(&mut run, d, None);
                }
                if let Some(rpr) = r.child("rPr") {
                    slide.merge_run(&mut run, rpr, None);
                }
                let text = r.child("t").map(Element::text).unwrap_or_default();
                let latin = run.latin.clone().unwrap_or_else(|| minor.latin.clone());
                let cs = run
                    .cs
                    .clone()
                    .filter(|c| !c.is_empty())
                    .unwrap_or_else(|| latin.clone());
                for (complex, piece) in script_runs(&text) {
                    let family = if complex { cs.clone() } else { latin.clone() };
                    spans.push(Span {
                        text: symbol_text(&family, &piece).unwrap_or(piece),
                        family,
                        size: run.size.unwrap_or(slide.default_size),
                        bold: run.bold.unwrap_or(false),
                        italic: run.italic.unwrap_or(false),
                        underline: run.underline.unwrap_or(false),
                        strike: run.strike.unwrap_or(false),
                        color: run.color.unwrap_or(Color::rgb(0)).pdf(),
                    });
                }
            }
        }
        spans
    }
}

#[must_use]
pub fn script_spans(span: &Span, cs: &str) -> Vec<Span> {
    script_runs(&span.text)
        .into_iter()
        .map(|(complex, piece)| {
            let family = if complex && !cs.is_empty() {
                cs.to_owned()
            } else {
                span.family.clone()
            };
            Span {
                text: symbol_text(&family, &piece).unwrap_or(piece),
                family,
                ..span.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_and_scripts() {
        assert_eq!(autonumber("arabicPeriod", 3), "3.");
        assert_eq!(autonumber("romanUcPeriod", 4), "IV.");
        assert_eq!(autonumber("alphaLcParenR", 2), "b)");
        assert_eq!(autonumber("thaiNumPeriod", 12), "๑๒.");
        let runs = script_runs("P02 ລາວ ok");
        assert_eq!(runs.len(), 3);
        assert!(runs[1].0);
        assert_eq!(bullet_char("\u{F0A7}", Some("Wingdings")), "\u{25AA}");
        let spans = script_spans(&Span::new("Sales ຍອດຂາຍ", "Calibri", 10.0), "Phetsarath OT");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].family, "Phetsarath OT");
    }
}
