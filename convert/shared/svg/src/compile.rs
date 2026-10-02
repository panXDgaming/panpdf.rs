use std::collections::{BTreeSet, HashMap};

use convert_pdf_canvas::{Gradient, GradientKind, Rgb};

use crate::css::{Sheet, declarations};
use crate::path::{self, Path, Seg};
use crate::style::{PROPERTIES, Paint, Style, attr_length};
use crate::values::{
    self, Aspect, Colour, IDENTITY, Length, Matrix, aspect, numbers, then, view_box,
    view_box_matrix,
};
use crate::xml::{Child, Element};
use crate::{Piece, Segment};

const MOST_OPS: usize = 1_000_000;
const MOST_USE_DEPTH: usize = 12;

#[derive(Clone, Debug)]
pub(crate) enum Brush {
    Solid(Colour),
    Gradient {
        colours: Gradient,
        alpha: Option<Gradient>,
        matrix: Matrix,
        opacity: f32,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct StrokeStyle {
    pub width: f32,
    pub cap: u8,
    pub join: u8,
    pub miter: f32,
    pub dash: Option<(Vec<f32>, f32)>,
}

#[derive(Clone, Debug)]
pub(crate) struct TextPart {
    pub dx: f32,
    pub dy: f32,
    pub segment: usize,
    pub visible: bool,
    pub alpha: f32,
}

type Chunk = (Option<f32>, Option<f32>, u8, Vec<TextPart>);

#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "shapes are most operations; boxing them would cost more than the size"
)]
pub(crate) enum Op {
    Save,
    Restore,
    Transform(Matrix),
    Clip {
        path: Path,
        even_odd: bool,
    },
    Shape {
        path: Path,
        bbox: [f32; 4],
        fill: Option<(Brush, bool)>,
        stroke: Option<(Brush, StrokeStyle)>,
    },
    TextStart,
    Text {
        x: Option<f32>,
        y: Option<f32>,
        anchor: u8,
        parts: Vec<TextPart>,
    },
    Image {
        index: usize,
        viewport: [f32; 4],
        aspect: Aspect,
    },
}

pub(crate) struct Compiler<'a> {
    ids: HashMap<&'a str, &'a Element>,
    sheet: Sheet,
    pub ops: Vec<Op>,
    pub segments: Vec<Segment>,
    pub images: Vec<Vec<u8>>,
    pub notes: BTreeSet<&'static str>,
    ancestors: Vec<&'a Element>,
    uses: Vec<*const Element>,
    viewport: (f32, f32),
}

const NOT_DRAWN: &[&str] = &[
    "defs",
    "symbol",
    "lineargradient",
    "radialgradient",
    "clippath",
    "mask",
    "pattern",
    "marker",
    "style",
    "title",
    "desc",
    "metadata",
    "filter",
    "font",
    "font-face",
    "cursor",
    "view",
    "stop",
];

fn to_rgb(c: Colour) -> Rgb {
    Rgb(c.r, c.g, c.b)
}

fn bbox_matrix(b: [f32; 4]) -> Matrix {
    [b[2] - b[0], 0.0, 0.0, b[3] - b[1], b[0], b[1]]
}

