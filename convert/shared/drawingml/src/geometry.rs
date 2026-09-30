use convert_office_read::Element;
use convert_pdf_canvas::Page;

pub const EMU: f32 = 12_700.0;

pub type Matrix = [f32; 6];

pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[must_use]
pub fn mul(a: Matrix, b: Matrix) -> Matrix {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

#[must_use]
pub fn translate(x: f32, y: f32) -> Matrix {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

#[must_use]
pub fn scale(x: f32, y: f32) -> Matrix {
    [x, 0.0, 0.0, y, 0.0, 0.0]
}

#[must_use]
pub fn rotate(degrees: f32) -> Matrix {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, s, -s, c, 0.0, 0.0]
}

#[must_use]
pub fn emu(value: Option<&str>) -> f32 {
    value.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) as f32 / EMU
}

#[must_use]
pub fn num(e: &Element, key: &str) -> Option<f32> {
    e.attr(key).and_then(|v| v.parse::<f32>().ok())
}

#[must_use]
pub fn flag(e: &Element, key: &str) -> Option<bool> {
    e.attr(key).map(|v| v == "1" || v == "true")
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xfrm {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rot: f32,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Xfrm {
    #[must_use]
    pub fn read(e: &Element) -> Option<Self> {
        let off = e.child("off")?;
        let ext = e.child("ext")?;
        Some(Self {
            x: emu(off.attr("x")),
            y: emu(off.attr("y")),
            w: emu(ext.attr("cx")),
            h: emu(ext.attr("cy")),
            rot: num(e, "rot").unwrap_or(0.0) / 60_000.0,
            flip_h: flag(e, "flipH").unwrap_or(false),
            flip_v: flag(e, "flipV").unwrap_or(false),
        })
    }

    #[must_use]
    pub fn turn_of(e: Option<&Element>) -> Self {
        e.map_or_else(Self::default, |e| Self {
            rot: num(e, "rot").unwrap_or(0.0) / 60_000.0,
            flip_h: flag(e, "flipH").unwrap_or(false),
            flip_v: flag(e, "flipV").unwrap_or(false),
            ..Self::default()
        })
    }

    #[must_use]
    pub fn matrix(&self, flips: bool) -> Matrix {
        let (sx, sy) = if flips {
            (
                if self.flip_h { -1.0 } else { 1.0 },
                if self.flip_v { -1.0 } else { 1.0 },
            )
        } else {
            (1.0, 1.0)
        };
        let turn = if !flips && self.flip_v {
            self.rot + 180.0
        } else {
            self.rot
        };
        mul(
            translate(self.x + self.w / 2.0, self.y + self.h / 2.0),
            mul(
                rotate(turn),
                mul(scale(sx, sy), translate(-self.w / 2.0, -self.h / 2.0)),
            ),
        )
    }
}

#[must_use]
pub fn is_line(geometry: Option<&Element>) -> bool {
    geometry.is_some_and(|g| {
        g.attr("prst")
            .is_some_and(|p| p.contains("line") || p.contains("Connector"))
    })
}

pub fn path(page: &mut Page, geometry: Option<&Element>, w: f32, h: f32) {
    let Some(g) = geometry else {
        page.rect(0.0, 0.0, w, h);
        return;
    };
    if g.local() == "custGeom" {
        custom(page, g, w, h);
        return;
    }
    let adj = |name: &str, default: f32| {
        g.path(&["avLst"])
            .and_then(|l| {
                l.children_named("gd")
                    .find(|gd| gd.attr("name") == Some(name))
            })
            .and_then(|gd| gd.attr("fmla"))
            .and_then(|f| {
                f.strip_prefix("val ")
                    .and_then(|v| v.trim().parse::<f32>().ok())
            })
            .unwrap_or(default)
            / 100_000.0
    };
    let m = w.min(h);
    let poly = |page: &mut Page, pts: &[(f32, f32)]| {
        for (k, &(x, y)) in pts.iter().enumerate() {
            if k == 0 {
                page.move_to(x, h - y);
            } else {
                page.line_to(x, h - y);
            }
        }
        page.close();
    };
    match g.attr("prst").unwrap_or("rect") {
        "ellipse" | "flowChartConnector" => {
            page.ellipse(0.0, 0.0, w, h);
        }
        "roundRect" | "flowChartAlternateProcess" => {
            let r = (m * adj("adj", 16_667.0)).min(m / 2.0);
            round_rect(page, w, h, r);
        }
        "line" | "straightConnector1" | "bentConnector2" | "bentConnector3"
        | "curvedConnector3" => {
            page.move_to(0.0, h).line_to(w, 0.0);
        }
        "triangle" | "flowChartExtract" => {
            poly(page, &[(w * adj("adj", 50_000.0), 0.0), (w, h), (0.0, h)]);
        }
        "rtTriangle" => poly(page, &[(0.0, 0.0), (w, h), (0.0, h)]),
        "diamond" | "flowChartDecision" => poly(
            page,
            &[(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)],
        ),
        "parallelogram" | "flowChartInputOutput" => {
            let a = m * adj("adj", 25_000.0);
            poly(page, &[(a, 0.0), (w, 0.0), (w - a, h), (0.0, h)]);
        }
        "trapezoid" => {
            let a = m * adj("adj", 25_000.0);
            poly(page, &[(a, 0.0), (w - a, 0.0), (w, h), (0.0, h)]);
        }
        "pentagon" | "homePlate" => {
            let a = m * adj("adj", 50_000.0);
            poly(
                page,
                &[(0.0, 0.0), (w - a, 0.0), (w, h / 2.0), (w - a, h), (0.0, h)],
            );
        }
        "chevron" => {
            let a = m * adj("adj", 50_000.0);
            poly(
                page,
                &[
                    (0.0, 0.0),
                    (w - a, 0.0),
                    (w, h / 2.0),
                    (w - a, h),
                    (0.0, h),
                    (a, h / 2.0),
                ],
            );
        }
        "hexagon" => {
            let a = m * adj("adj", 25_000.0);
            poly(
                page,
                &[
                    (a, 0.0),
                    (w - a, 0.0),
                    (w, h / 2.0),
                    (w - a, h),
                    (a, h),
                    (0.0, h / 2.0),
                ],
            );
        }
        "octagon" => {
            let a = m * adj("adj", 29_289.0);
            poly(
                page,
                &[
                    (a, 0.0),
                    (w - a, 0.0),
                    (w, a),
                    (w, h - a),
                    (w - a, h),
                    (a, h),
                    (0.0, h - a),
                    (0.0, a),
                ],
            );
        }
        "rightArrow" => {
            let (a1, a2) = (adj("adj1", 50_000.0), m * adj("adj2", 50_000.0));
            let (t, b) = (h * (1.0 - a1) / 2.0, h * (1.0 + a1) / 2.0);
            poly(
                page,
                &[
                    (0.0, t),
                    (w - a2, t),
                    (w - a2, 0.0),
                    (w, h / 2.0),
                    (w - a2, h),
                    (w - a2, b),
                    (0.0, b),
                ],
            );
        }
        "leftArrow" => {
            let (a1, a2) = (adj("adj1", 50_000.0), m * adj("adj2", 50_000.0));
            let (t, b) = (h * (1.0 - a1) / 2.0, h * (1.0 + a1) / 2.0);
            poly(
                page,
                &[
                    (w, t),
                    (a2, t),
                    (a2, 0.0),
                    (0.0, h / 2.0),
                    (a2, h),
                    (a2, b),
                    (w, b),
                ],
            );
        }
        "upArrow" => {
            let (a1, a2) = (adj("adj1", 50_000.0), m * adj("adj2", 50_000.0));
            let (l, r) = (w * (1.0 - a1) / 2.0, w * (1.0 + a1) / 2.0);
            poly(
                page,
                &[
                    (l, h),
                    (l, a2),
                    (0.0, a2),
                    (w / 2.0, 0.0),
                    (w, a2),
                    (r, a2),
                    (r, h),
                ],
            );
        }
        "downArrow" => {
            let (a1, a2) = (adj("adj1", 50_000.0), m * adj("adj2", 50_000.0));
            let (l, r) = (w * (1.0 - a1) / 2.0, w * (1.0 + a1) / 2.0);
            poly(
                page,
                &[
                    (l, 0.0),
                    (l, h - a2),
                    (0.0, h - a2),
                    (w / 2.0, h),
                    (w, h - a2),
                    (r, h - a2),
                    (r, 0.0),
                ],
            );
        }
        "plus" | "mathPlus" => {
            let a = m * adj("adj", 25_000.0);
            poly(
                page,
                &[
                    (a, 0.0),
                    (w - a, 0.0),
                    (w - a, a),
                    (w, a),
                    (w, h - a),
                    (w - a, h - a),
                    (w - a, h),
                    (a, h),
                    (a, h - a),
                    (0.0, h - a),
                    (0.0, a),
                    (a, a),
                ],
            );
        }
        "star5" | "star4" | "star6" | "star8" => {
            let points: usize = match g.attr("prst") {
                Some("star4") => 4,
                Some("star6") => 6,
                Some("star8") => 8,
                _ => 5,
            };
            let inner = adj("adj", if points == 5 { 19_098.0 } else { 25_000.0 }) * 2.0;
            let mut pts = Vec::new();
            for k in 0..points * 2 {
                let r = if k % 2 == 0 { 1.0 } else { inner.max(0.2) };
                let a = (-90.0 + k as f32 * 180.0 / points as f32).to_radians();
                pts.push((
                    w / 2.0 + a.cos() * w / 2.0 * r,
                    h / 2.0 + a.sin() * h / 2.0 * r,
                ));
            }
            poly(page, &pts);
        }
        _ => {
            page.rect(0.0, 0.0, w, h);
        }
    }
}

pub fn round_rect(page: &mut Page, w: f32, h: f32, r: f32) {
    const K: f32 = 0.552_284_8;
    page.move_to(r, 0.0)
        .line_to(w - r, 0.0)
        .curve_to(w - r + K * r, 0.0, w, r - K * r, w, r)
        .line_to(w, h - r)
        .curve_to(w, h - r + K * r, w - r + K * r, h, w - r, h)
        .line_to(r, h)
        .curve_to(r - K * r, h, 0.0, h - r + K * r, 0.0, h - r)
        .line_to(0.0, r)
        .curve_to(0.0, r - K * r, r - K * r, 0.0, r, 0.0)
        .close();
}

pub fn arrow(page: &mut Page, from: (f32, f32), to: (f32, f32), width: f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / len, dy / len);
    let size = (width * 3.0).max(4.0);
    let base = (to.0 - ux * size, to.1 - uy * size);
    let (px, py) = (-uy * size * 0.5, ux * size * 0.5);
    page.move_to(to.0, to.1)
        .line_to(base.0 + px, base.1 + py)
        .line_to(base.0 - px, base.1 - py)
        .close()
        .fill();
}

fn custom(page: &mut Page, g: &Element, w: f32, h: f32) {
    let Some(list) = g.child("pathLst") else {
        page.rect(0.0, 0.0, w, h);
        return;
    };
    for path in list.children_named("path") {
        let pw = num(path, "w").unwrap_or(w * EMU).max(1.0);
        let ph = num(path, "h").unwrap_or(h * EMU).max(1.0);
        let pt = |e: &Element| -> (f32, f32) {
            let x = num(e, "x").unwrap_or(0.0) * w / pw;
            let y = num(e, "y").unwrap_or(0.0) * h / ph;
            (x, h - y)
        };
        let mut current = (0.0, h);
        for cmd in path.elements() {
            let pts: Vec<(f32, f32)> = cmd.children_named("pt").map(pt).collect();
            match cmd.local() {
                "moveTo" if !pts.is_empty() => {
                    page.move_to(pts[0].0, pts[0].1);
                    current = pts[0];
                }
                "lnTo" if !pts.is_empty() => {
                    page.line_to(pts[0].0, pts[0].1);
                    current = pts[0];
                }
                "cubicBezTo" if pts.len() == 3 => {
                    page.curve_to(pts[0].0, pts[0].1, pts[1].0, pts[1].1, pts[2].0, pts[2].1);
                    current = pts[2];
                }
                "quadBezTo" if pts.len() == 2 => {
                    let c1 = (
                        current.0 + 2.0 / 3.0 * (pts[0].0 - current.0),
                        current.1 + 2.0 / 3.0 * (pts[0].1 - current.1),
                    );
                    let c2 = (
                        pts[1].0 + 2.0 / 3.0 * (pts[0].0 - pts[1].0),
                        pts[1].1 + 2.0 / 3.0 * (pts[0].1 - pts[1].1),
                    );
                    page.curve_to(c1.0, c1.1, c2.0, c2.1, pts[1].0, pts[1].1);
                    current = pts[1];
                }
                "arcTo" => {
                    let wr = num(cmd, "wR").unwrap_or(0.0) * w / pw;
                    let hr = num(cmd, "hR").unwrap_or(0.0) * h / ph;
                    let start = num(cmd, "stAng").unwrap_or(0.0) / 60_000.0;
                    let sweep = num(cmd, "swAng").unwrap_or(0.0) / 60_000.0;
                    let (s0, c0) = (-start).to_radians().sin_cos();
                    let center = (current.0 - wr * c0, current.1 - hr * s0);
                    let steps = ((sweep.abs() / 15.0).ceil() as usize).max(1);
                    for k in 1..=steps {
                        let a = (-(start + sweep * k as f32 / steps as f32)).to_radians();
                        let p = (center.0 + wr * a.cos(), center.1 + hr * a.sin());
                        page.line_to(p.0, p.1);
                        current = p;
                    }
                }
                "close" => {
                    page.close();
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrices() {
        let m = mul(translate(10.0, 0.0), rotate(90.0));
        assert!((m[1] - 1.0).abs() < 1e-5 && (m[4] - 10.0).abs() < 1e-5);
        let x = Xfrm {
            x: 10.0,
            y: 20.0,
            w: 30.0,
            h: 40.0,
            ..Xfrm::default()
        };
        assert_eq!(x.matrix(true), translate(10.0, 20.0));
    }
}
