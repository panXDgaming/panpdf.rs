use std::collections::HashMap;

use convert_layout::model::{Align, Border, LineHeight, Rgb, Sides, VerticalShift};
use convert_office_read::Element;

pub fn twips(value: &str) -> Option<f32> {
    #[allow(clippy::cast_precision_loss)]
    value.trim().parse::<f64>().ok().map(|v| (v / 20.0) as f32)
}

fn toggle(el: &Element) -> bool {
    !matches!(el.attr("val"), Some("0" | "false" | "off" | "none"))
}

pub fn hex_colour(value: &str) -> Option<Rgb> {
    if value.len() != 6 || value.eq_ignore_ascii_case("auto") {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn highlight(name: &str) -> Option<Rgb> {
    Some(match name {
        "yellow" => [255, 255, 0],
        "green" => [0, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "blue" => [0, 0, 255],
        "red" => [255, 0, 0],
        "darkBlue" => [0, 0, 139],
        "darkCyan" => [0, 139, 139],
        "darkGreen" => [0, 100, 0],
        "darkMagenta" => [128, 0, 128],
        "darkRed" => [139, 0, 0],
        "darkYellow" => [128, 128, 0],
        "darkGray" => [169, 169, 169],
        "lightGray" => [211, 211, 211],
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        _ => return None,
    })
}

pub fn border(el: &Element) -> Option<Option<Border>> {
    let val = el.attr("val").unwrap_or("single");
    if matches!(val, "nil" | "none") {
        return Some(None);
    }
    #[allow(clippy::cast_precision_loss)]
    let size = el
        .attr("sz")
        .and_then(|s| s.parse::<f32>().ok())
        .map_or(0.5, |eighths| eighths / 8.0)
        .max(0.25);
    let width = if val == "double" { size * 3.0 } else { size };
    let colour = el.attr("color").and_then(hex_colour).unwrap_or([0, 0, 0]);
    Some(Some(Border { width, colour }))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Borders {
    pub top: Option<Option<Border>>,
    pub left: Option<Option<Border>>,
    pub bottom: Option<Option<Border>>,
    pub right: Option<Option<Border>>,
    pub inside_h: Option<Option<Border>>,
    pub inside_v: Option<Option<Border>>,
}

impl Borders {
    pub fn read(el: &Element) -> Self {
        let mut b = Self::default();
        for side in el.elements() {
            let value = border(side);
            match side.local() {
                "top" => b.top = value,
                "left" | "start" => b.left = value,
                "bottom" => b.bottom = value,
                "right" | "end" => b.right = value,
                "insideH" => b.inside_h = value,
                "insideV" => b.inside_v = value,
                _ => {}
            }
        }
        b
    }

    pub fn over(&mut self, other: &Self) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f; } )* };
        }
        take!(top, left, bottom, right, inside_h, inside_v);
    }

    pub fn sides(&self) -> Sides<Option<Border>> {
        Sides {
            top: self.top.flatten(),
            right: self.right.flatten(),
            bottom: self.bottom.flatten(),
            left: self.left.flatten(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunProps {
    pub ascii: Option<String>,
    pub cs: Option<String>,
    pub east_asia: Option<String>,
    pub ascii_theme: Option<String>,
    pub cs_theme: Option<String>,
    pub size: Option<f32>,
    pub size_cs: Option<f32>,
    pub bold: Option<bool>,
    pub bold_cs: Option<bool>,
    pub italic: Option<bool>,
    pub italic_cs: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub colour: Option<Option<Rgb>>,
    pub highlight: Option<Option<Rgb>>,
    pub shift: Option<VerticalShift>,
    pub caps: Option<bool>,
    pub small_caps: Option<bool>,
    pub vanish: Option<bool>,
    pub spacing: Option<f32>,
    pub style: Option<String>,
}

impl RunProps {
    pub fn read(el: &Element) -> Self {
        let mut p = Self::default();
        for c in el.elements() {
            match c.local() {
                "rStyle" => p.style = c.attr("val").map(str::to_owned),
                "rFonts" => {
                    p.ascii = c
                        .attr("ascii")
                        .or_else(|| c.attr("hAnsi"))
                        .map(str::to_owned);
                    p.cs = c.attr("cs").map(str::to_owned);
                    p.east_asia = c.attr("eastAsia").map(str::to_owned);
                    p.ascii_theme = c
                        .attr("asciiTheme")
                        .or_else(|| c.attr("hAnsiTheme"))
                        .map(str::to_owned);
                    p.cs_theme = c.attr("cstheme").map(str::to_owned);
                }
                "sz" => {
                    p.size = c
                        .attr("val")
                        .and_then(|v| v.parse::<f32>().ok())
                        .map(|v| v / 2.0)
                }
                "szCs" => {
                    p.size_cs = c
                        .attr("val")
                        .and_then(|v| v.parse::<f32>().ok())
                        .map(|v| v / 2.0);
                }
                "b" => p.bold = Some(toggle(c)),
                "bCs" => p.bold_cs = Some(toggle(c)),
                "i" => p.italic = Some(toggle(c)),
                "iCs" => p.italic_cs = Some(toggle(c)),
                "u" => p.underline = Some(!matches!(c.attr("val"), Some("none" | "0"))),
                "strike" | "dstrike" => p.strike = Some(toggle(c)),
                "color" => p.colour = Some(c.attr("val").and_then(hex_colour)),
                "highlight" => p.highlight = Some(c.attr("val").and_then(highlight)),
                "shd" => {
                    if p.highlight.is_none() {
                        let fill = c.attr("fill").and_then(hex_colour);
                        if fill.is_some() {
                            p.highlight = Some(fill);
                        }
                    }
                }
                "vertAlign" => {
                    p.shift = Some(match c.attr("val") {
                        Some("superscript") => VerticalShift::Super,
                        Some("subscript") => VerticalShift::Sub,
                        _ => VerticalShift::None,
                    });
                }
                "caps" => p.caps = Some(toggle(c)),
                "smallCaps" => p.small_caps = Some(toggle(c)),
                "vanish" | "specVanish" => p.vanish = Some(toggle(c)),
                "spacing" => p.spacing = c.attr("val").and_then(twips),
                _ => {}
            }
        }
        p
    }

    pub fn over(&mut self, other: &Self) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f.clone_from(&other.$f); } )* };
        }
        take!(
            ascii,
            cs,
            east_asia,
            ascii_theme,
            cs_theme,
            size,
            size_cs,
            bold,
            bold_cs,
            italic,
            italic_cs,
            underline,
            strike,
            colour,
            highlight,
            shift,
            caps,
            small_caps,
            vanish,
            spacing,
            style
        );
    }
}

