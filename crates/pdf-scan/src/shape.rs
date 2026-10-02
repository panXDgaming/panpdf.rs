use crate::Corners;

const PHONE_FOCAL: f64 = 0.8;

const FOCAL_TRUSTED: std::ops::RangeInclusive<f64> = 0.6..=1.1;

#[must_use]
pub fn aspect_ratio(corners: &Corners, picture: (usize, usize)) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    let (u0, v0) = (picture.0 as f64 / 2.0, picture.1 as f64 / 2.0);
    let diagonal = (2.0 * u0).hypot(2.0 * v0);
    let point = |(x, y): (f32, f32)| [f64::from(x) - u0, f64::from(y) - v0, 1.0];
    let [tl, tr, br, bl] = corners.map(point);
    let (m1, m2, m3, m4) = (tl, tr, bl, br);
    let k2 = dot(cross(m1, m4), m3) / dot(cross(m2, m4), m3);
    let k3 = dot(cross(m1, m4), m2) / dot(cross(m3, m4), m2);
    if !k2.is_finite() || !k3.is_finite() {
        return None;
    }
    let n2 = [k2 * m2[0] - m1[0], k2 * m2[1] - m1[1], k2 * m2[2] - m1[2]];
    let n3 = [k3 * m3[0] - m1[0], k3 * m3[1] - m1[1], k3 * m3[2] - m1[2]];
    let both = n2[2] * n3[2];
    let own = (both.abs() > 1e-12)
        .then(|| -n2[0].mul_add(n3[0], n2[1] * n3[1]) / both)
        .filter(|f2| *f2 > 0.0 && FOCAL_TRUSTED.contains(&(f2.sqrt() / diagonal)));
    let f2 = own.unwrap_or((PHONE_FOCAL * diagonal).powi(2));
    let length = |n: [f64; 3]| n[0].mul_add(n[0], n[1] * n[1]) / f2 + n[2] * n[2];
    let ratio = (length(n2) / length(n3)).sqrt();
    (ratio.is_finite() && ratio > 0.05 && ratio < 20.0).then_some(ratio)
}

#[must_use]
pub fn true_size(corners: &Corners, picture: (usize, usize)) -> (usize, usize) {
    let measured = crate::flattened_size(corners);
    let Some(ratio) = aspect_ratio(corners, picture) else {
        return measured;
    };
    #[allow(clippy::cast_precision_loss)]
    let area = measured.0 as f64 * measured.1 as f64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (
        (area * ratio).sqrt().round().max(1.0) as usize,
        (area / ratio).sqrt().round().max(1.0) as usize,
    )
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1].mul_add(b[2], -(a[2] * b[1])),
        a[2].mul_add(b[0], -(a[0] * b[2])),
        a[0].mul_add(b[1], -(a[1] * b[0])),
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0].mul_add(b[0], a[1].mul_add(b[1], a[2] * b[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photographed(f: f64, turn: [f64; 3], far: f64) -> Corners {
        let (sx, cx) = turn[0].sin_cos();
        let (sy, cy) = turn[1].sin_cos();
        let (sz, cz) = turn[2].sin_cos();
        let spin = |p: [f64; 3]| {
            let p = [p[0], cx * p[1] - sx * p[2], sx * p[1] + cx * p[2]];
            let p = [cy * p[0] + sy * p[2], p[1], -sy * p[0] + cy * p[2]];
            [cz * p[0] - sz * p[1], sz * p[0] + cz * p[1], p[2]]
        };
        [(0.0, 0.0), (210.0, 0.0), (210.0, 297.0), (0.0, 297.0)].map(|(x, y)| {
            let p = spin([x - 105.0, y - 148.5, 0.0]);
            let z = p[2] + far;
            #[allow(clippy::cast_possible_truncation)]
            (
                (f * p[0] / z + 2000.0) as f32,
                (f * p[1] / z + 1500.0) as f32,
            )
        })
    }

    #[test]
    fn a_slanted_a4_page_comes_out_a4() {
        let a4 = 210.0 / 297.0;
        for turn in [[0.5, 0.2, 0.1], [0.3, -0.4, 0.3], [0.7, 0.1, -0.2]] {
            let corners = photographed(4000.0, turn, 600.0);
            let ratio = aspect_ratio(&corners, (4000, 3000)).expect("a ratio");
            assert!((ratio / a4 - 1.0).abs() < 0.01, "{turn:?}: {ratio} vs {a4}");
            let (w, h) = crate::flattened_size(&corners);
            #[allow(clippy::cast_precision_loss)]
            let measured = w as f64 / h as f64;
            assert!(
                (measured / a4 - 1.0).abs() > 0.03,
                "{turn:?}: measured {measured}"
            );
        }
    }

    #[test]
    fn a_page_seen_square_on_keeps_its_shape() {
        let corners = photographed(4000.0, [0.0, 0.0, 0.2], 600.0);
        let ratio = aspect_ratio(&corners, (4000, 3000)).expect("a ratio");
        assert!((ratio / (210.0 / 297.0) - 1.0).abs() < 0.01, "{ratio}");
    }

    #[test]
    fn the_true_size_keeps_the_measured_pixels() {
        let corners = photographed(4000.0, [0.6, 0.0, 0.0], 600.0);
        let (w, h) = true_size(&corners, (4000, 3000));
        let (mw, mh) = crate::flattened_size(&corners);
        #[allow(clippy::cast_precision_loss)]
        let (area, measured) = ((w * h) as f64, (mw * mh) as f64);
        assert!((area / measured - 1.0).abs() < 0.01);
        #[allow(clippy::cast_precision_loss)]
        let ratio = w as f64 / h as f64;
        assert!((ratio / (210.0 / 297.0) - 1.0).abs() < 0.01, "{w} x {h}");
    }
}
