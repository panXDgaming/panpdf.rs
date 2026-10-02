use crate::{Corners, Rgba};

pub const NET_WIDTH: usize = 192;
pub const NET_HEIGHT: usize = 256;

#[derive(Clone, Debug)]
struct Conv {
    out: usize,
    inputs: usize,
    kernel: usize,
    weights: Vec<f32>,
    biases: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct SheetNet {
    layers: Vec<Conv>,
}

#[derive(Clone, Debug)]
struct Planes {
    channels: usize,
    width: usize,
    height: usize,
    values: Vec<f32>,
}

impl SheetNet {
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let mut at = 0;
        let mut take = |n: usize| {
            let piece = bytes.get(at..at + n)?;
            at += n;
            Some(piece)
        };
        if take(4)? != b"PSN1" {
            return None;
        }
        let word = |b: &[u8]| usize::try_from(u32::from_le_bytes(b.try_into().ok()?)).ok();
        let count = word(take(4)?)?;
        let mut layers = Vec::with_capacity(count);
        for _ in 0..count {
            let out = word(take(4)?)?;
            let inputs = word(take(4)?)?;
            let kernel = word(take(4)?)?;
            let floats = |b: &[u8]| -> Vec<f32> {
                b.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes(*c))
                    .collect()
            };
            let weights = floats(take(out * inputs * kernel * kernel * 4)?);
            let biases = floats(take(out * 4)?);
            layers.push(Conv {
                out,
                inputs,
                kernel,
                weights,
                biases,
            });
        }
        (layers.len() == 12).then_some(Self { layers })
    }

    #[must_use]
    pub fn sheet_chance(&self, picture: &Rgba) -> Vec<f32> {
        let input = Planes::of(&shrink_to(picture, NET_WIDTH, NET_HEIGHT));
        let l = &self.layers;
        let s1 = l[1].run(&l[0].run(&input, true), true);
        let s2 = l[3].run(&l[2].run(&s1.pooled(), true), true);
        let s3 = l[5].run(&l[4].run(&s2.pooled(), true), true);
        let b = l[7].run(&l[6].run(&s3.pooled(), true), true);
        let d3 = l[8].run(&b.doubled().joined(&s3), true);
        let d2 = l[9].run(&d3.doubled().joined(&s2), true);
        l[10].run(&d2.doubled().joined(&s1), true).values
    }
}

impl Conv {
    fn run(&self, input: &Planes, relu: bool) -> Planes {
        let (w, h) = (input.width, input.height);
        #[allow(clippy::cast_possible_wrap)]
        let (wi, hi, half) = (w as isize, h as isize, (self.kernel / 2) as isize);
        let mut values = vec![0.0_f32; self.out * w * h];
        for o in 0..self.out {
            let plane = &mut values[o * w * h..(o + 1) * w * h];
            plane.fill(self.biases[o]);
            for i in 0..self.inputs.min(input.channels) {
                let source = &input.values[i * w * h..(i + 1) * w * h];
                for ky in 0..self.kernel {
                    for kx in 0..self.kernel {
                        let weight = self.weights
                            [((o * self.inputs + i) * self.kernel + ky) * self.kernel + kx];
                        #[allow(clippy::cast_possible_wrap)]
                        let (dy, dx) = (ky as isize - half, kx as isize - half);
                        let from = usize::try_from((-dx).max(0)).unwrap_or(0);
                        let to = usize::try_from((wi - dx).min(wi)).unwrap_or(0);
                        for y in 0..hi {
                            let sy = y + dy;
                            if sy < 0 || sy >= hi {
                                continue;
                            }
                            let (Ok(y), Ok(sy)) = (usize::try_from(y), usize::try_from(sy)) else {
                                continue;
                            };
                            let row = &source[sy * w..(sy + 1) * w];
                            let out = &mut plane[y * w..(y + 1) * w];
                            #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
                            let shifted = (from as isize + dx) as usize;
                            for (value, source) in out[from..to].iter_mut().zip(&row[shifted..]) {
                                *value += weight * source;
                            }
                        }
                    }
                }
            }
            if relu {
                for v in plane.iter_mut() {
                    *v = v.max(0.0);
                }
            }
        }
        Planes {
            channels: self.out,
            width: w,
            height: h,
            values,
        }
    }
}

