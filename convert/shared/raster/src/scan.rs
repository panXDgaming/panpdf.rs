use crate::{Image, to_u8, to_u32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Colour,
    Grey,
    BlackWhite,
    Original,
}

pub type Point = (f64, f64);

const SEARCH_SIDE: u32 = 480;

fn small_grey(image: &Image, side: u32) -> (Image, f64) {
    let grey = image.grey();
    let longest = grey.width.max(grey.height);
    if longest <= side {
        return (grey, 1.0);
    }
    let scale = f64::from(side) / f64::from(longest);
    let w = to_u32((f64::from(grey.width) * scale).round()).max(1);
    let h = to_u32((f64::from(grey.height) * scale).round()).max(1);
    (grey.resized(w, h), scale)
}

fn otsu(data: &[u8]) -> u8 {
    let mut histogram = [0u64; 256];
    for &v in data {
        histogram[usize::from(v)] += 1;
    }
    let total = data.len() as f64;
    let sum: f64 = histogram
        .iter()
        .enumerate()
        .map(|(i, &n)| i as f64 * n as f64)
        .sum();
    let (mut below, mut below_sum, mut best, mut best_at) = (0.0, 0.0, -1.0, 128u8);
    for (i, &n) in histogram.iter().enumerate() {
        below += n as f64;
        below_sum += i as f64 * n as f64;
        let above = total - below;
        if below == 0.0 || above == 0.0 {
            continue;
        }
        let mean_below = below_sum / below;
        let mean_above = (sum - below_sum) / above;
        let between = below * above * (mean_below - mean_above).powi(2);
        if between > best {
            best = between;
            best_at = u8::try_from(i).unwrap_or(128);
        }
    }
    best_at
}

#[must_use]
pub fn find_page(image: &Image) -> Option<[Point; 4]> {
    let (small, scale) = small_grey(image, SEARCH_SIDE);
    let (w, h) = (small.width as usize, small.height as usize);
    let threshold = otsu(&small.data);
    let bright: Vec<bool> = small.data.iter().map(|&v| v > threshold).collect();
    let mut label = vec![0u32; w * h];
    let mut best = (0usize, 0u32);
    let mut next = 0u32;
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !bright[start] || label[start] != 0 {
            continue;
        }
        next += 1;
        let mut size = 0usize;
        label[start] = next;
        stack.push(start);
        while let Some(at) = stack.pop() {
            size += 1;
            let (x, y) = (at % w, at / w);
            let mut visit = |n: usize| {
                if bright[n] && label[n] == 0 {
                    label[n] = next;
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(at - 1);
            }
            if x + 1 < w {
                visit(at + 1);
            }
            if y > 0 {
                visit(at - w);
            }
            if y + 1 < h {
                visit(at + w);
            }
        }
        if size > best.0 {
            best = (size, next);
        }
    }
    let area = (w * h) as f64;
    let share = best.0 as f64 / area;
    if !(0.15..=0.97).contains(&share) {
        return None;
    }
    let mut corners = [(0.0, 0.0); 4];
    let mut scores = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
    ];
    for (at, &l) in label.iter().enumerate() {
        if l != best.1 {
            continue;
        }
        let (x, y) = ((at % w) as f64, (at / w) as f64);
        let (sum, difference) = (x + y, x - y);
        if sum < scores[0] {
            scores[0] = sum;
            corners[0] = (x, y);
        }
        if difference > scores[1] {
            scores[1] = difference;
            corners[1] = (x + 1.0, y);
        }
        if sum > scores[2] {
            scores[2] = sum;
            corners[2] = (x + 1.0, y + 1.0);
        }
        if difference < scores[3] {
            scores[3] = difference;
            corners[3] = (x, y + 1.0);
        }
    }
    let middle = (
        corners.iter().map(|c| c.0).sum::<f64>() / 4.0,
        corners.iter().map(|c| c.1).sum::<f64>() / 4.0,
    );
    let quad = corners.map(|(x, y)| {
        let (x, y) = (x + (middle.0 - x) * 0.01, y + (middle.1 - y) * 0.01);
        (x / scale, y / scale)
    });
    let (width, height) = quad_size(&quad);
    if width < f64::from(image.width) * 0.2 || height < f64::from(image.height) * 0.2 {
        return None;
    }
    Some(quad)
}

fn distance(a: Point, b: Point) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[must_use]
pub fn quad_size(quad: &[Point; 4]) -> (f64, f64) {
    let width = distance(quad[0], quad[1]).max(distance(quad[3], quad[2]));
    let height = distance(quad[0], quad[3]).max(distance(quad[1], quad[2]));
    (width, height)
}

