use std::collections::HashMap;

use convert_office_read::Element;
use convert_pdf_canvas::{Line, Rgb, Span, layout};

use crate::geometry::{EMU, flag, num};
use crate::scene::{Frame, Scene, dash};
use crate::text::{RunProps, script_spans};
use crate::theme::Color;

#[derive(Clone, Debug, PartialEq)]
struct Font {
    size: f32,
    bold: bool,
    italic: bool,
    color: Rgb,
    latin: String,
    cs: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bar { horizontal: bool },
    Line,
    Area,
    Pie,
    Doughnut,
    Scatter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grouping {
    Clustered,
    Stacked,
    Percent,
}

#[derive(Clone, Debug, Default)]
struct Labels {
    value: bool,
    percent: bool,
    category: bool,
    series: bool,
    position: Option<String>,
    format: Option<String>,
    font: Option<Element>,
    separator: Option<String>,
    deleted: Vec<usize>,
}

impl Labels {
    fn read(e: Option<&Element>) -> Option<Self> {
        let e = e?;
        if e.child("delete").and_then(|d| flag(d, "val")) == Some(true) {
            return Some(Self::default());
        }
        let show = |k: &str| e.child(k).and_then(|d| flag(d, "val")).unwrap_or(false);
        Some(Self {
            value: show("showVal"),
            percent: show("showPercent"),
            category: show("showCatName"),
            series: show("showSerName"),
            position: e
                .child("dLblPos")
                .and_then(|p| p.attr("val"))
                .map(str::to_owned),
            format: e
                .child("numFmt")
                .filter(|f| f.attr("sourceLinked") != Some("1"))
                .and_then(|f| f.attr("formatCode"))
                .map(str::to_owned),
            font: e.child("txPr").cloned(),
            separator: e.child("separator").map(Element::text),
            deleted: e
                .children_named("dLbl")
                .filter(|d| d.child("delete").and_then(|x| flag(x, "val")) == Some(true))
                .filter_map(|d| d.child("idx").and_then(|i| num(i, "val")))
                .map(|v| v as usize)
                .collect(),
        })
    }

    fn any(&self) -> bool {
        self.value || self.percent || self.category || self.series
    }
}

#[derive(Clone, Debug, Default)]
struct Series {
    index: usize,
    order: usize,
    name: String,
    categories: Vec<String>,
    values: Vec<Option<f64>>,
    xs: Vec<Option<f64>>,
    format: String,
    shape: Option<Element>,
    points: HashMap<usize, Element>,
    marker: Option<Element>,
    labels: Option<Labels>,
    smooth: bool,
    value_ref: Option<String>,
    category_ref: Option<String>,
    x_ref: Option<String>,
}

#[derive(Clone, Debug)]
struct Group {
    kind: Kind,
    grouping: Grouping,
    series: Vec<Series>,
    vary: bool,
    gap: f32,
    overlap: f32,
    hole: f32,
    first_angle: f32,
    labels: Option<Labels>,
    axes: Vec<String>,
    markers: bool,
    style: String,
}

#[derive(Clone, Debug, Default)]
struct Axis {
    id: String,
    values: bool,
    deleted: bool,
    position: String,
    reversed: bool,
    min: Option<f64>,
    max: Option<f64>,
    major: Option<f64>,
    gridlines: Option<Element>,
    format: Option<String>,
    title: Option<Element>,
    font: Option<Element>,
    shape: Option<Element>,
    labels: bool,
    crosses_max: bool,
    between: Option<bool>,
}

type SecondAxis = (Axis, (f64, f64, bool), Font, String);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Scale {
    lo: f64,
    hi: f64,
    unit: f64,
}

impl Scale {
    fn at(&self, v: f64) -> f64 {
        if (self.hi - self.lo).abs() < 1e-12 {
            0.0
        } else {
            (v - self.lo) / (self.hi - self.lo)
        }
    }

    fn ticks(&self) -> Vec<f64> {
        let mut out = Vec::new();
        if self.unit <= 0.0 {
            return out;
        }
        let mut k = 0;
        loop {
            let v = self.lo + self.unit * f64::from(k);
            if v > self.hi + self.unit * 1e-6 || k > 200 {
                break;
            }
            out.push(if v.abs() < self.unit * 1e-9 { 0.0 } else { v });
            k += 1;
        }
        out
    }
}

fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(raw.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= raw * (1.0 - 1e-9) {
            return m * p;
        }
    }
    10.0 * p
}

fn auto_scale(mut lo: f64, mut hi: f64, count: f64, min: Option<f64>, max: Option<f64>) -> Scale {
    if !lo.is_finite() || !hi.is_finite() {
        lo = 0.0;
        hi = 1.0;
    }
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    if lo >= 0.0 && (hi <= 0.0 || (hi - lo) / hi > 1.0 / 6.0) {
        lo = 0.0;
    } else if hi <= 0.0 && (hi - lo) / -lo > 1.0 / 6.0 {
        hi = 0.0;
    }
    let span = hi - lo;
    let (mut top, mut bottom) = (hi, lo);
    if hi > 0.0 && max.is_none() {
        top = hi + span * 0.05;
    }
    if lo < 0.0 && min.is_none() {
        bottom = lo - span * 0.05;
    }
    if let Some(m) = min {
        bottom = m;
    }
    if let Some(m) = max {
        top = m;
    }
    if (top - bottom).abs() < 1e-12 {
        top = bottom + 1.0;
    }
    let count = count.max(1.0);
    let mut unit = nice_step((top - bottom) / count);
    loop {
        let steps = (top / unit - 1e-9).ceil() - (bottom / unit + 1e-9).floor();
        if steps <= count || unit > (top - bottom) * 10.0 {
            break;
        }
        unit = nice_step(unit * 1.01);
    }
    let lo = if min.is_some() {
        bottom
    } else {
        (bottom / unit + 1e-9).floor() * unit
    };
    let hi = if max.is_some() {
        top
    } else {
        (top / unit - 1e-9).ceil() * unit
    };
    Scale { lo, hi, unit }
}

fn cached_strings(e: Option<&Element>, format: &dyn Fn(&str, f64) -> String) -> Vec<String> {
    let Some(e) = e else { return Vec::new() };
    let cache = e
        .elements()
        .find_map(|r| match r.local() {
            "strRef" => r.child("strCache"),
            "numRef" => r.child("numCache"),
            "multiLvlStrRef" => r
                .child("multiLvlStrCache")
                .and_then(|c| c.children_named("lvl").next()),
            "strLit" | "numLit" => Some(r),
            _ => None,
        })
        .or_else(|| e.child("v").map(|_| e));
    let Some(cache) = cache else {
        return Vec::new();
    };
    let numeric = matches!(
        e.elements().next().map(Element::local),
        Some("numRef" | "numLit")
    );
    let code = cache
        .child("formatCode")
        .map(Element::text)
        .unwrap_or_default();
    let count = cache
        .child("ptCount")
        .and_then(|c| num(c, "val"))
        .map_or(0, |v| v as usize);
    let mut out = vec![String::new(); count];
    for pt in cache.children_named("pt") {
        let idx = pt
            .attr("idx")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if idx > 100_000 {
            continue;
        }
        let text = pt.child("v").map(Element::text).unwrap_or_default();
        let text = match (numeric, text.trim().parse::<f64>()) {
            (true, Ok(v)) => {
                let code = pt.attr("formatCode").unwrap_or(&code);
                format(if code.is_empty() { "General" } else { code }, v)
            }
            _ => text,
        };
        if out.len() <= idx {
            out.resize(idx + 1, String::new());
        }
        out[idx] = text;
    }
    out
}

fn cached_numbers(e: Option<&Element>) -> (Vec<Option<f64>>, String) {
    let Some(e) = e else {
        return (Vec::new(), String::new());
    };
    let cache = e.elements().find_map(|r| match r.local() {
        "numRef" => r.child("numCache"),
        "strRef" => r.child("strCache"),
        "numLit" | "strLit" => Some(r),
        _ => None,
    });
    let Some(cache) = cache else {
        return (Vec::new(), String::new());
    };
    let code = cache
        .child("formatCode")
        .map(Element::text)
        .unwrap_or_default();
    let count = cache
        .child("ptCount")
        .and_then(|c| num(c, "val"))
        .map_or(0, |v| v as usize);
    let mut out = vec![None; count.min(100_000)];
    for pt in cache.children_named("pt") {
        let idx = pt
            .attr("idx")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if idx > 100_000 {
            continue;
        }
        if out.len() <= idx {
            out.resize(idx + 1, None);
        }
        out[idx] = pt
            .child("v")
            .and_then(|v| v.text().trim().parse::<f64>().ok())
            .filter(|v| v.is_finite());
    }
    (out, code)
}

fn formula(e: Option<&Element>) -> Option<String> {
    e?.elements()
        .find_map(|r| r.child("f"))
        .map(Element::text)
        .filter(|f| !f.trim().is_empty())
}

fn visible_only(groups: &mut [Group], hidden: &dyn Fn(&str) -> Option<Vec<bool>>) {
    let flags = |f: &Option<String>| f.as_deref().and_then(hidden);
    for g in groups.iter_mut() {
        g.series
            .retain(|s| flags(&s.value_ref).is_none_or(|v| v.is_empty() || v.iter().any(|h| !h)));
        if g.kind == Kind::Scatter {
            for s in &mut g.series {
                let (ys, xs) = (flags(&s.value_ref), flags(&s.x_ref));
                let keep: Vec<bool> = (0..s.values.len())
                    .map(|k| {
                        !ys.as_ref().is_some_and(|v| v.get(k) == Some(&true))
                            && !xs.as_ref().is_some_and(|v| v.get(k) == Some(&true))
                    })
                    .collect();
                filter_points(s, &keep);
            }
            continue;
        }
        let count = g.series.iter().map(|s| s.values.len()).max().unwrap_or(0);
        let values: Vec<Option<Vec<bool>>> = g.series.iter().map(|s| flags(&s.value_ref)).collect();
        let categories = g.series.iter().find_map(|s| flags(&s.category_ref));
        let keep: Vec<bool> = (0..count)
            .map(|k| {
                let cat_hidden = categories.as_ref().is_some_and(|c| c.get(k) == Some(&true));
                let all_hidden = !values.is_empty()
                    && values
                        .iter()
                        .all(|v| v.as_ref().is_some_and(|v| v.get(k) == Some(&true)));
                !(cat_hidden || all_hidden)
            })
            .collect();
        if keep.iter().all(|k| *k) {
            continue;
        }
        for s in &mut g.series {
            filter_points(s, &keep);
        }
    }
}

