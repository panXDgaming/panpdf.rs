#![forbid(unsafe_code)]

pub mod jpeg;
#[cfg(feature = "paint")]
pub mod paint;
pub mod scan;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub channels: usize,
    pub data: Vec<u8>,
}

impl Image {
    #[must_use]
    pub fn filled(width: u32, height: u32, channels: usize, value: u8) -> Self {
        Self {
            width,
            height,
            channels,
            data: vec![value; width as usize * height as usize * channels],
        }
    }

    #[must_use]
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize * 3);
        for pixel in rgba.as_chunks::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for &channel in &pixel[..3] {
                let over = (u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255;
                data.push(u8::try_from(over).unwrap_or(u8::MAX));
            }
        }
        Self {
            width,
            height,
            channels: 3,
            data,
        }
    }

    fn index(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * self.channels
    }

    #[must_use]
    pub fn is_grey(&self) -> bool {
        self.channels == 1
            || self
                .data
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| p[0].abs_diff(p[1]) <= 2 && p[1].abs_diff(p[2]) <= 2)
    }

    #[must_use]
    pub fn grey(&self) -> Self {
        if self.channels == 1 {
            return self.clone();
        }
        let data = self
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                let sum = 299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2]);
                u8::try_from((sum + 500) / 1000).unwrap_or(u8::MAX)
            })
            .collect();
        Self {
            width: self.width,
            height: self.height,
            channels: 1,
            data,
        }
    }

    #[must_use]
    pub fn resized(&self, width: u32, height: u32) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return self.clone();
        }
        if width > self.width || height > self.height {
            return self.bilinear(width, height);
        }
        let c = self.channels;
        let mut data = vec![0u8; width as usize * height as usize * c];
        let sx = f64::from(self.width) / f64::from(width);
        let sy = f64::from(self.height) / f64::from(height);
        let mut sums = vec![0u64; c];
        for y in 0..height {
            let y0 = to_u32((f64::from(y) * sy).floor());
            let y1 = to_u32((f64::from(y + 1) * sy).ceil()).clamp(y0 + 1, self.height);
            for x in 0..width {
                let x0 = to_u32((f64::from(x) * sx).floor());
                let x1 = to_u32((f64::from(x + 1) * sx).ceil()).clamp(x0 + 1, self.width);
                sums.iter_mut().for_each(|s| *s = 0);
                for yy in y0..y1 {
                    let row = self.index(x0, yy);
                    let end = self.index(x1 - 1, yy) + c;
                    for (i, &v) in self.data[row..end].iter().enumerate() {
                        sums[i % c] += u64::from(v);
                    }
                }
                let n = u64::from((x1 - x0) * (y1 - y0));
                let out = (y as usize * width as usize + x as usize) * c;
                for (i, s) in sums.iter().enumerate() {
                    data[out + i] = u8::try_from((s + n / 2) / n).unwrap_or(u8::MAX);
                }
            }
        }
        Self {
            width,
            height,
            channels: c,
            data,
        }
    }

    fn bilinear(&self, width: u32, height: u32) -> Self {
        let c = self.channels;
        let mut data = Vec::with_capacity(width as usize * height as usize * c);
        let sx = f64::from(self.width) / f64::from(width);
        let sy = f64::from(self.height) / f64::from(height);
        let mut pixel = [0u8; 3];
        for y in 0..height {
            for x in 0..width {
                let fx = (f64::from(x) + 0.5) * sx - 0.5;
                let fy = (f64::from(y) + 0.5) * sy - 0.5;
                self.sample(fx, fy, &mut pixel[..c], 255);
                data.extend_from_slice(&pixel[..c]);
            }
        }
        Self {
            width,
            height,
            channels: c,
            data,
        }
    }

    pub fn sample(&self, x: f64, y: f64, out: &mut [u8], outside: u8) {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        if !(-0.5..w - 0.5).contains(&x) || !(-0.5..h - 0.5).contains(&y) {
            out.iter_mut().for_each(|o| *o = outside);
            return;
        }
        let x = x.clamp(0.0, w - 1.0);
        let y = y.clamp(0.0, h - 1.0);
        let (x0, y0) = (to_u32(x.floor()), to_u32(y.floor()));
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - f64::from(x0), y - f64::from(y0));
        let (a, b, c, d) = (
            self.index(x0, y0),
            self.index(x1, y0),
            self.index(x0, y1),
            self.index(x1, y1),
        );
        for (i, o) in out.iter_mut().enumerate() {
            let top = f64::from(self.data[a + i]) * (1.0 - fx) + f64::from(self.data[b + i]) * fx;
            let bottom =
                f64::from(self.data[c + i]) * (1.0 - fx) + f64::from(self.data[d + i]) * fx;
            *o = to_u8(top * (1.0 - fy) + bottom * fy);
        }
    }

    #[must_use]
    pub fn cropped(&self, x: u32, y: u32, width: u32, height: u32) -> Self {
        let x = x.min(self.width - 1);
        let y = y.min(self.height - 1);
        let width = width.clamp(1, self.width - x);
        let height = height.clamp(1, self.height - y);
        let mut data = Vec::with_capacity(width as usize * height as usize * self.channels);
        for row in y..y + height {
            let start = self.index(x, row);
            data.extend_from_slice(&self.data[start..start + width as usize * self.channels]);
        }
        Self {
            width,
            height,
            channels: self.channels,
            data,
        }
    }

    #[must_use]
    pub fn turned(&self, degrees: f64, background: u8) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let cx = f64::from(self.width) / 2.0 - 0.5;
        let cy = f64::from(self.height) / 2.0 - 0.5;
        let c = self.channels;
        let mut data = Vec::with_capacity(self.data.len());
        let mut pixel = [0u8; 3];
        for y in 0..self.height {
            for x in 0..self.width {
                let dx = f64::from(x) - cx;
                let dy = f64::from(y) - cy;
                let sx = cos * dx - sin * dy + cx;
                let sy = sin * dx + cos * dy + cy;
                self.sample(sx, sy, &mut pixel[..c], background);
                data.extend_from_slice(&pixel[..c]);
            }
        }
        Self {
            width: self.width,
            height: self.height,
            channels: c,
            data,
        }
    }

    #[must_use]
    pub fn quarter_turned(&self, quarters: u8) -> Self {
        let quarters = quarters % 4;
        if quarters == 0 {
            return self.clone();
        }
        let (w, h) = (self.width, self.height);
        let (nw, nh) = if quarters == 2 { (w, h) } else { (h, w) };
        let c = self.channels;
        let mut data = vec![0u8; self.data.len()];
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = match quarters {
                    1 => (h - 1 - y, x),
                    2 => (w - 1 - x, h - 1 - y),
                    _ => (y, w - 1 - x),
                };
                let from = self.index(x, y);
                let to = (ny as usize * nw as usize + nx as usize) * c;
                data[to..to + c].copy_from_slice(&self.data[from..from + c]);
            }
        }
        Self {
            width: nw,
            height: nh,
            channels: c,
            data,
        }
    }

    pub fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>, String> {
        jpeg::encode(self.width, self.height, self.channels, &self.data, quality)
    }

    #[must_use]
    pub fn rgb(&self) -> Vec<u8> {
        if self.channels == 3 {
            return self.data.clone();
        }
        self.data.iter().flat_map(|&v| [v, v, v]).collect()
    }

    #[cfg(feature = "files")]
    pub fn to_png(&self, dpi: Option<f64>) -> Result<Vec<u8>, String> {
        let density = dpi.map(|d| {
            let m = pdf_edit::png::per_metre(d);
            (m, m)
        });
        pdf_edit::png::write((self.width, self.height), &self.rgb(), density).map_err(str::to_owned)
    }
}

