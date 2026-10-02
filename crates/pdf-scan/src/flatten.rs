use crate::find::blur;
use crate::{Corners, Grey, Rgba};

#[must_use]
pub fn flattened_size(corners: &Corners) -> (usize, usize) {
    let length = |a: (f32, f32), b: (f32, f32)| f64::from(a.0 - b.0).hypot(f64::from(a.1 - b.1));
    let [tl, tr, br, bl] = *corners;
    let width = length(tl, tr).max(length(bl, br)).round().max(1.0);
    let height = length(tl, bl).max(length(tr, br)).round().max(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (width as usize, height as usize)
}

#[must_use]
pub fn page_size(corners: &Corners, long: usize) -> (usize, usize) {
    let (w, h) = flattened_size(corners);
    let longest = w.max(h).max(1);
    if longest >= long {
        return (w, h);
    }
    #[allow(clippy::cast_precision_loss)]
    let grow = long as f64 / longest as f64;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    (
        (w as f64 * grow).round() as usize,
        (h as f64 * grow).round() as usize,
    )
}

#[must_use]
pub fn flatten(picture: &Rgba, corners: &Corners, size: (usize, usize)) -> Rgba {
    let (width, height) = (size.0.max(1), size.1.max(1));
    #[allow(clippy::cast_precision_loss)]
    let page = [
        (0.0, 0.0),
        (width as f64, 0.0),
        (width as f64, height as f64),
        (0.0, height as f64),
    ];
    let sheet = corners.map(|(x, y)| (f64::from(x), f64::from(y)));
    let mut pixels = vec![255_u8; width * height * 4];
    let Some(map) = homography(&page, &sheet) else {
        return Rgba {
            width,
            height,
            pixels,
        };
    };
    for y in 0..height {
        for x in 0..width {
            #[allow(clippy::cast_precision_loss)]
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let w = map[6] * px + map[7] * py + 1.0;
            let sx = (map[0] * px + map[1] * py + map[2]) / w - 0.5;
            let sy = (map[3] * px + map[4] * py + map[5]) / w - 0.5;
            let at = (y * width + x) * 4;
            pixels[at..at + 4].copy_from_slice(&sample(picture, sx, sy));
        }
    }
    Rgba {
        width,
        height,
        pixels,
    }
}

fn nearest(v: f64) -> f64 {
    let floor = v.floor();
    if v - floor >= 0.5 { floor + 1.0 } else { floor }
}

fn sample(picture: &Rgba, x: f64, y: f64) -> [u8; 4] {
    #[allow(clippy::cast_precision_loss)]
    let (right, bottom) = ((picture.width - 1) as f64, (picture.height - 1) as f64);
    let (x, y) = (x.clamp(0.0, right), y.clamp(0.0, bottom));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = (
        (x0 + 1).min(picture.width - 1),
        (y0 + 1).min(picture.height - 1),
    );
    #[allow(clippy::cast_precision_loss)]
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    let at =
        |x: usize, y: usize, c: usize| f64::from(picture.pixels[(y * picture.width + x) * 4 + c]);
    let mut out = [0_u8; 4];
    for (c, value) in out.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
        let low = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            *value = nearest(top * (1.0 - fy) + low * fy).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn homography(from: &[(f64, f64); 4], to: &[(f64, f64); 4]) -> Option<[f64; 8]> {
    let mut rows = [[0.0_f64; 9]; 8];
    for i in 0..4 {
        let ((x, y), (u, v)) = (from[i], to[i]);
        rows[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        rows[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for column in 0..8 {
        let pivot = (column..8).max_by(|&a, &b| {
            rows[a][column]
                .abs()
                .partial_cmp(&rows[b][column].abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        if rows[pivot][column].abs() < 1e-12 {
            return None;
        }
        rows.swap(column, pivot);
        let pivot_row = rows[column];
        for (index, row) in rows.iter_mut().enumerate() {
            if index != column {
                let factor = row[column] / pivot_row[column];
                for (value, above) in row.iter_mut().zip(pivot_row.iter()).skip(column) {
                    *value -= factor * above;
                }
            }
        }
    }
    let mut out = [0.0; 8];
    for (i, value) in out.iter_mut().enumerate() {
        *value = rows[i][8] / rows[i][i];
    }
    Some(out)
}

#[must_use]
pub fn clean(page: &Rgba) -> Rgba {
    let grey = page.grey();
    let paper = paper_light(&grey);
    let mut pixels = Vec::with_capacity(grey.pixels.len() * 4);
    for (&ink, &around) in grey.pixels.iter().zip(&paper.pixels) {
        let ratio = f64::from(ink) / f64::from(around.max(1));
        let value = ((ratio - 0.55) / (0.92 - 0.55)).clamp(0.0, 1.0) * 255.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = nearest(value) as u8;
        pixels.extend_from_slice(&[v, v, v, 255]);
    }
    Rgba {
        width: grey.width,
        height: grey.height,
        pixels,
    }
}

#[must_use]
pub fn enhance(page: &Rgba) -> Rgba {
    let (w, h) = (page.width, page.height);
    let lights = paper_light_rgb(page);
    let even = |v: u8, p: u8| (f32::from(v) / f32::from(p.max(24)) * PAPER_WHITE).min(255.0);
    let mut evened = [
        vec![0.0_f32; w * h],
        vec![0.0_f32; w * h],
        vec![0.0_f32; w * h],
    ];
    let [red, green, blue] = &mut evened;
    let [lr, lg, lb] = &lights;
    let paper = lr.pixels.iter().zip(&lg.pixels).zip(&lb.pixels);
    for ((((pixel, r), g), b), ((pr, pg), pb)) in page
        .pixels
        .chunks_exact(4)
        .zip(red.iter_mut())
        .zip(green.iter_mut())
        .zip(blue.iter_mut())
        .zip(paper)
    {
        *r = even(pixel[0], *pr);
        *g = even(pixel[1], *pg);
        *b = even(pixel[2], *pb);
    }
    let [red, green, blue] = &evened;
    let light: Vec<f32> = red
        .iter()
        .zip(green)
        .zip(blue)
        .map(|((r, g), b)| 0.299 * r + 0.587 * g + 0.114 * b)
        .collect();
    let soft = box_blur_f32(&light, w, h, 1);
    let levels = InkLevels::new();
    let mut pixels = vec![255_u8; w * h * 4];
    let sharpened = light.iter().zip(&soft).zip(red).zip(green).zip(blue);
    for (out, ((((l, s), r), g), b)) in pixels.chunks_exact_mut(4).zip(sharpened) {
        let lift = SHARPEN * (l - s);
        for (o, v) in out.iter_mut().zip([r, g, b]) {
            *o = levels.of(((v + lift) / PAPER_WHITE).clamp(0.0, 1.0));
        }
    }
    Rgba {
        width: w,
        height: h,
        pixels,
    }
}

fn ink_curve(x: f32) -> u8 {
    let curved = PAPER_WHITE * x.powf(INK_CURVE);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let level = curved.round().clamp(0.0, 255.0) as u8;
    level
}

struct InkLevels {
    starts: Vec<f32>,
    bucket: Vec<u8>,
}

impl InkLevels {
    const BUCKETS: usize = 4096;

    fn new() -> Self {
        let top = ink_curve(1.0);
        let starts: Vec<f32> = (1..=top)
            .map(|level| {
                let (mut low, mut high) = (0_u32, 1.0_f32.to_bits());
                while low < high {
                    let middle = low + (high - low) / 2;
                    if ink_curve(f32::from_bits(middle)) >= level {
                        high = middle;
                    } else {
                        low = middle + 1;
                    }
                }
                f32::from_bits(low)
            })
            .collect();
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let bucket = (0..=Self::BUCKETS)
            .map(|b| {
                let from = b as f32 / Self::BUCKETS as f32;
                starts.partition_point(|&t| t <= from) as u8
            })
            .collect();
        Self { starts, bucket }
    }

    fn of(&self, x: f32) -> u8 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let b = ((x * Self::BUCKETS as f32) as usize).min(Self::BUCKETS);
        let mut level = self.bucket[b];
        while let Some(&start) = self.starts.get(usize::from(level)) {
            if start > x {
                break;
            }
            level += 1;
        }
        level
    }
}

const PAPER_WHITE: f32 = 247.0;

const SHARPEN: f32 = 0.6;

const INK_CURVE: f32 = 1.35;

const PAPER_STEP: usize = 4;

fn paper_light(picture: &Grey) -> Grey {
    let (w, h) = (picture.width, picture.height);
    let (sw, sh) = (w.div_ceil(PAPER_STEP).max(1), h.div_ceil(PAPER_STEP).max(1));
    let mut small = vec![0_u8; sw * sh];
    for y in 0..h {
        for x in 0..w {
            let at = (y / PAPER_STEP) * sw + x / PAPER_STEP;
            small[at] = small[at].max(picture.pixels[y * w + x]);
        }
    }
    let small = Grey {
        width: sw,
        height: sh,
        pixels: small,
    };
    let radius = (sw.max(sh) / 50).max(2);
    laid_over(&blur(&lightest(&small, radius), radius), w, h)
}

fn laid_over(smooth: &Grey, w: usize, h: usize) -> Grey {
    let (sw, sh) = (smooth.width, smooth.height);
    let mut pixels = Vec::with_capacity(w * h);
    #[allow(clippy::cast_precision_loss)]
    let columns: Vec<(usize, usize, f32)> = (0..w)
        .map(|x| {
            let fx = ((x as f32 + 0.5) / PAPER_STEP as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            (x0, (x0 + 1).min(sw - 1), tx)
        })
        .collect();
    #[allow(clippy::cast_precision_loss)]
    for y in 0..h {
        let fy = ((y as f32 + 0.5) / PAPER_STEP as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for &(x0, x1, tx) in &columns {
            let at = |x: usize, y: usize| f32::from(smooth.pixels[y * sw + x]);
            let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
            let low = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            pixels.push(nearest(f64::from(top * (1.0 - ty) + low * ty)) as u8);
        }
    }
    Grey {
        width: w,
        height: h,
        pixels,
    }
}

const PAPER_HUE: f32 = 0.05;

#[allow(clippy::many_single_char_names)]
fn paper_light_rgb(page: &Rgba) -> [Grey; 3] {
    let (w, h) = (page.width, page.height);
    let (sw, sh) = (w.div_ceil(PAPER_STEP).max(1), h.div_ceil(PAPER_STEP).max(1));
    let lightness =
        |p: [u8; 3]| u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29;
    let mut cells = vec![[0_u8; 3]; sw * sh];
    let mut best = vec![0_u32; sw * sh];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let p = [page.pixels[i], page.pixels[i + 1], page.pixels[i + 2]];
            let at = (y / PAPER_STEP) * sw + x / PAPER_STEP;
            let l = lightness(p);
            if l >= best[at] {
                best[at] = l;
                cells[at] = p;
            }
        }
    }
    let radius = (sw.max(sh) / 50).max(2);
    let lit = lightest_colour(&cells, sw, sh, radius, lightness);
    let hue = |p: [u8; 3]| {
        let sum = (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])).max(1);
        #[allow(clippy::cast_precision_loss)]
        (f32::from(p[0]) / sum as f32, f32::from(p[1]) / sum as f32)
    };
    let mut order: Vec<usize> = (0..lit.len()).collect();
    order.sort_unstable_by_key(|&i| std::cmp::Reverse(lightness(lit[i])));
    let lightest_fifth = &order[..(order.len() / 5).max(1)];
    let middle = |mut values: Vec<f32>| {
        values.sort_unstable_by(f32::total_cmp);
        values[values.len() / 2]
    };
    let paper = (
        middle(lightest_fifth.iter().map(|&i| hue(lit[i]).0).collect()),
        middle(lightest_fifth.iter().map(|&i| hue(lit[i]).1).collect()),
    );
    let paper_lightness = lightness(lit[lightest_fifth[lightest_fifth.len() / 2]]);
    let is_paper: Vec<bool> = lit
        .iter()
        .map(|&p| {
            let (r, g) = hue(p);
            (r - paper.0).hypot(g - paper.1) < PAPER_HUE && lightness(p) * 4 > paper_lightness
        })
        .collect();
    let mut values: [Vec<f32>; 3] =
        std::array::from_fn(|c| lit.iter().map(|p| f32::from(p[c])).collect());
    if is_paper.iter().any(|&p| p) {
        carry_in(&mut values, is_paper, sw, sh, radius);
    }
    values.map(|channel| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let small = Grey {
            width: sw,
            height: sh,
            pixels: channel
                .iter()
                .map(|v| v.round().clamp(0.0, 255.0) as u8)
                .collect(),
        };
        laid_over(&blur(&small, radius), w, h)
    })
}

fn carry_in(values: &mut [Vec<f32>; 3], mut known: Vec<bool>, w: usize, h: usize, radius: usize) {
    let mut reach = radius;
    while known.iter().any(|&k| !k) && reach <= 2 * w.max(h) {
        let weight: Vec<f32> = known.iter().map(|&k| f32::from(u8::from(k))).collect();
        let near = box_blur_f32(&weight, w, h, reach);
        let sums: Vec<Vec<f32>> = values
            .iter()
            .map(|channel| {
                let held: Vec<f32> = channel.iter().zip(&weight).map(|(v, k)| v * k).collect();
                box_blur_f32(&held, w, h, reach)
            })
            .collect();
        let mut now = known.clone();
        for i in 0..w * h {
            if !known[i] && near[i] > 1e-4 {
                for (channel, sum) in values.iter_mut().zip(&sums) {
                    channel[i] = sum[i] / near[i];
                }
                now[i] = true;
            }
        }
        known = now;
        reach *= 2;
    }
}

fn lightest_colour(
    cells: &[[u8; 3]],
    w: usize,
    h: usize,
    radius: usize,
    lightness: impl Fn([u8; 3]) -> u32,
) -> Vec<[u8; 3]> {
    let pass = |source: &[[u8; 3]], across: bool| -> Vec<[u8; 3]> {
        let mut out = vec![[0_u8; 3]; source.len()];
        let (lines, length) = if across { (h, w) } else { (w, h) };
        for line in 0..lines {
            let at = |i: usize| if across { line * w + i } else { i * w + line };
            for i in 0..length {
                let (from, to) = (i.saturating_sub(radius), (i + radius).min(length - 1));
                out[at(i)] = (from..=to)
                    .map(|j| source[at(j)])
                    .max_by_key(|&p| lightness(p))
                    .unwrap_or([0; 3]);
            }
        }
        out
    };
    pass(&pass(cells, true), false)
}

fn box_blur_f32(values: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    const BAND: usize = 64;
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let mut across = vec![0.0_f32; values.len()];
    let mut prefix = vec![0.0_f64; w + 1];
    for (row, out) in values.chunks_exact(w).zip(across.chunks_exact_mut(w)) {
        for (i, &v) in row.iter().enumerate() {
            prefix[i + 1] = prefix[i] + f64::from(v);
        }
        for (i, o) in out.iter_mut().enumerate() {
            let (from, to) = (i.saturating_sub(radius), (i + radius).min(w - 1));
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            {
                *o = ((prefix[to + 1] - prefix[from]) / (to - from + 1) as f64) as f32;
            }
        }
    }
    let mut out = vec![0.0_f32; values.len()];
    let mut sums = vec![0.0_f64; (h + 1) * BAND];
    for first in (0..w).step_by(BAND) {
        let band = BAND.min(w - first);
        for y in 0..h {
            let row = &across[y * w + first..y * w + first + band];
            let (before, now) = sums.split_at_mut((y + 1) * BAND);
            let before = &before[y * BAND..];
            for ((sum, &earlier), &v) in now[..band].iter_mut().zip(before).zip(row) {
                *sum = earlier + f64::from(v);
            }
        }
        for y in 0..h {
            let (from, to) = (y.saturating_sub(radius), (y + radius).min(h - 1));
            #[allow(clippy::cast_precision_loss)]
            let count = (to - from + 1) as f64;
            let (low, high) = (&sums[from * BAND..], &sums[(to + 1) * BAND..]);
            for (x, o) in out[y * w + first..y * w + first + band]
                .iter_mut()
                .enumerate()
            {
                #[allow(clippy::cast_possible_truncation)]
                {
                    *o = ((high[x] - low[x]) / count) as f32;
                }
            }
        }
    }
    out
}

fn lightest(picture: &Grey, radius: usize) -> Grey {
    let (w, h) = (picture.width, picture.height);
    let pass = |source: &[u8], across: bool| -> Vec<u8> {
        let mut out = vec![0_u8; source.len()];
        let (lines, length) = if across { (h, w) } else { (w, h) };
        for line in 0..lines {
            let at = |i: usize| if across { line * w + i } else { i * w + line };
            for i in 0..length {
                let (from, to) = (i.saturating_sub(radius), (i + radius).min(length - 1));
                out[at(i)] = (from..=to).map(|j| source[at(j)]).max().unwrap_or(0);
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

#[must_use]
pub fn rgba_of(grey: &Grey) -> Rgba {
    Rgba {
        width: grey.width,
        height: grey.height,
        pixels: grey.pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn a_tilted_sheet_comes_out_upright_with_its_corners_where_the_page_has_them() {
        let (w, h) = (400, 300);
        let corners: Corners = [(60.0, 40.0), (340.0, 70.0), (320.0, 260.0), (80.0, 240.0)];
        let mut pixels = vec![0_u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let inside = (0..4).all(|i| {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.0
                });
                let dot = (p.0 - 75.0).hypot(p.1 - 58.0) < 6.0;
                let v = if dot {
                    0
                } else if inside {
                    230
                } else {
                    0
                };
                pixels[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let picture = Rgba {
            width: w,
            height: h,
            pixels,
        };
        let size = flattened_size(&corners);
        let page = flatten(&picture, &corners, size);
        let at = |x: usize, y: usize| page.pixels[(y * page.width + x) * 4];
        assert!(at(page.width / 2, page.height / 2) > 200);
        assert!(at(page.width - 4, page.height - 4) > 150);
        let dark_near_corner = (0..40).any(|y| (0..40).any(|x| at(x, y) < 60));
        assert!(dark_near_corner);
    }

    #[test]
    fn a_shadow_across_the_paper_comes_out_white_and_the_ink_stays_dark() {
        let (w, h) = (300, 200);
        let mut grey = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let paper = 240.0 - 90.0 * (x as f64 / w as f64);
                let ink = (95..105).contains(&y);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                grey.push(if ink {
                    (paper * 0.4) as u8
                } else {
                    paper as u8
                });
            }
        }
        let page = clean(&rgba_of(&Grey {
            width: w,
            height: h,
            pixels: grey,
        }));
        let at = |x: usize, y: usize| page.pixels[(y * w + x) * 4];
        assert!(at(20, 40) > 240, "light paper white: {}", at(20, 40));
        assert!(at(280, 40) > 240, "shadowed paper white: {}", at(280, 40));
        assert!(
            at(280, 100) < 40,
            "ink in the shadow dark: {}",
            at(280, 100)
        );
    }

    #[allow(clippy::many_single_char_names)]
    fn letterhead() -> (Rgba, usize, usize) {
        let (w, h) = (600, 800);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let light = 1.0 - 0.35 * (x as f32 / w as f32);
                let (r, g, b) = if (60..200).contains(&y) {
                    (40.0, 90.0, 190.0)
                } else if (400..412).contains(&y) && x % 9 != 0 {
                    (30.0, 30.0, 30.0)
                } else {
                    (245.0, 240.0, 235.0)
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                pixels.extend_from_slice(&[
                    (r * light) as u8,
                    (g * light * 0.92) as u8,
                    (b * light * 0.72) as u8,
                    255,
                ]);
            }
        }
        (
            Rgba {
                width: w,
                height: h,
                pixels,
            },
            w,
            h,
        )
    }

    #[test]
    fn a_broad_band_of_colour_keeps_its_colour_and_the_paper_comes_out_white() {
        let (picture, w, _) = letterhead();
        let page = enhance(&picture);
        let at = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            (page.pixels[i], page.pixels[i + 1], page.pixels[i + 2])
        };
        for x in [40, 300, 560] {
            let (r, g, b) = at(x, 300);
            assert!(r > 230 && g > 230 && b > 230, "paper at {x}: {r} {g} {b}");
            assert!(r.abs_diff(b) < 14, "paper grey at {x}: {r} {g} {b}");
            let (r, g, b) = at(x, 130);
            assert!(b > 150 && r < 90 && b > g + 50, "band at {x}: {r} {g} {b}");
            let (r, g, b) = at(x + 4, 405);
            assert!(r < 80 && g < 80 && b < 80, "text at {x}: {r} {g} {b}");
        }
    }

    #[test]
    fn the_level_table_gives_what_the_power_gives() {
        let levels = InkLevels::new();
        let edges = levels
            .starts
            .iter()
            .flat_map(|t| [f32::from_bits(t.to_bits() - 1), *t]);
        let spread = (0..=1.0_f32.to_bits()).step_by(1021).map(f32::from_bits);
        for x in edges.chain(spread).chain([0.0, -0.0, 1.0]) {
            assert_eq!(levels.of(x), ink_curve(x), "at {x}");
        }
        let mut wrong = InkLevels::new();
        wrong.starts[100] = f32::from_bits(wrong.starts[100].to_bits() + 1);
        let x = levels.starts[100];
        assert_ne!(wrong.of(x), ink_curve(x));
    }

    #[test]
    fn the_blur_down_bands_of_columns_adds_as_one_column_at_a_time() {
        let plain = |values: &[f32], w: usize, h: usize, radius: usize| {
            let pass = |source: &[f32], across: bool| -> Vec<f32> {
                let mut out = vec![0.0_f32; source.len()];
                let (lines, length) = if across { (h, w) } else { (w, h) };
                let mut prefix = vec![0.0_f64; length + 1];
                for line in 0..lines {
                    let at = |i: usize| if across { line * w + i } else { i * w + line };
                    for i in 0..length {
                        prefix[i + 1] = prefix[i] + f64::from(source[at(i)]);
                    }
                    for i in 0..length {
                        let (from, to) = (i.saturating_sub(radius), (i + radius).min(length - 1));
                        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                        {
                            out[at(i)] =
                                ((prefix[to + 1] - prefix[from]) / (to - from + 1) as f64) as f32;
                        }
                    }
                }
                out
            };
            pass(&pass(values, true), false)
        };
        for (w, h, radius) in [(150, 41, 1), (150, 41, 7), (3, 90, 2), (1, 1, 1)] {
            #[allow(clippy::cast_precision_loss)]
            let values: Vec<f32> = (0..w * h)
                .map(|i| ((i * 7919) % 257) as f32 * 0.37 + 0.001 * i as f32)
                .collect();
            let (want, got) = (
                plain(&values, w, h, radius),
                box_blur_f32(&values, w, h, radius),
            );
            assert!(
                want.iter()
                    .zip(&got)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "{w} x {h}, radius {radius}"
            );
        }
    }

    #[test]
    fn nearest_ends_as_the_same_byte_as_round() {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = |v: f64| v.clamp(0.0, 255.0) as u8;
        let halves = (-4..=520).map(|i| f64::from(i) / 2.0);
        let near_halves = halves.clone().flat_map(|h| [h.next_down(), h.next_up()]);
        let spread = (0..200_000).map(|i| f64::from(i) * 0.001_3 - 3.0);
        for v in halves.chain(near_halves).chain(spread) {
            assert_eq!(byte(nearest(v)), byte(v.round()), "at {v}");
        }
        assert_ne!(byte(2.5_f64.round_ties_even()), byte(nearest(2.5)));
    }
}
