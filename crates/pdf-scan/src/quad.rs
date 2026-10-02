use crate::{Corners, Rgba};

const SIDE: usize = 256;
const MAP: usize = 64;

pub const SURE: f32 = 0.5;

#[derive(Clone, Debug)]
struct Tensor {
    c: usize,
    h: usize,
    w: usize,
    data: Vec<f32>,
}

impl Tensor {
    fn plane(&self) -> usize {
        self.h * self.w
    }
}

#[derive(Clone, Debug)]
enum Step {
    Conv {
        input: usize,
        out: usize,
        out_c: usize,
        in_per_group: usize,
        kernel: usize,
        stride: usize,
        pad: usize,
        groups: usize,
        weights: Vec<f32>,
        bias: Vec<f32>,
    },
    Relu {
        input: usize,
        out: usize,
    },
    HardSigmoid {
        input: usize,
        out: usize,
        alpha: f32,
        beta: f32,
    },
    Mul {
        a: usize,
        b: usize,
        out: usize,
    },
    Add {
        a: usize,
        b: usize,
        out: usize,
    },
    Mean {
        input: usize,
        out: usize,
    },
    Resize {
        input: usize,
        out: usize,
        h: usize,
        w: usize,
    },
}

impl Step {
    fn inputs(&self) -> Vec<usize> {
        match self {
            Self::Conv { input, .. }
            | Self::Relu { input, .. }
            | Self::HardSigmoid { input, .. }
            | Self::Mean { input, .. }
            | Self::Resize { input, .. } => vec![*input],
            Self::Mul { a, b, .. } | Self::Add { a, b, .. } => vec![*a, *b],
        }
    }