impl Planes {
    fn of(picture: &Rgba) -> Self {
        let (w, h) = (picture.width, picture.height);
        let mut values = vec![0.0_f32; 3 * w * h];
        for (at, p) in picture.pixels.as_chunks::<4>().0.iter().enumerate() {
            for c in 0..3 {
                values[c * w * h + at] = f32::from(p[c]) / 255.0;
            }
        }
        Self {
            channels: 3,
            width: w,
            height: h,
            values,
        }
    }

    fn pooled(&self) -> Self {
        let (w, h) = (self.width / 2, self.height / 2);
        let mut values = vec![0.0_f32; self.channels * w * h];
        for c in 0..self.channels {
            let source = &self.values[c * self.width * self.height..];
            for y in 0..h {
                for x in 0..w {
                    let at = |dx: usize, dy: usize| source[(2 * y + dy) * self.width + 2 * x + dx];
                    values[c * w * h + y * w + x] =
                        at(0, 0).max(at(1, 0)).max(at(0, 1)).max(at(1, 1));
                }
            }
        }
        Self {
            channels: self.channels,
            width: w,
            height: h,
            values,
        }
    }

    fn doubled(&self) -> Self {
        let (w, h) = (self.width * 2, self.height * 2);
        let mut values = vec![0.0_f32; self.channels * w * h];
        for c in 0..self.channels {
            for y in 0..h {
                for x in 0..w {
                    values[c * w * h + y * w + x] =
                        self.values[c * self.width * self.height + (y / 2) * self.width + x / 2];
                }
            }
        }
        Self {
            channels: self.channels,
            width: w,
            height: h,
            values,
        }
    }

    fn joined(&self, other: &Self) -> Self {
        let mut values = self.values.clone();
        values.extend_from_slice(&other.values);
        Self {
            channels: self.channels + other.channels,
            width: self.width,
            height: self.height,
            values,
        }
    }
}

fn shrink_to(picture: &Rgba, w: usize, h: usize) -> Rgba {
    let mut pixels = Vec::with_capacity(w * h * 4);
    #[allow(clippy::cast_precision_loss)]
    let (sx, sy) = (
        picture.width as f32 / w as f32,
        picture.height as f32 / h as f32,
    );
    for y in 0..h {
        for x in 0..w {
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let (px, py) = (
                (((x as f32 + 0.5) * sx) as usize).min(picture.width - 1),
                (((y as f32 + 0.5) * sy) as usize).min(picture.height - 1),
            );
            let at = (py * picture.width + px) * 4;
            pixels.extend_from_slice(&picture.pixels[at..at + 4]);
        }
    }
    Rgba {
        width: w,
        height: h,
        pixels,
    }
}

#[must_use]
pub fn find_sheet_with(net: &SheetNet, picture: &Rgba) -> Option<Corners> {
    if picture.width < 8 || picture.height < 8 {
        return None;
    }
    let features = net.sheet_chance(picture);
    let head = net.layers.last()?;
    let (w, h) = (NET_WIDTH, NET_HEIGHT);
    let light: Vec<bool> = (0..w * h)
        .map(|at| {
            let mut logit = head.biases[0];
            for c in 0..head.inputs {
                logit += head.weights[c] * features[c * w * h + at];
            }
            logit > 0.0
        })
        .collect();
    let corners = crate::find::sheet_from_region(&light, w, h)?;
    #[allow(clippy::cast_precision_loss)]
    let (sx, sy) = (
        picture.width as f64 / w as f64,
        picture.height as f64 / h as f64,
    );
    #[allow(clippy::cast_possible_truncation)]
    Some(corners.map(|(x, y)| (((x + 0.5) * sx) as f32, ((y + 0.5) * sy) as f32)))
}

static CARRIED: &[u8] = include_bytes!("../models/sheet-net.bin");

fn carried() -> Option<&'static SheetNet> {
    static NET: std::sync::OnceLock<Option<SheetNet>> = std::sync::OnceLock::new();
    NET.get_or_init(|| SheetNet::read(CARRIED)).as_ref()
}

#[must_use]
pub fn find_sheet_best(picture: &Rgba) -> Option<Corners> {
    carried()
        .and_then(|net| find_sheet_with(net, picture))
        .or_else(|| crate::find_sheet_in_colour(picture))
}
