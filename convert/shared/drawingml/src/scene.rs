use std::collections::HashMap;

use convert_office_read::{Element, Package};
use convert_pdf_canvas::{Canvas, FontBook, ImageInfo, Page, PageId};

use crate::geometry::{EMU, Matrix, Xfrm, arrow, emu, is_line, mul, num, path, scale};
use crate::geometry::{flag, translate};
use crate::theme::{Color, Palette, Theme};

pub trait Inherit {
    fn placeholders(&self, ph: &Element) -> [Option<&Element>; 2];
    fn master_style(&self, kind: Option<&str>) -> Option<&Element>;
}

pub struct NoInherit;

impl Inherit for NoInherit {
    fn placeholders(&self, _ph: &Element) -> [Option<&Element>; 2] {
        [None, None]
    }

    fn master_style(&self, _kind: Option<&str>) -> Option<&Element> {
        None
    }
}

pub struct Scene<'a, 'b> {
    pub canvas: &'a mut Canvas,
    pub fonts: &'a mut FontBook,
    pub package: &'a Package<'b>,
    pub images: &'a mut HashMap<String, Option<ImageInfo>>,
    pub theme: &'a Theme,
    pub map: &'a HashMap<String, String>,
    pub page: PageId,
    pub base: Matrix,
    pub notes: &'a mut Vec<String>,
    pub default_size: f32,
    pub default_text: Option<&'a Element>,
    pub format: &'a dyn Fn(&str, f64) -> String,
    pub hidden: &'a dyn Fn(&str) -> Option<Vec<bool>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub m: Matrix,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Outline {
    pub color: Option<Color>,
    pub width: f32,
    pub dash: Vec<f32>,
    pub ends: (bool, bool),
}