    fn output(&self) -> usize {
        match self {
            Self::Conv { out, .. }
            | Self::Relu { out, .. }
            | Self::HardSigmoid { out, .. }
            | Self::Mul { out, .. }
            | Self::Add { out, .. }
            | Self::Mean { out, .. }
            | Self::Resize { out, .. } => *out,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DocQuad {
    slots: usize,
    input: usize,
    heat: usize,
    mask: usize,
    steps: Vec<Step>,
    last_read: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub corners: Corners,
    pub sureness: f32,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let piece = self.bytes.get(self.at..self.at + n)?;
        self.at += n;
        Some(piece)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<usize> {
        self.take(2)
            .map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    }
    fn u32(&mut self) -> Option<usize> {
        let b = self.take(4)?;
        usize::try_from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok()
    }
    fn f32(&mut self) -> Option<f32> {
        self.take(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn halves(&mut self, n: usize) -> Option<Vec<f32>> {
        Some(
            self.take(n * 2)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| half_to_f32(u16::from_le_bytes(*b)))
                .collect(),
        )
    }
}

fn half_to_f32(h: u16) -> f32 {
    let sign = u32::from(h >> 15) << 31;
    let exponent = u32::from((h >> 10) & 0x1f);
    let fraction = u32::from(h & 0x3ff);
    let bits = match exponent {
        0 if fraction == 0 => sign,
        0 => {
            let mut e = 127 - 15 + 1;
            let mut f = fraction;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((f & 0x3ff) << 13)
        }
        0x1f => sign | 0x7f80_0000 | (fraction << 13),
        _ => sign | ((exponent + 127 - 15) << 23) | (fraction << 13),
    };
    f32::from_bits(bits)
}

impl DocQuad {
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != b"PDQ1" {
            return None;
        }
        let (slots, input, heat, mask, count) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let mut steps = Vec::with_capacity(count);
        for _ in 0..count {
            let step = match r.u8()? {
                1 => {
                    let (input, out) = (r.u32()?, r.u32()?);
                    let (out_c, in_per_group) = (r.u16()?, r.u16()?);
                    let (kernel, stride, pad) = (
                        usize::from(r.u8()?),
                        usize::from(r.u8()?),
                        usize::from(r.u8()?),
                    );
                    let groups = r.u16()?;
                    let weights = r.halves(out_c * in_per_group * kernel * kernel)?;
                    let bias = r.halves(out_c)?;
                    if groups == 0 || stride == 0 || out_c % groups != 0 {
                        return None;
                    }
                    Step::Conv {
                        input,
                        out,
                        out_c,
                        in_per_group,
                        kernel,
                        stride,
                        pad,
                        groups,
                        weights,
                        bias,
                    }
                }
                2 => Step::Relu {
                    input: r.u32()?,
                    out: r.u32()?,
                },
                3 => Step::HardSigmoid {
                    input: r.u32()?,
                    out: r.u32()?,
                    alpha: r.f32()?,
                    beta: r.f32()?,
                },
                4 => Step::Mul {
                    a: r.u32()?,
                    b: r.u32()?,
                    out: r.u32()?,
                },
                5 => Step::Add {
                    a: r.u32()?,
                    b: r.u32()?,
                    out: r.u32()?,
                },
                6 => Step::Mean {
                    input: r.u32()?,
                    out: r.u32()?,
                },
                7 => Step::Resize {
                    input: r.u32()?,
                    out: r.u32()?,
                    h: r.u16()?,
                    w: r.u16()?,
                },
                _ => return None,
            };
            if step
                .inputs()
                .iter()
                .chain([&step.output()])
                .any(|&s| s >= slots)
            {
                return None;
            }
            steps.push(step);
        }
        let mut last_read = vec![0; slots];
        for (index, step) in steps.iter().enumerate() {
            for slot in step.inputs() {
                last_read[slot] = index;
            }
        }
        (input < slots && heat < slots && mask < slots).then_some(Self {
            slots,
            input,
            heat,
            mask,
            steps,
            last_read,
        })
    }

    fn run(&self, picture: Tensor) -> Option<(Tensor, Tensor)> {
        let mut slots: Vec<Option<Tensor>> = vec![None; self.slots];
        slots[self.input] = Some(picture);
        for (index, step) in self.steps.iter().enumerate() {
            let value = {
                let get = |s: usize| slots[s].as_ref();
                match step {
                    Step::Conv {
                        input,
                        out_c,
                        in_per_group,
                        kernel,
                        stride,
                        pad,
                        groups,
                        weights,
                        bias,
                        ..
                    } => conv(
                        get(*input)?,
                        Shape {
                            out_c: *out_c,
                            in_per_group: *in_per_group,
                            kernel: *kernel,
                            stride: *stride,
                            pad: *pad,
                            groups: *groups,
                        },
                        weights,
                        bias,
                    )?,
                    Step::Relu { input, .. } => map(get(*input)?, |v| v.max(0.0)),
                    Step::HardSigmoid {
                        input, alpha, beta, ..
                    } => map(get(*input)?, |v| alpha.mul_add(v, *beta).clamp(0.0, 1.0)),
                    Step::Mul { a, b, .. } => combine(get(*a)?, get(*b)?, |x, y| x * y)?,
                    Step::Add { a, b, .. } => combine(get(*a)?, get(*b)?, |x, y| x + y)?,
                    Step::Mean { input, .. } => mean(get(*input)?),
                    Step::Resize { input, h, w, .. } => resize(get(*input)?, *h, *w),
                }
            };
            slots[step.output()] = Some(value);
            for slot in step.inputs() {
                if self.last_read[slot] == index && slot != self.heat && slot != self.mask {
                    slots[slot] = None;
                }
            }
        }
        Some((slots[self.heat].take()?, slots[self.mask].take()?))
    }

    #[must_use]
    pub fn look(&self, picture: &Rgba, turns: u8) -> Option<Found> {
        let turned = turn(picture, turns);
        let (input, scale, ox, oy) = letterbox(&turned);
        let (heat, _mask) = self.run(input)?;
        if heat.c != 4 || heat.h != MAP || heat.w != MAP {
            return None;
        }
        let mut corners = [(0.0_f32, 0.0_f32); 4];
        let mut sureness = 1.0_f32;
        for (c, corner) in corners.iter_mut().enumerate() {
            let map = &heat.data[c * MAP * MAP..(c + 1) * MAP * MAP];
            let (px, py, top) = peak(map);
            sureness = sureness.min(sigmoid(top));
            #[allow(clippy::cast_precision_loss)]
            let step = (SIDE / MAP) as f32;
            let (x, y) = (
                ((px + 0.5) * step - ox) / scale,
                ((py + 0.5) * step - oy) / scale,
            );
            *corner = unturn((x, y), (turned.width, turned.height), turns);
        }
        Some(Found {
            corners: clockwise(corners),
            sureness,
        })
    }

    #[must_use]
    pub fn look_all_ways(&self, picture: &Rgba) -> Option<Found> {
        (0..4)
            .filter_map(|turns| self.look(picture, turns))
            .max_by(|a, b| a.sureness.total_cmp(&b.sureness))
    }
}

fn sigmoid(v: f32) -> f32 {
    1.0 / (1.0 + (-v).exp())
}

fn peak(map: &[f32]) -> (f32, f32, f32) {
    let (best, &top) = map
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap_or((0, &0.0));
    let (bx, by) = (best % MAP, best / MAP);
    let probability = |v: f32| sigmoid(v);
    let floor = probability(top) * 0.5;
    let (mut sx, mut sy, mut sw) = (0.0_f32, 0.0_f32, 0.0_f32);
    for y in by.saturating_sub(3)..(by + 4).min(MAP) {
        for x in bx.saturating_sub(3)..(bx + 4).min(MAP) {
            let weight = (probability(map[y * MAP + x]) - floor).max(0.0);
            #[allow(clippy::cast_precision_loss)]
            {
                sx += weight * x as f32;
                sy += weight * y as f32;
            }
            sw += weight;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    if sw > 0.0 {
        (sx / sw, sy / sw, top)
    } else {
        (bx as f32, by as f32, top)
    }
}

fn clockwise(points: Corners) -> Corners {
    let pick = |score: &dyn Fn((f32, f32)) -> f32| {
        points
            .iter()
            .copied()
            .max_by(|a, b| score(*a).total_cmp(&score(*b)))
            .unwrap_or((0.0, 0.0))
    };
    [
        pick(&|(x, y)| -x - y),
        pick(&|(x, y)| x - y),
        pick(&|(x, y)| x + y),
        pick(&|(x, y)| y - x),
    ]
}

fn turn(picture: &Rgba, turns: u8) -> Rgba {
    let turns = turns % 4;
    if turns == 0 {
        return picture.clone();
    }
    let (w, h) = (picture.width, picture.height);
    let (nw, nh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    let mut pixels = vec![0_u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (tx, ty) = match turns {
                1 => (y, w - 1 - x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (h - 1 - y, x),
            };
            let from = (y * w + x) * 4;
            let to = (ty * nw + tx) * 4;
            pixels[to..to + 4].copy_from_slice(&picture.pixels[from..from + 4]);
        }
    }
    Rgba {
        width: nw,
        height: nh,
        pixels,
    }
}

fn unturn(point: (f32, f32), size: (usize, usize), turns: u8) -> (f32, f32) {
    let (mut x, mut y) = point;
    let (mut w, mut h) = size;
    for _ in 0..turns % 4 {
        #[allow(clippy::cast_precision_loss)]
        let (ox, oy) = (h as f32 - y, x);
        (x, y) = (ox, oy);
        (w, h) = (h, w);
    }
    (x, y)
}

fn letterbox(picture: &Rgba) -> (Tensor, f32, f32, f32) {
    let (w, h) = (picture.width.max(1), picture.height.max(1));
    #[allow(clippy::cast_precision_loss)]
    let scale = SIDE as f32 / w.max(h) as f32;
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let step = ((1.0 / scale) / 2.0).floor().max(1.0) as usize;
    let small = if step > 1 {
        picture.shrunk(step)
    } else {
        picture.clone()
    };
    #[allow(clippy::cast_precision_loss)]
    let s = scale * step as f32;
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let (nw, nh) = (
        ((w as f32 * scale).round() as usize).clamp(1, SIDE),
        ((h as f32 * scale).round() as usize).clamp(1, SIDE),
    );
    let (ox, oy) = ((SIDE - nw) / 2, (SIDE - nh) / 2);
    let mut data = vec![0.0_f32; 3 * SIDE * SIDE];
    let (sw, sh) = (small.width, small.height);
    for y in 0..nh {
        #[allow(clippy::cast_precision_loss)]
        let fy = ((y as f32 + 0.5) / s - 0.5).clamp(0.0, (sh - 1) as f32);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let y0 = fy as usize;
        let (y1, ty) = ((y0 + 1).min(sh - 1), fy - fy.floor());
        for x in 0..nw {
            #[allow(clippy::cast_precision_loss)]
            let fx = ((x as f32 + 0.5) / s - 0.5).clamp(0.0, (sw - 1) as f32);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let x0 = fx as usize;
            let (x1, tx) = ((x0 + 1).min(sw - 1), fx - fx.floor());
            for c in 0..3 {
                let at = |x: usize, y: usize| f32::from(small.pixels[(y * sw + x) * 4 + c]);
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let low = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                data[c * SIDE * SIDE + (y + oy) * SIDE + x + ox] =
                    (top * (1.0 - ty) + low * ty) / 255.0;
            }
        }
    }
    #[allow(clippy::cast_precision_loss)]
    (
        Tensor {
            c: 3,
            h: SIDE,
            w: SIDE,
            data,
        },
        scale,
        ox as f32,
        oy as f32,
    )
}

#[derive(Clone, Copy)]
struct Shape {
    out_c: usize,
    in_per_group: usize,
    kernel: usize,
    stride: usize,
    pad: usize,
    groups: usize,
}

#[allow(
    clippy::many_single_char_names,
    clippy::too_many_lines,
    reason = "one kernel's three ways of walking the same sums, kept side by side"
)]
fn conv(input: &Tensor, shape: Shape, weights: &[f32], bias: &[f32]) -> Option<Tensor> {
    let Shape {
        out_c,
        in_per_group,
        kernel,
        stride,
        pad,
        groups,
    } = shape;
    if input.c != in_per_group * groups {
        return None;
    }
    let oh = (input.h + 2 * pad).checked_sub(kernel)? / stride + 1;
    let ow = (input.w + 2 * pad).checked_sub(kernel)? / stride + 1;
    let plane = oh * ow;
    let mut data = vec![0.0_f32; out_c * plane];
    let per_group = out_c / groups;
    let one_channel = |o: usize, out: &mut [f32]| {
        out.fill(bias[o]);
        let g = o / per_group;
        let w =
            &weights[o * in_per_group * kernel * kernel..(o + 1) * in_per_group * kernel * kernel];
        if kernel == 1 && stride == 1 && pad == 0 {
            let first = g * in_per_group;
            let mut c = 0;
            while c + 4 <= in_per_group {
                let (w0, w1, w2, w3) = (w[c], w[c + 1], w[c + 2], w[c + 3]);
                let at =
                    |k: usize| &input.data[(first + c + k) * plane..(first + c + k + 1) * plane];
                let four = at(0).iter().zip(at(1)).zip(at(2)).zip(at(3));
                for (v, (((a, b), cc), d)) in out.iter_mut().zip(four) {
                    *v += w0 * a + w1 * b + w2 * cc + w3 * d;
                }
                c += 4;
            }
            for (c, &wv) in w.iter().enumerate().take(in_per_group).skip(c) {
                let src = &input.data[(first + c) * plane..(first + c + 1) * plane];
                for (v, s) in out.iter_mut().zip(src) {
                    *v += wv * s;
                }
            }
            return;
        }
        for c in 0..in_per_group {
            let src = &input.data[(g * in_per_group + c) * input.plane()
                ..(g * in_per_group + c + 1) * input.plane()];
            for ky in 0..kernel {
                for kx in 0..kernel {
                    let wv = w[(c * kernel + ky) * kernel + kx];
                    for y in 0..oh {
                        let Some(iy) = (y * stride + ky)
                            .checked_sub(pad)
                            .filter(|&iy| iy < input.h)
                        else {
                            continue;
                        };
                        let row = &src[iy * input.w..(iy + 1) * input.w];
                        let dst = &mut out[y * ow..(y + 1) * ow];
                        let first = pad.saturating_sub(kx).div_ceil(stride);
                        let last = ((input.w + pad).saturating_sub(kx + 1) / stride + 1).min(ow);
                        if stride == 1 {
                            let from = first + kx - pad;
                            for (v, s) in dst[first..last].iter_mut().zip(&row[from..]) {
                                *v += wv * s;
                            }
                        } else {
                            for (x, v) in dst.iter_mut().enumerate().take(last).skip(first) {
                                *v += wv * row[x * stride + kx - pad];
                            }
                        }
                    }
                }
            }
        }
    };
    let four_channels = |o: usize, outs: &mut [f32]| {
        let (o0, rest) = outs.split_at_mut(plane);
        let (o1, rest) = rest.split_at_mut(plane);
        let (o2, o3) = rest.split_at_mut(plane);
        for (k, out) in [&mut *o0, &mut *o1, &mut *o2, &mut *o3]
            .into_iter()
            .enumerate()
        {
            out.fill(bias[o + k]);
        }
        let size = in_per_group * kernel * kernel;
        let w = |k: usize| &weights[(o + k) * size..(o + k + 1) * size];
        let (wa, wb, wc, wd) = (w(0), w(1), w(2), w(3));
        for c in 0..in_per_group {
            let src = &input.data[c * input.plane()..(c + 1) * input.plane()];
            for ky in 0..kernel {
                for kx in 0..kernel {
                    let at = (c * kernel + ky) * kernel + kx;
                    let (w0, w1, w2, w3) = (wa[at], wb[at], wc[at], wd[at]);
                    let first = pad.saturating_sub(kx);
                    let last = ((input.w + pad).saturating_sub(kx + 1) + 1).min(ow);
                    if first >= last {
                        continue;
                    }
                    let from = first + kx - pad;
                    for y in 0..oh {
                        let Some(iy) = (y + ky).checked_sub(pad).filter(|&iy| iy < input.h) else {
                            continue;
                        };
                        let row = &src[iy * input.w + from..iy * input.w + from + (last - first)];
                        let span = y * ow + first..y * ow + last;
                        let four = o0[span.clone()]
                            .iter_mut()
                            .zip(&mut o1[span.clone()])
                            .zip(&mut o2[span.clone()])
                            .zip(&mut o3[span]);
                        for ((((a, b), cc), d), s) in four.zip(row) {
                            *a += w0 * s;
                            *b += w1 * s;
                            *cc += w2 * s;
                            *d += w3 * s;
                        }
                    }
                }
            }
        }
    };
    let blocked = groups == 1 && stride == 1 && kernel > 1;
    let three = blocked && kernel == 3 && pad == 1 && oh == input.h && ow == input.w;
    let channels = |o: usize, part: &mut [f32]| {
        let mut done = 0;
        if blocked {
            for outs in part.chunks_exact_mut(4 * plane) {
                if three {
                    let size = in_per_group * 9;
                    let w = |k: usize| &weights[(o + done + k) * size..(o + done + k + 1) * size];
                    let b = |k: usize| bias[o + done + k];
                    three_by_three(
                        input,
                        [w(0), w(1), w(2), w(3)],
                        [b(0), b(1), b(2), b(3)],
                        outs,
                    );
                } else {
                    four_channels(o + done, outs);
                }
                done += 4;
            }
        }
        for (i, out) in part[done * plane..].chunks_mut(plane).enumerate() {
            one_channel(o + done + i, out);
        }
    };
    let threads = cores().min(out_c).max(1);
    if threads == 1 || out_c * plane * in_per_group * kernel * kernel < 1 << 18 {
        channels(0, &mut data);
    } else {
        let per_thread = out_c.div_ceil(threads).next_multiple_of(4);
        std::thread::scope(|scope| {
            for (t, part) in data.chunks_mut(per_thread * plane).enumerate() {
                let channels = &channels;
                scope.spawn(move || channels(t * per_thread, part));
            }
        });
    }
    Some(Tensor {
        c: out_c,
        h: oh,
        w: ow,
        data,
    })
}

fn three_by_three(input: &Tensor, weights: [&[f32]; 4], bias: [f32; 4], outs: &mut [f32]) {
    let (w, h) = (input.w, input.h);
    let plane = w * h;
    let (o0, rest) = outs.split_at_mut(plane);
    let (o1, rest) = rest.split_at_mut(plane);
    let (o2, o3) = rest.split_at_mut(plane);
    let mut outs = [o0, o1, o2, o3];
    for (out, b) in outs.iter_mut().zip(bias) {
        out.fill(b);
    }
    for c in 0..input.c {
        let src = &input.data[c * plane..(c + 1) * plane];
        let taps: [[f32; 9]; 4] =
            std::array::from_fn(|k| std::array::from_fn(|t| weights[k][c * 9 + t]));
        for y in 0..h {
            let row = |ky: usize| {
                (y + ky)
                    .checked_sub(1)
                    .filter(|&iy| iy < h)
                    .map(|iy| &src[iy * w..(iy + 1) * w])
            };
            let rows = [row(0), row(1), row(2)];
            let line = y * w;
            let edge = |outs: &mut [&mut [f32]; 4], x: usize| {
                for (out, taps) in outs.iter_mut().zip(&taps) {
                    let mut v = out[line + x];
                    for (ky, r) in rows.iter().enumerate() {
                        let Some(r) = r else { continue };
                        for kx in 0..3 {
                            if let Some(s) = (x + kx).checked_sub(1).and_then(|ix| r.get(ix)) {
                                v += taps[ky * 3 + kx] * s;
                            }
                        }
                    }
                    out[line + x] = v;
                }
            };
            let [Some(r0), Some(r1), Some(r2)] = rows else {
                for x in 0..w {
                    edge(&mut outs, x);
                }
                continue;
            };
            if w < 3 {
                for x in 0..w {
                    edge(&mut outs, x);
                }
                continue;
            }
            edge(&mut outs, 0);
            for (out, k) in outs.iter_mut().zip(&taps) {
                let windows = r0.windows(3).zip(r1.windows(3)).zip(r2.windows(3));
                for (v, ((t, m), l)) in out[line + 1..line + w - 1].iter_mut().zip(windows) {
                    *v = *v
                        + k[0] * t[0]
                        + k[1] * t[1]
                        + k[2] * t[2]
                        + k[3] * m[0]
                        + k[4] * m[1]
                        + k[5] * m[2]
                        + k[6] * l[0]
                        + k[7] * l[1]
                        + k[8] * l[2];
                }
            }
            edge(&mut outs, w - 1);
        }
    }
}

fn cores() -> usize {
    if cfg!(target_arch = "wasm32") {
        return 1;
    }
    std::thread::available_parallelism().map_or(1, |n| n.get().min(4))
}

fn map(input: &Tensor, f: impl Fn(f32) -> f32) -> Tensor {
    Tensor {
        c: input.c,
        h: input.h,
        w: input.w,
        data: input.data.iter().map(|&v| f(v)).collect(),
    }
}

fn combine(a: &Tensor, b: &Tensor, f: impl Fn(f32, f32) -> f32) -> Option<Tensor> {
    if a.c != b.c {
        return None;
    }
    let (big, small, swapped) = if a.plane() >= b.plane() {
        (a, b, false)
    } else {
        (b, a, true)
    };
    let plane = big.plane();
    let data = if small.plane() == plane {
        a.data.iter().zip(&b.data).map(|(&x, &y)| f(x, y)).collect()
    } else if small.plane() == 1 {
        big.data
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let s = small.data[i / plane];
                if swapped { f(s, v) } else { f(v, s) }
            })
            .collect()
    } else {
        return None;
    };
    Some(Tensor {
        c: big.c,
        h: big.h,
        w: big.w,
        data,
    })
}

fn mean(input: &Tensor) -> Tensor {
    let plane = input.plane().max(1);
    #[allow(clippy::cast_precision_loss)]
    let data = input
        .data
        .chunks(plane)
        .map(|channel| channel.iter().sum::<f32>() / plane as f32)
        .collect();
    Tensor {
        c: input.c,
        h: 1,
        w: 1,
        data,
    }
}

fn resize(input: &Tensor, h: usize, w: usize) -> Tensor {
    #[allow(clippy::cast_precision_loss)]
    let source = |o: usize, size_in: usize, size_out: usize| {
        let at = ((o as f32 + 0.5) * size_in as f32 / size_out as f32 - 0.5)
            .clamp(0.0, (size_in - 1) as f32);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i0 = at as usize;
        (i0, (i0 + 1).min(size_in - 1), at - at.floor())
    };
    let ys: Vec<_> = (0..h).map(|y| source(y, input.h, h)).collect();
    let xs: Vec<_> = (0..w).map(|x| source(x, input.w, w)).collect();
    let mut data = Vec::with_capacity(input.c * h * w);
    for channel in input.data.chunks(input.plane()) {
        for &(y0, y1, ty) in &ys {
            for &(x0, x1, tx) in &xs {
                let at = |x: usize, y: usize| channel[y * input.w + x];
                let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
                let low = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
                data.push(top * (1.0 - ty) + low * ty);
            }
        }
    }
    Tensor {
        c: input.c,
        h,
        w,
        data,
    }
}

static CARRIED: &[u8] = include_bytes!("../models/docquad.bin");

#[must_use]
pub fn carried() -> Option<&'static DocQuad> {
    static NET: std::sync::OnceLock<Option<DocQuad>> = std::sync::OnceLock::new();
    NET.get_or_init(|| DocQuad::read(CARRIED)).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_precision_reads_back() {
        assert!((half_to_f32(0x3c00) - 1.0).abs() < 1e-7);
        assert!((half_to_f32(0xc000) + 2.0).abs() < 1e-7);
        assert!((half_to_f32(0x3555) - 0.333_251_95).abs() < 1e-6);
        assert!(half_to_f32(0x0001) > 0.0 && half_to_f32(0x0001) < 1e-7);
    }

    #[test]
    fn a_point_turned_and_turned_back_is_where_it_was() {
        let picture = Rgba {
            width: 6,
            height: 4,
            pixels: (0..6 * 4 * 4)
                .map(|v| u8::try_from(v % 251).unwrap_or(0))
                .collect(),
        };
        for turns in 0..4_u8 {
            let turned = turn(&picture, turns);
            let want = &picture.pixels[(6 + 4) * 4..(6 + 4) * 4 + 4];
            let at = (0..turned.width * turned.height)
                .find(|&i| &turned.pixels[i * 4..i * 4 + 4] == want)
                .expect("the pixel");
            #[allow(clippy::cast_precision_loss)]
            let (x, y) = unturn(
                (
                    (at % turned.width) as f32 + 0.5,
                    (at / turned.width) as f32 + 0.5,
                ),
                (turned.width, turned.height),
                turns,
            );
            assert!(
                (x - 4.5).abs() < 1e-4 && (y - 1.5).abs() < 1e-4,
                "{turns}: {x} {y}"
            );
        }
    }

    #[allow(clippy::many_single_char_names)]
    fn photo(corners: Corners) -> Rgba {
        let (w, h) = (720, 960);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                #[allow(clippy::cast_precision_loss)]
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let inside = (0..4).all(|i| {
                    let (a, b) = (corners[i], corners[(i + 1) % 4]);
                    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) >= 0.0
                });
                let texture = u8::try_from((x * 7 + y * 3) % 23).unwrap_or(0);
                let px: [u8; 4] = if inside {
                    if y % 40 < 6 && x % 13 > 3 {
                        [40, 40, 40, 255]
                    } else {
                        [236, 236, 232, 255]
                    }
                } else {
                    [90 + texture, 55 + texture, 30, 255]
                };
                pixels.extend_from_slice(&px);
            }
        }
        Rgba {
            width: w,
            height: h,
            pixels,
        }
    }

    #[test]
    fn the_carried_network_finds_a_sheet_on_a_table() {
        let net = carried().expect("the carried network reads");
        let truth: Corners = [
            (150.0, 170.0),
            (590.0, 140.0),
            (620.0, 800.0),
            (120.0, 830.0),
        ];
        let found = net.look_all_ways(&photo(truth)).expect("an answer");
        assert!(found.sureness > SURE, "sureness {}", found.sureness);
        for (got, want) in found.corners.iter().zip(truth) {
            let miss = (got.0 - want.0).hypot(got.1 - want.1);
            assert!(miss < 24.0, "{got:?} vs {want:?}: {miss}");
        }
    }
}