fn filter_points(s: &mut Series, keep: &[bool]) {
    let mut new_index: HashMap<usize, usize> = HashMap::new();
    let mut next = 0;
    for (k, &kept) in keep.iter().enumerate() {
        if kept {
            new_index.insert(k, next);
            next += 1;
        }
    }
    let pick = |len: usize| -> Vec<usize> {
        (0..len)
            .filter(|k| keep.get(*k).is_none_or(|x| *x))
            .collect()
    };
    s.values = pick(s.values.len())
        .into_iter()
        .map(|k| s.values[k])
        .collect();
    s.categories = pick(s.categories.len())
        .into_iter()
        .map(|k| s.categories[k].clone())
        .collect();
    s.xs = pick(s.xs.len()).into_iter().map(|k| s.xs[k]).collect();
    s.points = std::mem::take(&mut s.points)
        .into_iter()
        .filter_map(|(k, v)| {
            if keep.get(k).is_none_or(|x| *x) {
                Some((new_index.get(&k).copied().unwrap_or(k), v))
            } else {
                None
            }
        })
        .collect();
    if let Some(l) = s.labels.as_mut() {
        l.deleted = l
            .deleted
            .iter()
            .filter_map(|k| new_index.get(k).copied())
            .collect();
    }
}

fn series_name(e: &Element, index: usize) -> String {
    let Some(tx) = e.child("tx") else {
        return format!("Series{}", index + 1);
    };
    if let Some(v) = tx.child("v") {
        return v.text();
    }
    tx.descendants("v")
        .into_iter()
        .map(Element::text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_series(e: &Element, position: usize, format: &dyn Fn(&str, f64) -> String) -> Series {
    let index = e
        .child("idx")
        .and_then(|i| num(i, "val"))
        .map_or(position, |v| v as usize);
    let order = e
        .child("order")
        .and_then(|i| num(i, "val"))
        .map_or(position, |v| v as usize);
    let (values, code) = cached_numbers(e.child("val").or_else(|| e.child("yVal")));
    let categories = cached_strings(e.child("cat").or_else(|| e.child("xVal")), format);
    let (xs, _) = match e.child("xVal") {
        Some(x)
            if x.elements()
                .any(|r| matches!(r.local(), "numRef" | "numLit")) =>
        {
            cached_numbers(Some(x))
        }
        _ => (Vec::new(), String::new()),
    };
    let points = e
        .children_named("dPt")
        .filter_map(|p| {
            let idx = p.child("idx").and_then(|i| num(i, "val"))? as usize;
            Some((idx, p.child("spPr")?.clone()))
        })
        .collect();
    Series {
        index,
        order,
        name: series_name(e, index),
        categories,
        values,
        xs,
        format: code,
        shape: e.child("spPr").cloned(),
        points,
        marker: e.child("marker").cloned(),
        labels: Labels::read(e.child("dLbls")),
        smooth: e
            .child("smooth")
            .and_then(|v| flag(v, "val"))
            .unwrap_or(false),
        value_ref: formula(e.child("val").or_else(|| e.child("yVal"))),
        category_ref: formula(e.child("cat")),
        x_ref: formula(e.child("xVal")),
    }
}

fn read_group(e: &Element, format: &dyn Fn(&str, f64) -> String) -> Option<Group> {
    let val = |k: &str| e.child(k).and_then(|c| c.attr("val"));
    let kind = match e.local() {
        "barChart" | "bar3DChart" => Kind::Bar {
            horizontal: val("barDir") == Some("bar"),
        },
        "lineChart" | "line3DChart" => Kind::Line,
        "areaChart" | "area3DChart" => Kind::Area,
        "pieChart" | "pie3DChart" => Kind::Pie,
        "doughnutChart" => Kind::Doughnut,
        "scatterChart" => Kind::Scatter,
        _ => return None,
    };
    let grouping = match val("grouping") {
        Some("stacked") => Grouping::Stacked,
        Some("percentStacked") => Grouping::Percent,
        _ => Grouping::Clustered,
    };
    let mut series: Vec<Series> = e
        .children_named("ser")
        .enumerate()
        .map(|(k, s)| read_series(s, k, format))
        .collect();
    series.sort_by_key(|s| s.order);
    let number = |k: &str| val(k).and_then(|v| v.trim_end_matches('%').parse::<f32>().ok());
    Some(Group {
        kind,
        grouping,
        series,
        vary: val("varyColors").is_some_and(|v| v == "1" || v == "true"),
        gap: number("gapWidth").unwrap_or(150.0),
        overlap: number("overlap").unwrap_or(if grouping == Grouping::Clustered {
            0.0
        } else {
            100.0
        }),
        hole: number("holeSize").unwrap_or(50.0),
        first_angle: number("firstSliceAng").unwrap_or(0.0),
        labels: Labels::read(e.child("dLbls")),
        axes: e
            .children_named("axId")
            .filter_map(|a| a.attr("val").map(str::to_owned))
            .collect(),
        markers: val("marker").is_none_or(|v| v == "1" || v == "true"),
        style: val("scatterStyle").unwrap_or("lineMarker").to_owned(),
    })
}

fn read_axis(e: &Element) -> Axis {
    let val = |k: &str| e.child(k).and_then(|c| c.attr("val"));
    let scaling = e.child("scaling");
    let bound = |k: &str| {
        scaling
            .and_then(|s| s.child(k))
            .and_then(|c| c.attr("val"))
            .and_then(|v| v.parse::<f64>().ok())
    };
    Axis {
        id: val("axId").unwrap_or_default().to_owned(),
        values: e.local() == "valAx",
        deleted: val("delete").is_some_and(|v| v == "1" || v == "true"),
        position: val("axPos").unwrap_or("b").to_owned(),
        reversed: scaling
            .and_then(|s| s.child("orientation"))
            .and_then(|o| o.attr("val"))
            == Some("maxMin"),
        min: bound("min"),
        max: bound("max"),
        major: val("majorUnit")
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| *v > 0.0),
        gridlines: e.child("majorGridlines").cloned(),
        format: e
            .child("numFmt")
            .filter(|f| f.attr("sourceLinked") != Some("1"))
            .and_then(|f| f.attr("formatCode"))
            .map(str::to_owned),
        title: e.child("title").cloned(),
        font: e.child("txPr").cloned(),
        shape: e.child("spPr").cloned(),
        labels: val("tickLblPos") != Some("none"),
        crosses_max: val("crosses") == Some("max"),
        between: val("crossBetween").map(|v| v == "between"),
    }
}

fn accent_for(scene: &Scene<'_, '_>, index: usize) -> Color {
    let names = [
        "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    ];
    let base = scene
        .theme
        .colors
        .get(names[index % 6])
        .copied()
        .unwrap_or(0x4472C4);
    let c = Color::rgb(base);
    let round = index / 6;
    let (m, off) = match round % 5 {
        0 => (1.0, 0.0),
        1 => (0.6, 0.0),
        2 => (0.8, 0.2),
        3 => (0.8, 0.0),
        _ => (0.6, 0.4),
    };
    Color {
        r: (c.r * m + off).min(1.0),
        g: (c.g * m + off).min(1.0),
        b: (c.b * m + off).min(1.0),
        a: 1.0,
    }
}

#[derive(Clone, Debug)]
struct Paint {
    fill: Option<Color>,
    line: Option<Color>,
    width: f32,
    dash: Vec<f32>,
}

#[must_use]
pub fn simple_format(code: &str, v: f64) -> String {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        return general(v);
    }
    let section = code.split(';').next().unwrap_or(code);
    let percent = section.contains('%');
    let decimals = section.split_once('.').map_or(0, |(_, d)| {
        d.chars().take_while(|c| *c == '0' || *c == '#').count()
    });
    let grouped = section.contains(",");
    let v = if percent { v * 100.0 } else { v };
    let mut text = format!("{:.*}", decimals, v.abs());
    if grouped {
        let (int, frac) = text
            .split_once('.')
            .map_or((text.as_str(), ""), |(a, b)| (a, b));
        let mut out = String::new();
        for (k, c) in int.chars().enumerate() {
            if k > 0 && (int.len() - k) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        if !frac.is_empty() {
            out.push('.');
            out.push_str(frac);
        }
        text = out;
    }
    let sign = if v < 0.0 && text.chars().any(|c| c.is_ascii_digit() && c != '0') {
        "-"
    } else {
        ""
    };
    format!("{sign}{text}{}", if percent { "%" } else { "" })
}

#[must_use]
pub fn general(v: f64) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    let a = v.abs();
    if !(1e-9..1e11).contains(&a) {
        let text = format!("{v:.5E}");
        return text;
    }
    let digits = (10 - (a.log10().floor() as i32 + 1)).clamp(0, 10) as usize;
    let mut text = format!("{v:.digits$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" { "0".to_owned() } else { text }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.w
    }

    fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

struct Block {
    lines: Vec<Line>,
    width: f32,
    height: f32,
}

impl Scene<'_, '_> {
    fn chart_font(&self, base: &Font, tx: Option<&Element>) -> Font {
        let mut font = base.clone();
        let Some(tx) = tx else { return font };
        let Some(d) = tx
            .children_named("p")
            .next()
            .and_then(|p| p.path(&["pPr", "defRPr"]))
        else {
            return font;
        };
        let mut run = RunProps::default();
        self.merge_run(&mut run, d, None);
        self.apply_run(&mut font, &run);
        font
    }

    fn apply_run(&self, font: &mut Font, run: &RunProps) {
        if let Some(s) = run.size {
            font.size = s;
        }
        if let Some(b) = run.bold {
            font.bold = b;
        }
        if let Some(i) = run.italic {
            font.italic = i;
        }
        if let Some(c) = run.color {
            font.color = c.pdf();
        }
        if let Some(l) = run.latin.as_ref().filter(|l| !l.is_empty()) {
            font.latin = self.theme.font(l);
        }
        if let Some(l) = run.cs.as_ref().filter(|l| !l.is_empty()) {
            font.cs = self.theme.font(l);
        }
    }

    fn spans(&self, text: &str, font: &Font) -> Vec<Span> {
        let span = Span {
            text: text.to_owned(),
            family: font.latin.clone(),
            size: font.size,
            bold: font.bold,
            italic: font.italic,
            underline: false,
            strike: false,
            color: font.color,
        };
        script_spans(&span, &font.cs)
    }

    fn rich_spans(&self, rich: &Element, base: &Font) -> Vec<Span> {
        let mut out: Vec<Span> = Vec::new();
        for (k, p) in rich.children_named("p").enumerate() {
            let mut para = base.clone();
            if let Some(d) = p.path(&["pPr", "defRPr"]) {
                let mut run = RunProps::default();
                self.merge_run(&mut run, d, None);
                self.apply_run(&mut para, &run);
            }
            if k > 0 {
                out.extend(self.spans("\n", &para));
            }
            for r in p.elements().filter(|e| matches!(e.local(), "r" | "fld")) {
                let mut font = para.clone();
                if let Some(rpr) = r.child("rPr") {
                    let mut run = RunProps::default();
                    self.merge_run(&mut run, rpr, None);
                    self.apply_run(&mut font, &run);
                }
                let text = r.child("t").map(Element::text).unwrap_or_default();
                out.extend(self.spans(&text, &font));
            }
        }
        out
    }

    fn block(&mut self, spans: &[Span], width: Option<f32>) -> Block {
        if spans.iter().all(|s| s.text.trim().is_empty()) {
            return Block {
                lines: Vec::new(),
                width: 0.0,
                height: 0.0,
            };
        }
        let lines = layout(self.canvas, self.fonts, spans, width);
        let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let height = lines.iter().map(Line::height).sum();
        Block {
            lines,
            width,
            height,
        }
    }

    fn draw_block(&mut self, block: &Block, x: f32, y: f32, width: f32, align: &str, h: f32) {
        let mut top = y;
        for line in &block.lines {
            let lx = match align {
                "ctr" => x + (width - line.width) / 2.0,
                "r" => x + width - line.width,
                _ => x,
            };
            let baseline = top + line.ascent;
            line.draw(self.canvas.page(self.page), lx, h - baseline, 0.0);
            top += line.height();
        }
    }

    fn series_paint(&self, shape: Option<&Element>, auto: Color, kind: Kind) -> Paint {
        let line_kind = matches!(kind, Kind::Line | Kind::Scatter);
        let mut paint = Paint {
            fill: (!line_kind).then_some(auto),
            line: line_kind.then_some(auto),
            width: if line_kind { 2.25 } else { 0.75 },
            dash: Vec::new(),
        };
        let Some(sp) = shape else { return paint };
        if let Some(f) = sp.elements().find(|c| {
            matches!(
                c.local(),
                "noFill" | "solidFill" | "gradFill" | "pattFill" | "blipFill"
            )
        }) {
            paint.fill = match f.local() {
                "noFill" => None,
                "solidFill" => self.palette(None).first_color(f),
                "gradFill" => f
                    .path(&["gsLst", "gs"])
                    .and_then(|g| self.palette(None).first_color(g)),
                "pattFill" => f
                    .child("fgClr")
                    .and_then(|g| self.palette(None).first_color(g)),
                _ => Some(auto),
            };
        }
        if let Some(ln) = sp.child("ln") {
            if let Some(w) = num(ln, "w") {
                paint.width = w / EMU;
            }
            if ln.child("noFill").is_some() {
                paint.line = None;
            } else if let Some(f) = ln.child("solidFill") {
                paint.line = self.palette(None).first_color(f);
            } else if let Some(g) = ln.path(&["gradFill", "gsLst", "gs"]) {
                paint.line = self.palette(None).first_color(g);
            }
            paint.dash = dash(ln);
        }
        paint
    }

    fn box_paint(
        &self,
        shape: Option<&Element>,
        fill: Option<Color>,
        line: Option<Color>,
    ) -> Paint {
        let mut p = Paint {
            fill,
            line,
            width: 0.75,
            dash: Vec::new(),
        };
        if let Some(sp) = shape {
            let q = self.series_paint(Some(sp), Color::rgb(0), Kind::Pie);
            if sp.elements().any(|c| {
                matches!(
                    c.local(),
                    "noFill" | "solidFill" | "gradFill" | "pattFill" | "blipFill"
                )
            }) {
                p.fill = q.fill;
            }
            if let Some(ln) = sp.child("ln") {
                p.line = if ln.child("noFill").is_some() {
                    None
                } else {
                    q.line.or(line)
                };
                if num(ln, "w").is_some() {
                    p.width = q.width;
                }
                p.dash = q.dash;
            }
        }
        p
    }

    fn fill_color(&mut self, c: Color) {
        let page = self.page();
        page.set_fill(c.pdf());
        if c.a < 1.0 {
            page.set_alpha(c.a, 1.0);
        }
    }

    fn stroke_paint(&mut self, p: &Paint) -> bool {
        let Some(c) = p.line else { return false };
        if p.width <= 0.0 {
            return false;
        }
        let page = self.page();
        page.set_stroke(c.pdf()).set_line_width(p.width);
        if c.a < 1.0 {
            page.set_alpha(1.0, c.a);
        }
        if !p.dash.is_empty() {
            let pattern: Vec<f32> = p.dash.iter().map(|d| d * p.width.max(0.5)).collect();
            page.set_dash(&pattern, 0.0);
        }
        true
    }

    fn paint_rect(&mut self, r: Rect, p: &Paint, h: f32) {
        if r.w <= 0.0 && r.h <= 0.0 {
            return;
        }
        if let Some(c) = p.fill {
            self.page().save();
            self.fill_color(c);
            self.page().rect(r.x, h - r.bottom(), r.w, r.h).fill();
            self.page().restore();
        }
        self.page().save();
        if self.stroke_paint(p) {
            self.page().rect(r.x, h - r.bottom(), r.w, r.h).stroke();
        }
        self.page().restore();
    }

    fn stroke_polyline(&mut self, pts: &[(f32, f32)], p: &Paint, smooth: bool, h: f32) {
        if pts.len() < 2 {
            return;
        }
        self.page().save();
        if self.stroke_paint(p) {
            let page = self.canvas.page(self.page);
            page.set_cap(convert_pdf_canvas::Cap::Round);
            page.move_to(pts[0].0, h - pts[0].1);
            if smooth && pts.len() > 2 {
                for k in 0..pts.len() - 1 {
                    let p0 = pts[k.saturating_sub(1)];
                    let (p1, p2) = (pts[k], pts[k + 1]);
                    let p3 = pts[(k + 2).min(pts.len() - 1)];
                    let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
                    let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
                    page.curve_to(c1.0, h - c1.1, c2.0, h - c2.1, p2.0, h - p2.1);
                }
            } else {
                for q in &pts[1..] {
                    page.line_to(q.0, h - q.1);
                }
            }
            page.stroke();
        }
        self.page().restore();
    }

    fn marker(&mut self, symbol: &str, at: (f32, f32), size: f32, p: &Paint, h: f32) {
        let r = size / 2.0;
        let (x, y) = (at.0, h - at.1);
        let page = self.canvas.page(self.page);
        page.save();
        if let Some(c) = p.fill {
            page.set_fill(c.pdf());
        }
        if let Some(c) = p.line {
            page.set_stroke(c.pdf()).set_line_width(0.75);
        }
        let filled = p.fill.is_some();
        let stroked = p.line.is_some();
        let finish = |page: &mut convert_pdf_canvas::Page| {
            match (filled, stroked) {
                (true, true) => page.fill_stroke(),
                (true, false) => page.fill(),
                (false, true) => page.stroke(),
                (false, false) => page.end_path(),
            };
        };
        match symbol {
            "circle" | "dot" => {
                page.ellipse(x - r, y - r, size, size);
                finish(page);
            }
            "square" => {
                page.rect(x - r, y - r, size, size);
                finish(page);
            }
            "triangle" => {
                page.move_to(x, y + r)
                    .line_to(x + r, y - r)
                    .line_to(x - r, y - r)
                    .close();
                finish(page);
            }
            "x" | "star" | "plus" | "dash" => {
                if let Some(c) = p.line.or(p.fill) {
                    page.set_stroke(c.pdf()).set_line_width(1.0);
                }
                match symbol {
                    "x" => {
                        page.move_to(x - r, y - r).line_to(x + r, y + r);
                        page.move_to(x - r, y + r).line_to(x + r, y - r);
                    }
                    "plus" => {
                        page.move_to(x - r, y).line_to(x + r, y);
                        page.move_to(x, y - r).line_to(x, y + r);
                    }
                    "dash" => {
                        page.move_to(x - r, y).line_to(x + r, y);
                    }
                    _ => {
                        page.move_to(x - r, y - r).line_to(x + r, y + r);
                        page.move_to(x - r, y + r).line_to(x + r, y - r);
                        page.move_to(x, y - r).line_to(x, y + r);
                    }
                }
                page.stroke();
            }
            _ => {
                page.move_to(x, y + r)
                    .line_to(x + r, y)
                    .line_to(x, y - r)
                    .line_to(x - r, y)
                    .close();
                finish(page);
            }
        }
        page.restore();
    }

    #[allow(clippy::too_many_lines)]
    pub fn chart(&mut self, root: &Element, _part: &str, frame: Frame) {
        let Frame { m, w, h } = frame;
        if w <= 1.0 || h <= 1.0 {
            return;
        }
        let Some(chart) = root.child("chart") else {
            return;
        };
        let format = self.format;
        let plot = chart.child("plotArea");
        let mut groups: Vec<Group> = plot
            .map(|p| p.elements().filter_map(|g| read_group(g, format)).collect())
            .unwrap_or_default();
        if chart
            .child("plotVisOnly")
            .and_then(|p| flag(p, "val"))
            .unwrap_or(true)
        {
            visible_only(&mut groups, self.hidden);
            groups.retain(|g| !g.series.is_empty());
        }
        let unsupported: Vec<String> = plot
            .map(|p| {
                p.elements()
                    .map(Element::local)
                    .filter(|n| n.ends_with("Chart") && !is_drawn(n))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let axes: Vec<Axis> = plot
            .map(|p| {
                p.elements()
                    .filter(|a| matches!(a.local(), "valAx" | "catAx" | "dateAx" | "serAx"))
                    .map(read_axis)
                    .collect()
            })
            .unwrap_or_default();
        let minor = self.theme.minor.clone();
        let chart_default = Font {
            size: 10.0,
            bold: false,
            italic: false,
            color: Rgb::BLACK,
            latin: if minor.latin.is_empty() {
                "Calibri".to_owned()
            } else {
                minor.latin.clone()
            },
            cs: minor.cs.clone(),
        };
        let has_chart_font = root.child("txPr").is_some();
        let base = self.chart_font(&chart_default, root.child("txPr"));
        self.page().save();
        self.apply(m, h);
        let area = self.box_paint(
            root.child("spPr"),
            Some(Color::rgb(0xFFFFFF)),
            Some(Color::rgb(0xD9D9D9)),
        );
        self.paint_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
            &area,
            h,
        );
        self.page().rect(0.0, 0.0, w, h).clip();
        let pad = 7.0_f32.min(w / 10.0).min(h / 10.0);
        let mut room = Rect {
            x: pad,
            y: pad,
            w: w - 2.0 * pad,
            h: h - 2.0 * pad,
        };
        let deleted_title = chart
            .child("autoTitleDeleted")
            .and_then(|d| flag(d, "val"))
            .unwrap_or(false);
        let title_el = chart.child("title");
        let title_font = {
            let mut f = base.clone();
            if !has_chart_font {
                f.size = 18.0 * base.size / 10.0;
                f.bold = true;
            } else {
                f.size = base.size * 1.4;
            }
            self.chart_font(&f, title_el.and_then(|t| t.child("txPr")))
        };
        let title_spans: Vec<Span> = match title_el {
            Some(t) => {
                if let Some(rich) = t.path(&["tx", "rich"]) {
                    let f = self.chart_font(&title_font, Some(rich));
                    self.rich_spans(rich, &f)
                } else if let Some(tx) = t.child("tx") {
                    let text = tx
                        .descendants("v")
                        .into_iter()
                        .map(Element::text)
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.spans(&text, &title_font)
                } else {
                    let only: Vec<&Series> = groups.iter().flat_map(|g| &g.series).collect();
                    let text = if only.len() == 1 {
                        only[0].name.clone()
                    } else {
                        "Chart Title".to_owned()
                    };
                    self.spans(&text, &title_font)
                }
            }
            None => Vec::new(),
        };
        let title_overlay = title_el
            .and_then(|t| t.child("overlay"))
            .and_then(|o| flag(o, "val"))
            .unwrap_or(false);
        if !deleted_title && !title_spans.is_empty() {
            let block = self.block(&title_spans, Some(room.w));
            let x = room.x;
            let y = room.y;
            let rw = room.w;
            self.draw_block(&block, x, y, rw, "ctr", h);
            if !title_overlay {
                room.y += block.height + 4.0;
                room.h -= block.height + 4.0;
            }
        }
        if groups.is_empty() {
            if !unsupported.is_empty() {
                self.notes.push(format!(
                    "a chart of a kind not drawn yet ({}) is shown as a frame",
                    unsupported.join(", ")
                ));
                let f = Font {
                    color: Rgb(0.45, 0.45, 0.45),
                    ..base.clone()
                };
                let spans = self.spans(&format!("[{}]", unsupported.join(", ")), &f);
                let block = self.block(&spans, Some(room.w));
                let y = room.y + (room.h - block.height) / 2.0;
                let (x, rw) = (room.x, room.w);
                self.draw_block(&block, x, y, rw, "ctr", h);
            }
            self.page().restore();
            return;
        }
        if !unsupported.is_empty() {
            self.notes.push(format!(
                "a chart's {} part is not drawn",
                unsupported.join(", ")
            ));
        }
        let legend_el = chart.child("legend");
        let legend_font = self.chart_font(&base, legend_el.and_then(|l| l.child("txPr")));
        let mut legend_entries: Vec<(String, Paint, Kind, Option<String>)> = Vec::new();
        if let Some(legend) = legend_el {
            let position = legend
                .child("legendPos")
                .and_then(|p| p.attr("val"))
                .unwrap_or("r")
                .to_owned();
            let deleted: Vec<usize> = legend
                .children_named("legendEntry")
                .filter(|e| e.child("delete").and_then(|d| flag(d, "val")) == Some(true))
                .filter_map(|e| e.child("idx").and_then(|i| num(i, "val")))
                .map(|v| v as usize)
                .collect();
            let by_point = groups.len() == 1
                && groups[0].series.len() == 1
                && (groups[0].vary || matches!(groups[0].kind, Kind::Pie | Kind::Doughnut));
            if by_point {
                let g = &groups[0];
                let s = &g.series[0];
                for k in 0..s.values.len().max(s.categories.len()) {
                    if deleted.contains(&k) {
                        continue;
                    }
                    let name = s
                        .categories
                        .get(k)
                        .cloned()
                        .unwrap_or_else(|| (k + 1).to_string());
                    let paint = self.point_paint(g, s, k);
                    legend_entries.push((name, paint, g.kind, None));
                }
            } else {
                let mut all: Vec<(&Group, &Series)> = groups
                    .iter()
                    .flat_map(|g| g.series.iter().map(move |s| (g, s)))
                    .collect();
                all.sort_by_key(|(_, s)| s.order);
                if matches!(position.as_str(), "r" | "l" | "tr")
                    && groups.iter().all(|g| {
                        g.kind == Kind::Bar { horizontal: false }
                            && g.grouping != Grouping::Clustered
                    })
                {
                    all.reverse();
                }
                for (g, s) in all {
                    if deleted.contains(&s.index) {
                        continue;
                    }
                    let paint =
                        self.series_paint(s.shape.as_ref(), accent_for(self, s.index), g.kind);
                    let marker = self.marker_symbol(g, s);
                    legend_entries.push((s.name.clone(), paint, g.kind, marker));
                }
            }
            let overlay = legend
                .child("overlay")
                .and_then(|o| flag(o, "val"))
                .unwrap_or(false);
            let manual = manual_layout(legend.child("layout"), w, h);
            self.draw_legend(
                &legend_entries,
                &legend_font,
                &position,
                overlay,
                manual,
                &mut room,
                legend.child("spPr"),
                h,
            );
        }
        let plot_manual = plot.and_then(|p| {
            let layout = p.child("layout")?;
            let inner = layout
                .path(&["manualLayout", "layoutTarget"])
                .and_then(|t| t.attr("val"))
                == Some("inner");
            manual_layout(Some(layout), w, h).map(|r| (r, inner))
        });
        let plot_paint = self.box_paint(plot.and_then(|p| p.child("spPr")), None, None);
        let round = matches!(groups[0].kind, Kind::Pie | Kind::Doughnut);
        if round {
            let r = plot_manual.map_or(room, |(r, _)| r);
            self.paint_rect(r, &plot_paint, h);
            for g in &groups {
                self.draw_round(g, r, &base, h);
            }
            self.page().restore();
            return;
        }
        self.draw_axes_chart(&groups, &axes, room, plot_manual, &plot_paint, &base, h);
        self.page().restore();
    }

    fn marker_symbol(&self, g: &Group, s: &Series) -> Option<String> {
        if !matches!(g.kind, Kind::Line | Kind::Scatter) {
            return None;
        }
        let own = s
            .marker
            .as_ref()
            .and_then(|m| m.child("symbol"))
            .and_then(|m| m.attr("val"));
        match own {
            Some("none") => None,
            Some("auto") | None => {
                let on = match g.kind {
                    Kind::Scatter => !matches!(g.style.as_str(), "line" | "smooth" | "none"),
                    _ => g.markers,
                };
                on.then(|| {
                    [
                        "diamond", "square", "triangle", "x", "star", "circle", "plus",
                    ][s.index % 7]
                        .to_owned()
                })
            }
            Some(other) => Some(other.to_owned()),
        }
    }

    fn point_paint(&self, g: &Group, s: &Series, k: usize) -> Paint {
        let auto = if g.vary || matches!(g.kind, Kind::Pie | Kind::Doughnut) && g.series.len() == 1
        {
            accent_for(self, k)
        } else {
            accent_for(self, s.index)
        };
        let base_shape = if g.vary && !s.points.contains_key(&k) {
            s.shape.as_ref().map(|sp| {
                let mut sp = sp.clone();
                sp.children.retain(|n| match n {
                    convert_office_read::xml::Node::Element(e) => {
                        !matches!(e.local(), "solidFill" | "gradFill" | "pattFill" | "noFill")
                    }
                    convert_office_read::xml::Node::Text(_) => true,
                });
                sp
            })
        } else {
            s.shape.clone()
        };
        let mut paint = self.series_paint(base_shape.as_ref(), auto, g.kind);
        if let Some(sp) = s.points.get(&k) {
            let own = self.series_paint(Some(sp), paint.fill.unwrap_or(auto), g.kind);
            if sp.elements().any(|c| {
                matches!(
                    c.local(),
                    "noFill" | "solidFill" | "gradFill" | "pattFill" | "blipFill"
                )
            }) {
                paint.fill = own.fill;
            }
            if sp.child("ln").is_some() {
                paint.line = own.line;
                paint.width = own.width;
            }
        }
        paint
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_legend(
        &mut self,
        entries: &[(String, Paint, Kind, Option<String>)],
        font: &Font,
        position: &str,
        overlay: bool,
        manual: Option<Rect>,
        room: &mut Rect,
        shape: Option<&Element>,
        h: f32,
    ) {
        if entries.is_empty() {
            return;
        }
        let symbol = font.size * 0.7;
        let line_symbol = font.size * 1.8;
        let gap = 4.0;
        let blocks: Vec<(Block, f32)> = entries
            .iter()
            .map(|(name, _, kind, _)| {
                let spans = self.spans(name, font);
                let lead = if matches!(kind, Kind::Line | Kind::Scatter) {
                    line_symbol
                } else {
                    symbol
                } + gap;
                let block = self.block(&spans, Some((room.w * 0.4).max(40.0)));
                (block, lead)
            })
            .collect();
        let vertical = matches!(position, "r" | "l" | "tr");
        let entry_w = |b: &(Block, f32)| b.1 + b.0.width;
        let row_h = blocks
            .iter()
            .map(|b| b.0.height)
            .fold(font.size * 1.2, f32::max);
        let mut placed: Vec<(f32, f32)> = Vec::new();
        let (lw, lh);
        if vertical {
            let widest = blocks.iter().map(entry_w).fold(0.0, f32::max);
            lw = widest + 8.0;
            let mut y = 4.0;
            for b in &blocks {
                placed.push((4.0, y));
                y += b.0.height.max(font.size * 1.2) + 2.0;
            }
            lh = y + 2.0;
        } else {
            let limit = room.w;
            let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
            let mut used = 0.0;
            for (k, b) in blocks.iter().enumerate() {
                let need = entry_w(b) + 10.0;
                if used + need > limit && !rows.last().is_some_and(Vec::is_empty) {
                    rows.push(Vec::new());
                    used = 0.0;
                }
                if let Some(r) = rows.last_mut() {
                    r.push(k);
                }
                used += need;
            }
            let widest_row = rows
                .iter()
                .map(|r| r.iter().map(|&k| entry_w(&blocks[k]) + 10.0).sum::<f32>())
                .fold(0.0, f32::max);
            lw = widest_row + 4.0;
            lh = rows.len() as f32 * (row_h + 2.0) + 6.0;
            placed = vec![(0.0, 0.0); blocks.len()];
            for (ri, row) in rows.iter().enumerate() {
                let row_w: f32 = row.iter().map(|&k| entry_w(&blocks[k]) + 10.0).sum();
                let mut x = (lw - row_w) / 2.0 + 5.0;
                for &k in row {
                    placed[k] = (x, 3.0 + ri as f32 * (row_h + 2.0));
                    x += entry_w(&blocks[k]) + 10.0;
                }
            }
        }
        let lw = lw.min(room.w);
        let lh = lh.min(room.h);
        let at = manual.unwrap_or_else(|| match position {
            "b" => Rect {
                x: room.x + (room.w - lw) / 2.0,
                y: room.bottom() - lh,
                w: lw,
                h: lh,
            },
            "t" => Rect {
                x: room.x + (room.w - lw) / 2.0,
                y: room.y,
                w: lw,
                h: lh,
            },
            "l" => Rect {
                x: room.x,
                y: room.y + (room.h - lh) / 2.0,
                w: lw,
                h: lh,
            },
            "tr" => Rect {
                x: room.right() - lw,
                y: room.y,
                w: lw,
                h: lh,
            },
            _ => Rect {
                x: room.right() - lw,
                y: room.y + (room.h - lh) / 2.0,
                w: lw,
                h: lh,
            },
        });
        if !overlay && manual.is_none() {
            match position {
                "b" => room.h -= lh + 4.0,
                "t" => {
                    room.y += lh + 4.0;
                    room.h -= lh + 4.0;
                }
                "l" => {
                    room.x += lw + 4.0;
                    room.w -= lw + 4.0;
                }
                _ => room.w -= lw + 4.0,
            }
        }
        let paint = self.box_paint(shape, None, None);
        self.paint_rect(at, &paint, h);
        for (k, (block, lead)) in blocks.iter().enumerate() {
            let (ex, ey) = placed[k];
            let (x, y) = (at.x + ex, at.y + ey);
            let mid = y + block
                .lines
                .first()
                .map_or(font.size * 0.6, |l| l.ascent - font.size * 0.3);
            let (_, paint, kind, marker) = &entries[k];
            if matches!(kind, Kind::Line | Kind::Scatter) {
                let p = Paint {
                    width: paint.width.min(3.0),
                    ..paint.clone()
                };
                self.stroke_polyline(&[(x, mid), (x + line_symbol, mid)], &p, false, h);
                if let Some(sym) = marker {
                    let mp = Paint {
                        fill: paint.line,
                        line: paint.line,
                        width: 0.75,
                        dash: Vec::new(),
                    };
                    self.marker(sym, (x + line_symbol / 2.0, mid), 5.0, &mp, h);
                }
            } else {
                let r = Rect {
                    x,
                    y: mid - symbol / 2.0,
                    w: symbol,
                    h: symbol,
                };
                let p = Paint {
                    width: paint.width.min(1.0),
                    ..paint.clone()
                };
                self.paint_rect(r, &p, h);
            }
            let tx = x + lead;
            let bw = block.width;
            self.draw_block(block, tx, y, bw, "l", h);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn draw_round(&mut self, g: &Group, r: Rect, base: &Font, h: f32) {
        let rings: Vec<&Series> = if g.kind == Kind::Pie {
            g.series.iter().take(1).collect()
        } else {
            g.series.iter().collect()
        };
        if rings.is_empty() {
            return;
        }
        let radius = (r.w.min(r.h) / 2.0 - 2.0).max(1.0);
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let hole = if g.kind == Kind::Doughnut {
            (g.hole / 100.0).clamp(0.1, 0.9)
        } else {
            0.0
        };
        let ring_w = (1.0 - hole) * radius / rings.len() as f32;
        for (ri, s) in rings.iter().enumerate() {
            let values: Vec<f64> = s.values.iter().map(|v| v.unwrap_or(0.0).abs()).collect();
            let total: f64 = values.iter().sum();
            if total <= 0.0 {
                continue;
            }
            let outer = radius - ri as f32 * ring_w;
            let inner = if g.kind == Kind::Doughnut {
                outer - ring_w
            } else {
                0.0
            };
            let mut angle = f64::from(g.first_angle);
            let labels = s.labels.clone().or_else(|| g.labels.clone());
            let label_font = self.chart_font(base, labels.as_ref().and_then(|l| l.font.as_ref()));
            let mut texts: Vec<(String, f32, f32, bool)> = Vec::new();
            for (k, v) in values.iter().enumerate() {
                let sweep = v / total * 360.0;
                let paint = self.point_paint(g, s, k);
                let paint = Paint {
                    line: paint.line.or(Some(Color::rgb(0xFFFFFF))),
                    ..paint
                };
                if sweep > 0.0 {
                    self.sector(
                        (cx, cy),
                        inner,
                        outer,
                        angle as f32,
                        sweep as f32,
                        &paint,
                        h,
                    );
                }
                if let Some(l) = labels
                    .as_ref()
                    .filter(|l| l.any() && !l.deleted.contains(&k))
                    && sweep > 0.0
                {
                    let mid = (angle + sweep / 2.0).to_radians() as f32;
                    let outside = l.position.as_deref() == Some("outEnd");
                    let at = if g.kind == Kind::Doughnut {
                        (inner + outer) / 2.0
                    } else if outside {
                        outer + label_font.size
                    } else {
                        outer * 0.65
                    };
                    let (lx, ly) = (cx + at * mid.sin(), cy - at * mid.cos());
                    let text = self.label_text(l, s, k, *v, total, s.categories.get(k));
                    texts.push((text, lx, ly, outside));
                }
                angle += sweep;
            }
            for (text, lx, ly, _) in texts {
                let spans = self.spans(&text, &label_font);
                let block = self.block(&spans, Some(radius.max(30.0)));
                let x = lx - block.width / 2.0;
                let y = ly - block.height / 2.0;
                let bw = block.width;
                self.draw_block(&block, x, y, bw, "ctr", h);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sector(
        &mut self,
        c: (f32, f32),
        inner: f32,
        outer: f32,
        start: f32,
        sweep: f32,
        p: &Paint,
        h: f32,
    ) {
        let steps = ((sweep / 5.0).ceil() as usize).max(2);
        let point = |radius: f32, a: f32| {
            let t = a.to_radians();
            (c.0 + radius * t.sin(), h - (c.1 - radius * t.cos()))
        };
        let page = self.canvas.page(self.page);
        page.save();
        let full = sweep >= 359.99;
        let outline = |page: &mut convert_pdf_canvas::Page| {
            let first = point(outer, start);
            page.move_to(first.0, first.1);
            for k in 1..=steps {
                let q = point(outer, start + sweep * k as f32 / steps as f32);
                page.line_to(q.0, q.1);
            }
            if inner > 0.0 {
                if full {
                    page.close();
                    let q = point(inner, start + sweep);
                    page.move_to(q.0, q.1);
                } else {
                    let q = point(inner, start + sweep);
                    page.line_to(q.0, q.1);
                }
                for k in (0..steps).rev() {
                    let q = point(inner, start + sweep * k as f32 / steps as f32);
                    page.line_to(q.0, q.1);
                }
            } else if !full {
                let q = (c.0, h - c.1);
                page.line_to(q.0, q.1);
            }
            page.close();
        };
        if let Some(f) = p.fill {
            page.set_fill(f.pdf());
            if f.a < 1.0 {
                page.set_alpha(f.a, 1.0);
            }
            outline(page);
            page.fill_even_odd();
        }
        page.restore();
        self.page().save();
        if self.stroke_paint(&Paint {
            width: p.width.min(1.5),
            ..p.clone()
        }) {
            let page = self.canvas.page(self.page);
            outline(page);
            page.stroke();
        }
        self.page().restore();
    }

    fn label_text(
        &self,
        l: &Labels,
        s: &Series,
        k: usize,
        v: f64,
        total: f64,
        category: Option<&String>,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();
        if l.series {
            parts.push(s.name.clone());
        }
        if l.category {
            parts.push(category.cloned().unwrap_or_else(|| (k + 1).to_string()));
        }
        if l.value {
            let code = l.format.clone().unwrap_or_else(|| s.format.clone());
            let code = if code.is_empty() {
                "General".to_owned()
            } else {
                code
            };
            parts.push((self.format)(&code, v));
        }
        if l.percent && total > 0.0 {
            let code = l
                .format
                .clone()
                .filter(|f| f.contains('%'))
                .unwrap_or_else(|| "0%".to_owned());
            parts.push((self.format)(&code, v / total));
        }
        let sep = l.separator.clone().unwrap_or_else(|| ", ".to_owned());
        parts.join(&sep)
    }

    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn draw_axes_chart(
        &mut self,
        groups: &[Group],
        axes: &[Axis],
        room: Rect,
        manual: Option<(Rect, bool)>,
        plot_paint: &Paint,
        base: &Font,
        h: f32,
    ) {
        let find = |id: &str| axes.iter().find(|a| a.id == id);
        let pair = |g: &Group| -> (Option<&Axis>, Option<&Axis>) {
            let list: Vec<&Axis> = g.axes.iter().filter_map(|id| find(id)).collect();
            if g.kind == Kind::Scatter {
                let x = list
                    .iter()
                    .find(|a| matches!(a.position.as_str(), "b" | "t"))
                    .or(list.first())
                    .copied();
                let y = list
                    .iter()
                    .find(|a| matches!(a.position.as_str(), "l" | "r"))
                    .or(list.get(1))
                    .copied();
                (x, y)
            } else {
                (
                    list.iter().find(|a| !a.values).copied(),
                    list.iter().find(|a| a.values).copied(),
                )
            }
        };
        let horizontal = groups
            .iter()
            .any(|g| matches!(g.kind, Kind::Bar { horizontal: true }));
        let categories: Vec<String> = groups
            .iter()
            .flat_map(|g| &g.series)
            .map(|s| s.categories.clone())
            .max_by_key(Vec::len)
            .unwrap_or_default();
        let count = groups
            .iter()
            .flat_map(|g| &g.series)
            .map(|s| s.values.len())
            .chain(std::iter::once(categories.len()))
            .max()
            .unwrap_or(0)
            .max(1);
        let categories: Vec<String> = (0..count)
            .map(|k| {
                categories
                    .get(k)
                    .filter(|c| !c.is_empty())
                    .cloned()
                    .unwrap_or_else(|| (k + 1).to_string())
            })
            .collect();
        let mut ranges: HashMap<String, (f64, f64, bool)> = HashMap::new();
        let mut x_range: Option<(f64, f64)> = None;
        for g in groups {
            let (_, val_ax) = pair(g);
            let key = val_ax.map_or_else(String::new, |a| a.id.clone());
            let entry = ranges.entry(key).or_insert((f64::MAX, f64::MIN, false));
            if g.grouping == Grouping::Percent && g.kind != Kind::Scatter {
                entry.0 = entry.0.min(0.0);
                entry.1 = entry.1.max(1.0);
                entry.2 = true;
                let neg = g
                    .series
                    .iter()
                    .flat_map(|s| s.values.iter().flatten())
                    .any(|v| *v < 0.0);
                if neg {
                    entry.0 = -1.0;
                }
            } else if g.grouping == Grouping::Stacked && g.kind != Kind::Scatter {
                for k in 0..count {
                    let (mut pos, mut neg) = (0.0, 0.0);
                    for s in &g.series {
                        let v = s.values.get(k).copied().flatten().unwrap_or(0.0);
                        if v >= 0.0 {
                            pos += v;
                        } else {
                            neg += v;
                        }
                    }
                    entry.0 = entry.0.min(neg);
                    entry.1 = entry.1.max(pos);
                }
            } else {
                for v in g.series.iter().flat_map(|s| s.values.iter().flatten()) {
                    entry.0 = entry.0.min(*v);
                    entry.1 = entry.1.max(*v);
                }
            }
            if g.kind == Kind::Scatter {
                for s in &g.series {
                    let xs: Vec<f64> = if s.xs.is_empty() {
                        (1..=s.values.len()).map(|v| v as f64).collect()
                    } else {
                        s.xs.iter().flatten().copied().collect()
                    };
                    for x in xs {
                        let r = x_range.get_or_insert((x, x));
                        r.0 = r.0.min(x);
                        r.1 = r.1.max(x);
                    }
                }
            }
        }
        let first = &groups[0];
        let (cat_axis, val_axis) = pair(first);
        let cat_axis = cat_axis.cloned().unwrap_or_default();
        let val_axis = val_axis.cloned().unwrap_or(Axis {
            values: true,
            labels: true,
            position: if horizontal { "b" } else { "l" }.to_owned(),
            ..Axis::default()
        });
        let scatter = first.kind == Kind::Scatter;
        let val_font = self.chart_font(base, val_axis.font.as_ref());
        let cat_font = self.chart_font(base, cat_axis.font.as_ref());
        let range = ranges
            .get(&val_axis.id)
            .copied()
            .unwrap_or((0.0, 1.0, false));
        let range = if range.0 > range.1 {
            (0.0, 1.0, range.2)
        } else {
            range
        };
        let value_len = if horizontal { room.w } else { room.h };
        let make_scale = |len: f32, axis: &Axis, range: (f64, f64, bool)| {
            let steps = f64::from((len / (val_font.size * 1.6)).floor().clamp(2.0, 10.0));
            let mut s = auto_scale(range.0, range.1, steps, axis.min, axis.max);
            if range.2 {
                s.unit = axis.major.unwrap_or(if len > 150.0 { 0.1 } else { 0.2 });
                if axis.max.is_none() {
                    s.hi = 1.0;
                }
            } else if let Some(u) = axis.major {
                s.unit = u;
            }
            s
        };
        let mut scale = make_scale(value_len, &val_axis, range);
        let percent = range.2;
        let val_code = val_axis.format.clone().unwrap_or_else(|| {
            if percent {
                "0%".to_owned()
            } else {
                first
                    .series
                    .first()
                    .map(|s| s.format.clone())
                    .filter(|f| !f.is_empty())
                    .unwrap_or_else(|| "General".to_owned())
            }
        });
        let format = self.format;
        let tick_text = |v: f64| format(&val_code, v);
        let x_font = if scatter {
            self.chart_font(base, cat_axis.font.as_ref())
        } else {
            cat_font.clone()
        };
        let x_code = cat_axis
            .format
            .clone()
            .unwrap_or_else(|| "General".to_owned());
        let label_width =
            |scene: &mut Self, scale: &Scale, font: &Font, fmt: &dyn Fn(f64) -> String| {
                scale
                    .ticks()
                    .iter()
                    .map(|v| {
                        let spans = scene.spans(&fmt(*v), font);
                        scene.block(&spans, None).width
                    })
                    .fold(0.0, f32::max)
            };
        let title_block = |scene: &mut Self, axis: &Axis, width: f32| -> Option<Block> {
            let t = axis.title.as_ref()?;
            if axis.deleted {
                return None;
            }
            let mut f = base.clone();
            f.bold = true;
            let f = scene.chart_font(&f, t.child("txPr"));
            let spans = if let Some(rich) = t.path(&["tx", "rich"]) {
                let f = scene.chart_font(&f, Some(rich));
                scene.rich_spans(rich, &f)
            } else {
                let text = t
                    .descendants("v")
                    .into_iter()
                    .map(Element::text)
                    .collect::<Vec<_>>()
                    .join(" ");
                scene.spans(if text.is_empty() { "Axis Title" } else { &text }, &f)
            };
            Some(scene.block(&spans, Some(width.max(20.0))))
        };
        let show_val_labels = !val_axis.deleted && val_axis.labels;
        let show_cat_labels = !cat_axis.deleted && cat_axis.labels;
        let fmt_val = |v: f64| tick_text(v);
        let fmt_x = |v: f64| format(&x_code, v);
        let second: Option<SecondAxis> = if horizontal || scatter {
            None
        } else {
            groups
                .iter()
                .filter_map(|g| pair(g).1.map(|a| (g, a)))
                .find(|(_, a)| a.id != val_axis.id && !a.deleted && a.labels)
                .map(|(g, a)| {
                    let r = ranges.get(&a.id).copied().unwrap_or((0.0, 1.0, false));
                    let r = if r.0 > r.1 { (0.0, 1.0, r.2) } else { r };
                    let code = a.format.clone().unwrap_or_else(|| {
                        g.series
                            .first()
                            .map(|s| s.format.clone())
                            .filter(|f| !f.is_empty())
                            .unwrap_or_else(|| "General".to_owned())
                    });
                    (a.clone(), r, self.chart_font(base, a.font.as_ref()), code)
                })
        };
        let inner = if let Some((r, true)) = manual {
            r
        } else {
            let outer = manual.map_or(room, |(r, _)| r);
            let mut inner = outer;
            let val_title =
                title_block(self, &val_axis, if horizontal { outer.w } else { outer.h });
            let cat_title =
                title_block(self, &cat_axis, if horizontal { outer.h } else { outer.w });
            let val_w = if show_val_labels {
                label_width(self, &scale, &val_font, &fmt_val) + 4.0
            } else {
                0.0
            };
            let line_h = |f: &Font| f.size * 1.25;
            if horizontal {
                let cat_w = if show_cat_labels {
                    categories
                        .iter()
                        .map(|c| {
                            let spans = self.spans(c, &cat_font);
                            self.block(&spans, Some(outer.w * 0.3)).width
                        })
                        .fold(0.0, f32::max)
                        + 5.0
                } else {
                    0.0
                };
                inner.x += cat_w;
                inner.w -= cat_w;
                if show_val_labels {
                    inner.h -= line_h(&val_font) + 2.0;
                }
                if let Some(b) = &val_title {
                    inner.h -= b.height + 4.0;
                }
                if let Some(b) = &cat_title {
                    inner.x += b.height + 4.0;
                    inner.w -= b.height + 4.0;
                }
                inner.w -= val_w / 2.0;
            } else {
                let right = val_axis.position == "r";
                if right {
                    inner.w -= val_w;
                } else {
                    inner.x += val_w;
                    inner.w -= val_w;
                }
                if let Some((axis, range, font, code)) = &second {
                    let s = make_scale(outer.h, axis, *range);
                    inner.w -= label_width(self, &s, font, &|v| format(code, v)) + 6.0;
                }
                if let Some(b) = &val_title {
                    if right {
                        inner.w -= b.height + 4.0;
                    } else {
                        inner.x += b.height + 4.0;
                        inner.w -= b.height + 4.0;
                    }
                }
                let cat_h = if show_cat_labels {
                    if scatter {
                        line_h(&x_font) + 2.0
                    } else {
                        let band = inner.w / count as f32;
                        categories
                            .iter()
                            .map(|c| {
                                let spans = self.spans(c, &cat_font);
                                self.block(&spans, Some(band.max(20.0))).height
                            })
                            .fold(0.0, f32::max)
                            .min(line_h(&cat_font) * 3.0)
                            + 3.0
                    }
                } else {
                    0.0
                };
                inner.h -= cat_h;
                if let Some(b) = &cat_title {
                    inner.h -= b.height + 4.0;
                }
                if scatter && show_cat_labels {
                    inner.w -= 8.0;
                }
                inner.y += val_font.size * 0.5;
                inner.h -= val_font.size * 0.5;
            }
            if let Some(b) = val_title {
                if horizontal {
                    let y = inner.bottom()
                        + if show_val_labels {
                            line_h(&val_font) + 4.0
                        } else {
                            4.0
                        };
                    let (x, w) = (inner.x, inner.w);
                    self.draw_block(&b, x, y, w, "ctr", h);
                } else {
                    let x = if val_axis.position == "r" {
                        inner.right() + val_w + 2.0
                    } else {
                        outer.x
                    };
                    self.draw_turned(&b, x, inner.y, inner.h, h);
                }
            }
            if let Some(b) = cat_title {
                if horizontal {
                    self.draw_turned(&b, outer.x, inner.y, inner.h, h);
                } else {
                    let y = outer.bottom() - b.height;
                    let (x, w) = (inner.x, inner.w);
                    self.draw_block(&b, x, y, w, "ctr", h);
                }
            }
            inner
        };
        if inner.w <= 1.0 || inner.h <= 1.0 {
            return;
        }
        scale = make_scale(if horizontal { inner.w } else { inner.h }, &val_axis, range);
        self.paint_rect(inner, plot_paint, h);
        let value_pos = |v: f64, s: &Scale, reversed: bool| -> f32 {
            let t = s.at(v).clamp(0.0, 1.0) as f32;
            let t = if reversed { 1.0 - t } else { t };
            if horizontal {
                inner.x + inner.w * t
            } else {
                inner.bottom() - inner.h * t
            }
        };
        let grid_default = Some(Color::rgb(0xD9D9D9));
        if let Some(gl) = &val_axis.gridlines {
            let paint = self.box_paint(gl.child("spPr"), None, grid_default);
            for v in scale.ticks() {
                let p = value_pos(v, &scale, val_axis.reversed);
                let pts = if horizontal {
                    [(p, inner.y), (p, inner.bottom())]
                } else {
                    [(inner.x, p), (inner.right(), p)]
                };
                self.stroke_polyline(
                    &pts,
                    &Paint {
                        width: paint.width.min(1.0),
                        ..paint.clone()
                    },
                    false,
                    h,
                );
            }
        }
        let x_scale = x_range.map(|(lo, hi)| {
            let steps = f64::from((inner.w / (x_font.size * 4.0)).floor().clamp(2.0, 10.0));
            let mut s = auto_scale(lo, hi, steps, cat_axis.min, cat_axis.max);
            if let Some(u) = cat_axis.major {
                s.unit = u;
            }
            s
        });
        if scatter && let (Some(gl), Some(xs)) = (&cat_axis.gridlines, x_scale) {
            let paint = self.box_paint(gl.child("spPr"), None, grid_default);
            for v in xs.ticks() {
                let t = xs.at(v) as f32;
                let x = inner.x + inner.w * if cat_axis.reversed { 1.0 - t } else { t };
                self.stroke_polyline(&[(x, inner.y), (x, inner.bottom())], &paint, false, h);
            }
        }
        let cat_reversed = cat_axis.reversed;
        let band_len = if horizontal { inner.h } else { inner.w } / count as f32;
        let band_start = |k: usize| -> f32 {
            let k = if cat_reversed { count - 1 - k } else { k };
            if horizontal {
                inner.bottom() - band_len * (k + 1) as f32
            } else {
                inner.x + band_len * k as f32
            }
        };
        let zero = 0.0_f64.clamp(scale.lo, scale.hi);
        let mut order: Vec<&Group> = groups.iter().collect();
        order.sort_by_key(|g| match g.kind {
            Kind::Area => 0,
            Kind::Bar { .. } => 1,
            _ => 2,
        });
        let mut label_jobs: Vec<(String, f32, f32, Font, &'static str)> = Vec::new();
        for g in order {
            let (_, g_val) = pair(g);
            let g_scale = match g_val {
                Some(a) if a.id != val_axis.id => {
                    let r = ranges.get(&a.id).copied().unwrap_or((0.0, 1.0, false));
                    let r = if r.0 > r.1 { (0.0, 1.0, r.2) } else { r };
                    make_scale(if horizontal { inner.w } else { inner.h }, a, r)
                }
                _ => scale,
            };
            let g_rev = g_val.is_some_and(|a| a.reversed);
            let zero = 0.0_f64.clamp(g_scale.lo, g_scale.hi);
            let labels_of = |s: &Series| s.labels.clone().or_else(|| g.labels.clone());
            match g.kind {
                Kind::Bar { .. } => {
                    let n = if g.grouping == Grouping::Clustered {
                        g.series.len().max(1)
                    } else {
                        1
                    };
                    let overlap = if g.grouping == Grouping::Clustered {
                        g.overlap / 100.0
                    } else {
                        1.0
                    };
                    let gap = g.gap / 100.0;
                    let bar = band_len / (n as f32 - (n as f32 - 1.0) * overlap + gap);
                    let mut pos_stack = vec![0.0_f64; count];
                    let mut neg_stack = vec![0.0_f64; count];
                    let totals: Vec<f64> = (0..count)
                        .map(|k| {
                            g.series
                                .iter()
                                .map(|s| s.values.get(k).copied().flatten().unwrap_or(0.0).abs())
                                .sum()
                        })
                        .collect();
                    for (si, s) in g.series.iter().enumerate() {
                        let slot = if g.grouping == Grouping::Clustered {
                            si
                        } else {
                            0
                        };
                        let labels = labels_of(s);
                        let lf =
                            self.chart_font(base, labels.as_ref().and_then(|l| l.font.as_ref()));
                        for k in 0..count {
                            let Some(v) = s.values.get(k).copied().flatten() else {
                                continue;
                            };
                            let v_shown = if g.grouping == Grouping::Percent {
                                if totals[k] > 0.0 { v / totals[k] } else { 0.0 }
                            } else {
                                v
                            };
                            let (from, to) = if g.grouping == Grouping::Clustered {
                                (zero, v_shown)
                            } else if v_shown >= 0.0 {
                                let f = pos_stack[k];
                                pos_stack[k] += v_shown;
                                (f, pos_stack[k])
                            } else {
                                let f = neg_stack[k];
                                neg_stack[k] += v_shown;
                                (f, neg_stack[k])
                            };
                            let a = value_pos(from, &g_scale, g_rev);
                            let b = value_pos(to, &g_scale, g_rev);
                            let offset = gap * bar / 2.0 + slot as f32 * bar * (1.0 - overlap);
                            let start = band_start(k);
                            let rect = if horizontal {
                                let y1 = start + band_len - offset;
                                Rect {
                                    x: a.min(b),
                                    y: y1 - bar,
                                    w: (a - b).abs(),
                                    h: bar,
                                }
                            } else {
                                Rect {
                                    x: start + offset,
                                    y: a.min(b),
                                    w: bar,
                                    h: (a - b).abs(),
                                }
                            };
                            let paint = self.point_paint(g, s, k);
                            self.paint_rect(rect, &paint, h);
                            if let Some(l) = labels
                                .as_ref()
                                .filter(|l| l.any() && !l.deleted.contains(&k))
                            {
                                let text = self.label_text(l, s, k, v, 0.0, categories.get(k));
                                let pos = l.position.clone().unwrap_or_else(|| {
                                    if g.grouping == Grouping::Clustered {
                                        "outEnd"
                                    } else {
                                        "ctr"
                                    }
                                    .to_owned()
                                });
                                let (lx, ly, anchor) = if horizontal {
                                    let y = rect.y + rect.h / 2.0;
                                    match pos.as_str() {
                                        "ctr" => (rect.x + rect.w / 2.0, y, "ctr"),
                                        "inEnd" => (
                                            if v >= 0.0 {
                                                rect.right() - 2.0
                                            } else {
                                                rect.x + 2.0
                                            },
                                            y,
                                            if v >= 0.0 { "r" } else { "l" },
                                        ),
                                        "inBase" => (
                                            if v >= 0.0 {
                                                rect.x + 2.0
                                            } else {
                                                rect.right() - 2.0
                                            },
                                            y,
                                            if v >= 0.0 { "l" } else { "r" },
                                        ),
                                        _ => (
                                            if v >= 0.0 {
                                                rect.right() + 2.0
                                            } else {
                                                rect.x - 2.0
                                            },
                                            y,
                                            if v >= 0.0 { "l" } else { "r" },
                                        ),
                                    }
                                } else {
                                    let x = rect.x + rect.w / 2.0;
                                    let top_end = v >= 0.0;
                                    match pos.as_str() {
                                        "ctr" => (x, rect.y + rect.h / 2.0, "mid"),
                                        "inEnd" => (
                                            x,
                                            if top_end {
                                                rect.y + 2.0
                                            } else {
                                                rect.bottom() - 2.0
                                            },
                                            if top_end { "top" } else { "bottom" },
                                        ),
                                        "inBase" => (
                                            x,
                                            if top_end {
                                                rect.bottom() - 2.0
                                            } else {
                                                rect.y + 2.0
                                            },
                                            if top_end { "bottom" } else { "top" },
                                        ),
                                        _ => (
                                            x,
                                            if top_end {
                                                rect.y - 2.0
                                            } else {
                                                rect.bottom() + 2.0
                                            },
                                            if top_end { "bottom" } else { "top" },
                                        ),
                                    }
                                };
                                label_jobs.push((text, lx, ly, lf.clone(), anchor));
                            }
                        }
                    }
                }
                Kind::Line | Kind::Area | Kind::Scatter => {
                    let mut stack = vec![0.0_f64; count];
                    let totals: Vec<f64> = (0..count)
                        .map(|k| {
                            g.series
                                .iter()
                                .map(|s| s.values.get(k).copied().flatten().unwrap_or(0.0))
                                .sum()
                        })
                        .collect();
                    let between = g_val.and_then(|a| a.between).or(val_axis.between);
                    let on_ticks = match between {
                        Some(b) => !b,
                        None => g.kind == Kind::Area,
                    };
                    let x_of = |k: usize| -> f32 {
                        if on_ticks && count > 1 {
                            let t = k as f32 / (count - 1) as f32;
                            let t = if cat_reversed { 1.0 - t } else { t };
                            if horizontal {
                                inner.bottom() - inner.h * t
                            } else {
                                inner.x + inner.w * t
                            }
                        } else {
                            band_start(k) + band_len / 2.0
                        }
                    };
                    let mut previous: Vec<(f32, f32)> = Vec::new();
                    for s in &g.series {
                        let labels = labels_of(s);
                        let lf =
                            self.chart_font(base, labels.as_ref().and_then(|l| l.font.as_ref()));
                        let mut pts: Vec<(f32, f32)> = Vec::new();
                        let mut shown: Vec<(usize, f64)> = Vec::new();
                        let n = if g.kind == Kind::Scatter {
                            s.values.len()
                        } else {
                            count
                        };
                        for k in 0..n {
                            let raw = s.values.get(k).copied().flatten();
                            let Some(v) = raw.or(if g.kind == Kind::Area {
                                Some(0.0)
                            } else {
                                None
                            }) else {
                                continue;
                            };
                            let v = match g.grouping {
                                Grouping::Clustered => v,
                                Grouping::Stacked => {
                                    stack[k] += v;
                                    stack[k]
                                }
                                Grouping::Percent => {
                                    stack[k] += v;
                                    if totals[k] != 0.0 {
                                        stack[k] / totals[k]
                                    } else {
                                        0.0
                                    }
                                }
                            };
                            let pt = if g.kind == Kind::Scatter {
                                let x = if s.xs.is_empty() {
                                    Some((k + 1) as f64)
                                } else {
                                    s.xs.get(k).copied().flatten()
                                };
                                let Some(x) = x else { continue };
                                let xs = x_scale.unwrap_or(Scale {
                                    lo: 0.0,
                                    hi: 1.0,
                                    unit: 1.0,
                                });
                                let t = xs.at(x) as f32;
                                let t = if cat_axis.reversed { 1.0 - t } else { t };
                                (inner.x + inner.w * t, value_pos(v, &g_scale, g_rev))
                            } else if horizontal {
                                (value_pos(v, &g_scale, g_rev), x_of(k))
                            } else {
                                (x_of(k), value_pos(v, &g_scale, g_rev))
                            };
                            pts.push(pt);
                            shown.push((k, raw.unwrap_or(0.0)));
                        }
                        let paint =
                            self.series_paint(s.shape.as_ref(), accent_for(self, s.index), g.kind);
                        let smooth = s.smooth;
                        if g.kind == Kind::Area {
                            if pts.len() >= 2 {
                                let floor: Vec<(f32, f32)> =
                                    if g.grouping != Grouping::Clustered && !previous.is_empty() {
                                        previous.clone()
                                    } else {
                                        pts.iter()
                                            .map(|p| {
                                                let z = value_pos(zero, &g_scale, g_rev);
                                                if horizontal { (z, p.1) } else { (p.0, z) }
                                            })
                                            .collect()
                                    };
                                let fill_paint = Paint {
                                    fill: paint.fill,
                                    line: None,
                                    ..paint.clone()
                                };
                                self.polygon(&pts, &floor, &fill_paint, h);
                                if s.shape.as_ref().is_some_and(|sp| sp.child("ln").is_some()) {
                                    self.stroke_polyline(
                                        &pts,
                                        &Paint {
                                            fill: None,
                                            ..paint.clone()
                                        },
                                        false,
                                        h,
                                    );
                                }
                            }
                            previous = pts.clone();
                        } else {
                            let draw_line = !(g.kind == Kind::Scatter && g.style == "marker")
                                || s.shape
                                    .as_ref()
                                    .is_some_and(|sp| sp.path(&["ln", "solidFill"]).is_some());
                            if draw_line {
                                self.stroke_polyline(&pts, &paint, smooth, h);
                            }
                            if let Some(sym) = self.marker_symbol(g, s) {
                                let size = s
                                    .marker
                                    .as_ref()
                                    .and_then(|m| m.child("size"))
                                    .and_then(|z| num(z, "val"))
                                    .unwrap_or(5.0)
                                    .clamp(2.0, 72.0);
                                let own = s.marker.as_ref().and_then(|m| m.child("spPr"));
                                let auto = paint.line.unwrap_or(accent_for(self, s.index));
                                let mp = if own.is_some() {
                                    let q = self.series_paint(own, auto, Kind::Pie);
                                    Paint {
                                        line: if own.is_some_and(|o| o.child("ln").is_some()) {
                                            q.line
                                        } else {
                                            Some(auto)
                                        },
                                        ..q
                                    }
                                } else {
                                    Paint {
                                        fill: Some(auto),
                                        line: Some(auto),
                                        width: 0.75,
                                        dash: Vec::new(),
                                    }
                                };
                                for p in &pts {
                                    self.marker(&sym, *p, size, &mp, h);
                                }
                            }
                        }
                        if let Some(l) = labels.as_ref().filter(|l| l.any()) {
                            for (p, (k, v)) in pts.iter().zip(&shown) {
                                if l.deleted.contains(k) {
                                    continue;
                                }
                                let text = self.label_text(l, s, *k, *v, 0.0, categories.get(*k));
                                let (lx, ly, anchor) = match l.position.as_deref() {
                                    Some("ctr") => (p.0, p.1, "mid"),
                                    Some("t") => (p.0, p.1 - 4.0, "bottom"),
                                    Some("b") => (p.0, p.1 + 4.0, "top"),
                                    Some("l") => (p.0 - 5.0, p.1, "r"),
                                    _ if g.kind == Kind::Area => (p.0, p.1 + 4.0, "top"),
                                    _ => (p.0 + 5.0, p.1, "l"),
                                };
                                label_jobs.push((text, lx, ly, lf.clone(), anchor));
                            }
                        }
                    }
                }
                Kind::Pie | Kind::Doughnut => {}
            }
        }
        let axis_default = Some(Color::rgb(0xBFBFBF));
        if !cat_axis.deleted {
            let paint = self.box_paint(cat_axis.shape.as_ref(), None, axis_default);
            if scatter {
                let y = value_pos(zero, &scale, val_axis.reversed);
                self.stroke_polyline(&[(inner.x, y), (inner.right(), y)], &paint, false, h);
            } else if horizontal {
                let x = value_pos(zero, &scale, val_axis.reversed);
                self.stroke_polyline(&[(x, inner.y), (x, inner.bottom())], &paint, false, h);
            } else {
                let y = if val_axis.crosses_max {
                    inner.y
                } else {
                    value_pos(zero, &scale, val_axis.reversed)
                };
                self.stroke_polyline(&[(inner.x, y), (inner.right(), y)], &paint, false, h);
            }
        }
        if !val_axis.deleted
            && val_axis
                .shape
                .as_ref()
                .is_some_and(|s| s.child("ln").is_some_and(|l| l.child("noFill").is_none()))
        {
            let paint = self.box_paint(val_axis.shape.as_ref(), None, axis_default);
            if horizontal {
                self.stroke_polyline(
                    &[(inner.x, inner.bottom()), (inner.right(), inner.bottom())],
                    &paint,
                    false,
                    h,
                );
            } else {
                let x = if val_axis.position == "r" {
                    inner.right()
                } else {
                    inner.x
                };
                self.stroke_polyline(&[(x, inner.y), (x, inner.bottom())], &paint, false, h);
            }
        }
        if show_val_labels {
            for v in scale.ticks() {
                let text = fmt_val(v);
                let spans = self.spans(&text, &val_font);
                let block = self.block(&spans, None);
                let p = value_pos(v, &scale, val_axis.reversed);
                if horizontal {
                    let x = p - block.width / 2.0;
                    let bw = block.width;
                    self.draw_block(&block, x, inner.bottom() + 3.0, bw, "ctr", h);
                } else {
                    let y = p - block.height / 2.0;
                    let bw = block.width;
                    if val_axis.position == "r" {
                        self.draw_block(&block, inner.right() + 4.0, y, bw, "l", h);
                    } else {
                        self.draw_block(&block, inner.x - 4.0 - bw, y, bw, "r", h);
                    }
                }
            }
        }
        if let Some((axis, range, font, code)) = &second {
            let s = make_scale(inner.h, axis, *range);
            for v in s.ticks() {
                let spans = self.spans(&format(code, v), font);
                let block = self.block(&spans, None);
                let y = value_pos(v, &s, axis.reversed) - block.height / 2.0;
                let bw = block.width;
                self.draw_block(&block, inner.right() + 4.0, y, bw, "l", h);
            }
        }
        if show_cat_labels {
            if scatter {
                if let Some(xs) = x_scale {
                    for v in xs.ticks() {
                        let t = xs.at(v) as f32;
                        let x = inner.x + inner.w * if cat_axis.reversed { 1.0 - t } else { t };
                        let spans = self.spans(&fmt_x(v), &x_font);
                        let block = self.block(&spans, None);
                        let bw = block.width;
                        let y =
                            value_pos(scale.lo.max(zero.min(scale.lo)), &scale, val_axis.reversed);
                        let y = y.max(inner.bottom()) + 3.0;
                        self.draw_block(&block, x - bw / 2.0, y, bw, "ctr", h);
                    }
                }
            } else {
                let widths: Vec<Block> = categories
                    .iter()
                    .map(|c| {
                        let spans = self.spans(c, &cat_font);
                        self.block(
                            &spans,
                            if horizontal {
                                None
                            } else {
                                Some(band_len.max(20.0))
                            },
                        )
                    })
                    .collect();
                let need = widths
                    .iter()
                    .map(|b| if horizontal { b.height } else { b.width })
                    .fold(0.0, f32::max);
                let skip = if horizontal {
                    ((need / band_len).ceil() as usize).max(1)
                } else {
                    let widest_line = widths.iter().map(|b| b.width).fold(0.0, f32::max);
                    ((widest_line + 2.0) / band_len).ceil().max(1.0) as usize
                };
                for (k, block) in widths.iter().enumerate() {
                    if k % skip != 0 {
                        continue;
                    }
                    let start = band_start(k);
                    if horizontal {
                        let y = start + band_len / 2.0 - block.height / 2.0;
                        let bw = block.width;
                        self.draw_block(block, inner.x - 4.0 - bw, y, bw, "r", h);
                    } else {
                        let y = if val_axis.crosses_max {
                            inner.bottom()
                        } else {
                            inner
                                .bottom()
                                .max(value_pos(zero, &scale, val_axis.reversed))
                        };
                        let y = inner.bottom().max(y) + 3.0;
                        self.draw_block(block, start, y, band_len, "ctr", h);
                    }
                }
            }
        }
        for (text, x, y, font, anchor) in label_jobs {
            let spans = self.spans(&text, &font);
            let block = self.block(&spans, None);
            let (bx, by) = match anchor {
                "bottom" => (x - block.width / 2.0, y - block.height),
                "top" => (x - block.width / 2.0, y),
                "l" => (x, y - block.height / 2.0),
                "r" => (x - block.width, y - block.height / 2.0),
                _ => (x - block.width / 2.0, y - block.height / 2.0),
            };
            let bw = block.width;
            self.draw_block(&block, bx, by, bw, "ctr", h);
        }
    }

    fn draw_turned(&mut self, block: &Block, x: f32, y: f32, len: f32, h: f32) {
        let page = self.canvas.page(self.page);
        page.save();
        let cx = x;
        let cy = h - (y + len / 2.0);
        page.transform([0.0, 1.0, -1.0, 0.0, cx, cy]);
        let mut top = 0.0;
        for line in &block.lines {
            let baseline = top + line.ascent;
            let lx = -line.width / 2.0;
            line.draw(page, lx, -baseline, 0.0);
            top += line.height();
        }
        page.restore();
    }

    fn polygon(&mut self, upper: &[(f32, f32)], floor: &[(f32, f32)], p: &Paint, h: f32) {
        let Some(c) = p.fill else { return };
        let page = self.canvas.page(self.page);
        page.save().set_fill(c.pdf());
        if c.a < 1.0 {
            page.set_alpha(c.a, 1.0);
        }
        page.move_to(upper[0].0, h - upper[0].1);
        for q in &upper[1..] {
            page.line_to(q.0, h - q.1);
        }
        for q in floor.iter().rev() {
            page.line_to(q.0, h - q.1);
        }
        page.close().fill();
        page.restore();
    }
}

fn is_drawn(name: &str) -> bool {
    matches!(
        name,
        "barChart"
            | "bar3DChart"
            | "lineChart"
            | "line3DChart"
            | "areaChart"
            | "area3DChart"
            | "pieChart"
            | "pie3DChart"
            | "doughnutChart"
            | "scatterChart"
    )
}

fn manual_layout(layout: Option<&Element>, w: f32, h: f32) -> Option<Rect> {
    let m = layout?.child("manualLayout")?;
    let v = |k: &str| {
        m.child(k)
            .and_then(|e| e.attr("val"))
            .and_then(|v| v.parse::<f32>().ok())
    };
    let mode = |k: &str| m.child(k).and_then(|e| e.attr("val")).unwrap_or("factor");
    if mode("xMode") != "edge" || mode("yMode") != "edge" {
        return None;
    }
    let (x, y, fw, fh) = (v("x")?, v("y")?, v("w")?, v("h")?);
    if fw <= 0.0 || fh <= 0.0 {
        return None;
    }
    Some(Rect {
        x: x * w,
        y: y * h,
        w: fw * w,
        h: fh * h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_and_formats() {
        let s = auto_scale(40.0, 80.0, 10.0, None, None);
        assert_eq!((s.lo, s.unit), (0.0, 10.0));
        assert!((s.hi - 90.0).abs() < 1e-9);
        let s = auto_scale(80.0, 210.0, 10.0, None, None);
        assert_eq!((s.lo, s.hi, s.unit), (0.0, 250.0, 50.0));
        let s = auto_scale(-12.0, 30.0, 5.0, None, None);
        assert!(s.lo <= -12.0 && s.hi >= 30.0);
        let s = auto_scale(1000.0, 1010.0, 5.0, None, None);
        assert!(
            s.lo > 900.0,
            "a narrow band away from zero keeps its own low end"
        );
        assert_eq!(nice_step(0.3), 0.5);
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(general(1234.5), "1234.5");
        assert_eq!(general(0.1 + 0.2), "0.3");
        assert_eq!(simple_format("0%", 0.256), "26%");
        assert_eq!(simple_format("#,##0.00", 1234.5), "1,234.50");
        assert_eq!(simple_format("0", -3.4), "-3");
    }

    #[test]
    fn caches() {
        let e = convert_office_read::xml::parse(
            "<c:val><c:numRef><c:f>S!B2:B4</c:f><c:numCache><c:formatCode>0.0</c:formatCode>\
             <c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>1.5</c:v></c:pt>\
             <c:pt idx=\"2\"><c:v>3</c:v></c:pt></c:numCache></c:numRef></c:val>",
        )
        .unwrap();
        let (values, code) = cached_numbers(Some(&e));
        assert_eq!(values, vec![Some(1.5), None, Some(3.0)]);
        assert_eq!(code, "0.0");
        let strings = cached_strings(Some(&e), &simple_format);
        assert_eq!(strings, vec!["1.5", "", "3.0"]);
    }
}