#[derive(Clone, Debug, Default)]
pub struct ParaProps {
    pub style: Option<String>,
    pub align: Option<Align>,
    pub left: Option<f32>,
    pub right: Option<f32>,
    pub first: Option<f32>,
    pub before: Option<f32>,
    pub after: Option<f32>,
    pub before_auto: Option<bool>,
    pub after_auto: Option<bool>,
    pub line: Option<LineHeight>,
    pub keep_next: Option<bool>,
    pub page_break_before: Option<bool>,
    pub contextual: Option<bool>,
    pub shading: Option<Option<Rgb>>,
    pub borders: Option<Borders>,
    pub tabs: Option<Vec<(f32, bool)>>,
    pub num: Option<(String, usize)>,
    pub ilvl: Option<usize>,
    pub bidi: Option<bool>,
    pub mark: Option<RunProps>,
}

impl ParaProps {
    pub fn read(el: &Element) -> Self {
        let mut p = Self::default();
        for c in el.elements() {
            match c.local() {
                "pStyle" => p.style = c.attr("val").map(str::to_owned),
                "jc" => {
                    p.align = Some(match c.attr("val").unwrap_or("left") {
                        "center" => Align::Center,
                        "right" | "end" => Align::Right,
                        "both" | "distribute" | "thaiDistribute" | "lowKashida"
                        | "mediumKashida" | "highKashida" => Align::Justify,
                        _ => Align::Left,
                    });
                }
                "ind" => {
                    if let Some(v) = c.attr("left").or_else(|| c.attr("start")).and_then(twips) {
                        p.left = Some(v);
                    }
                    if let Some(v) = c.attr("right").or_else(|| c.attr("end")).and_then(twips) {
                        p.right = Some(v);
                    }
                    if let Some(v) = c.attr("hanging").and_then(twips) {
                        p.first = Some(-v);
                    } else if let Some(v) = c.attr("firstLine").and_then(twips) {
                        p.first = Some(v);
                    }
                }
                "spacing" => {
                    if let Some(v) = c.attr("before").and_then(twips) {
                        p.before = Some(v);
                    }
                    if let Some(v) = c.attr("after").and_then(twips) {
                        p.after = Some(v);
                    }
                    if let Some(v) = c.attr("beforeAutospacing") {
                        p.before_auto = Some(matches!(v, "1" | "true" | "on"));
                    }
                    if let Some(v) = c.attr("afterAutospacing") {
                        p.after_auto = Some(matches!(v, "1" | "true" | "on"));
                    }
                    if let Some(line) = c.attr("line").and_then(|v| v.parse::<f32>().ok()) {
                        p.line = Some(match c.attr("lineRule").unwrap_or("auto") {
                            "exact" => LineHeight::Exact(line / 20.0),
                            "atLeast" => LineHeight::AtLeast(line / 20.0),
                            _ => LineHeight::Multiple(line / 240.0),
                        });
                    }
                }
                "keepNext" => p.keep_next = Some(toggle(c)),
                "pageBreakBefore" => p.page_break_before = Some(toggle(c)),
                "contextualSpacing" => p.contextual = Some(toggle(c)),
                "shd" => p.shading = Some(c.attr("fill").and_then(hex_colour)),
                "pBdr" => p.borders = Some(Borders::read(c)),
                "tabs" => {
                    let mut stops: Vec<(f32, bool)> = c
                        .children_named("tab")
                        .filter(|t| t.attr("val") != Some("clear"))
                        .filter_map(|t| {
                            let pos = t.attr("pos").and_then(twips)?;
                            Some((
                                pos,
                                matches!(t.attr("val"), Some("right" | "end" | "decimal")),
                            ))
                        })
                        .collect();
                    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
                    p.tabs = Some(stops);
                }
                "numPr" => {
                    let id = c
                        .child("numId")
                        .and_then(|n| n.attr("val"))
                        .map(str::to_owned);
                    let level = c
                        .child("ilvl")
                        .and_then(|n| n.attr("val"))
                        .and_then(|v| v.parse().ok());
                    if let Some(id) = id {
                        p.num = Some((id, level.unwrap_or(0)));
                    } else if let Some(level) = level {
                        p.ilvl = Some(level);
                    }
                }
                "bidi" => p.bidi = Some(toggle(c)),
                "rPr" => p.mark = Some(RunProps::read(c)),
                _ => {}
            }
        }
        p
    }

