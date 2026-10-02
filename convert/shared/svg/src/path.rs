use crate::values::{Matrix, apply, number_prefix};

pub const MOST_SEGMENTS: usize = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

pub type Path = Vec<Seg>;

struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl Scanner<'_> {
    fn skip(&mut self) {
        let b = self.text.as_bytes();
        while self.at < b.len() && (b[self.at].is_ascii_whitespace() || b[self.at] == b',') {
            self.at += 1;
        }
    }

    fn number(&mut self) -> Option<f32> {
        self.skip();
        let (v, used) = number_prefix(&self.text[self.at..])?;
        self.at += used;
        Some(v)
    }

    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let b = *self.text.as_bytes().get(self.at)?;
        let v = match b {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.at += 1;
        Some(v)
    }

    fn command(&mut self) -> Option<u8> {
        self.skip();
        let b = *self.text.as_bytes().get(self.at)?;
        if b.is_ascii_alphabetic() {
            self.at += 1;
            Some(b)
        } else {
            None
        }
    }

    fn more_numbers(&mut self) -> bool {
        self.skip();
        self.text
            .as_bytes()
            .get(self.at)
            .is_some_and(|b| matches!(b, b'0'..=b'9' | b'.' | b'-' | b'+'))
    }
}

#[must_use]
pub fn parse(d: &str) -> Path {
    let mut out = Vec::new();
    let mut s = Scanner { text: d, at: 0 };
    let (mut cx, mut cy) = (0.0_f32, 0.0_f32);
    let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
    let mut last_cubic: Option<(f32, f32)> = None;
    let mut last_quad: Option<(f32, f32)> = None;
    let mut command = 0_u8;
    let mut started = false;
    loop {
        if out.len() > MOST_SEGMENTS {
            break;
        }
        let next = if s.more_numbers() && command != 0 {
            match command {
                b'M' => b'L',
                b'm' => b'l',
                c => c,
            }
        } else {
            match s.command() {
                Some(c) => c,
                None => break,
            }
        };
        command = next;
        let rel = command.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        let upper = command.to_ascii_uppercase();
        if !started && upper != b'M' {
            break;
        }
        let mut cubic_ctrl = None;
        let mut quad_ctrl = None;
        let ok = (|| -> Option<()> {
            match upper {
                b'M' => {
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    out.push(Seg::Move(x, y));
                    (cx, cy, sx, sy) = (x, y, x, y);
                    started = true;
                }
                b'L' => {
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    out.push(Seg::Line(x, y));
                    (cx, cy) = (x, y);
                }
                b'H' => {
                    let x = s.number()? + ox;
                    out.push(Seg::Line(x, cy));
                    cx = x;
                }
                b'V' => {
                    let y = s.number()? + oy;
                    out.push(Seg::Line(cx, y));
                    cy = y;
                }
                b'C' => {
                    let (x1, y1) = (s.number()? + ox, s.number()? + oy);
                    let (x2, y2) = (s.number()? + ox, s.number()? + oy);
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    out.push(Seg::Cubic(x1, y1, x2, y2, x, y));
                    cubic_ctrl = Some((x2, y2));
                    (cx, cy) = (x, y);
                }
                b'S' => {
                    let (x1, y1) =
                        last_cubic.map_or((cx, cy), |(px, py)| (2.0 * cx - px, 2.0 * cy - py));
                    let (x2, y2) = (s.number()? + ox, s.number()? + oy);
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    out.push(Seg::Cubic(x1, y1, x2, y2, x, y));
                    cubic_ctrl = Some((x2, y2));
                    (cx, cy) = (x, y);
                }
                b'Q' => {
                    let (qx, qy) = (s.number()? + ox, s.number()? + oy);
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    out.push(quad(cx, cy, qx, qy, x, y));
                    quad_ctrl = Some((qx, qy));
                    (cx, cy) = (x, y);
                }
                b'T' => {
                    let (qx, qy) =
                        last_quad.map_or((cx, cy), |(px, py)| (2.0 * cx - px, 2.0 * cy - py));
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    out.push(quad(cx, cy, qx, qy, x, y));
                    quad_ctrl = Some((qx, qy));
                    (cx, cy) = (x, y);
                }
                b'A' => {
                    let (rx, ry, angle) = (s.number()?, s.number()?, s.number()?);
                    let (large, sweep) = (s.flag()?, s.flag()?);
                    let (x, y) = (s.number()? + ox, s.number()? + oy);
                    arc(&mut out, (cx, cy), (rx, ry), angle, large, sweep, (x, y));
                    (cx, cy) = (x, y);
                }
                b'Z' => {
                    out.push(Seg::Close);
                    (cx, cy) = (sx, sy);
                    command = 0;
                }
                _ => return None,
            }
            Some(())
        })();
        if ok.is_none() {
            break;
        }
        last_cubic = cubic_ctrl;
        last_quad = quad_ctrl;
    }
    out
}