fn union(a: Option<[f32; 4]>, b: [f32; 4]) -> [f32; 4] {
    a.map_or(b, |a| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}

fn transform_box(b: [f32; 4], m: Matrix) -> [f32; 4] {
    let corners = [
        values::apply(m, b[0], b[1]),
        values::apply(m, b[2], b[1]),
        values::apply(m, b[0], b[3]),
        values::apply(m, b[2], b[3]),
    ];
    let mut out = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (x, y) in corners {
        out = [out[0].min(x), out[1].min(y), out[2].max(x), out[3].max(y)];
    }
    out
}

impl<'a> Compiler<'a> {
    pub(crate) fn new(root: &'a Element, extra_css: &str) -> Self {
        let mut ids = HashMap::new();
        let mut sheet = Sheet::default();
        sheet.add(extra_css);
        root.walk(&mut |e| {
            if let Some(id) = e.attr("id") {
                ids.entry(id).or_insert(e);
            }
            if e.name == "style" {
                sheet.add(&e.text());
            }
        });
        Self {
            ids,
            sheet,
            ops: Vec::new(),
            segments: Vec::new(),
            images: Vec::new(),
            notes: BTreeSet::new(),
            ancestors: Vec::new(),
            uses: Vec::new(),
            viewport: (300.0, 150.0),
        }
    }

    fn full(&self) -> bool {
        self.ops.len() > MOST_OPS
    }

    fn style_of(&self, e: &Element, parent: &Style) -> Style {
        let mut props: Vec<(String, String)> = e
            .attrs
            .iter()
            .filter(|(k, _)| PROPERTIES.contains(&k.as_str()))
            .cloned()
            .collect();
        if !self.sheet.is_empty() {
            props.extend(self.sheet.matching(e, &self.ancestors));
        }
        if let Some(style) = e.attr("style") {
            props.extend(declarations(style));
        }
        let mut style = parent.inherit();
        style.apply(parent, &props, self.viewport);
        style
    }

    fn len(&self, e: &Element, name: &str, style: &Style, axis: u8, default: f32) -> f32 {
        let of = match axis {
            0 => self.viewport.0,
            1 => self.viewport.1,
            _ => ((self.viewport.0.powi(2) + self.viewport.1.powi(2)) / 2.0).sqrt(),
        };
        attr_length(e.attr(name)).map_or(default, |l| l.resolve(style.font_size, of))
    }

    pub(crate) fn root(&mut self, root: &'a Element, size: (f32, f32), base: &Style) {
        self.viewport = size;
        let style = self.style_of(root, base);
        if !style.display {
            return;
        }
        self.ancestors.push(root);
        let opacity = style.opacity;
        self.children(root, &style, opacity);
        self.ancestors.pop();
    }

    fn children(&mut self, e: &'a Element, style: &Style, opacity: f32) {
        for child in e.elements() {
            if self.full() {
                self.notes.insert("parts past the size limit");
                return;
            }
            self.element(child, style, opacity);
        }
    }

    fn note_unsupported(&mut self, name: &str) -> bool {
        let note = match name {
            "foreignobject" => "foreignObject",
            "script" => "scripts",
            "animate" | "animatemotion" | "animatetransform" | "animatecolor" | "set" => {
                "animation (the first state is drawn)"
            }
            "textpath" => "text on a path",
            _ => return false,
        };
        self.notes.insert(note);
        true
    }

    fn element(&mut self, e: &'a Element, parent: &Style, opacity: f32) {
        let name = e.name.as_str();
        if NOT_DRAWN.contains(&name) || self.note_unsupported(name) || name.contains(':') {
            return;
        }
        if !matches!(
            name,
            "g" | "a"
                | "switch"
                | "svg"
                | "use"
                | "path"
                | "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "text"
                | "image"
        ) {
            return;
        }
        let style = self.style_of(e, parent);
        if !style.display {
            return;
        }
        let opacity = opacity * style.opacity;
        if opacity <= 0.0 {
            return;
        }
        if style.filter {
            self.notes.insert("filters (drawn unfiltered)");
        }
        if style.mask {
            self.notes.insert("masks (drawn unmasked)");
        }
        if style.markers {
            self.notes.insert("markers");
        }
        let mut m = e
            .attr("transform")
            .map_or(Some(IDENTITY), values::transform)
            .unwrap_or(IDENTITY);
        if let Some(css) = style.transform {
            m = css;
        }
        if !values::usable(m) {
            return;
        }
        let transformed = m != IDENTITY;
        let clip = style.clip_path.clone();
        let wrap = transformed || clip.is_some();
        if wrap {
            self.ops.push(Op::Save);
            if transformed {
                self.ops.push(Op::Transform(m));
            }
            if let Some(id) = clip {
                self.clip(&id, e, &style);
            }
        }
        self.ancestors.push(e);
        match name {
            "g" | "a" => self.children(e, &style, opacity),
            "switch" => {
                if let Some(first) = e
                    .elements()
                    .find(|c| c.name != "foreignobject" && !NOT_DRAWN.contains(&c.name.as_str()))
                {
                    self.element(first, &style, opacity);
                } else if e.elements().any(|c| c.name == "foreignobject") {
                    self.notes.insert("foreignObject");
                }
            }
            "svg" => self.nested_svg(e, &style, opacity),
            "use" => self.use_element(e, &style, opacity),
            "text" => self.text(e, &style, opacity),
            "image" => self.image(e, &style),
            _ => {
                if let Some(path) = self.geometry(e, &style) {
                    self.shape(path, e.name == "line", &style, opacity);
                }
            }
        }
        self.ancestors.pop();
        if wrap {
            self.ops.push(Op::Restore);
        }
    }

    fn geometry(&self, e: &Element, s: &Style) -> Option<Path> {
        let path = match e.name.as_str() {
            "path" => path::parse(e.attr("d")?),
            "rect" => {
                let (x, y) = (self.len(e, "x", s, 0, 0.0), self.len(e, "y", s, 1, 0.0));
                let (w, h) = (
                    self.len(e, "width", s, 0, 0.0),
                    self.len(e, "height", s, 1, 0.0),
                );
                if w <= 0.0 || h <= 0.0 {
                    return None;
                }
                let rx = attr_length(e.attr("rx")).map(|l| l.resolve(s.font_size, self.viewport.0));
                let ry = attr_length(e.attr("ry")).map(|l| l.resolve(s.font_size, self.viewport.1));
                let (rx, ry) = match (rx, ry) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a),
                    (None, Some(b)) => (b, b),
                    (None, None) => (0.0, 0.0),
                };
                path::rect(x, y, w, h, rx, ry)
            }
            "circle" => {
                let r = self.len(e, "r", s, 2, 0.0);
                if r <= 0.0 {
                    return None;
                }
                path::ellipse(
                    self.len(e, "cx", s, 0, 0.0),
                    self.len(e, "cy", s, 1, 0.0),
                    r,
                    r,
                )
            }
            "ellipse" => {
                let rx = attr_length(e.attr("rx")).map(|l| l.resolve(s.font_size, self.viewport.0));
                let ry = attr_length(e.attr("ry")).map(|l| l.resolve(s.font_size, self.viewport.1));
                let (rx, ry) = match (rx, ry) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a),
                    (None, Some(b)) => (b, b),
                    (None, None) => return None,
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return None;
                }
                path::ellipse(
                    self.len(e, "cx", s, 0, 0.0),
                    self.len(e, "cy", s, 1, 0.0),
                    rx,
                    ry,
                )
            }
            "line" => vec![
                Seg::Move(self.len(e, "x1", s, 0, 0.0), self.len(e, "y1", s, 1, 0.0)),
                Seg::Line(self.len(e, "x2", s, 0, 0.0), self.len(e, "y2", s, 1, 0.0)),
            ],
            "polyline" | "polygon" => path::poly(&numbers(e.attr("points")?), e.name == "polygon"),
            _ => return None,
        };
        (!path.is_empty()).then_some(path)
    }

    fn shape(&mut self, path: Path, line: bool, s: &Style, opacity: f32) {
        if !s.visible {
            return;
        }
        let Some(bbox) = path::bounds(&path) else {
            return;
        };
        let fill = if line {
            None
        } else {
            self.brush(&s.fill.clone(), s, s.fill_opacity * opacity, bbox)
                .map(|b| (b, s.fill_even_odd))
        };
        let stroke = if s.stroke_width > 0.0 {
            self.brush(&s.stroke.clone(), s, s.stroke_opacity * opacity, bbox)
                .map(|b| {
                    (
                        b,
                        StrokeStyle {
                            width: s.stroke_width,
                            cap: s.cap,
                            join: s.join,
                            miter: s.miter,
                            dash: s.dash.clone().map(|d| (d, s.dash_offset)),
                        },
                    )
                })
        } else {
            None
        };
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.ops.push(Op::Shape {
            path,
            bbox,
            fill,
            stroke,
        });
    }

    fn brush(&mut self, paint: &Paint, s: &Style, opacity: f32, bbox: [f32; 4]) -> Option<Brush> {
        let solid = |c: Colour| {
            let a = c.a * opacity;
            (a > 0.0).then_some(Brush::Solid(Colour { a, ..c }))
        };
        match paint {
            Paint::None => None,
            Paint::Colour(c) => solid(*c),
            Paint::Current => solid(s.color),
            Paint::Url(id, fallback) => {
                let target = self.ids.get(id.as_str()).copied();
                match target.map(|t| t.name.as_str()) {
                    Some("lineargradient" | "radialgradient") => {
                        self.gradient(target?, s, opacity, bbox)
                    }
                    other => {
                        if other == Some("pattern") {
                            self.notes.insert("patterns");
                        }
                        match fallback {
                            Some(f) => self.brush(&f.clone(), s, opacity, bbox),
                            None => None,
                        }
                    }
                }
            }
        }
    }

    fn gradient(
        &mut self,
        e: &'a Element,
        s: &Style,
        opacity: f32,
        bbox: [f32; 4],
    ) -> Option<Brush> {
        let mut chain = vec![e];
        while chain.len() < 8 {
            let Some(next) = chain
                .last()
                .and_then(|g| g.attr("href"))
                .and_then(|h| h.strip_prefix('#'))
                .and_then(|id| self.ids.get(id).copied())
                .filter(|g| {
                    g.name.ends_with("gradient") && !chain.iter().any(|c| std::ptr::eq(*c, *g))
                })
            else {
                break;
            };
            chain.push(next);
        }
        let attr = |name: &str| chain.iter().find_map(|g| g.attr(name));
        let stops_from = chain
            .iter()
            .find(|g| g.elements().any(|c| c.name == "stop"))?;
        let mut stops = Vec::new();
        let base = Style::default();
        for stop in stops_from.elements().filter(|c| c.name == "stop") {
            let st = self.style_of(stop, &base);
            let offset = stop.attr("offset").map_or(0.0, |o| {
                let o = o.trim();
                o.strip_suffix('%')
                    .map_or_else(
                        || o.parse::<f32>().ok(),
                        |p| p.trim().parse::<f32>().ok().map(|v| v / 100.0),
                    )
                    .unwrap_or(0.0)
            });
            let c = st.stop_colour;
            stops.push((
                offset.clamp(0.0, 1.0),
                c,
                (c.a * st.stop_opacity).clamp(0.0, 1.0),
            ));
        }
        if stops.is_empty() {
            return None;
        }
        if stops.len() == 1 {
            let (_, c, a) = stops[0];
            return (a * opacity > 0.0).then_some(Brush::Solid(Colour {
                a: a * opacity,
                ..c
            }));
        }
        let bbox_units =
            attr("gradientunits").is_none_or(|u| u != "userSpaceOnUse" && u != "userspaceonuse");
        if bbox_units && (bbox[2] - bbox[0] <= 0.0 || bbox[3] - bbox[1] <= 0.0) {
            return None;
        }
        if attr("spreadmethod").is_some_and(|m| m != "pad") {
            self.notes
                .insert("gradient spreadMethod reflect/repeat (drawn as pad)");
        }
        let gt = attr("gradienttransform")
            .and_then(values::transform)
            .unwrap_or(IDENTITY);
        let coord = |name: &str, default: Length, axis: u8| -> f32 {
            let l = attr(name).and_then(values::length).unwrap_or(default);
            if bbox_units {
                match l {
                    Length::Percent(v) => v / 100.0,
                    other => other.resolve(s.font_size, 1.0),
                }
            } else {
                let of = match axis {
                    0 => self.viewport.0,
                    1 => self.viewport.1,
                    _ => ((self.viewport.0.powi(2) + self.viewport.1.powi(2)) / 2.0).sqrt(),
                };
                l.resolve(s.font_size, of)
            }
        };
        let kind = if e.name == "lineargradient" {
            GradientKind::Axial {
                x1: coord("x1", Length::Percent(0.0), 0),
                y1: coord("y1", Length::Percent(0.0), 1),
                x2: coord("x2", Length::Percent(100.0), 0),
                y2: coord("y2", Length::Percent(0.0), 1),
            }
        } else {
            let cx = coord("cx", Length::Percent(50.0), 0);
            let cy = coord("cy", Length::Percent(50.0), 1);
            let r = coord("r", Length::Percent(50.0), 2);
            let mut fx = attr("fx").map_or(cx, |_| coord("fx", Length::User(0.0), 0));
            let mut fy = attr("fy").map_or(cy, |_| coord("fy", Length::User(0.0), 1));
            let (dx, dy) = (fx - cx, fy - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d > r * 0.99 && d > 0.0 {
                fx = cx + dx * r * 0.99 / d;
                fy = cy + dy * r * 0.99 / d;
            }
            if r <= 0.0 {
                let (_, c, a) = *stops.last()?;
                return (a * opacity > 0.0).then_some(Brush::Solid(Colour {
                    a: a * opacity,
                    ..c
                }));
            }
            GradientKind::Radial {
                fx,
                fy,
                fr: coord("fr", Length::User(0.0), 2),
                cx,
                cy,
                r,
            }
        };
        let matrix = if bbox_units {
            then(gt, bbox_matrix(bbox))
        } else {
            gt
        };
        if !values::usable(matrix) {
            return None;
        }
        let colours = Gradient {
            kind,
            stops: stops.iter().map(|(o, c, _)| (*o, to_rgb(*c))).collect(),
        };
        let first_alpha = stops[0].2;
        let even = stops
            .iter()
            .all(|(_, _, a)| (a - first_alpha).abs() < 0.004);
        let (alpha, opacity) = if even {
            (None, opacity * first_alpha)
        } else {
            (
                Some(Gradient {
                    kind,
                    stops: stops
                        .iter()
                        .map(|(o, _, a)| (*o, Rgb(*a, *a, *a)))
                        .collect(),
                }),
                opacity,
            )
        };
        (opacity > 0.0).then_some(Brush::Gradient {
            colours,
            alpha,
            matrix,
            opacity,
        })
    }

    fn bbox(&mut self, e: &'a Element, s: &Style, depth: usize) -> Option<[f32; 4]> {
        if depth > MOST_USE_DEPTH {
            return None;
        }
        match e.name.as_str() {
            "g" | "a" | "svg" | "switch" | "symbol" => {
                let mut out = None;
                for c in e.elements() {
                    let cs = self.style_of(c, s);
                    if !cs.display || NOT_DRAWN.contains(&c.name.as_str()) {
                        continue;
                    }
                    if let Some(b) = self.bbox(c, &cs, depth + 1) {
                        let m = c
                            .attr("transform")
                            .and_then(values::transform)
                            .unwrap_or(IDENTITY);
                        out = Some(union(out, transform_box(b, m)));
                    }
                }
                out
            }
            "use" => {
                let target = e
                    .attr("href")?
                    .strip_prefix('#')
                    .and_then(|id| self.ids.get(id).copied())?;
                let ts = self.style_of(target, s);
                let b = self.bbox(target, &ts, depth + 1)?;
                let x = self.len(e, "x", s, 0, 0.0);
                let y = self.len(e, "y", s, 1, 0.0);
                Some([b[0] + x, b[1] + y, b[2] + x, b[3] + y])
            }
            "text" => {
                let x = self.len(e, "x", s, 0, 0.0);
                let y = self.len(e, "y", s, 1, 0.0);
                let n = crate::xml::Element::text(e).chars().count().max(1) as f32;
                Some([x, y - s.font_size, x + n * s.font_size * 0.5, y])
            }
            "image" => {
                let x = self.len(e, "x", s, 0, 0.0);
                let y = self.len(e, "y", s, 1, 0.0);
                let w = self.len(e, "width", s, 0, 0.0);
                let h = self.len(e, "height", s, 1, 0.0);
                Some([x, y, x + w, y + h])
            }
            _ => path::bounds(&self.geometry(e, s)?),
        }
    }

    fn clip(&mut self, id: &str, e: &'a Element, s: &Style) {
        let Some(target) = self.ids.get(id).copied().filter(|t| t.name == "clippath") else {
            return;
        };
        let mut m = target
            .attr("transform")
            .and_then(values::transform)
            .unwrap_or(IDENTITY);
        if target
            .attr("clippathunits")
            .is_some_and(|u| u.eq_ignore_ascii_case("objectBoundingBox"))
        {
            let Some(b) = self.bbox(e, s, 0) else {
                return;
            };
            m = then(m, bbox_matrix(b));
        }
        if target.attr("clip-path").is_some() {
            self.notes.insert("clip paths of clip paths");
        }
        let mut combined: Path = Vec::new();
        let mut even_odd = false;
        let base = Style::default();
        for child in target.elements() {
            let (shape, extra) = if child.name == "use" {
                let Some(t) = child
                    .attr("href")
                    .and_then(|h| h.strip_prefix('#'))
                    .and_then(|id| self.ids.get(id).copied())
                else {
                    continue;
                };
                let x = self.len(child, "x", &base, 0, 0.0);
                let y = self.len(child, "y", &base, 1, 0.0);
                (t, [1.0, 0.0, 0.0, 1.0, x, y])
            } else {
                (child, IDENTITY)
            };
            if shape.name == "text" {
                self.notes.insert("text as a clip path");
                continue;
            }
            let cs = self.style_of(shape, &base);
            if !cs.display {
                continue;
            }
            let Some(p) = self.geometry(shape, &cs) else {
                continue;
            };
            let own = shape
                .attr("transform")
                .and_then(values::transform)
                .unwrap_or(IDENTITY);
            let child_t = child
                .attr("transform")
                .filter(|_| !std::ptr::eq(child, shape))
                .and_then(values::transform)
                .unwrap_or(IDENTITY);
            let full = then(then(then(own, extra), child_t), m);
            combined.extend(path::transformed(&p, full));
            even_odd = cs.clip_even_odd;
        }
        if combined.is_empty() {
            combined = vec![Seg::Move(0.0, 0.0), Seg::Line(0.0, 0.0), Seg::Close];
        }
        self.ops.push(Op::Clip {
            path: combined,
            even_odd,
        });
    }

    fn viewport_into(&mut self, e: &'a Element, s: &Style, rect: [f32; 4], opacity: f32) {
        let outer = self.viewport;
        self.ops.push(Op::Save);
        self.ops.push(Op::Clip {
            path: path::rect(rect[0], rect[1], rect[2], rect[3], 0.0, 0.0),
            even_odd: false,
        });
        match view_box(e.attr("viewbox")) {
            Some(vb) => {
                let m = view_box_matrix(vb, rect, aspect(e.attr("preserveaspectratio")));
                if values::usable(m) {
                    self.ops.push(Op::Transform(m));
                }
                self.viewport = (vb[2], vb[3]);
            }
            None => {
                self.ops
                    .push(Op::Transform([1.0, 0.0, 0.0, 1.0, rect[0], rect[1]]));
                self.viewport = (rect[2], rect[3]);
            }
        }
        self.children(e, s, opacity);
        self.ops.push(Op::Restore);
        self.viewport = outer;
    }

    fn nested_svg(&mut self, e: &'a Element, s: &Style, opacity: f32) {
        let rect = [
            self.len(e, "x", s, 0, 0.0),
            self.len(e, "y", s, 1, 0.0),
            self.len(e, "width", s, 0, self.viewport.0),
            self.len(e, "height", s, 1, self.viewport.1),
        ];
        if rect[2] > 0.0 && rect[3] > 0.0 {
            self.viewport_into(e, s, rect, opacity);
        }
    }

    fn use_element(&mut self, e: &'a Element, s: &Style, opacity: f32) {
        let Some(href) = e.attr("href") else {
            return;
        };
        let Some(id) = href.strip_prefix('#') else {
            self.notes.insert("use of another file");
            return;
        };
        let Some(target) = self.ids.get(id).copied() else {
            return;
        };
        let key: *const Element = target;
        if self.uses.len() >= MOST_USE_DEPTH
            || self.uses.contains(&key)
            || self.ancestors.iter().any(|a| std::ptr::eq(*a, target))
        {
            return;
        }
        let (x, y) = (self.len(e, "x", s, 0, 0.0), self.len(e, "y", s, 1, 0.0));
        self.uses.push(key);
        self.ops.push(Op::Save);
        if x != 0.0 || y != 0.0 {
            self.ops.push(Op::Transform([1.0, 0.0, 0.0, 1.0, x, y]));
        }
        if target.name == "symbol" || target.name == "svg" {
            let ts = self.style_of(target, s);
            if ts.display {
                let w = self.len(e, "width", s, 0, 0.0);
                let h = self.len(e, "height", s, 1, 0.0);
                let w = if e.attr("width").is_some() {
                    w
                } else {
                    self.len(target, "width", &ts, 0, self.viewport.0)
                };
                let h = if e.attr("height").is_some() {
                    h
                } else {
                    self.len(target, "height", &ts, 1, self.viewport.1)
                };
                if w > 0.0 && h > 0.0 {
                    self.ancestors.push(target);
                    self.viewport_into(target, &ts, [0.0, 0.0, w, h], opacity * ts.opacity);
                    self.ancestors.pop();
                }
            }
        } else {
            self.element(target, s, opacity);
        }
        self.ops.push(Op::Restore);
        self.uses.pop();
    }

    fn image(&mut self, e: &Element, s: &Style) {
        if !s.visible {
            return;
        }
        let Some(href) = e.attr("href") else {
            return;
        };
        let bytes = crate::data_uri(href);
        let Some(bytes) = bytes else {
            self.notes
                .insert("pictures not embedded in the drawing or not found");
            return;
        };
        if crate::is_svg(&bytes) {
            self.notes.insert("SVG pictures inside a drawing");
            return;
        }
        let x = self.len(e, "x", s, 0, 0.0);
        let y = self.len(e, "y", s, 1, 0.0);
        let w = self.len(e, "width", s, 0, 0.0);
        let h = self.len(e, "height", s, 1, 0.0);
        let index = self.images.len();
        self.images.push(bytes);
        self.ops.push(Op::Image {
            index,
            viewport: [x, y, w, h],
            aspect: aspect(e.attr("preserveaspectratio")),
        });
    }
}