fn solve8(mut a: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        for row in 0..8 {
            if row != col {
                let factor = a[row][col] / a[col][col];
                let pivot_row = a[col];
                for (value, pivot_value) in a[row].iter_mut().zip(pivot_row).skip(col) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let mut x = [0.0; 8];
    for (i, xi) in x.iter_mut().enumerate() {
        *xi = a[i][8] / a[i][i];
    }
    Some(x)
}

#[must_use]
pub fn unwarp(image: &Image, quad: &[Point; 4], width: u32, height: u32) -> Option<Image> {
    let (w, h) = (f64::from(width), f64::from(height));
    let targets = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let mut rows = [[0.0; 9]; 8];
    for (i, (&(u, v), &(x, y))) in targets.iter().zip(quad.iter()).enumerate() {
        rows[2 * i] = [u, v, 1.0, 0.0, 0.0, 0.0, -u * x, -v * x, x];
        rows[2 * i + 1] = [0.0, 0.0, 0.0, u, v, 1.0, -u * y, -v * y, y];
    }
    let m = solve8(rows)?;
    let c = image.channels;
    let mut data = Vec::with_capacity(width as usize * height as usize * c);
    let mut pixel = [0u8; 3];
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let d = m[6] * u + m[7] * v + 1.0;
            let sx = (m[0] * u + m[1] * v + m[2]) / d - 0.5;
            let sy = (m[3] * u + m[4] * v + m[5]) / d - 0.5;
            image.sample(sx, sy, &mut pixel[..c], 255);
            data.extend_from_slice(&pixel[..c]);
        }
    }
    Some(Image {
        width,
        height,
        channels: c,
        data,
    })
}

#[must_use]
pub fn skew(image: &Image) -> f64 {
    let (small, _) = small_grey(image, 1200);
    let threshold = otsu(&small.data);
    let w = small.width as usize;
    let dark: Vec<(f64, f64)> = small
        .data
        .iter()
        .enumerate()
        .filter(|&(_, &v)| v <= threshold.min(160))
        .map(|(at, _)| ((at % w) as f64, (at / w) as f64))
        .collect();
    if dark.len() < 200 || dark.len() > small.data.len() / 2 {
        return 0.0;
    }
    let rows = small.height as usize + small.width as usize;
    let score = |degrees: f64| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let mut histogram = vec![0u32; 2 * rows + 1];
        for &(x, y) in &dark {
            let r = y * cos + x * sin;
            let bin = to_u32(r.round() + rows as f64) as usize;
            if let Some(slot) = histogram.get_mut(bin) {
                *slot += 1;
            }
        }
        histogram
            .iter()
            .map(|&n| f64::from(n) * f64::from(n))
            .sum::<f64>()
    };
    let search = |from: f64, to: f64, step: f64| {
        let mut best = (f64::NEG_INFINITY, 0.0);
        let mut angle = from;
        while angle <= to + 1e-9 {
            let s = score(angle);
            if s > best.0 {
                best = (s, angle);
            }
            angle += step;
        }
        best.1
    };
    let coarse = search(-5.0, 5.0, 0.25);
    let fine = search(coarse - 0.25, coarse + 0.25, 0.05);
    if fine.abs() < 0.1 { 0.0 } else { fine }
}

#[must_use]
pub fn whiten(image: &Image, look: Look) -> Image {
    if look == Look::Original {
        return image.clone();
    }
    let base = if look == Look::Colour {
        image.clone()
    } else {
        image.grey()
    };
    let grey = image.grey();
    let cell = (grey.width.max(grey.height) / 40).max(4);
    let (cw, ch) = (grey.width.div_ceil(cell), grey.height.div_ceil(cell));
    let mut cells = Image::filled(cw, ch, 1, 0);
    for y in 0..grey.height {
        for x in 0..grey.width {
            let v = grey.data[(y * grey.width + x) as usize];
            let at = ((y / cell) * cw + x / cell) as usize;
            cells.data[at] = cells.data[at].max(v);
        }
    }
    let smooth = |source: &Image| {
        let mut out = source.clone();
        for y in 0..source.height {
            for x in 0..source.width {
                let mut sum = 0u32;
                let mut n = 0u32;
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
                        if nx >= 0
                            && ny >= 0
                            && nx < i64::from(source.width)
                            && ny < i64::from(source.height)
                        {
                            sum += u32::from(
                                source.data[(ny as usize) * source.width as usize + nx as usize],
                            );
                            n += 1;
                        }
                    }
                }
                out.data[(y * source.width + x) as usize] =
                    u8::try_from(sum / n.max(1)).unwrap_or(255);
            }
        }
        out
    };
    let paper = smooth(&smooth(&cells));
    let c = base.channels;
    let mut out = base.clone();
    let mut factors = vec![0.0f64; grey.data.len()];
    let mut level = [0u8; 1];
    for y in 0..grey.height {
        for x in 0..grey.width {
            let fx = (f64::from(x) + 0.5) / f64::from(cell) - 0.5;
            let fy = (f64::from(y) + 0.5) / f64::from(cell) - 0.5;
            paper.sample(
                fx.clamp(0.0, f64::from(cw - 1)),
                fy.clamp(0.0, f64::from(ch - 1)),
                &mut level,
                255,
            );
            factors[(y * grey.width + x) as usize] = 255.0 / (0.92 * f64::from(level[0].max(48)));
        }
    }
    for (i, factor) in factors.iter().enumerate() {
        for k in 0..c {
            out.data[i * c + k] = to_u8(f64::from(base.data[i * c + k]) * factor);
        }
    }
    let mut histogram = [0usize; 256];
    for (i, factor) in factors.iter().enumerate() {
        histogram[usize::from(to_u8(f64::from(grey.data[i]) * factor))] += 1;
    }
    let mut seen = 0;
    let mut black = 0u8;
    for (v, &n) in histogram.iter().enumerate() {
        seen += n;
        if seen * 100 >= grey.data.len() {
            black = u8::try_from(v).unwrap_or(0);
            break;
        }
    }
    let black = f64::from(black.min(120));
    for v in &mut out.data {
        *v = to_u8((f64::from(*v) - black) * 255.0 / (255.0 - black));
    }
    if look == Look::BlackWhite {
        for v in &mut out.data {
            *v = if *v > 170 { 255 } else { 0 };
        }
    }
    out
}

