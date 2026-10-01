use std::f32::consts::PI;

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const LUMA_QUANT: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];

const CHROMA_QUANT: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];

const DC_LUMA_BITS: [u8; 16] = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
const DC_LUMA_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const DC_CHROMA_BITS: [u8; 16] = [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
const DC_CHROMA_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const AC_LUMA_BITS: [u8; 16] = [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
const AC_LUMA_VALUES: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const AC_CHROMA_BITS: [u8; 16] = [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
const AC_CHROMA_VALUES: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];

struct Codes([(u16, u8); 256]);

impl Codes {
    fn new(bits: &[u8; 16], values: &[u8]) -> Self {
        let mut codes = [(0u16, 0u8); 256];
        let mut code: u16 = 0;
        let mut at = 0;
        for (length, &count) in bits.iter().enumerate() {
            for _ in 0..count {
                let length = u8::try_from(length + 1).unwrap_or(16);
                codes[usize::from(values[at])] = (code, length);
                code = code.wrapping_add(1);
                at += 1;
            }
            code <<= 1;
        }
        Self(codes)
    }
}

struct Bits {
    out: Vec<u8>,
    buffer: u32,
    count: u32,
}

impl Bits {
    fn put(&mut self, value: u16, length: u8) {
        if length == 0 {
            return;
        }
        let mask = (1u32 << length) - 1;
        self.buffer = (self.buffer << length) | (u32::from(value) & mask);
        self.count += u32::from(length);
        while self.count >= 8 {
            self.count -= 8;
            let byte = u8::try_from((self.buffer >> self.count) & 0xff).unwrap_or(0);
            self.out.push(byte);
            if byte == 0xff {
                self.out.push(0);
            }
        }
        self.buffer &= (1u32 << self.count) - 1;
    }

    fn flush(&mut self) {
        if self.count > 0 {
            let pad = u8::try_from(8 - self.count).unwrap_or(0);
            self.put((1u16 << pad) - 1, pad);
        }
    }
}

fn category(value: i32) -> (u8, u16) {
    let magnitude = value.unsigned_abs();
    let size = u8::try_from(32 - magnitude.leading_zeros()).unwrap_or(0);
    let bits = if value < 0 {
        value - 1 + (1 << size)
    } else {
        value
    };
    (size, u16::try_from(bits & 0xffff).unwrap_or(0))
}

fn scaled(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let quality = u32::from(quality.clamp(1, 100));
    let scale = if quality < 50 {
        5000 / quality
    } else {
        200 - 2 * quality
    };
    let mut table = [0u16; 64];
    for (out, &value) in table.iter_mut().zip(base) {
        let scaled = (u32::from(value) * scale + 50) / 100;
        *out = u16::try_from(scaled.clamp(1, 255)).unwrap_or(255);
    }
    table
}

fn cosines() -> [[f32; 8]; 8] {
    let mut table = [[0.0f32; 8]; 8];
    for (u, row) in table.iter_mut().enumerate() {
        let c = if u == 0 {
            std::f32::consts::FRAC_1_SQRT_2
        } else {
            1.0
        };
        for (x, value) in row.iter_mut().enumerate() {
            let angle = (2.0 * x as f32 + 1.0) * u as f32 * PI / 16.0;
            *value = 0.5 * c * angle.cos();
        }
    }
    table
}

struct Encoder {
    bits: Bits,
    cos: [[f32; 8]; 8],
    quant: [[f32; 64]; 2],
    dc: [Codes; 2],
    ac: [Codes; 2],
}

impl Encoder {
    fn block(&mut self, samples: &[f32; 64], table: usize, previous_dc: &mut i32) {
        let mut rows = [0.0f32; 64];
        for y in 0..8 {
            for u in 0..8 {
                let mut sum = 0.0;
                for x in 0..8 {
                    sum += self.cos[u][x] * samples[y * 8 + x];
                }
                rows[y * 8 + u] = sum;
            }
        }
        let mut coefficients = [0i32; 64];
        for u in 0..8 {
            for v in 0..8 {
                let mut sum = 0.0;
                for y in 0..8 {
                    sum += self.cos[v][y] * rows[y * 8 + u];
                }
                let at = v * 8 + u;
                {
                    coefficients[at] = (sum * self.quant[table][at]).round() as i32;
                }
            }
        }
        let dc = coefficients[0];
        let (size, value) = category(dc - *previous_dc);
        *previous_dc = dc;
        let (code, length) = self.dc[table].0[usize::from(size)];
        self.bits.put(code, length);
        self.bits.put(value, size);
        let mut run = 0u8;
        for &natural in &ZIGZAG[1..] {
            let coefficient = coefficients[natural];
            if coefficient == 0 {
                run += 1;
                continue;
            }
            while run >= 16 {
                let (code, length) = self.ac[table].0[0xf0];
                self.bits.put(code, length);
                run -= 16;
            }
            let (size, value) = category(coefficient);
            let (code, length) = self.ac[table].0[usize::from((run << 4) | size)];
            self.bits.put(code, length);
            self.bits.put(value, size);
            run = 0;
        }
        if run > 0 {
            let (code, length) = self.ac[table].0[0];
            self.bits.put(code, length);
        }
    }
}

fn marker(out: &mut Vec<u8>, kind: u8, body: &[u8]) {
    out.extend_from_slice(&[0xff, kind]);
    let length = u16::try_from(body.len() + 2).unwrap_or(u16::MAX);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
}

pub fn encode(
    width: u32,
    height: u32,
    channels: usize,
    pixels: &[u8],
    quality: u8,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > 65_535 || height > 65_535 {
        return Err(format!("a JPEG cannot be {width} by {height} pixels"));
    }
    if channels != 1 && channels != 3 {
        return Err(format!("a JPEG here has 1 or 3 channels, not {channels}"));
    }
    let (w, h) = (width as usize, height as usize);
    if pixels.len() < w * h * channels {
        return Err("fewer samples than the picture's size".to_owned());
    }
    let luma = scaled(&LUMA_QUANT, quality);
    let chroma = scaled(&CHROMA_QUANT, quality);
    let reciprocal = |table: &[u16; 64]| {
        let mut out = [0.0f32; 64];
        for (o, &q) in out.iter_mut().zip(table) {
            *o = 1.0 / f32::from(q);
        }
        out
    };
    let mut out = Vec::with_capacity(w * h * channels / 8 + 1024);
    out.extend_from_slice(&[0xff, 0xd8]);
    marker(
        &mut out,
        0xe0,
        &[b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0],
    );
    for (id, table) in [(0u8, &luma), (1u8, &chroma)]
        .into_iter()
        .take(if channels == 1 { 1 } else { 2 })
    {
        let mut body = vec![id];
        body.extend(
            ZIGZAG
                .iter()
                .map(|&n| u8::try_from(table[n]).unwrap_or(255)),
        );
        marker(&mut out, 0xdb, &body);
    }
    let [w_hi, w_lo] = u16::try_from(width).unwrap_or(0).to_be_bytes();
    let [h_hi, h_lo] = u16::try_from(height).unwrap_or(0).to_be_bytes();
    let mut frame = vec![8, h_hi, h_lo, w_hi, w_lo];
    if channels == 1 {
        frame.extend_from_slice(&[1, 1, 0x11, 0]);
    } else {
        frame.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
    }
    marker(&mut out, 0xc0, &frame);
    let tables: [(u8, &[u8; 16], &[u8]); 4] = [
        (0x00, &DC_LUMA_BITS, &DC_LUMA_VALUES),
        (0x10, &AC_LUMA_BITS, &AC_LUMA_VALUES),
        (0x01, &DC_CHROMA_BITS, &DC_CHROMA_VALUES),
        (0x11, &AC_CHROMA_BITS, &AC_CHROMA_VALUES),
    ];
    for (class, bits, values) in tables.iter().take(if channels == 1 { 2 } else { 4 }) {
        let mut body = vec![*class];
        body.extend_from_slice(*bits);
        body.extend_from_slice(values);
        marker(&mut out, 0xc4, &body);
    }
    if channels == 1 {
        marker(&mut out, 0xda, &[1, 1, 0x00, 0, 63, 0]);
    } else {
        marker(&mut out, 0xda, &[3, 1, 0x00, 2, 0x11, 3, 0x11, 0, 63, 0]);
    }
    let mut encoder = Encoder {
        bits: Bits {
            out,
            buffer: 0,
            count: 0,
        },
        cos: cosines(),
        quant: [reciprocal(&luma), reciprocal(&chroma)],
        dc: [
            Codes::new(&DC_LUMA_BITS, &DC_LUMA_VALUES),
            Codes::new(&DC_CHROMA_BITS, &DC_CHROMA_VALUES),
        ],
        ac: [
            Codes::new(&AC_LUMA_BITS, &AC_LUMA_VALUES),
            Codes::new(&AC_CHROMA_BITS, &AC_CHROMA_VALUES),
        ],
    };
    let at = |x: usize, y: usize| (y.min(h - 1) * w + x.min(w - 1)) * channels;
    let mut block = [0.0f32; 64];
    if channels == 1 {
        let mut dc = 0;
        for by in (0..h).step_by(8) {
            for bx in (0..w).step_by(8) {
                for y in 0..8 {
                    for x in 0..8 {
                        block[y * 8 + x] = f32::from(pixels[at(bx + x, by + y)]) - 128.0;
                    }
                }
                encoder.block(&block, 0, &mut dc);
            }
        }
    } else {
        let mut dc = [0i32; 3];
        let mut ys = [0.0f32; 256];
        let mut cb = [0.0f32; 256];
        let mut cr = [0.0f32; 256];
        for my in (0..h).step_by(16) {
            for mx in (0..w).step_by(16) {
                for y in 0..16 {
                    for x in 0..16 {
                        let p = at(mx + x, my + y);
                        let (r, g, b) = (
                            f32::from(pixels[p]),
                            f32::from(pixels[p + 1]),
                            f32::from(pixels[p + 2]),
                        );
                        ys[y * 16 + x] = 0.299 * r + 0.587 * g + 0.114 * b - 128.0;
                        cb[y * 16 + x] = -0.168_736 * r - 0.331_264 * g + 0.5 * b;
                        cr[y * 16 + x] = 0.5 * r - 0.418_688 * g - 0.081_312 * b;
                    }
                }
                for (ox, oy) in [(0, 0), (8, 0), (0, 8), (8, 8)] {
                    for y in 0..8 {
                        for x in 0..8 {
                            block[y * 8 + x] = ys[(oy + y) * 16 + ox + x];
                        }
                    }
                    encoder.block(&block, 0, &mut dc[0]);
                }
                for (plane, previous) in [(&cb, 1), (&cr, 2)] {
                    for y in 0..8 {
                        for x in 0..8 {
                            let s = 2 * y * 16 + 2 * x;
                            block[y * 8 + x] =
                                (plane[s] + plane[s + 1] + plane[s + 16] + plane[s + 17]) * 0.25;
                        }
                    }
                    encoder.block(&block, 1, &mut dc[previous]);
                }
            }
        }
    }
    encoder.bits.flush();
    let mut out = encoder.bits.out;
    out.extend_from_slice(&[0xff, 0xd9]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    u8::try_from(x * 255 / width).unwrap(),
                    u8::try_from(y * 255 / height).unwrap(),
                    128,
                ]);
            }
        }
        pixels
    }

    #[test]
    fn colour_round_trips_through_the_engines_decoder() {
        let (w, h) = (37, 21);
        let pixels = gradient(w, h);
        let jpeg = encode(w, h, 3, &pixels, 95).unwrap();
        let back = pdf_paint::picture::jpeg_pixels(&jpeg).expect("decoded");
        assert_eq!((back.width, back.height, back.channels), (w, h, 3));
        let error: u64 = pixels
            .iter()
            .zip(&back.samples)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        let mean = error as f64 / pixels.len() as f64;
        assert!(mean < 3.0, "mean error {mean}");
    }

    #[test]
    fn grey_round_trips() {
        let (w, h) = (64, 9);
        let pixels: Vec<u8> = (0..w * h).map(|i| u8::try_from(i % 251).unwrap()).collect();
        let jpeg = encode(w, h, 1, &pixels, 90).unwrap();
        let back = pdf_paint::picture::jpeg_pixels(&jpeg).expect("decoded");
        assert_eq!((back.width, back.height, back.channels), (w, h, 1));
    }

    #[test]
    fn lower_quality_is_smaller() {
        let pixels = gradient(200, 200);
        let high = encode(200, 200, 3, &pixels, 90).unwrap();
        let low = encode(200, 200, 3, &pixels, 30).unwrap();
        assert!(low.len() < high.len());
    }

    #[test]
    fn nonsense_is_refused() {
        assert!(encode(0, 5, 3, &[], 50).is_err());
        assert!(encode(2, 2, 2, &[0; 8], 50).is_err());
        assert!(encode(2, 2, 3, &[0; 5], 50).is_err());
    }
}