    pub fn over(&mut self, other: &Self) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f.clone_from(&other.$f); } )* };
        }
        take!(
            style,
            align,
            left,
            right,
            first,
            before,
            after,
            before_auto,
            after_auto,
            line,
            keep_next,
            page_break_before,
            contextual,
            shading,
            tabs,
            num,
            bidi
        );
        if let Some(level) = other.ilvl {
            if let Some(num) = &mut self.num {
                num.1 = level;
            } else {
                self.ilvl = Some(level);
            }
        }
        if let Some(b) = &other.borders {
            let mut mine = self.borders.unwrap_or_default();
            mine.over(b);
            self.borders = Some(mine);
        }
        if let Some(m) = &other.mark {
            let mut mine = self.mark.clone().unwrap_or_default();
            mine.over(m);
            self.mark = Some(mine);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub kind: String,
    pub based_on: Option<String>,
    pub para: ParaProps,
    pub run: RunProps,
    pub table_borders: Option<Borders>,
    pub cell_margins: Option<Sides<Option<f32>>>,
}

#[derive(Clone, Debug, Default)]
pub struct Level {
    pub start: i64,
    pub format: String,
    pub text: String,
    pub para: ParaProps,
    pub run: RunProps,
    pub suffix: String,
}

#[derive(Clone, Debug, Default)]
pub struct Styles {
    pub styles: HashMap<String, Style>,
    pub default_para: Option<String>,
    pub default_table: Option<String>,
    pub doc_run: RunProps,
    pub doc_para: ParaProps,
    pub abstract_nums: HashMap<String, Vec<Level>>,
    pub nums: HashMap<String, (String, HashMap<usize, i64>)>,
    pub theme_minor: String,
    pub theme_major: String,
    pub theme_minor_cs: Option<String>,
    pub theme_major_cs: Option<String>,
}

