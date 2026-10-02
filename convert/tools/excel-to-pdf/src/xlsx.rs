use std::collections::{BTreeMap, HashMap};

use convert_office_read::xml::{Event, Reader, local};
use convert_office_read::{Element, Package};

pub type Argb = u32;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub font: Option<Font>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(Vec<TextRun>),
    Bool(bool),
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub value: Option<Value>,
    pub style: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    pub name: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<Argb>,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            name: "Calibri".to_owned(),
            size: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineStyle {
    #[default]
    None,
    Hair,
    Thin,
    Medium,
    Thick,
    Double,
    Dotted,
    Dashed,
    MediumDashed,
    DashDot,
    MediumDashDot,
    DashDotDot,
    MediumDashDotDot,
    SlantDashDot,
}

impl LineStyle {
    fn parse(text: &str) -> Self {
        match text {
            "hair" => Self::Hair,
            "thin" => Self::Thin,
            "medium" => Self::Medium,
            "thick" => Self::Thick,
            "double" => Self::Double,
            "dotted" => Self::Dotted,
            "dashed" => Self::Dashed,
            "mediumDashed" => Self::MediumDashed,
            "dashDot" => Self::DashDot,
            "mediumDashDot" => Self::MediumDashDot,
            "dashDotDot" => Self::DashDotDot,
            "mediumDashDotDot" => Self::MediumDashDotDot,
            "slantDashDot" => Self::SlantDashDot,
            _ => Self::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Edge {
    pub style: LineStyle,
    pub color: Argb,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Border {
    pub left: Edge,
    pub right: Edge,
    pub top: Edge,
    pub bottom: Edge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HAlign {
    #[default]
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VAlign {
    Top,
    Center,
    #[default]
    Bottom,
    Justify,
    Distributed,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Xf {
    pub num_fmt: u32,
    pub font: usize,
    pub fill: Option<Argb>,
    pub border: Border,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub wrap: bool,
    pub shrink: bool,
    pub indent: u32,
    pub rotation: i32,
    pub reading_order: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Styles {
    pub num_fmts: HashMap<u32, String>,
    pub fonts: Vec<Font>,
    pub xfs: Vec<Xf>,
}

impl Styles {
    #[must_use]
    pub fn xf(&self, index: usize) -> Xf {
        self.xfs.get(index).cloned().unwrap_or_default()
    }

    #[must_use]
    pub fn font(&self, index: usize) -> Font {
        self.fonts.get(index).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub r0: u32,
    pub c0: u32,
    pub r1: u32,
    pub c1: u32,
}

impl Area {
    #[must_use]
    pub const fn contains(&self, r: u32, c: u32) -> bool {
        r >= self.r0 && r <= self.r1 && c >= self.c0 && c <= self.c1
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageSetup {
    pub present: bool,
    pub paper: Option<u32>,
    pub landscape: Option<bool>,
    pub scale: Option<f32>,
    pub fit_to_page: bool,
    pub fit_width: u32,
    pub fit_height: u32,
    pub margins: Option<[f32; 6]>,
    pub grid_lines: bool,
    pub center_h: bool,
    pub center_v: bool,
    pub over_then_down: bool,
    pub header: String,
    pub footer: String,
    pub first_header: String,
    pub first_footer: String,
    pub even_header: String,
    pub even_footer: String,
    pub different_first: bool,
    pub different_odd_even: bool,
    pub first_page_number: Option<u32>,
    pub errors: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub hidden: bool,
    pub cells: BTreeMap<(u32, u32), Cell>,
    pub cols: BTreeMap<u32, (f32, bool)>,
    pub rows: BTreeMap<u32, (Option<f32>, bool)>,
    pub default_col_width: Option<f32>,
    pub base_col_width: Option<f32>,
    pub default_row_height: Option<f32>,
    pub merges: Vec<Area>,
    pub print_area: Option<Vec<Area>>,
    pub print_titles_rows: Option<(u32, u32)>,
    pub print_titles_cols: Option<(u32, u32)>,
    pub setup: PageSetup,
    pub row_breaks: Vec<u32>,
    pub col_breaks: Vec<u32>,
    pub show_grid: bool,
    pub drawings: Vec<Drawing>,
    pub chart_sheet: bool,
    pub right_to_left: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marker {
    pub row: u32,
    pub col: u32,
    pub row_off: f32,
    pub col_off: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    Cells(Marker, Marker),
    Cell(Marker, f32, f32),
    Absolute(f32, f32, f32, f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawing {
    pub place: Place,
    pub element: Element,
    pub part: String,
}

const EMU_PER_POINT: f32 = 12_700.0;

fn marker(e: &Element) -> Marker {
    let n = |k: &str| {
        e.child(k)
            .map(Element::text)
            .and_then(|t| t.trim().parse::<f64>().ok())
    };
    Marker {
        row: n("row").map_or(0, |v| v.max(0.0) as u32),
        col: n("col").map_or(0, |v| v.max(0.0) as u32),
        row_off: n("rowOff").map_or(0.0, |v| v as f32 / EMU_PER_POINT),
        col_off: n("colOff").map_or(0.0, |v| v as f32 / EMU_PER_POINT),
    }
}

fn extent(e: Option<&Element>) -> Option<(f32, f32)> {
    let e = e?;
    let v = |k: &str| {
        e.attr(k)
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v as f32 / EMU_PER_POINT)
    };
    Some((v("cx")?, v("cy")?))
}

fn read_drawing(package: &Package<'_>, part: &str) -> Vec<Drawing> {
    let Ok(root) = package.xml(part) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut anchors: Vec<&Element> = Vec::new();
    for e in root.elements() {
        if e.local() == "AlternateContent" {
            if let Some(choice) = e.elements().next() {
                anchors.extend(choice.elements());
            }
        } else {
            anchors.push(e);
        }
    }
    let emu = |e: Option<&Element>, k: &str| {
        e.and_then(|e| e.attr(k))
            .and_then(|v| v.parse::<f64>().ok())
            .map_or(0.0, |v| v as f32 / EMU_PER_POINT)
    };
    for anchor in anchors {
        let place = match anchor.local() {
            "twoCellAnchor" => {
                let (Some(from), Some(to)) = (anchor.child("from"), anchor.child("to")) else {
                    continue;
                };
                Place::Cells(marker(from), marker(to))
            }
            "oneCellAnchor" => {
                let (Some(from), Some((w, h))) =
                    (anchor.child("from"), extent(anchor.child("ext")))
                else {
                    continue;
                };
                Place::Cell(marker(from), w, h)
            }
            "absoluteAnchor" => {
                let pos = anchor.child("pos");
                let Some((w, h)) = extent(anchor.child("ext")) else {
                    continue;
                };
                Place::Absolute(emu(pos, "x"), emu(pos, "y"), w, h)
            }
            _ => continue,
        };
        if anchor.child("clientData").is_some_and(|c| {
            c.attr("fPrintsWithSheet")
                .is_some_and(|v| v == "0" || v == "false")
        }) {
            continue;
        }
        let object = anchor.elements().find_map(|e| match e.local() {
            "sp" | "grpSp" | "graphicFrame" | "cxnSp" | "pic" => Some(e),
            "AlternateContent" => e
                .elements()
                .next()
                .and_then(|choice| choice.elements().next()),
            _ => None,
        });
        let Some(object) = object else { continue };
        if object
            .elements()
            .find(|c| c.local().starts_with("nv"))
            .and_then(|nv| nv.child("cNvPr"))
            .is_some_and(|c| truthy(c.attr("hidden")))
        {
            continue;
        }
        out.push(Drawing {
            place,
            element: object.clone(),
            part: part.to_owned(),
        });
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    pub styles: Styles,
    pub theme: convert_drawingml::Theme,
    pub date1904: bool,
    pub notes: Vec<String>,
}

#[must_use]
pub fn cell_ref(text: &str) -> Option<(u32, u32)> {
    let text = text.trim().trim_start_matches('$');
    let letters: String = text.chars().take_while(char::is_ascii_alphabetic).collect();
    let digits = text[letters.len()..].trim_start_matches('$');
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut col = 0_u32;
    for c in letters.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = digits.parse().ok()?;
    Some((row.checked_sub(1)?, col - 1))
}

fn col_letters(text: &str) -> Option<u32> {
    let letters = text.trim().trim_start_matches('$');
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut col = 0_u32;
    for c in letters.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    Some(col - 1)
}

#[must_use]
pub fn area(text: &str) -> Option<Area> {
    let (a, b) = text.split_once(':').unwrap_or((text, text));
    if let (Some(a), Some(b)) = (cell_ref(a), cell_ref(b)) {
        return Some(Area {
            r0: a.0.min(b.0),
            c0: a.1.min(b.1),
            r1: a.0.max(b.0),
            c1: a.1.max(b.1),
        });
    }
    if let (Some(a), Some(b)) = (col_letters(a), col_letters(b)) {
        return Some(Area {
            r0: 0,
            c0: a.min(b),
            r1: 1_048_575,
            c1: a.max(b),
        });
    }
    let row = |t: &str| {
        t.trim()
            .trim_start_matches('$')
            .parse::<u32>()
            .ok()?
            .checked_sub(1)
    };
    if let (Some(a), Some(b)) = (row(a), row(b)) {
        return Some(Area {
            r0: a.min(b),
            c0: 0,
            r1: a.max(b),
            c1: 16_383,
        });
    }
    None
}

const INDEXED: [u32; 66] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000,
    0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000,
    0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC,
    0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF,
    0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF,
    0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600,
    0x666699, 0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399,
    0x333333, 0x000000, 0xFFFFFF,
];

fn theme_colors(package: &Package<'_>, workbook_part: &str) -> Vec<u32> {
    let mut out = vec![
        0xFFFFFF, 0x000000, 0xE7E6E6, 0x44546A, 0x4472C4, 0xED7D31, 0xA5A5A5, 0xFFC000, 0x5B9BD5,
        0x70AD47, 0x0563C1, 0x954F72,
    ];
    let Some(rel) = package
        .rels(workbook_part)
        .into_iter()
        .find(|r| r.is("theme"))
    else {
        return out;
    };
    let Ok(theme) = package.xml(&rel.target) else {
        return out;
    };
    let Some(scheme) = theme.descendants("clrScheme").into_iter().next() else {
        return out;
    };
    let order = [
        "lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5",
        "accent6", "hlink", "folHlink",
    ];
    for (slot, name) in order.iter().enumerate() {
        if let Some(entry) = scheme.child(name)
            && let Some(color) = entry.elements().next()
        {
            let value = match color.local() {
                "srgbClr" => color
                    .attr("val")
                    .and_then(|v| u32::from_str_radix(v, 16).ok()),
                "sysClr" => color
                    .attr("lastClr")
                    .and_then(|v| u32::from_str_radix(v, 16).ok()),
                _ => None,
            };
            if let Some(value) = value {
                out[slot] = value;
            }
        }
    }
    out
}

#[must_use]
pub fn tint(rgb: u32, tint: f64) -> u32 {
    if tint == 0.0 {
        return rgb;
    }
    let (r, g, b) = (
        f64::from((rgb >> 16) & 255) / 255.0,
        f64::from((rgb >> 8) & 255) / 255.0,
        f64::from(rgb & 255) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let mut l = (max + min) / 2.0;
    let (mut h, s);
    if (max - min).abs() < 1e-12 {
        h = 0.0;
        s = 0.0;
    } else {
        let d = max - min;
        s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        h = if (max - r).abs() < 1e-12 {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if (max - g).abs() < 1e-12 {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h /= 6.0;
    }
    l = if tint < 0.0 {
        l * (1.0 + tint)
    } else {
        l * (1.0 - tint) + tint
    };
    let hue = |p: f64, q: f64, mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        (
            hue(p, q, h + 1.0 / 3.0),
            hue(p, q, h),
            hue(p, q, h - 1.0 / 3.0),
        )
    };
    let to = |v: f64| ((v.clamp(0.0, 1.0) * 255.0).round() as u32) & 255;
    (to(r) << 16) | (to(g) << 8) | to(b)
}

struct Colors {
    theme: Vec<u32>,
    indexed: Vec<u32>,
}

impl Colors {
    fn read(&self, element: Option<&Element>) -> Option<Argb> {
        let element = element?;
        if element.attr("auto") == Some("1") || element.attr("auto") == Some("true") {
            return None;
        }
        let base = if let Some(rgb) = element.attr("rgb") {
            let v = u32::from_str_radix(rgb.trim(), 16).ok()?;
            if rgb.len() <= 6 { 0xFF00_0000 | v } else { v }
        } else if let Some(theme) = element.attr("theme") {
            let index: usize = theme.parse().ok()?;
            0xFF00_0000 | *self.theme.get(index)?
        } else {
            let indexed = element.attr("indexed")?;
            let index: usize = indexed.parse().ok()?;
            if index == 64 {
                return None;
            }
            0xFF00_0000 | *self.indexed.get(index)?
        };
        let t: f64 = element
            .attr("tint")
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0);
        Some((base & 0xFF00_0000) | tint(base & 0x00FF_FFFF, t))
    }
}

fn truthy(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true"))
}

fn read_font(element: &Element, colors: &Colors) -> Font {
    let val = |name: &str| element.child(name).and_then(|e| e.attr("val"));
    let flag = |name: &str| {
        element
            .child(name)
            .is_some_and(|e| e.attr("val").is_none_or(|v| v != "0" && v != "false"))
    };
    Font {
        name: val("name")
            .or_else(|| val("rFont"))
            .unwrap_or("Calibri")
            .to_owned(),
        size: val("sz").and_then(|s| s.parse().ok()).unwrap_or(11.0),
        bold: flag("b"),
        italic: flag("i"),
        underline: element
            .child("u")
            .is_some_and(|e| e.attr("val") != Some("none")),
        strike: flag("strike"),
        color: colors.read(element.child("color")),
    }
}

fn read_styles(package: &Package<'_>, part: Option<&str>, colors: &Colors) -> Styles {
    let mut styles = Styles::default();
    let Some(root) = part.and_then(|p| package.xml(p).ok()) else {
        styles.fonts.push(Font::default());
        return styles;
    };
    if let Some(formats) = root.child("numFmts") {
        for format in formats.children_named("numFmt") {
            if let (Some(id), Some(code)) = (
                format.attr("numFmtId").and_then(|v| v.parse().ok()),
                format.attr("formatCode"),
            ) {
                styles.num_fmts.insert(id, code.to_owned());
            }
        }
    }
    if let Some(fonts) = root.child("fonts") {
        styles.fonts = fonts
            .children_named("font")
            .map(|f| read_font(f, colors))
            .collect();
    }
    if styles.fonts.is_empty() {
        styles.fonts.push(Font::default());
    }
    let fills: Vec<Option<Argb>> = root
        .child("fills")
        .map(|fills| {
            fills
                .children_named("fill")
                .map(|fill| {
                    if let Some(pattern) = fill.child("patternFill") {
                        let kind = pattern.attr("patternType").unwrap_or("none");
                        if kind == "none" {
                            return None;
                        }
                        let fg = colors.read(pattern.child("fgColor"));
                        let bg = colors.read(pattern.child("bgColor"));
                        if kind == "solid" {
                            return fg.or(bg).or(Some(0xFF00_0000));
                        }
                        let coverage = match kind {
                            "gray125" => 0.125,
                            "gray0625" => 0.0625,
                            "lightGray" => 0.25,
                            "mediumGray" => 0.5,
                            "darkGray" => 0.75,
                            _ => 0.5,
                        };
                        let fg = fg.unwrap_or(0xFF00_0000);
                        let back = bg.unwrap_or(0xFFFF_FFFF);
                        Some(mix(fg, back, coverage))
                    } else if let Some(gradient) = fill.child("gradientFill") {
                        gradient
                            .children_named("stop")
                            .next()
                            .and_then(|stop| colors.read(stop.child("color")))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let borders: Vec<Border> = root
        .child("borders")
        .map(|borders| {
            borders
                .children_named("border")
                .map(|border| {
                    let edge = |name: &str, alt: &str| {
                        let e = border.child(name).or_else(|| border.child(alt));
                        e.map_or_else(Edge::default, |e| Edge {
                            style: LineStyle::parse(e.attr("style").unwrap_or("none")),
                            color: colors.read(e.child("color")).unwrap_or(0xFF00_0000),
                        })
                    };
                    Border {
                        left: edge("left", "start"),
                        right: edge("right", "end"),
                        top: edge("top", "top"),
                        bottom: edge("bottom", "bottom"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(xfs) = root.child("cellXfs") {
        for xf in xfs.children_named("xf") {
            let index = |name: &str| {
                xf.attr(name)
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0)
            };
            let alignment = xf.child("alignment");
            let a = |name: &str| alignment.and_then(|al| al.attr(name));
            styles.xfs.push(Xf {
                num_fmt: xf
                    .attr("numFmtId")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                font: index("fontId"),
                fill: fills.get(index("fillId")).copied().flatten(),
                border: borders.get(index("borderId")).copied().unwrap_or_default(),
                h_align: match a("horizontal") {
                    Some("left") => HAlign::Left,
                    Some("center") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    Some("fill") => HAlign::Fill,
                    Some("justify") => HAlign::Justify,
                    Some("centerContinuous") => HAlign::CenterContinuous,
                    Some("distributed") => HAlign::Distributed,
                    _ => HAlign::General,
                },
                v_align: match a("vertical") {
                    Some("top") => VAlign::Top,
                    Some("center") => VAlign::Center,
                    Some("justify") => VAlign::Justify,
                    Some("distributed") => VAlign::Distributed,
                    _ => VAlign::Bottom,
                },
                wrap: truthy(a("wrapText")),
                shrink: truthy(a("shrinkToFit")),
                indent: a("indent").and_then(|v| v.parse().ok()).unwrap_or(0),
                rotation: a("textRotation").and_then(|v| v.parse().ok()).unwrap_or(0),
                reading_order: a("readingOrder").and_then(|v| v.parse().ok()).unwrap_or(0),
            });
        }
    }
    styles
}

fn mix(fg: Argb, bg: Argb, coverage: f64) -> Argb {
    let channel = |shift: u32| {
        let f = f64::from((fg >> shift) & 255);
        let b = f64::from((bg >> shift) & 255);
        ((f * coverage + b * (1.0 - coverage)).round() as u32) & 255
    };
    0xFF00_0000 | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn read_text(element: &Element, colors: &Colors) -> Vec<TextRun> {
    let mut runs = Vec::new();
    for child in element.elements() {
        match child.local() {
            "t" => runs.push(TextRun {
                text: child.text(),
                font: None,
            }),
            "r" => runs.push(TextRun {
                text: child.child("t").map(Element::text).unwrap_or_default(),
                font: child.child("rPr").map(|p| read_font(p, colors)),
            }),
            _ => {}
        }
    }
    runs
}

fn read_shared_strings(
    package: &Package<'_>,
    part: Option<&str>,
    colors: &Colors,
) -> Vec<Vec<TextRun>> {
    let Some(root) = part.and_then(|p| package.xml(p).ok()) else {
        return Vec::new();
    };
    root.children_named("si")
        .map(|si| read_text(si, colors))
        .collect()
}

#[allow(clippy::too_many_lines)]
fn read_sheet(
    text: &str,
    sheet: &mut Sheet,
    strings: &[Vec<TextRun>],
    colors: &Colors,
) -> Result<(), String> {
    let mut reader = Reader::new(text);
    let mut row: u32 = 0;
    let mut col: u32 = 0;
    let mut cell: Option<(u32, u32, String, usize)> = None;
    let mut value_text: Option<String> = None;
    let mut in_value = false;
    let mut inline: Option<String> = None;
    let mut in_inline_t = false;
    let mut depth_is = 0;
    let mut current_breaks: Option<bool> = None;
    let mut header_target: Option<usize> = None;
    while let Some(event) = reader.next() {
        let event = event.map_err(|e| e.to_string())?;
        match event {
            Event::Start { name, attrs, .. } => {
                let attr = |key: &str| {
                    attrs
                        .iter()
                        .find(|(k, _)| local(k) == key)
                        .map(|(_, v)| v.as_ref())
                };
                match local(name) {
                    "row" => {
                        if let Some(r) = attr("r").and_then(|v| v.parse::<u32>().ok()) {
                            row = r.saturating_sub(1);
                        }
                        col = 0;
                        let height = attr("ht").and_then(|v| v.parse::<f32>().ok());
                        let hidden = truthy(attr("hidden"));
                        if height.is_some() || hidden {
                            sheet.rows.insert(row, (height, hidden));
                        }
                    }
                    "c" => {
                        let (r, c) = attr("r").and_then(cell_ref).unwrap_or((row, col));
                        row = r;
                        col = c;
                        let kind = attr("t").unwrap_or("n").to_owned();
                        let style = attr("s").and_then(|v| v.parse().ok()).unwrap_or(0);
                        cell = Some((r, c, kind, style));
                        value_text = None;
                        inline = None;
                    }
                    "v" if cell.is_some() => {
                        in_value = true;
                        value_text = Some(String::new());
                    }
                    "is" if cell.is_some() => {
                        depth_is += 1;
                        inline = Some(String::new());
                    }
                    "t" if depth_is > 0 => in_inline_t = true,
                    "col" => {
                        let min: u32 = attr("min").and_then(|v| v.parse().ok()).unwrap_or(1);
                        let max: u32 = attr("max").and_then(|v| v.parse().ok()).unwrap_or(min);
                        let width: Option<f32> = attr("width").and_then(|v| v.parse().ok());
                        let hidden = truthy(attr("hidden"));
                        if let Some(width) = width.or(hidden.then_some(0.0)) {
                            for c in min.max(1)..=max.min(16_384) {
                                sheet.cols.insert(c - 1, (width, hidden));
                            }
                        }
                    }
                    "sheetFormatPr" => {
                        sheet.default_col_width =
                            attr("defaultColWidth").and_then(|v| v.parse().ok());
                        sheet.base_col_width = attr("baseColWidth").and_then(|v| v.parse().ok());
                        sheet.default_row_height =
                            attr("defaultRowHeight").and_then(|v| v.parse().ok());
                    }
                    "sheetView" => {
                        sheet.show_grid = attr("showGridLines") != Some("0");
                        sheet.right_to_left = truthy(attr("rightToLeft"));
                    }
                    "mergeCell" => {
                        if let Some(a) = attr("ref").and_then(area) {
                            sheet.merges.push(a);
                        }
                    }
                    "pageSetUpPr" => {
                        if truthy(attr("fitToPage")) {
                            sheet.setup.fit_to_page = true;
                            sheet.setup.present = true;
                        }
                    }
                    "printOptions" => {
                        sheet.setup.grid_lines = truthy(attr("gridLines"));
                        sheet.setup.center_h = truthy(attr("horizontalCentered"));
                        sheet.setup.center_v = truthy(attr("verticalCentered"));
                    }
                    "pageMargins" => {
                        let m = |key: &str, default: f32| {
                            attr(key).and_then(|v| v.parse().ok()).unwrap_or(default)
                        };
                        sheet.setup.margins = Some([
                            m("left", 0.7),
                            m("right", 0.7),
                            m("top", 0.75),
                            m("bottom", 0.75),
                            m("header", 0.3),
                            m("footer", 0.3),
                        ]);
                    }
                    "pageSetup" => {
                        let s = &mut sheet.setup;
                        s.paper = attr("paperSize").and_then(|v| v.parse().ok());
                        s.landscape = attr("orientation").map(|v| v == "landscape");
                        s.scale = attr("scale").and_then(|v| v.parse().ok());
                        s.fit_width = attr("fitToWidth").and_then(|v| v.parse().ok()).unwrap_or(1);
                        s.fit_height = attr("fitToHeight")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1);
                        s.over_then_down = attr("pageOrder") == Some("overThenDown");
                        if truthy(attr("useFirstPageNumber")) {
                            s.first_page_number =
                                attr("firstPageNumber").and_then(|v| v.parse().ok());
                        }
                        s.errors = attr("errors")
                            .filter(|e| *e != "displayed")
                            .map(str::to_owned);
                        if s.paper.is_some() || s.landscape.is_some() || s.scale.is_some() {
                            s.present = true;
                        }
                    }
                    "brk" => {
                        if let Some(id) = attr("id").and_then(|v| v.parse::<u32>().ok()) {
                            match current_breaks {
                                Some(true) => sheet.row_breaks.push(id),
                                Some(false) => sheet.col_breaks.push(id),
                                None => {}
                            }
                        }
                    }
                    "rowBreaks" => current_breaks = Some(true),
                    "colBreaks" => current_breaks = Some(false),
                    "oddHeader" => header_target = Some(0),
                    "oddFooter" => header_target = Some(1),
                    "evenHeader" => header_target = Some(2),
                    "evenFooter" => header_target = Some(3),
                    "firstHeader" => header_target = Some(4),
                    "firstFooter" => header_target = Some(5),
                    "headerFooter" => {
                        sheet.setup.different_first = truthy(attr("differentFirst"));
                        sheet.setup.different_odd_even = truthy(attr("differentOddEven"));
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                if in_value {
                    if let Some(v) = value_text.as_mut() {
                        v.push_str(&text);
                    }
                } else if in_inline_t {
                    if let Some(v) = inline.as_mut() {
                        v.push_str(&text);
                    }
                } else if let Some(target) = header_target {
                    let s = &mut sheet.setup;
                    match target {
                        0 => &mut s.header,
                        1 => &mut s.footer,
                        2 => &mut s.even_header,
                        3 => &mut s.even_footer,
                        4 => &mut s.first_header,
                        _ => &mut s.first_footer,
                    }
                    .push_str(&text);
                }
            }
            Event::End { name } => match local(name) {
                "v" => in_value = false,
                "t" => in_inline_t = false,
                "is" => depth_is -= 1,
                "rowBreaks" | "colBreaks" => current_breaks = None,
                "oddHeader" | "oddFooter" | "evenHeader" | "evenFooter" | "firstHeader"
                | "firstFooter" => header_target = None,
                "c" => {
                    if let Some((r, c, kind, style)) = cell.take() {
                        let value = match kind.as_str() {
                            "s" => value_text
                                .as_deref()
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .and_then(|i| strings.get(i).cloned())
                                .map(Value::Text),
                            "inlineStr" => inline.take().map(|t| {
                                Value::Text(vec![TextRun {
                                    text: t,
                                    font: None,
                                }])
                            }),
                            "str" => value_text.take().map(|t| {
                                Value::Text(vec![TextRun {
                                    text: t,
                                    font: None,
                                }])
                            }),
                            "b" => value_text.as_deref().map(|v| Value::Bool(v.trim() == "1")),
                            "e" => value_text.take().map(Value::Error),
                            "d" => value_text.take().map(|t| {
                                Value::Text(vec![TextRun {
                                    text: t,
                                    font: None,
                                }])
                            }),
                            _ => value_text
                                .as_deref()
                                .and_then(|v| v.trim().parse::<f64>().ok())
                                .map(Value::Number),
                        };
                        let _ = colors;
                        sheet.cells.insert((r, c), Cell { value, style });
                        col = c + 1;
                    }
                }
                _ => {}
            },
        }
    }
    Ok(())
}

type Titles = (Option<(u32, u32)>, Option<(u32, u32)>);

pub fn read(package: &Package<'_>) -> Result<Workbook, String> {
    let workbook_part = package
        .main_part()
        .unwrap_or_else(|| "xl/workbook.xml".to_owned());
    let root = package.xml(&workbook_part).map_err(|e| e.to_string())?;
    if root.local() != "workbook" {
        return Err("this is not an Excel workbook".to_owned());
    }
    let rels = package.rels(&workbook_part);
    let colors = Colors {
        theme: theme_colors(package, &workbook_part),
        indexed: {
            let mut indexed: Vec<u32> = INDEXED.to_vec();
            if let Some(custom) = rels
                .iter()
                .find(|r| r.is("styles"))
                .and_then(|r| package.xml(&r.target).ok())
                .and_then(|s| s.path(&["colors", "indexedColors"]).cloned())
            {
                for (slot, rgb) in custom.children_named("rgbColor").enumerate() {
                    if let Some(v) = rgb
                        .attr("rgb")
                        .and_then(|v| u32::from_str_radix(v, 16).ok())
                        && slot < indexed.len()
                    {
                        indexed[slot] = v & 0x00FF_FFFF;
                    }
                }
            }
            indexed
        },
    };
    let styles_part = rels
        .iter()
        .find(|r| r.is("styles"))
        .map(|r| r.target.clone());
    let strings_part = rels
        .iter()
        .find(|r| r.is("sharedStrings"))
        .map(|r| r.target.clone());
    let styles = read_styles(package, styles_part.as_deref(), &colors);
    let strings = read_shared_strings(package, strings_part.as_deref(), &colors);
    let date1904 = root
        .child("workbookPr")
        .is_some_and(|p| truthy(p.attr("date1904")));
    let mut sheets = Vec::new();
    let notes = Vec::new();
    let mut print_areas: HashMap<usize, Vec<Area>> = HashMap::new();
    let mut print_titles: HashMap<usize, Titles> = HashMap::new();
    if let Some(names) = root.child("definedNames") {
        for name in names.children_named("definedName") {
            let Some(sheet) = name
                .attr("localSheetId")
                .and_then(|v| v.parse::<usize>().ok())
            else {
                continue;
            };
            let text = name.text();
            let parts = text.split(',').map(|part| {
                let part = part.trim();
                part.rsplit_once('!').map_or(part, |(_, r)| r).to_owned()
            });
            match name.attr("name") {
                Some("_xlnm.Print_Area") => {
                    let areas: Vec<Area> = parts.filter_map(|p| area(&p)).collect();
                    if !areas.is_empty() {
                        print_areas.insert(sheet, areas);
                    }
                }
                Some("_xlnm.Print_Titles") => {
                    let entry = print_titles.entry(sheet).or_default();
                    for part in parts {
                        if let Some(a) = area(&part) {
                            if a.c0 == 0 && a.c1 >= 16_383 {
                                entry.0 = Some((a.r0, a.r1));
                            } else if a.r0 == 0 && a.r1 >= 1_048_575 {
                                entry.1 = Some((a.c0, a.c1));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let list = root.child("sheets").ok_or("the workbook lists no sheets")?;
    for (index, entry) in list.children_named("sheet").enumerate() {
        let name = entry.attr("name").unwrap_or("Sheet").to_owned();
        let hidden = matches!(entry.attr("state"), Some("hidden" | "veryHidden"));
        let Some(rel) = entry
            .attr("r:id")
            .or_else(|| entry.attr("id"))
            .and_then(|id| rels.iter().find(|r| r.id == id))
        else {
            continue;
        };
        let chart_sheet = rel.is("chartsheet");
        if !rel.is("worksheet") && !chart_sheet {
            continue;
        }
        let Ok(bytes) = package.bytes(&rel.target) else {
            continue;
        };
        let text = convert_office_read::xml::decode(&bytes);
        let mut sheet = Sheet {
            name,
            hidden,
            show_grid: true,
            chart_sheet,
            ..Sheet::default()
        };
        read_sheet(&text, &mut sheet, &strings, &colors)
            .map_err(|e| format!("sheet '{}': {e}", sheet.name))?;
        for drawing in package.rels(&rel.target).iter().filter(|r| r.is("drawing")) {
            sheet
                .drawings
                .extend(read_drawing(package, &drawing.target));
        }
        sheet.print_area = print_areas.remove(&index);
        if let Some((rows, cols)) = print_titles.remove(&index) {
            sheet.print_titles_rows = rows;
            sheet.print_titles_cols = cols;
        }
        sheets.push(sheet);
    }
    let theme = rels
        .iter()
        .find(|r| r.is("theme"))
        .and_then(|r| package.xml(&r.target).ok());
    Ok(Workbook {
        sheets,
        styles,
        theme: convert_drawingml::Theme::read(theme.as_ref()),
        date1904,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references() {
        assert_eq!(cell_ref("A1"), Some((0, 0)));
        assert_eq!(cell_ref("$AB$12"), Some((11, 27)));
        assert_eq!(
            area("B2:A1"),
            Some(Area {
                r0: 0,
                c0: 0,
                r1: 1,
                c1: 1
            })
        );
        assert_eq!(area("$1:$2").map(|a| (a.r0, a.r1)), Some((0, 1)));
        assert_eq!(area("$A:$B").map(|a| (a.c0, a.c1)), Some((0, 1)));
        assert_eq!(tint(0x4472C4, 0.0), 0x4472C4);
        assert_eq!(tint(0x000000, 0.5), 0x808080);
    }

    #[test]
    fn drawing_markers() {
        let e = convert_office_read::xml::parse(
            "<xdr:from><xdr:col>4</xdr:col><xdr:colOff>127000</xdr:colOff>\
             <xdr:row>2</xdr:row><xdr:rowOff>25400</xdr:rowOff></xdr:from>",
        )
        .unwrap();
        let m = marker(&e);
        assert_eq!((m.row, m.col), (2, 4));
        assert!((m.col_off - 10.0).abs() < 1e-4 && (m.row_off - 2.0).abs() < 1e-4);
    }
}