impl<'b> Scene<'_, 'b> {
    #[must_use]
    pub fn palette(&self, placeholder: Option<Color>) -> Palette<'_> {
        Palette {
            theme: self.theme,
            map: self.map,
            placeholder,
        }
    }

    pub fn page(&mut self) -> &mut Page {
        self.canvas.page(self.page)
    }

    pub fn apply(&mut self, m: Matrix, h: f32) {
        let local = [1.0, 0.0, 0.0, -1.0, 0.0, h];
        let full = mul(self.base, mul(m, local));
        self.page().transform(full);
    }

    pub fn image(&mut self, part: &str, id: &str) -> Option<ImageInfo> {
        let rel = self.package.rels(part).into_iter().find(|r| r.id == id)?;
        if rel.external {
            return None;
        }
        if let Some(found) = self.images.get(&rel.target) {
            return *found;
        }
        let info = self
            .package
            .bytes(&rel.target)
            .ok()
            .and_then(|bytes| self.canvas.add_image(&bytes).ok());
        if info.is_none() {
            self.notes.push(format!(
                "picture {} is not JPEG or PNG and was left out",
                rel.target
            ));
        }
        self.images.insert(rel.target.clone(), info);
        info
    }

    pub fn paint_fill(
        &mut self,
        fill: &Element,
        part: &str,
        placeholder: Option<Color>,
        w: f32,
        h: f32,
    ) {
        match fill.local() {
            "solidFill" => {
                if let Some(c) = self.palette(placeholder).first_color(fill) {
                    let page = self.page();
                    page.save().set_fill(c.pdf());
                    if c.a < 1.0 {
                        page.set_alpha(c.a, 1.0);
                    }
                    page.fill().restore();
                    return;
                }
            }
            "pattFill" => {
                if let Some(c) = fill
                    .child("fgClr")
                    .and_then(|f| self.palette(placeholder).first_color(f))
                {
                    self.page().save().set_fill(c.pdf()).fill().restore();
                    return;
                }
            }
            "gradFill" => {
                let stops: Vec<(f32, Color)> = fill
                    .path(&["gsLst"])
                    .map(|l| {
                        l.children_named("gs")
                            .filter_map(|gs| {
                                Some((
                                    num(gs, "pos").unwrap_or(0.0) / 100_000.0,
                                    self.palette(placeholder).first_color(gs)?,
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if !stops.is_empty() {
                    let angle =
                        fill.child("lin").and_then(|l| num(l, "ang")).unwrap_or(0.0) / 60_000.0;
                    self.gradient(&stops, angle, w, h);
                    return;
                }
            }
            "blipFill" => {
                if let Some(id) = fill
                    .child("blip")
                    .and_then(|b| b.attr("r:embed").or_else(|| b.attr("embed")))
                    && let Some(info) = self.image(part, id)
                {
                    let page = self.page();
                    page.save().clip().image(info.id, 0.0, 0.0, w, h).restore();
                    return;
                }
            }
            _ => {}
        }
        self.page().end_path();
    }

    fn gradient(&mut self, stops: &[(f32, Color)], angle: f32, w: f32, h: f32) {
        let mut stops = stops.to_vec();
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        let at = |t: f32| -> Color {
            let mut prev = stops[0];
            for &(pos, c) in &stops {
                if t <= pos {
                    let span = (pos - prev.0).max(1e-6);
                    let k = ((t - prev.0) / span).clamp(0.0, 1.0);
                    return Color {
                        r: prev.1.r + (c.r - prev.1.r) * k,
                        g: prev.1.g + (c.g - prev.1.g) * k,
                        b: prev.1.b + (c.b - prev.1.b) * k,
                        a: prev.1.a + (c.a - prev.1.a) * k,
                    };
                }
                prev = (pos, c);
            }
            prev.1
        };
        let (s, c) = angle.to_radians().sin_cos();
        let (dx, dy) = (c, -s);
        let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)];
        let project = |(x, y): (f32, f32)| x * dx + y * dy;
        let lo = corners.iter().map(|&p| project(p)).fold(f32::MAX, f32::min);
        let hi = corners.iter().map(|&p| project(p)).fold(f32::MIN, f32::max);
        let big = w + h;
        let page = self.page();
        page.save().clip();
        page.transform([dx, dy, -dy, dx, 0.0, 0.0]);
        let bands = 64;
        for k in 0..bands {
            let t0 = k as f32 / bands as f32;
            let t1 = (k + 1) as f32 / bands as f32;
            let color = at((t0 + t1) / 2.0);
            let x0 = lo + (hi - lo) * t0;
            let x1 = lo + (hi - lo) * t1;
            page.save().set_fill(color.pdf());
            if color.a < 1.0 {
                page.set_alpha(color.a, 1.0);
            }
            page.rect(x0 - 0.2, -big, x1 - x0 + 0.4, 2.0 * big)
                .fill()
                .restore();
        }
        page.restore();
    }

    pub fn tree(
        &mut self,
        tree: &Element,
        part: &str,
        parent: Matrix,
        own: bool,
        host: &dyn Inherit,
    ) {
        for child in tree.elements() {
            self.element(child, part, parent, own, host, None);
        }
    }

    pub fn element(
        &mut self,
        e: &Element,
        part: &str,
        parent: Matrix,
        own: bool,
        host: &dyn Inherit,
        place: Option<Xfrm>,
    ) {
        let hidden = e
            .elements()
            .find(|c| c.local().starts_with("nv"))
            .and_then(|nv| nv.child("cNvPr"))
            .and_then(|c| flag(c, "hidden"))
            .unwrap_or(false);
        if hidden {
            return;
        }
        match e.local() {
            "sp" | "cxnSp" => self.shape(e, part, parent, own, host, place),
            "pic" => self.picture(e, part, parent, place),
            "grpSp" => {
                let x = e.path(&["grpSpPr", "xfrm"]);
                let Some(xfrm) = boxed(x, place) else {
                    self.tree(e, part, parent, own, host);
                    return;
                };
                let x = x.cloned().unwrap_or_default();
                let ch_off = x.child("chOff");
                let ch_ext = x.child("chExt");
                let (cx, cy) = (
                    emu(ch_off.and_then(|o| o.attr("x"))),
                    emu(ch_off.and_then(|o| o.attr("y"))),
                );
                let (cw, chh) = (
                    emu(ch_ext.and_then(|o| o.attr("cx"))),
                    emu(ch_ext.and_then(|o| o.attr("cy"))),
                );
                let sx = if cw > 0.0 { xfrm.w / cw } else { 1.0 };
                let sy = if chh > 0.0 { xfrm.h / chh } else { 1.0 };
                let m = mul(
                    parent,
                    mul(xfrm.matrix(true), mul(scale(sx, sy), translate(-cx, -cy))),
                );
                self.tree(e, part, m, own, host);
            }
            "graphicFrame" => self.frame(e, part, parent, place),
            "AlternateContent" => {
                if let Some(branch) = e.child("Fallback").or_else(|| e.child("Choice")) {
                    self.tree(branch, part, parent, own, host);
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_lines)]
    fn shape(
        &mut self,
        e: &Element,
        part: &str,
        parent: Matrix,
        own: bool,
        host: &dyn Inherit,
        place: Option<Xfrm>,
    ) {
        let nv = e.child("nvSpPr").or_else(|| e.child("nvCxnSpPr"));
        let ph = nv.and_then(|n| n.path(&["nvPr", "ph"]));
        if ph.is_some() && !own {
            return;
        }
        let inherited = ph.map_or([None, None], |p| host.placeholders(p));
        let sp_pr = e.child("spPr");
        let own_xfrm = sp_pr.and_then(|s| s.child("xfrm"));
        let xfrm = match place {
            Some(_) => boxed(own_xfrm, place),
            None => own_xfrm.and_then(Xfrm::read).or_else(|| {
                inherited
                    .iter()
                    .rev()
                    .flatten()
                    .find_map(|s| s.path(&["spPr", "xfrm"]).and_then(Xfrm::read))
            }),
        };
        let Some(xfrm) = xfrm else { return };
        let style = e.child("style");
        let fill = sp_pr.and_then(|s| {
            s.elements()
                .find(|c| {
                    matches!(
                        c.local(),
                        "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill"
                    )
                })
                .cloned()
        });
        let (fill, fill_ph) = match fill {
            Some(f) => (Some(f), None),
            None => style
                .and_then(|s| s.child("fillRef"))
                .map_or((None, None), |r| {
                    let idx = num(r, "idx").unwrap_or(0.0) as usize;
                    let ph = self.palette(None).first_color(r);
                    let f = (idx > 0)
                        .then(|| self.theme.fills.get(idx - 1).cloned())
                        .flatten();
                    (f, ph)
                }),
        };
        let line = sp_pr.and_then(|s| s.child("ln")).cloned();
        let line_ref = style.and_then(|s| s.child("lnRef"));
        let outline = self.outline(line.as_ref(), line_ref);
        let geometry = sp_pr.and_then(|s| s.child("prstGeom").or_else(|| s.child("custGeom")));
        let m = mul(parent, xfrm.matrix(true));
        let (w, h) = (xfrm.w, xfrm.h);
        let line_shape = is_line(geometry) || e.local() == "cxnSp";
        if fill.as_ref().is_some_and(|f| f.local() != "noFill") && !line_shape {
            let fill = fill.clone().unwrap_or_default();
            self.page().save();
            self.apply(m, h);
            path(self.page(), geometry, w, h);
            self.paint_fill(&fill, part, fill_ph, w, h);
            self.page().restore();
        }
        if let Some(color) = outline.color
            && outline.width > 0.0
        {
            let width = outline.width;
            self.page().save();
            self.apply(m, h);
            {
                let page = self.page();
                page.set_stroke(color.pdf()).set_line_width(width);
                if color.a < 1.0 {
                    page.set_alpha(1.0, color.a);
                }
                if !outline.dash.is_empty() {
                    let pattern: Vec<f32> =
                        outline.dash.iter().map(|d| d * width.max(0.5)).collect();
                    page.set_dash(&pattern, 0.0);
                }
            }
            path(self.page(), geometry, w, h);
            self.page().stroke();
            if line_shape {
                let (head, tail) = outline.ends;
                let page = self.page();
                page.set_dash(&[], 0.0).set_fill(color.pdf());
                if tail {
                    arrow(page, (0.0, h), (w, 0.0), width);
                }
                if head {
                    arrow(page, (w, 0.0), (0.0, h), width);
                }
            }
            self.page().restore();
        }
        if let Some(body) = e.child("txBody") {
            let text_m = mul(parent, xfrm.matrix(false));
            let font_color = style
                .and_then(|s| s.child("fontRef"))
                .and_then(|r| self.palette(None).first_color(r));
            self.text(
                body,
                ph,
                &inherited,
                Frame { m: text_m, w, h },
                font_color,
                host,
            );
        }
    }

    #[must_use]
    pub fn outline(&self, line: Option<&Element>, line_ref: Option<&Element>) -> Outline {
        let theme_line = line_ref.and_then(|r| {
            let idx = num(r, "idx").unwrap_or(0.0) as usize;
            (idx > 0).then(|| self.theme.lines.get(idx - 1)).flatten()
        });
        let ref_color = line_ref.and_then(|r| self.palette(None).first_color(r));
        let mut out = Outline {
            width: theme_line
                .and_then(|l| num(l, "w"))
                .map_or(0.75, |w| w / EMU),
            ..Outline::default()
        };
        if line_ref.is_some_and(|r| num(r, "idx").unwrap_or(0.0) > 0.0) {
            out.color = ref_color;
        }
        if let Some(l) = line {
            if let Some(w) = num(l, "w") {
                out.width = w / EMU;
            }
            if l.child("noFill").is_some() {
                out.color = None;
            } else if let Some(fill) = l.child("solidFill") {
                out.color = self.palette(ref_color).first_color(fill);
            } else if let Some(fill) = l.child("gradFill")
                && let Some(gs) = fill.path(&["gsLst", "gs"])
            {
                out.color = self.palette(ref_color).first_color(gs);
            }
            out.dash = dash(l);
            let end = |name: &str| {
                l.child(name)
                    .and_then(|e| e.attr("type"))
                    .is_some_and(|t| t != "none")
            };
            out.ends = (end("headEnd"), end("tailEnd"));
        }
        out
    }

    fn picture(&mut self, e: &Element, part: &str, parent: Matrix, place: Option<Xfrm>) {
        let own_xfrm = e.path(&["spPr", "xfrm"]);
        let xfrm = match place {
            Some(_) => boxed(own_xfrm, place),
            None => own_xfrm.and_then(Xfrm::read),
        };
        let Some(xfrm) = xfrm else { return };
        let Some(blip_fill) = e.child("blipFill") else {
            return;
        };
        let Some(id) = blip_fill
            .child("blip")
            .and_then(|b| b.attr("r:embed").or_else(|| b.attr("embed")))
        else {
            return;
        };
        let Some(info) = self.image(part, id) else {
            return;
        };
        let (w, h) = (xfrm.w, xfrm.h);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let crop = blip_fill.child("srcRect").map_or([0.0; 4], |r| {
            ["l", "t", "r", "b"].map(|k| num(r, k).unwrap_or(0.0) / 100_000.0)
        });
        let fx = (1.0 - crop[0] - crop[2]).max(0.01);
        let fy = (1.0 - crop[1] - crop[3]).max(0.01);
        let (full_w, full_h) = (w / fx, h / fy);
        let geometry = e.path(&["spPr", "prstGeom"]).cloned();
        let m = mul(parent, xfrm.matrix(true));
        self.page().save();
        self.apply(m, h);
        path(self.page(), geometry.as_ref(), w, h);
        let page = self.page();
        page.clip();
        page.image(
            info.id,
            -full_w * crop[0],
            -full_h * crop[3],
            full_w,
            full_h,
        );
        page.restore();
        let line = e.path(&["spPr", "ln"]).cloned();
        let outline = self.outline(line.as_ref(), None);
        if line.is_some()
            && let Some(color) = outline.color
        {
            self.page().save();
            self.apply(m, h);
            self.page()
                .set_stroke(color.pdf())
                .set_line_width(outline.width);
            path(self.page(), geometry.as_ref(), w, h);
            self.page().stroke().restore();
        }
    }

    fn frame(&mut self, e: &Element, part: &str, parent: Matrix, place: Option<Xfrm>) {
        let own_xfrm = e.child("xfrm");
        let xfrm = match place {
            Some(_) => boxed(own_xfrm, place),
            None => own_xfrm.and_then(Xfrm::read),
        };
        let Some(xfrm) = xfrm else { return };
        let Some(data) = e.path(&["graphic", "graphicData"]) else {
            return;
        };
        let frame = Frame {
            m: mul(parent, xfrm.matrix(true)),
            w: xfrm.w,
            h: xfrm.h,
        };
        let uri = data.attr("uri").unwrap_or("");
        if let Some(table) = data.child("tbl") {
            self.table(table, frame.m);
        } else if uri.contains("chartex") {
            let chart = data
                .elements()
                .find_map(|c| c.attr("r:id").or_else(|| c.attr("id")))
                .and_then(|id| self.package.rels(part).into_iter().find(|r| r.id == id))
                .and_then(|r| self.package.xml(&r.target).ok());
            let title = chart
                .as_ref()
                .and_then(|c| c.descendants("title").into_iter().next().map(Element::text))
                .unwrap_or_default();
            self.placeholder_frame(frame, title.trim());
            self.notes.push(
                "an Office 2016 chart (chartex) is shown as a frame with its title".to_owned(),
            );
        } else if uri.ends_with("/chart") || data.child("chart").is_some() {
            let chart = data
                .child("chart")
                .and_then(|c| c.attr("r:id").or_else(|| c.attr("id")))
                .and_then(|id| self.package.rels(part).into_iter().find(|r| r.id == id))
                .filter(|r| !r.external)
                .and_then(|r| self.package.xml(&r.target).ok().map(|x| (x, r.target)));
            match chart {
                Some((root, chart_part)) => self.chart(&root, &chart_part, frame),
                None => self.notes.push("a chart's part is missing".to_owned()),
            }
        } else if uri.contains("diagram") {
            self.notes
                .push("a SmartArt diagram was left out".to_owned());
        }
    }

    pub fn placeholder_frame(&mut self, frame: Frame, title: &str) {
        let (w, h) = (frame.w, frame.h);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.page().save();
        self.apply(frame.m, h);
        let grey = convert_pdf_canvas::Rgb(0.55, 0.55, 0.55);
        self.page()
            .set_fill(convert_pdf_canvas::Rgb::WHITE)
            .set_stroke(grey)
            .set_line_width(0.75)
            .rect(0.0, 0.0, w, h)
            .fill_stroke();
        if !title.is_empty() {
            let family = self.theme.minor.latin.clone();
            let span = convert_pdf_canvas::Span {
                color: convert_pdf_canvas::Rgb(0.25, 0.25, 0.25),
                ..convert_pdf_canvas::Span::new(title, &family, 14.0)
            };
            let spans = crate::text::script_spans(&span, &self.theme.minor.cs);
            let lines = convert_pdf_canvas::layout(
                self.canvas,
                self.fonts,
                &spans,
                Some((w - 14.4).max(1.0)),
            );
            let mut y = h - 7.2;
            for line in &lines {
                y -= line.ascent;
                let page = self.canvas.page(self.page);
                line.draw(page, (w - line.width) / 2.0, y, 0.0);
                y -= line.descent + line.gap;
            }
        }
        self.page().restore();
    }

    #[allow(clippy::too_many_lines)]
    fn table(&mut self, tbl: &Element, m: Matrix) {
        use convert_pdf_canvas::{Direction, layout_directed};
        let direction_of = |tc: &Element| {
            if tc
                .path(&["txBody", "p", "pPr"])
                .and_then(|p| flag(p, "rtl"))
                .unwrap_or(false)
            {
                Direction::Rtl
            } else {
                Direction::Ltr
            }
        };
        let cols: Vec<f32> = tbl
            .path(&["tblGrid"])
            .map(|g| {
                g.children_named("gridCol")
                    .map(|c| emu(c.attr("w")))
                    .collect()
            })
            .unwrap_or_default();
        if cols.is_empty() {
            return;
        }
        let props = tbl.child("tblPr");
        let first_row = props.and_then(|p| flag(p, "firstRow")).unwrap_or(false);
        let band_row = props.and_then(|p| flag(p, "bandRow")).unwrap_or(false);
        let table_rtl = props.and_then(|p| flag(p, "rtl")).unwrap_or(false);
        let styled =
            props.is_some_and(|p| p.child("tableStyleId").is_some()) || first_row || band_row;
        let accent = self
            .theme
            .colors
            .get("accent1")
            .copied()
            .unwrap_or(0x4472C4);
        let rows: Vec<&Element> = tbl.children_named("tr").collect();
        let x_of: Vec<f32> = std::iter::once(0.0)
            .chain(cols.iter().scan(0.0, |acc, w| {
                *acc += w;
                Some(*acc)
            }))
            .collect();
        let mut heights: Vec<f32> = rows.iter().map(|r| emu(r.attr("h"))).collect();
        let cell_margins = |tc: &Element| {
            let pr = tc.child("tcPr");
            let get = |k: &str, d: f32| pr.and_then(|p| p.attr(k)).map_or(d, |v| emu(Some(v)));
            (
                get("marL", 7.2),
                get("marR", 7.2),
                get("marT", 3.6),
                get("marB", 3.6),
            )
        };
        for (ri, row) in rows.iter().enumerate() {
            for (ci, tc) in row.children_named("tc").enumerate() {
                let span = tc
                    .attr("gridSpan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                let row_span = tc
                    .attr("rowSpan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                if tc.attr("hMerge").is_none() && tc.attr("vMerge").is_none() && row_span == 1 {
                    let width = x_of[(ci + span).min(cols.len())] - x_of[ci.min(cols.len())];
                    let (ml, mr, mt, mb) = cell_margins(tc);
                    let spans = self.cell_spans(tc);
                    if !spans.is_empty() {
                        let lines = layout_directed(
                            self.canvas,
                            self.fonts,
                            &spans,
                            Some((width - ml - mr).max(1.0)),
                            direction_of(tc),
                        );
                        let needed: f32 = lines
                            .iter()
                            .map(|l| {
                                l.pieces.iter().map(|p| p.style.size).fold(0.0, f32::max) * 1.2
                            })
                            .sum::<f32>()
                            + mt
                            + mb;
                        heights[ri] = heights[ri].max(needed);
                    }
                }
            }
        }
        let y_of: Vec<f32> = std::iter::once(0.0)
            .chain(heights.iter().scan(0.0, |acc, h| {
                *acc += h;
                Some(*acc)
            }))
            .collect();
        let total_h = *y_of.last().unwrap_or(&0.0);
        self.page().save();
        self.apply(m, total_h);
        let local = |x: f32, y: f32| (x, total_h - y);
        for (ri, row) in rows.iter().enumerate() {
            for (this, tc) in row.children_named("tc").enumerate() {
                if tc.attr("hMerge").is_some() || tc.attr("vMerge").is_some() || this >= cols.len()
                {
                    continue;
                }
                let span = tc
                    .attr("gridSpan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                let row_span = tc
                    .attr("rowSpan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1);
                let (x0, x1) = (x_of[this], x_of[(this + span).min(cols.len())]);
                let (x0, x1) = if table_rtl {
                    let total_w = x_of[cols.len()];
                    (total_w - x1, total_w - x0)
                } else {
                    (x0, x1)
                };
                let (y0, y1) = (y_of[ri], y_of[(ri + row_span).min(rows.len())]);
                let pr = tc.child("tcPr");
                let own_fill = pr.and_then(|p| {
                    p.elements()
                        .find(|c| matches!(c.local(), "solidFill" | "noFill" | "gradFill"))
                });
                let fill = match own_fill {
                    Some(f) if f.local() == "noFill" => None,
                    Some(f) => self.palette(None).first_color(f).or_else(|| {
                        f.path(&["gsLst", "gs"])
                            .and_then(|g| self.palette(None).first_color(g))
                    }),
                    None if styled && first_row && ri == 0 => Some(Color::rgb(accent)),
                    None if styled && band_row => {
                        let band = if first_row { ri % 2 == 1 } else { ri % 2 == 0 };
                        let base = Color::rgb(accent);
                        let t = if band { 0.2 } else { 0.4 };
                        Some(Color {
                            r: base.r + (1.0 - base.r) * (1.0 - t),
                            g: base.g + (1.0 - base.g) * (1.0 - t),
                            b: base.b + (1.0 - base.b) * (1.0 - t),
                            a: 1.0,
                        })
                    }
                    None => None,
                };
                if let Some(f) = fill {
                    let (lx, ly) = local(x0, y1);
                    self.page().fill_rect(lx, ly, x1 - x0, y1 - y0, f.pdf());
                }
                for (name, a, b) in [
                    ("lnL", (x0, y0), (x0, y1)),
                    ("lnR", (x1, y0), (x1, y1)),
                    ("lnT", (x0, y0), (x1, y0)),
                    ("lnB", (x0, y1), (x1, y1)),
                ] {
                    let ln = pr.and_then(|p| p.child(name));
                    let (color, width) = match ln {
                        Some(l) if l.child("noFill").is_some() => (None, 0.0),
                        Some(l) => (
                            l.child("solidFill")
                                .and_then(|f| self.palette(None).first_color(f)),
                            num(l, "w").map_or(0.75, |w| w / EMU),
                        ),
                        None if styled => (Some(Color::rgb(0xFFFFFF)), 1.0),
                        None => (None, 0.0),
                    };
                    if let Some(c) = color {
                        let (ax, ay) = local(a.0, a.1);
                        let (bx, by) = local(b.0, b.1);
                        self.page().stroke_line(ax, ay, bx, by, width, c.pdf());
                    }
                }
                let (ml, mr, mt, mb) = cell_margins(tc);
                let mut spans = self.cell_spans(tc);
                if styled && first_row && ri == 0 && own_fill.is_none() {
                    for s in &mut spans {
                        s.bold = true;
                        if s.color == convert_pdf_canvas::Rgb::BLACK {
                            s.color = convert_pdf_canvas::Rgb::WHITE;
                        }
                    }
                }
                if spans.is_empty() {
                    continue;
                }
                let width = (x1 - x0 - ml - mr).max(1.0);
                let direction = direction_of(tc);
                let lines =
                    layout_directed(self.canvas, self.fonts, &spans, Some(width), direction);
                let heights_l: Vec<f32> = lines
                    .iter()
                    .map(|l| l.pieces.iter().map(|p| p.style.size).fold(0.0, f32::max) * 1.2)
                    .collect();
                let text_h: f32 = heights_l.iter().sum();
                let anchor = pr.and_then(|p| p.attr("anchor")).unwrap_or("t");
                let mut top = match anchor {
                    "ctr" => y0 + (y1 - y0 - text_h) / 2.0,
                    "b" => y1 - mb - text_h,
                    _ => y0 + mt,
                };
                let align = tc
                    .path(&["txBody", "p", "pPr"])
                    .and_then(|p| p.attr("algn"))
                    .unwrap_or(if direction == Direction::Rtl {
                        "r"
                    } else {
                        "l"
                    })
                    .to_owned();
                for (line, lh) in lines.iter().zip(&heights_l) {
                    let size = lh / 1.2;
                    let baseline = top + lh - 0.26 * size;
                    let x = match align.as_str() {
                        "ctr" => x0 + ml + (width - line.width) / 2.0,
                        "r" => x1 - mr - line.width,
                        _ => x0 + ml,
                    };
                    let (lx, ly) = local(x, baseline);
                    line.draw(self.canvas.page(self.page), lx, ly, 0.0);
                    top += lh;
                }
            }
        }
        self.page().restore();
    }
}

fn boxed(own: Option<&Element>, place: Option<Xfrm>) -> Option<Xfrm> {
    match place {
        Some(p) => {
            let turn = Xfrm::turn_of(own);
            Some(Xfrm {
                rot: turn.rot,
                flip_h: turn.flip_h,
                flip_v: turn.flip_v,
                ..p
            })
        }
        None => own.and_then(Xfrm::read),
    }
}

#[must_use]
pub fn dash(l: &Element) -> Vec<f32> {
    match l.child("prstDash").and_then(|d| d.attr("val")) {
        Some("dash" | "sysDash") => vec![4.0, 3.0],
        Some("dot" | "sysDot") => vec![1.0, 2.0],
        Some("lgDash") => vec![8.0, 3.0],
        Some("dashDot" | "sysDashDot") => vec![4.0, 3.0, 1.0, 3.0],
        Some("lgDashDot") => vec![8.0, 3.0, 1.0, 3.0],
        Some("lgDashDotDot" | "sysDashDotDot") => vec![8.0, 3.0, 1.0, 3.0, 1.0, 3.0],
        _ => Vec::new(),
    }
}
