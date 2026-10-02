use crate::Grey;

pub type Corners = [(f32, f32); 4];

const LOOKED_AT: usize = 240;

const SMALLEST_SHARE: f64 = 0.12;
const LARGEST_SHARE: f64 = 0.96;

const SQUARE_ENOUGH: f64 = 0.8;

#[must_use]
pub fn find_sheet(picture: &Grey) -> Option<Corners> {
    if picture.width < 8
        || picture.height < 8
        || picture.pixels.len() < picture.width * picture.height
    {
        return None;
    }
    let (small, scale) = shrink(picture);
    best_sheet(&[blur(&small, 2)], scale)
}

#[must_use]
pub fn find_sheet_in_colour(picture: &crate::Rgba) -> Option<Corners> {
    if picture.width < 8 || picture.height < 8 {
        return None;
    }
    let grey = picture.grey();
    let white = crate::Grey {
        width: picture.width,
        height: picture.height,
        pixels: picture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                let high = p[0].max(p[1]).max(p[2]);
                let low = p[0].min(p[1]).min(p[2]);
                high.saturating_sub((high - low).saturating_mul(2))
            })
            .collect(),
    };
    let (small_grey, scale) = shrink(&grey);
    let (small_white, _) = shrink(&white);
    best_sheet(&[blur(&small_grey, 2), blur(&small_white, 2)], scale)
}

