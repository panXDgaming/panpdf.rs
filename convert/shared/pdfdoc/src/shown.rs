#[must_use]
pub fn shown_to_user(crop: [f64; 4], rotate: i64) -> [f64; 6] {
    let [x0, y0, x1, y1] = [
        crop[0].min(crop[2]),
        crop[1].min(crop[3]),
        crop[0].max(crop[2]),
        crop[1].max(crop[3]),
    ];
    match rotate.rem_euclid(360) {
        90 => [0.0, 1.0, -1.0, 0.0, x1, y0],
        180 => [-1.0, 0.0, 0.0, -1.0, x1, y1],
        270 => [0.0, -1.0, 1.0, 0.0, x0, y1],
        _ => [1.0, 0.0, 0.0, 1.0, x0, y0],
    }
}

#[must_use]
pub fn shown_size(crop: [f64; 4], rotate: i64) -> (f64, f64) {
    let w = (crop[2] - crop[0]).abs();
    let h = (crop[3] - crop[1]).abs();
    if rotate.rem_euclid(180) == 90 {
        (h, w)
    } else {
        (w, h)
    }
}

#[must_use]
pub fn apply(m: [f64; 6], (x, y): (f64, f64)) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

#[must_use]
pub fn compose(first: [f64; 6], then: [f64; 6]) -> [f64; 6] {
    [
        first[0] * then[0] + first[1] * then[2],
        first[0] * then[1] + first[1] * then[3],
        first[2] * then[0] + first[3] * then[2],
        first[2] * then[1] + first[3] * then[3],
        first[4] * then[0] + first[5] * then[2] + then[4],
        first[4] * then[1] + first[5] * then[3] + then[5],
    ]
}

#[must_use]
pub fn invert(m: [f64; 6]) -> [f64; 6] {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-12 {
        return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }
    let a = m[3] / det;
    let b = -m[1] / det;
    let c = -m[2] / det;
    let d = m[0] / det;
    [a, b, c, d, -(m[4] * a + m[5] * c), -(m[4] * b + m[5] * d)]
}

#[must_use]
pub fn rect_to_user(m: [f64; 6], r: [f64; 4]) -> [f64; 4] {
    let corners = [
        apply(m, (r[0], r[1])),
        apply(m, (r[2], r[1])),
        apply(m, (r[0], r[3])),
        apply(m, (r[2], r[3])),
    ];
    let xs = corners.iter().map(|c| c.0);
    let ys = corners.iter().map(|c| c.1);
    [
        xs.clone().fold(f64::INFINITY, f64::min),
        ys.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
        ys.fold(f64::NEG_INFINITY, f64::max),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_land_where_the_reader_shows_them() {
        let crop = [0.0, 0.0, 600.0, 800.0];
        let m = shown_to_user(crop, 90);
        assert_eq!(apply(m, (0.0, 0.0)), (600.0, 0.0));
        assert_eq!(apply(m, (800.0, 600.0)), (0.0, 800.0));
        assert_eq!(shown_size(crop, 90), (800.0, 600.0));
        for rotate in [0, 90, 180, 270] {
            let m = shown_to_user(crop, rotate);
            let (w, h) = shown_size(crop, rotate);
            let r = rect_to_user(m, [0.0, 0.0, w, h]);
            assert_eq!(r, [0.0, 0.0, 600.0, 800.0]);
        }
        let back = invert(m);
        assert_eq!(apply(back, apply(m, (3.0, 4.0))), (3.0, 4.0));
        let s = [2.0, 0.0, 0.0, 3.0, 10.0, 20.0];
        let t = compose(s, shown_to_user(crop, 0));
        assert_eq!(apply(t, (1.0, 1.0)), (12.0, 23.0));
    }
}