struct Char {
    c: char,
    style: usize,
    x: Option<f32>,
    y: Option<f32>,
    dx: f32,
    dy: f32,
}

struct TextGather {
    chars: Vec<Char>,
    styles: Vec<Style>,
    last_space: bool,
    preserve: bool,
    ranges: Vec<(usize, usize, usize)>,
    lists: Vec<[Vec<Length>; 4]>,
}

impl<'a> Compiler<'a> {
    fn gather(&mut self, e: &'a Element, s: &Style, g: &mut TextGather) {
        let style_index = g.styles.len();
        g.styles.push(s.clone());
        let start = g.chars.len();
        let preserve = g.preserve;
        if let Some(space) = e.attr("xml:space") {
            g.preserve = space == "preserve";
        }
        for child in &e.children {
            match child {
                Child::Text(text) => {
                    for c in text.chars() {
                        let c = match c {
                            '\n' | '\r' if !g.preserve => continue,
                            '\n' | '\r' | '\t' => ' ',
                            c => c,
                        };
                        if c == ' ' && g.last_space && !g.preserve {
                            continue;
                        }
                        g.last_space = c == ' ';
                        g.chars.push(Char {
                            c,
                            style: style_index,
                            x: None,
                            y: None,
                            dx: 0.0,
                            dy: 0.0,
                        });
                    }
                }
                Child::Element(child) => {
                    let name = child.name.as_str();
                    if self.note_unsupported(name) && name != "textpath" {
                        continue;
                    }
                    if !matches!(name, "tspan" | "a" | "textpath") {
                        continue;
                    }
                    let cs = self.style_of(child, s);
                    if !cs.display {
                        continue;
                    }
                    self.ancestors.push(child);
                    self.gather(child, &cs, g);
                    self.ancestors.pop();
                }
            }
        }
        g.preserve = preserve;
        let lists =
            ["x", "y", "dx", "dy"].map(|n| e.attr(n).map(values::lengths).unwrap_or_default());
        if e.attr("rotate").is_some() {
            self.notes.insert("rotated letters in text");
        }
        if e.attr("textlength").is_some() {
            self.notes.insert("textLength");
        }
        if lists.iter().any(|l| !l.is_empty()) {
            g.ranges.push((start, g.chars.len(), style_index));
            g.lists.push(lists);
        }
    }