fn best_sheet(smooth: &[Grey], scale: f64) -> Option<Corners> {
    let mut best: Option<([(f64, f64); 4], f64)> = None;
    for picture in smooth {
        let level = otsu(&picture.pixels);
        let walls = edges(picture);
        for shift in [0_i16, -24, 24, -48] {
            let Some(level) = u8::try_from(i16::from(level) + shift).ok() else {
                continue;
            };
            for walled in [None, Some(&walls)] {
                let Some(corners) = sheet_at_level(picture, level, walled) else {
                    continue;
                };
                let score = edge_support(picture, &corners);
                if score >= EDGES_SEEN && best.is_none_or(|(_, held)| score > held) {
                    best = Some((corners, score));
                }
            }
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    best.map(|(corners, _)| {
        corners.map(|(x, y)| (((x + 0.5) * scale) as f32, ((y + 0.5) * scale) as f32))
    })
}

const EDGES_SEEN: f64 = 0.55;

fn sheet_at_level(smooth: &Grey, level: u8, walls: Option<&Vec<bool>>) -> Option<[(f64, f64); 4]> {
    let light: Vec<bool> = smooth
        .pixels
        .iter()
        .enumerate()
        .map(|(at, &v)| v > level && !walls.is_some_and(|walls| walls[at]))
        .collect();
    sheet_from_region(&light, smooth.width, smooth.height)
}

pub(crate) fn sheet_from_region(
    light: &[bool],
    width: usize,
    height: usize,
) -> Option<[(f64, f64); 4]> {
    let (region, area) = largest_region(light, width, height)?;
    #[allow(clippy::cast_precision_loss)]
    let share = area as f64 / (width * height) as f64;
    if !(SMALLEST_SHARE..=LARGEST_SHARE).contains(&share) {
        return None;
    }
    let rough = diagonal_extremes(&region, width)?;
    #[allow(clippy::cast_precision_loss)]
    let (right, bottom) = ((width - 1) as f64, (height - 1) as f64);
    let on_edge = rough
        .iter()
        .filter(|(x, y)| *x <= 0.0 || *y <= 0.0 || *x >= right || *y >= bottom)
        .count();
    if on_edge >= 2 {
        return None;
    }
    let corners = refine(&region, light, width, height, rough);
    #[allow(clippy::cast_precision_loss)]
    let quad_share = quad_area(&corners) / area as f64;
    (is_convex(&corners) && (SQUARE_ENOUGH..1.25).contains(&quad_share)).then_some(corners)
}

#[allow(clippy::many_single_char_names)]
fn refine(
    region: &[usize],
    light: &[bool],
    width: usize,
    height: usize,
    rough: [(f64, f64); 4],
) -> [(f64, f64); 4] {
    let inside = |x: usize, y: usize| light[y * width + x];
    let mut sides: [Vec<(f64, f64)>; 4] = Default::default();
    for &at in region {
        let (x, y) = (at % width, at / width);
        let border = x == 0
            || y == 0
            || x + 1 == width
            || y + 1 == height
            || !inside(x - 1, y)
            || !inside(x + 1, y)
            || !inside(x, y - 1)
            || !inside(x, y + 1);
        if !border {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let p = (x as f64, y as f64);
        let mut nearest = (f64::INFINITY, 0);
        for side in 0..4 {
            let (a, b) = (rough[side], rough[(side + 1) % 4]);
            let d = distance_to_middle(p, a, b);
            if d < nearest.0 {
                nearest = (d, side);
            }
        }
        if nearest.0 < 6.0 {
            sides[nearest.1].push(p);
        }
    }
    let lines: Vec<Option<Line>> = sides.iter().map(|points| fit_line(points)).collect();
    let mut out = rough;
    for corner in 0..4 {
        let (before, after) = ((corner + 3) % 4, corner);
        if let (Some(a), Some(b)) = (lines[before], lines[after])
            && let Some(point) = crossing(a, b)
            && (point.0 - rough[corner].0).hypot(point.1 - rough[corner].1) < 20.0
        {
            out[corner] = point;
        }
    }
    out
}

fn distance_to_middle(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.mul_add(dx, dy * dy);
    if length < 1e-9 {
        return f64::INFINITY;
    }
    let t = ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length;
    if !(0.1..=0.9).contains(&t) {
        return f64::INFINITY;
    }
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    (p.0 - qx).hypot(p.1 - qy)
}

fn fit_line(points: &[(f64, f64)]) -> Option<Line> {
    if points.len() < 8 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let n = points.len() as f64;
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    let (mx, my) = (mx / n, my / n);
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

type Line = ((f64, f64), (f64, f64));

#[allow(clippy::many_single_char_names)]
fn crossing(a: Line, b: Line) -> Option<(f64, f64)> {
    let ((p, d), (q, e)) = (a, b);
    let denominator = d.0 * e.1 - d.1 * e.0;
    if denominator.abs() < 1e-6 {
        return None;
    }
    let t = ((q.0 - p.0) * e.1 - (q.1 - p.1) * e.0) / denominator;
    Some((p.0 + t * d.0, p.1 + t * d.1))
}

fn edges(smooth: &Grey) -> Vec<bool> {
    let (w, h) = (smooth.width, smooth.height);
    let at = |x: usize, y: usize| i32::from(smooth.pixels[y * w + x]);
    let mut steep = vec![0_i32; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let gx = at(x + 1, y - 1) + 2 * at(x + 1, y) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2 * at(x - 1, y)
                - at(x - 1, y + 1);
            let gy = at(x - 1, y + 1) + 2 * at(x, y + 1) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2 * at(x, y - 1)
                - at(x + 1, y - 1);
            steep[y * w + x] = gx.abs() + gy.abs();
        }
    }
    let mut sorted = steep.clone();
    sorted.sort_unstable();
    let tenth = sorted[sorted.len() * 9 / 10].max(WALL_AT_LEAST);
    steep.iter().map(|&g| g >= tenth).collect()
}

const WALL_AT_LEAST: i32 = 40;

fn edge_support(smooth: &Grey, corners: &[(f64, f64); 4]) -> f64 {
    let at = |x: f64, y: f64| -> Option<f64> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (x, y) = (x.round() as usize, y.round() as usize);
        (x < smooth.width && y < smooth.height)
            .then(|| f64::from(smooth.pixels[y * smooth.width + x]))
    };
    let centre = corners
        .iter()
        .fold((0.0, 0.0), |(x, y), c| (x + c.0 / 4.0, y + c.1 / 4.0));
    let (mut seen, mut looked) = (0_u32, 0_u32);
    for side in 0..4 {
        let (a, b) = (corners[side], corners[(side + 1) % 4]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy).max(1e-9);
        let (mut nx, mut ny) = (dy / length, -dx / length);
        let (mx, my) = (f64::midpoint(a.0, b.0), f64::midpoint(a.1, b.1));
        if (mx + nx - centre.0).hypot(my + ny - centre.1) < (mx - centre.0).hypot(my - centre.1) {
            nx = -nx;
            ny = -ny;
        }
        for step in 1..20 {
            let t = f64::from(step) / 20.0;
            let (px, py) = (a.0 + t * dx, a.1 + t * dy);
            if let (Some(inner), Some(outer)) = (
                at(px - 3.0 * nx, py - 3.0 * ny),
                at(px + 3.0 * nx, py + 3.0 * ny),
            ) {
                looked += 1;
                if inner - outer > 10.0 {
                    seen += 1;
                }
            }
        }
    }
    if looked == 0 {
        return 0.0;
    }
    f64::from(seen) / f64::from(looked)
}

fn shrink(picture: &Grey) -> (Grey, f64) {
    let longest = picture.width.max(picture.height);
    let step = longest.div_ceil(LOOKED_AT).max(1);
    let (width, height) = (picture.width / step, picture.height / step);
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0_u32;
            for dy in 0..step {
                let row = (y * step + dy) * picture.width;
                for dx in 0..step {
                    sum += u32::from(picture.pixels[row + x * step + dx]);
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            pixels.push((sum / (step * step) as u32) as u8);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    (
        Grey {
            width,
            height,
            pixels,
        },
        step as f64,
    )
}

pub(crate) fn blur(picture: &Grey, radius: usize) -> Grey {
    let (w, h) = (picture.width, picture.height);
    let pass = |source: &[u8], across: bool| -> Vec<u8> {
        let mut out = vec![0_u8; source.len()];
        let (lines, length) = if across { (h, w) } else { (w, h) };
        for line in 0..lines {
            let at = |i: usize| if across { line * w + i } else { i * w + line };
            let mut sum = 0_u32;
            let mut count = 0_u32;
            for i in 0..=radius.min(length - 1) {
                sum += u32::from(source[at(i)]);
                count += 1;
            }
            for i in 0..length {
                #[allow(clippy::cast_possible_truncation)]
                {
                    out[at(i)] = (sum / count) as u8;
                }
                if i + radius + 1 < length {
                    sum += u32::from(source[at(i + radius + 1)]);
                    count += 1;
                }
                if i >= radius {
                    sum -= u32::from(source[at(i - radius)]);
                    count -= 1;
                }
            }
        }
        out
    };
    let across = pass(&picture.pixels, true);
    Grey {
        width: w,
        height: h,
        pixels: pass(&across, false),
    }
}

fn otsu(pixels: &[u8]) -> u8 {
    let mut histogram = [0_u64; 256];
    for &v in pixels {
        histogram[usize::from(v)] += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let total = pixels.len() as f64;
    #[allow(clippy::cast_precision_loss)]
    let sum_all: f64 = histogram
        .iter()
        .enumerate()
        .map(|(v, &n)| v as f64 * n as f64)
        .sum();
    let (mut below, mut sum_below, mut best, mut level) = (0.0_f64, 0.0_f64, 0.0_f64, 0_u8);
    for (v, &n) in histogram.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        {
            below += n as f64;
            sum_below += v as f64 * n as f64;
        }
        let above = total - below;
        if below == 0.0 || above == 0.0 {
            continue;
        }
        let spread = below * above * (sum_below / below - (sum_all - sum_below) / above).powi(2);
        if spread > best {
            best = spread;
            level = u8::try_from(v).unwrap_or(u8::MAX);
        }
    }
    level
}

fn largest_region(light: &[bool], width: usize, height: usize) -> Option<(Vec<usize>, usize)> {
    let mut seen = vec![false; light.len()];
    let mut best: Vec<usize> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..light.len() {
        if !light[start] || seen[start] {
            continue;
        }
        let mut region = Vec::new();
        seen[start] = true;
        stack.push(start);
        while let Some(at) = stack.pop() {
            region.push(at);
            let (x, y) = (at % width, at / width);
            let mut visit = |next: usize| {
                if light[next] && !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            };
            if x > 0 {
                visit(at - 1);
            }
            if x + 1 < width {
                visit(at + 1);
            }
            if y > 0 {
                visit(at - width);
            }
            if y + 1 < height {
                visit(at + width);
            }
        }
        if region.len() > best.len() {
            best = region;
        }
    }
    let area = best.len();
    (area > 0).then_some((best, area))
}

fn diagonal_extremes(region: &[usize], width: usize) -> Option<[(f64, f64); 4]> {
    let mut corners = [(0.0_f64, 0.0_f64); 4];
    let mut scores = [f64::NEG_INFINITY; 4];
    for &at in region {
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = ((at % width) as f64, (at / width) as f64);
        for (index, score) in [-x - y, x - y, x + y, y - x].into_iter().enumerate() {
            if score > scores[index] {
                scores[index] = score;
                corners[index] = (x, y);
            }
        }
    }
    scores.iter().all(|s| s.is_finite()).then_some(corners)
}

pub(crate) fn quad_area(corners: &[(f64, f64); 4]) -> f64 {
    let mut twice = 0.0;
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        twice += a.0 * b.1 - b.0 * a.1;
    }
    twice.abs() / 2.0
}

fn is_convex(corners: &[(f64, f64); 4]) -> bool {
    let mut turns = [false; 4];
    for (i, turn) in turns.iter_mut().enumerate() {
        let (a, b, c) = (corners[i], corners[(i + 1) % 4], corners[(i + 2) % 4]);
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() < 1e-9 {
            return false;
        }
        *turn = cross > 0.0;
    }
    turns.iter().all(|turn| *turn == turns[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn photo(width: usize, height: usize, corners: [(f64, f64); 4]) -> Grey {
        let mut pixels = vec![60_u8; width * height];
        for y in 0..height {
            for x in 0..width {
                #[allow(clippy::cast_precision_loss)]
                let p = (x as f64 + 0.5, y as f64 + 0.5);
                let inside = (0..4).all(|i| {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.0
                });
                if inside {
                    pixels[y * width + x] = if y % 23 < 3 && x % 7 != 0 { 40 } else { 225 };
                }
            }
        }
        Grey {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn a_tilted_sheet_on_a_dark_table_is_found_at_its_corners() {
        let truth = [
            (210.0, 120.0),
            (820.0, 190.0),
            (760.0, 1050.0),
            (150.0, 980.0),
        ];
        let found = find_sheet(&photo(960, 1280, truth)).expect("a sheet");
        for (got, want) in found.iter().zip(truth) {
            let off = (f64::from(got.0) - want.0).hypot(f64::from(got.1) - want.1);
            assert!(off < 14.0, "{got:?} vs {want:?}: {off}");
        }
    }

    #[test]
    fn a_frame_with_no_sheet_finds_none() {
        let (w, h) = (320, 240);
        let pixels = (0..w * h)
            .map(|i| u8::try_from((i % w) * 255 / w).unwrap_or(0))
            .collect();
        assert_eq!(
            find_sheet(&Grey {
                width: w,
                height: h,
                pixels
            }),
            None
        );
    }

    #[test]
    fn a_sheet_filling_the_whole_frame_is_not_claimed() {
        let full = [(0.0, 0.0), (320.0, 0.0), (320.0, 240.0), (0.0, 240.0)];
        assert_eq!(find_sheet(&photo(320, 240, full)), None);
    }
}
