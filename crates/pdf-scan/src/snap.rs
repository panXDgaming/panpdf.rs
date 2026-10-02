use crate::{Corners, Grey};

#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn snap_to_edges(picture: &Grey, corners: &Corners) -> Corners {
    let (w, h) = (picture.width, picture.height);
    if w < 16 || h < 16 {
        return *corners;
    }
    #[allow(clippy::cast_precision_loss)]
    let reach = ((w as f64).hypot(h as f64) * 0.012).max(4.0);
    let sample = |x: f64, y: f64| -> Option<f64> {
        #[allow(clippy::cast_precision_loss)]
        if x < 0.0 || y < 0.0 || x >= (w - 1) as f64 || y >= (h - 1) as f64 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x0, y0) = (x as usize, y as usize);
        let (fx, fy) = (x.fract(), y.fract());
        let v = |x: usize, y: usize| f64::from(picture.pixels[y * w + x]);
        let top = v(x0, y0) * (1.0 - fx) + v(x0 + 1, y0) * fx;
        let low = v(x0, y0 + 1) * (1.0 - fx) + v(x0 + 1, y0 + 1) * fx;
        Some(top * (1.0 - fy) + low * fy)
    };
    let points = corners.map(|(x, y)| (f64::from(x), f64::from(y)));
    let centre = points
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0 / 4.0, y + p.1 / 4.0));
    let mut lines: [Option<Line>; 4] = [None; 4];
    for (side, line) in lines.iter_mut().enumerate() {
        let (a, b) = (points[side], points[(side + 1) % 4]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        if length < 8.0 {
            continue;
        }
        let (mut nx, mut ny) = (dy / length, -dx / length);
        let (mx, my) = (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1));
        if (mx + nx - centre.0).hypot(my + ny - centre.1) < (mx - centre.0).hypot(my - centre.1) {
            (nx, ny) = (-nx, -ny);
        }
        let mut found = Vec::new();
        for step in 2..=38 {
            let t = f64::from(step) / 40.0;
            let (px, py) = (a.0 + t * dx, a.1 + t * dy);
            #[allow(clippy::cast_possible_truncation)]
            let tries = (reach * 2.0) as i32;
            let rises: Vec<(f64, f64)> = (-tries..=tries)
                .filter_map(|k| {
                    let d = f64::from(k) / 2.0;
                    let (qx, qy) = (px + d * nx, py + d * ny);
                    let inner = sample(qx - 1.5 * nx, qy - 1.5 * ny)?;
                    let outer = sample(qx + 1.5 * nx, qy + 1.5 * ny)?;
                    Some((inner - outer, d))
                })
                .collect();
            let steepest = rises.iter().map(|r| r.0).fold(0.0, f64::max);
            let outermost = rises
                .iter()
                .filter(|r| r.0 > 12.0 && r.0 >= 0.6 * steepest)
                .map(|r| r.1)
                .fold(f64::NEG_INFINITY, f64::max);
            if outermost.is_finite() {
                found.push((px + outermost * nx, py + outermost * ny));
            }
        }
        if found.len() < 12 {
            continue;
        }
        let Some(first) = fit(&found) else {
            continue;
        };
        let near: Vec<(f64, f64)> = found
            .iter()
            .copied()
            .filter(|&p| off_line(p, first) < 2.0_f64.max(reach / 6.0))
            .collect();
        if near.len() >= 10 {
            *line = fit(&near);
        }
    }
    let mut out = *corners;
    for (corner, place) in out.iter_mut().enumerate() {
        let (before, after) = ((corner + 3) % 4, corner);
        if let (Some(a), Some(b)) = (lines[before], lines[after])
            && let Some(point) = cross(a, b)
            && (point.0 - points[corner].0).hypot(point.1 - points[corner].1) < reach * 1.5
        {
            #[allow(clippy::cast_possible_truncation)]
            {
                *place = (point.0 as f32, point.1 as f32);
            }
        }
    }
    out
}

type Line = ((f64, f64), (f64, f64));

fn fit(points: &[(f64, f64)]) -> Option<Line> {
    if points.len() < 2 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let n = points.len() as f64;
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0 / n, y + p.1 / n));
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for p in points {
        let (dx, dy) = (p.0 - mx, p.1 - my);
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    Some(((mx, my), (angle.cos(), angle.sin())))
}

fn off_line(p: (f64, f64), ((x, y), (dx, dy)): Line) -> f64 {
    ((p.0 - x) * dy - (p.1 - y) * dx).abs()
}

#[allow(clippy::many_single_char_names)]
fn cross(a: Line, b: Line) -> Option<(f64, f64)> {
    let ((p, d), (q, e)) = (a, b);
    let denominator = d.0 * e.1 - d.1 * e.0;
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / denominator;
    Some((p.0 + t * d.0, p.1 + t * d.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::many_single_char_names)]
    fn page(corners: Corners, radius: usize) -> Grey {
        let (w, h) = (900, 1200);
        let mut pixels = vec![50_u8; w * h];
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let inside = (0..4).all(|i| {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.0
                });
                if inside {
                    pixels[y * w + x] = if y % 30 < 6 && x % 11 > 2 { 40 } else { 230 };
                }
            }
        }
        let grey = Grey {
            width: w,
            height: h,
            pixels,
        };
        if radius == 0 {
            grey
        } else {
            crate::find::blur(&grey, radius)
        }
    }

    const SHEET: Corners = [
        (150.0, 120.0),
        (760.0, 160.0),
        (720.0, 1080.0),
        (110.0, 1040.0),
    ];

    #[test]
    fn corners_found_a_little_off_are_pulled_onto_the_edges() {
        let picture = page(SHEET, 1);
        let off: Corners = [
            (158.0, 128.0),
            (752.0, 150.0),
            (728.0, 1072.0),
            (104.0, 1048.0),
        ];
        let snapped = snap_to_edges(&picture, &off);
        for (got, want) in snapped.iter().zip(SHEET) {
            let miss = (got.0 - want.0).hypot(got.1 - want.1);
            assert!(miss < 3.5, "{got:?} vs {want:?}: {miss}");
        }
    }

    #[test]
    fn a_side_with_no_edge_keeps_its_corners() {
        let flat = Grey {
            width: 400,
            height: 400,
            pixels: vec![200; 160_000],
        };
        let corners: Corners = [(50.0, 50.0), (350.0, 50.0), (350.0, 350.0), (50.0, 350.0)];
        assert_eq!(snap_to_edges(&flat, &corners), corners);
    }
}