    fn text(&mut self, e: &'a Element, s: &Style, opacity: f32) {
        let mut g = TextGather {
            chars: Vec::new(),
            styles: Vec::new(),
            last_space: true,
            preserve: false,
            ranges: Vec::new(),
            lists: Vec::new(),
        };
        self.gather(e, s, &mut g);
        if !g.preserve {
            while g.chars.last().is_some_and(|c| c.c == ' ') {
                g.chars.pop();
            }
        }
        let order: Vec<usize> = {
            let mut o: Vec<usize> = (0..g.ranges.len()).collect();
            o.sort_by_key(|&i| {
                (
                    g.ranges[i].0,
                    std::cmp::Reverse(g.ranges[i].1),
                    std::cmp::Reverse(i),
                )
            });
            o
        };
        let (vw, vh) = self.viewport;
        for i in order {
            let (start, end, style) = g.ranges[i];
            let em = g.styles[style].font_size;
            let [xs, ys, dxs, dys] = &g.lists[i];
            let upto = end.min(g.chars.len());
            for (k, ch) in g.chars[start..upto].iter_mut().enumerate() {
                if let Some(v) = xs.get(k) {
                    ch.x = Some(v.resolve(em, vw));
                }
                if let Some(v) = ys.get(k) {
                    ch.y = Some(v.resolve(em, vh));
                }
                if let Some(v) = dxs.get(k) {
                    ch.dx = v.resolve(em, vw);
                }
                if let Some(v) = dys.get(k) {
                    ch.dy = v.resolve(em, vh);
                }
            }
        }
        if g.chars.is_empty() {
            return;
        }
        self.ops.push(Op::TextStart);
        let text: String = g.chars.iter().map(|c| c.c).collect();
        let mut index_of = HashMap::new();
        for (i, (at, _)) in text.char_indices().enumerate() {
            index_of.insert(at, i);
        }
        let clusters = convert_pdf_canvas::graphemes(&text);
        let mut chunk: Option<Chunk> = None;
        let baseline = s.baseline * s.font_size;
        let mut first = true;
        for range in clusters {
            let Some(&i) = index_of.get(&range.start) else {
                continue;
            };
            let head = &g.chars[i];
            let st = &g.styles[head.style];
            let (visible, colour, alpha) = self.text_paint(st, opacity);
            let absolute = head.x.is_some() || head.y.is_some();
            if absolute || first {
                if let Some((x, y, anchor, parts)) = chunk.take() {
                    self.ops.push(Op::Text {
                        x,
                        y,
                        anchor,
                        parts,
                    });
                }
                chunk = Some((
                    head.x,
                    head.y
                        .map(|y| y + baseline)
                        .or((first && baseline != 0.0).then_some(baseline)),
                    st.anchor,
                    Vec::new(),
                ));
                first = false;
            }
            let Some((_, _, _, parts)) = chunk.as_mut() else {
                continue;
            };
            let new_part = parts.last().is_none_or(|p: &TextPart| {
                head.dx != 0.0
                    || head.dy != 0.0
                    || p.visible != visible
                    || (p.alpha - alpha).abs() > 0.004
            });
            if new_part {
                self.segments.push(Segment {
                    pieces: Vec::new(),
                    rtl: s.rtl,
                });
                parts.push(TextPart {
                    dx: head.dx,
                    dy: head.dy,
                    segment: self.segments.len() - 1,
                    visible,
                    alpha,
                });
            }
            let Some(segment) = self.segments.last_mut() else {
                continue;
            };
            let piece_text = &text[range];
            let same = segment.pieces.last().is_some_and(|p| {
                p.family == st.font_family
                    && p.size == st.font_size
                    && p.bold == st.bold
                    && p.italic == st.italic
                    && p.colour == colour
                    && p.letter_spacing == st.letter_spacing
                    && p.underline == st.underline
                    && p.strike == st.strike
            });
            if same {
                if let Some(p) = segment.pieces.last_mut() {
                    p.text.push_str(piece_text);
                }
            } else {
                segment.pieces.push(Piece {
                    text: piece_text.to_owned(),
                    family: st.font_family.clone(),
                    size: st.font_size,
                    bold: st.bold,
                    italic: st.italic,
                    colour,
                    letter_spacing: st.letter_spacing,
                    underline: st.underline,
                    strike: st.strike,
                });
            }
        }
        if let Some((x, y, anchor, parts)) = chunk.take() {
            self.ops.push(Op::Text {
                x,
                y,
                anchor,
                parts,
            });
        }
    }

    fn text_paint(&mut self, st: &Style, opacity: f32) -> (bool, [u8; 3], f32) {
        if !st.visible {
            return (false, [0, 0, 0], 1.0);
        }
        if !matches!(st.stroke, Paint::None) {
            self.notes.insert("outlined (stroked) text, drawn filled");
        }
        let fill = match &st.fill {
            Paint::None => match &st.stroke {
                Paint::None => return (false, [0, 0, 0], 1.0),
                other => other.clone(),
            },
            other => other.clone(),
        };
        let a = st.fill_opacity * opacity;
        match self.brush(&fill, st, a, [0.0, 0.0, 1.0, 1.0]) {
            Some(Brush::Solid(c)) => (true, c.bytes(), c.a),
            Some(Brush::Gradient {
                colours, opacity, ..
            }) => {
                self.notes
                    .insert("gradient text, drawn in its first colour");
                let c = colours.stops.first().map_or(Rgb::BLACK, |s| s.1);
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                (true, [q(c.0), q(c.1), q(c.2)], opacity)
            }
            None => (false, [0, 0, 0], 1.0),
        }
    }
}