#[must_use]
pub fn clean(image: &Image, look: Look, find: bool) -> (Image, String) {
    let mut said = Vec::new();
    let mut page = None;
    if find && let Some(quad) = find_page(image) {
        let (w, h) = quad_size(&quad);
        if let Some(cut) = unwarp(image, &quad, to_u32(w.round()), to_u32(h.round())) {
            said.push(format!(
                "page found, cut out at {} x {}",
                cut.width, cut.height
            ));
            page = Some(cut);
        }
    }
    let page = page.unwrap_or_else(|| {
        let angle = skew(image);
        if angle == 0.0 {
            said.push("no page edges found; not tilted".to_owned());
            image.clone()
        } else {
            said.push(format!(
                "no page edges found; straightened by {angle:.2} degrees"
            ));
            image.turned(-angle, 255)
        }
    });
    (whiten(&page, look), said.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photograph() -> Image {
        let (w, h) = (400u32, 300u32);
        let mut image = Image::filled(w, h, 3, 40);
        for y in 50..250 {
            for x in 100..300 {
                let at = ((y * w + x) * 3) as usize;
                image.data[at..at + 3].copy_from_slice(&[240, 238, 230]);
            }
        }
        image
    }

    #[test]
    fn a_page_on_a_desk_is_found() {
        let quad = find_page(&photograph()).expect("found");
        let near = |a: Point, b: Point| distance(a, b) < 3.0;
        assert!(near(quad[0], (100.0, 50.0)), "{quad:?}");
        assert!(near(quad[2], (300.0, 250.0)), "{quad:?}");
        let (w, h) = quad_size(&quad);
        let cut = unwarp(&photograph(), &quad, to_u32(w), to_u32(h)).unwrap();
        let dark = cut.data.iter().filter(|&&v| v < 128).count();
        assert!(dark * 100 < cut.data.len(), "{dark} dark samples");
    }

    #[test]
    fn a_plain_picture_has_no_page() {
        assert!(find_page(&Image::filled(100, 100, 1, 200)).is_none());
    }

    #[test]
    fn tilted_lines_are_measured() {
        let (w, h) = (600u32, 600u32);
        let mut image = Image::filled(w, h, 1, 255);
        for line in 0..12 {
            let base = 60.0 + f64::from(line) * 40.0;
            for x in 50..550 {
                let y = base - (f64::from(x) - 300.0) * 2f64.to_radians().tan();
                for dy in 0..6 {
                    let yy = to_u32(y) + dy;
                    if yy < h {
                        image.data[(yy * w + x) as usize] = 0;
                    }
                }
            }
        }
        let angle = skew(&image);
        assert!((angle - 2.0).abs() < 0.3, "measured {angle}");
    }

    #[test]
    fn shadows_are_whitened() {
        let (w, h) = (200u32, 100u32);
        let mut image = Image::filled(w, h, 1, 0);
        for y in 0..h {
            for x in 0..w {
                image.data[(y * w + x) as usize] =
                    to_u8(230.0 - 80.0 * f64::from(x) / f64::from(w));
            }
        }
        let out = whiten(&image, Look::Grey);
        let least = out.data.iter().copied().min().unwrap();
        assert!(least > 200, "darkest paper {least}");
    }
}