fn quad(x0: f32, y0: f32, qx: f32, qy: f32, x: f32, y: f32) -> Seg {
    Seg::Cubic(
        x0 + 2.0 / 3.0 * (qx - x0),
        y0 + 2.0 / 3.0 * (qy - y0),
        x + 2.0 / 3.0 * (qx - x),
        y + 2.0 / 3.0 * (qy - y),
        x,
        y,
    )
}

pub fn arc(
    out: &mut Path,
    from: (f32, f32),
    radii: (f32, f32),
    angle: f32,
    large: bool,
    sweep: bool,
    to: (f32, f32),
) {
    let (x1, y1) = (f64::from(from.0), f64::from(from.1));
    let (x2, y2) = (f64::from(to.0), f64::from(to.1));
    let (mut rx, mut ry) = (f64::from(radii.0).abs(), f64::from(radii.1).abs());
    if (x1 - x2).abs() < 1e-9 && (y1 - y2).abs() < 1e-9 {
        return;
    }
    if rx < 1e-9 || ry < 1e-9 {
        out.push(Seg::Line(to.0, to.1));
        return;
    }
    let phi = f64::from(angle).to_radians();
    let (sin, cos) = phi.sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos * dx + sin * dy;
    let y1p = -sin * dx + cos * dy;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den > 0.0 { (num / den).sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cos * cxp - sin * cyp + (x1 + x2) / 2.0;
    let cy = sin * cxp + cos * cyp + (y1 + y2) / 2.0;
    let angle_of = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let theta1 = angle_of(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle_of(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    if !delta.is_finite() || !theta1.is_finite() {
        out.push(Seg::Line(to.0, to.1));
        return;
    }
    let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f64| {
        let (s, c) = t.sin_cos();
        (
            cx + rx * c * cos - ry * s * sin,
            cy + rx * c * sin + ry * s * cos,
        )
    };
    let derivative = |t: f64| {
        let (s, c) = t.sin_cos();
        (-rx * s * cos - ry * c * sin, -rx * s * sin + ry * c * cos)
    };
    let mut t = theta1;
    for i in 0..pieces {
        let t2 = t + step;
        let (p0, d0) = (point(t), derivative(t));
        let (p3, d3) = (point(t2), derivative(t2));
        let end = if i + 1 == pieces { (x2, y2) } else { p3 };
        out.push(Seg::Cubic(
            (p0.0 + k * d0.0) as f32,
            (p0.1 + k * d0.1) as f32,
            (p3.0 - k * d3.0) as f32,
            (p3.1 - k * d3.1) as f32,
            end.0 as f32,
            end.1 as f32,
        ));
        t = t2;
    }
}

#[must_use]
pub fn rect(x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) -> Path {
    let rx = rx.clamp(0.0, w / 2.0);
    let ry = ry.clamp(0.0, h / 2.0);
    if rx <= 0.0 || ry <= 0.0 {
        return vec![
            Seg::Move(x, y),
            Seg::Line(x + w, y),
            Seg::Line(x + w, y + h),
            Seg::Line(x, y + h),
            Seg::Close,
        ];
    }
    let mut out = vec![Seg::Move(x + rx, y), Seg::Line(x + w - rx, y)];
    arc(
        &mut out,
        (x + w - rx, y),
        (rx, ry),
        0.0,
        false,
        true,
        (x + w, y + ry),
    );
    out.push(Seg::Line(x + w, y + h - ry));
    arc(
        &mut out,
        (x + w, y + h - ry),
        (rx, ry),
        0.0,
        false,
        true,
        (x + w - rx, y + h),
    );
    out.push(Seg::Line(x + rx, y + h));
    arc(
        &mut out,
        (x + rx, y + h),
        (rx, ry),
        0.0,
        false,
        true,
        (x, y + h - ry),
    );
    out.push(Seg::Line(x, y + ry));
    arc(
        &mut out,
        (x, y + ry),
        (rx, ry),
        0.0,
        false,
        true,
        (x + rx, y),
    );
    out.push(Seg::Close);
    out
}

#[must_use]
pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Path {
    const K: f32 = 0.552_284_8;
    vec![
        Seg::Move(cx + rx, cy),
        Seg::Cubic(cx + rx, cy + K * ry, cx + K * rx, cy + ry, cx, cy + ry),
        Seg::Cubic(cx - K * rx, cy + ry, cx - rx, cy + K * ry, cx - rx, cy),
        Seg::Cubic(cx - rx, cy - K * ry, cx - K * rx, cy - ry, cx, cy - ry),
        Seg::Cubic(cx + K * rx, cy - ry, cx + rx, cy - K * ry, cx + rx, cy),
        Seg::Close,
    ]
}

#[must_use]
pub fn poly(points: &[f32], closed: bool) -> Path {
    let mut out = Vec::with_capacity(points.len() / 2 + 1);
    for (i, p) in points.as_chunks::<2>().0.iter().enumerate() {
        out.push(if i == 0 {
            Seg::Move(p[0], p[1])
        } else {
            Seg::Line(p[0], p[1])
        });
    }
    if closed && !out.is_empty() {
        out.push(Seg::Close);
    }
    out
}

#[must_use]
pub fn transformed(path: &Path, m: Matrix) -> Path {
    path.iter()
        .map(|s| match *s {
            Seg::Move(x, y) => {
                let (x, y) = apply(m, x, y);
                Seg::Move(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = apply(m, x, y);
                Seg::Line(x, y)
            }
            Seg::Cubic(a, b, c, d, e, f) => {
                let (a, b) = apply(m, a, b);
                let (c, d) = apply(m, c, d);
                let (e, f) = apply(m, e, f);
                Seg::Cubic(a, b, c, d, e, f)
            }
            Seg::Close => Seg::Close,
        })
        .collect()
}

#[must_use]
pub fn bounds(path: &Path) -> Option<[f32; 4]> {
    let mut b = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    let mut add = |x: f32, y: f32| {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    };
    let (mut cx, mut cy) = (0.0, 0.0);
    for s in path {
        match *s {
            Seg::Move(x, y) | Seg::Line(x, y) => {
                add(x, y);
                (cx, cy) = (x, y);
            }
            Seg::Cubic(x1, y1, x2, y2, x, y) => {
                add(x, y);
                for t in extremes(cx, x1, x2, x)
                    .into_iter()
                    .chain(extremes(cy, y1, y2, y))
                {
                    let at = |p0: f32, p1: f32, p2: f32, p3: f32| {
                        let u = 1.0 - t;
                        u * u * u * p0
                            + 3.0 * u * u * t * p1
                            + 3.0 * u * t * t * p2
                            + t * t * t * p3
                    };
                    add(at(cx, x1, x2, x), at(cy, y1, y2, y));
                }
                (cx, cy) = (x, y);
            }
            Seg::Close => {}
        }
    }
    (b[0] <= b[2] && b.iter().all(|v| v.is_finite())).then_some(b)
}

fn extremes(p0: f32, p1: f32, p2: f32, p3: f32) -> Vec<f32> {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let mut out = Vec::new();
    if a.abs() < 1e-9 {
        if b.abs() > 1e-9 {
            out.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let r = disc.sqrt();
            out.push((-b + r) / (2.0 * a));
            out.push((-b - r) / (2.0 * a));
        }
    }
    out.retain(|t| (0.0..=1.0).contains(t));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_reads() {
        let p = parse("M10 10h10v10H10zm5 5l1 1c1 1 2 2 3 3s4 4 5 5q1 1 2 2t3 3a5 5 0 1 0 10 0L");
        assert!(matches!(p[0], Seg::Move(10.0, 10.0)));
        assert!(matches!(p[4], Seg::Close));
        assert!(matches!(p[5], Seg::Move(15.0, 15.0)));
        assert!(p.iter().filter(|s| matches!(s, Seg::Cubic(..))).count() >= 6);
        assert!(matches!(p.last(), Some(Seg::Cubic(..))));
        let flags = parse("M0 0a1 1 0 00.5.5");
        assert!(flags.len() >= 2, "{flags:?}");
        assert!(parse("L 1 2").is_empty());
    }

    #[test]
    fn a_half_circle_arc_ends_where_asked_and_bulges() {
        let mut out = Vec::new();
        arc(
            &mut out,
            (0.0, 0.0),
            (5.0, 5.0),
            0.0,
            false,
            true,
            (10.0, 0.0),
        );
        let Some(Seg::Cubic(.., x, y)) = out.last() else {
            panic!()
        };
        assert_eq!((*x, *y), (10.0, 0.0));
        let mut all = vec![Seg::Move(0.0, 0.0)];
        all.extend(out);
        let b = bounds(&all).unwrap();
        assert!((b[3] - b[1] - 5.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn garbage_never_panics() {
        for d in [
            "",
            "M",
            "M1",
            "Mx",
            "M1 2 A",
            "M0 0 A 0 0 0 1 1 0 0",
            "M0 0a1e40 1e40 0 1 1 5 5",
            "z",
            "M0,0 T 1 1 S 1 1 1 1",
        ] {
            let p = parse(d);
            let _ = bounds(&p);
        }
    }
}