#[must_use]
pub fn to_u32(value: f64) -> u32 {
    if value.is_nan() || value <= 0.0 {
        0
    } else if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

#[must_use]
pub fn to_u8(value: f64) -> u8 {
    u8::try_from(to_u32(value.round()).min(255)).unwrap_or(u8::MAX)
}

#[cfg(feature = "files")]
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let file = pdf_edit::image_file::ImageFile::read(bytes).map_err(|e| e.reason().to_owned())?;
    let most = file.width.max(file.height);
    let thumbnail = file
        .thumbnail(most)
        .ok_or_else(|| "the picture's pixels could not be decoded".to_owned())?;
    let image = Image::from_rgba(thumbnail.width, thumbnail.height, &thumbnail.rgba);
    Ok(if image.is_grey() { image.grey() } else { image })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(width: u32, height: u32) -> Image {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.push(u8::try_from((x + y) % 256).unwrap());
            }
        }
        Image {
            width,
            height,
            channels: 1,
            data,
        }
    }

    #[test]
    fn shrinking_averages() {
        let image = Image {
            width: 2,
            height: 2,
            channels: 1,
            data: vec![0, 100, 200, 100],
        };
        assert_eq!(image.resized(1, 1).data, vec![100]);
        let big = ramp(100, 50).resized(10, 5);
        assert_eq!((big.width, big.height, big.data.len()), (10, 5, 50));
    }

    #[test]
    fn quarter_turns_come_back() {
        let image = ramp(5, 3);
        let once = image.quarter_turned(1);
        assert_eq!((once.width, once.height), (3, 5));
        assert_eq!(once.data[2], image.data[0]);
        assert_eq!(once.quarter_turned(3), image);
        assert_eq!(image.quarter_turned(2).quarter_turned(2), image);
    }

    #[test]
    fn a_turn_of_nothing_is_the_picture() {
        let image = ramp(9, 7);
        assert_eq!(image.turned(0.0, 255), image);
    }

    #[test]
    fn transparency_lies_over_white() {
        let image = Image::from_rgba(1, 1, &[0, 0, 0, 0]);
        assert_eq!(image.data, vec![255, 255, 255]);
        assert!(image.is_grey());
    }

    #[test]
    fn cropping_keeps_the_rectangle() {
        let image = ramp(10, 10).cropped(2, 3, 4, 2);
        assert_eq!((image.width, image.height), (4, 2));
        assert_eq!(image.data[0], 5);
    }
}