impl Styles {
    pub fn read(
        styles: Option<&Element>,
        numbering: Option<&Element>,
        theme: Option<&Element>,
    ) -> Self {
        let mut out = Self {
            theme_minor: "Calibri".into(),
            theme_major: "Calibri Light".into(),
            ..Self::default()
        };
        if let Some(theme) = theme {
            let face = |which: &str, slot: &str| {
                theme
                    .descendants(which)
                    .first()
                    .and_then(|f| f.child(slot))
                    .and_then(|l| l.attr("typeface"))
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
            };
            if let Some(f) = face("minorFont", "latin") {
                out.theme_minor = f;
            }
            if let Some(f) = face("majorFont", "latin") {
                out.theme_major = f;
            }
            out.theme_minor_cs = face("minorFont", "cs");
            out.theme_major_cs = face("majorFont", "cs");
        }
        if let Some(root) = styles {
            if let Some(defaults) = root.child("docDefaults") {
                if let Some(r) = defaults.path(&["rPrDefault", "rPr"]) {
                    out.doc_run = RunProps::read(r);
                }
                if let Some(p) = defaults.path(&["pPrDefault", "pPr"]) {
                    out.doc_para = ParaProps::read(p);
                }
            }
            for s in root.children_named("style") {
                let Some(id) = s.attr("styleId") else {
                    continue;
                };
                let kind = s.attr("type").unwrap_or("paragraph").to_owned();
                let default = matches!(s.attr("default"), Some("1" | "true"));
                if default && kind == "paragraph" {
                    out.default_para = Some(id.to_owned());
                }
                if default && kind == "table" {
                    out.default_table = Some(id.to_owned());
                }
                let table_pr = s.child("tblPr");
                out.styles.insert(
                    id.to_owned(),
                    Style {
                        kind,
                        based_on: s
                            .child("basedOn")
                            .and_then(|b| b.attr("val"))
                            .map(str::to_owned),
                        para: s.child("pPr").map(ParaProps::read).unwrap_or_default(),
                        run: s.child("rPr").map(RunProps::read).unwrap_or_default(),
                        table_borders: table_pr
                            .and_then(|t| t.child("tblBorders"))
                            .map(Borders::read),
                        cell_margins: table_pr.and_then(|t| t.child("tblCellMar")).map(margins),
                    },
                );
            }
        }
        if let Some(root) = numbering {
            for a in root.children_named("abstractNum") {
                let Some(id) = a.attr("abstractNumId") else {
                    continue;
                };
                let mut levels = Vec::new();
                for l in a.children_named("lvl") {
                    let level: usize = l
                        .attr("ilvl")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(levels.len());
                    let lvl = Level {
                        start: l
                            .child("start")
                            .and_then(|s| s.attr("val"))
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1),
                        format: l
                            .child("numFmt")
                            .and_then(|s| s.attr("val"))
                            .unwrap_or("decimal")
                            .to_owned(),
                        text: l
                            .child("lvlText")
                            .and_then(|s| s.attr("val"))
                            .unwrap_or("")
                            .to_owned(),
                        para: l.child("pPr").map(ParaProps::read).unwrap_or_default(),
                        run: l.child("rPr").map(RunProps::read).unwrap_or_default(),
                        suffix: l
                            .child("suff")
                            .and_then(|s| s.attr("val"))
                            .unwrap_or("tab")
                            .to_owned(),
                    };
                    if levels.len() <= level {
                        levels.resize(level + 1, Level::default());
                    }
                    levels[level] = lvl;
                }
                out.abstract_nums.insert(id.to_owned(), levels);
            }
            for n in root.children_named("num") {
                let Some(id) = n.attr("numId") else { continue };
                let Some(abs) = n.child("abstractNumId").and_then(|a| a.attr("val")) else {
                    continue;
                };
                let mut starts = HashMap::new();
                for o in n.children_named("lvlOverride") {
                    let level: usize = o.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                    if let Some(s) = o
                        .child("startOverride")
                        .and_then(|s| s.attr("val"))
                        .and_then(|v| v.parse().ok())
                    {
                        starts.insert(level, s);
                    }
                }
                out.nums.insert(id.to_owned(), (abs.to_owned(), starts));
            }
        }
        out
    }

    pub fn chain(&self, id: &str) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut at = Some(id.to_owned());
        let mut guard = 0;
        while let Some(name) = at {
            guard += 1;
            let Some(style) = self.styles.get(&name) else {
                break;
            };
            out.push(style);
            at = style.based_on.clone();
            if guard > 20 {
                break;
            }
        }
        out.reverse();
        out
    }

    pub fn para_props(&self, style: Option<&str>) -> (ParaProps, RunProps) {
        let mut para = self.doc_para.clone();
        let mut run = self.doc_run.clone();
        let id = style
            .map(str::to_owned)
            .or_else(|| self.default_para.clone());
        if let Some(id) = id {
            for s in self.chain(&id) {
                para.over(&s.para);
                run.over(&s.run);
            }
        }
        (para, run)
    }

    pub fn char_props(&self, style: &str) -> RunProps {
        let mut run = RunProps::default();
        for s in self.chain(style) {
            run.over(&s.run);
        }
        run
    }

    pub fn level(&self, num: &str, level: usize) -> Option<(&Level, String, i64)> {
        let (abs, starts) = self.nums.get(num)?;
        let levels = self.abstract_nums.get(abs)?;
        let lvl = levels.get(level)?;
        let start = starts.get(&level).copied().unwrap_or(lvl.start);
        Some((lvl, abs.clone(), start))
    }

    pub fn levels(&self, num: &str) -> Option<&Vec<Level>> {
        let (abs, _) = self.nums.get(num)?;
        self.abstract_nums.get(abs)
    }

    pub fn theme_font(&self, theme: &str) -> Option<String> {
        let major = theme.starts_with("major");
        if theme.ends_with("Bidi") {
            return if major {
                self.theme_major_cs.clone()
            } else {
                self.theme_minor_cs.clone()
            };
        }
        Some(if major {
            self.theme_major.clone()
        } else {
            self.theme_minor.clone()
        })
    }
}

pub fn margins(el: &Element) -> Sides<Option<f32>> {
    let get = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| el.child(n))
            .and_then(|c| c.attr("w"))
            .and_then(twips)
    };
    Sides {
        top: get(&["top"]),
        right: get(&["right", "end"]),
        bottom: get(&["bottom"]),
        left: get(&["left", "start"]),
    }
}
