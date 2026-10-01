#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageError {
    Unknown,
    Unreadable(&'static str),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => f.write_str("picture is not JPEG, PNG or GIF"),
            Self::Unreadable(why) => write!(f, "picture not readable: {why}"),
        }
    }
}

impl std::error::Error for ImageError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Space {
    Gray,
    Rgb,
    Cmyk,
}

impl Space {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Gray => "/DeviceGray",
            Self::Rgb => "/DeviceRGB",
            Self::Cmyk => "/DeviceCMYK",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Picture {
    pub width: u32,
    pub height: u32,
    pub space: Space,
    pub data: Vec<u8>,
    pub jpeg: bool,
    pub inverted: bool,
    pub alpha: Option<Vec<u8>>,
    pub palette: Option<Vec<u8>>,
    pub key: Option<u8>,
}

pub(crate) fn read(bytes: &[u8]) -> Result<Picture, ImageError> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg(bytes);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return png(bytes);
    }
    if convert_gif::Gif::is_gif(bytes) {
        return gif(bytes);
    }
    Err(ImageError::Unknown)
}

fn gif(bytes: &[u8]) -> Result<Picture, ImageError> {
    let gif = convert_gif::Gif::read(bytes).map_err(|e| ImageError::Unreadable(e.0))?;
    let key = gif.transparent.filter(|_| gif.has_transparency());
    Ok(Picture {
        width: gif.width,
        height: gif.height,
        space: Space::Rgb,
        data: pdf_syntax::deflate_zlib(&gif.indices),
        jpeg: false,
        inverted: false,
        alpha: None,
        palette: Some(gif.palette),
        key,
    })
}

fn jpeg(bytes: &[u8]) -> Result<Picture, ImageError> {
    let mut at = 2;
    let mut adobe = false;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xFF {
            return Err(ImageError::Unreadable("JPEG marker expected"));
        }
        let marker = bytes[at + 1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            at += 2;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let body = bytes
            .get(at + 4..at + 2 + len)
            .ok_or(ImageError::Unreadable("JPEG segment cut short"))?;
        if marker == 0xEE && body.starts_with(b"Adobe") {
            adobe = true;
        }
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if body.len() < 6 {
                return Err(ImageError::Unreadable("JPEG frame header cut short"));
            }
            let height = u32::from(u16::from_be_bytes([body[1], body[2]]));
            let width = u32::from(u16::from_be_bytes([body[3], body[4]]));
            let space = match body[5] {
                1 => Space::Gray,
                3 => Space::Rgb,
                4 => Space::Cmyk,
                _ => return Err(ImageError::Unreadable("JPEG component count")),
            };
            if width == 0 || height == 0 {
                return Err(ImageError::Unreadable("JPEG with no size"));
            }
            return Ok(Picture {
                width,
                height,
                space,
                data: bytes.to_vec(),
                jpeg: true,
                inverted: adobe && space == Space::Cmyk,
                alpha: None,
                palette: None,
                key: None,
            });
        }
        if marker == 0xDA {
            break;
        }
        at += 2 + len;
    }
    Err(ImageError::Unreadable("JPEG with no frame header"))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[allow(clippy::too_many_lines)]
fn png(bytes: &[u8]) -> Result<Picture, ImageError> {
    let bad = ImageError::Unreadable;
    let mut at = 8;
    let mut header = None;
    let mut palette: &[u8] = &[];
    let mut transparency: &[u8] = &[];
    let mut idat = Vec::new();
    while at + 8 <= bytes.len() {
        let len = be32(bytes, at).ok_or(bad("PNG chunk"))? as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or(bad("PNG chunk cut short"))?;
        match kind {
            b"IHDR" if len >= 13 => header = Some(body),
            b"PLTE" => palette = body,
            b"tRNS" => transparency = body,
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    let header = header.ok_or(bad("PNG without IHDR"))?;
    let width = be32(header, 0).ok_or(bad("PNG header"))?;
    let height = be32(header, 4).ok_or(bad("PNG header"))?;
    let depth = header[8];
    let color = header[9];
    let interlaced = header[12] == 1;
    if width == 0 || height == 0 || width > 1 << 15 || height > 1 << 15 {
        return Err(bad("PNG size"));
    }
    let channels: usize = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(bad("PNG colour type")),
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) {
        return Err(bad("PNG bit depth"));
    }
    let bits = channels * usize::from(depth);
    let (w, h) = (width as usize, height as usize);
    let raw_limit = (w * bits).div_ceil(8).saturating_add(1) * h * 2 + 1024;
    let raw = pdf_syntax::inflate_zlib(&idat, raw_limit).map_err(|_| bad("PNG data"))?;
    let samples = if interlaced {
        adam7(&raw, w, h, bits, depth, channels)?
    } else {
        let (samples, _) = unfilter_pass(&raw, w, h, bits, depth, channels)?;
        samples
    };
    let max = (1_u32 << depth) - 1;
    let to8 = |v: u16| -> u8 {
        if depth == 16 {
            (v >> 8) as u8
        } else {
            ((u32::from(v) * 255 + max / 2) / max) as u8
        }
    };
    let pixels = w * h;
    let mut colour_out = Vec::with_capacity(pixels * 3);
    let mut alpha = Vec::with_capacity(pixels);
    let mut any_alpha = false;
    let gray_key = (color == 0 && transparency.len() >= 2)
        .then(|| u16::from_be_bytes([transparency[0], transparency[1]]));
    let rgb_key = (color == 2 && transparency.len() >= 6).then(|| {
        [
            u16::from_be_bytes([transparency[0], transparency[1]]),
            u16::from_be_bytes([transparency[2], transparency[3]]),
            u16::from_be_bytes([transparency[4], transparency[5]]),
        ]
    });
    let gray_out = matches!(color, 0 | 4);
    for p in 0..pixels {
        let s = &samples[p * channels..(p + 1) * channels];
        let a: u8 = match color {
            0 => {
                colour_out.push(to8(s[0]));
                if gray_key == Some(s[0]) { 0 } else { 255 }
            }
            2 => {
                colour_out.extend([to8(s[0]), to8(s[1]), to8(s[2])]);
                if rgb_key == Some([s[0], s[1], s[2]]) {
                    0
                } else {
                    255
                }
            }
            3 => {
                let index = usize::from(s[0]);
                let rgb = palette.get(index * 3..index * 3 + 3).unwrap_or(&[0, 0, 0]);
                colour_out.extend_from_slice(rgb);
                transparency.get(index).copied().unwrap_or(255)
            }
            4 => {
                colour_out.push(to8(s[0]));
                to8(s[1])
            }
            _ => {
                colour_out.extend([to8(s[0]), to8(s[1]), to8(s[2])]);
                to8(s[3])
            }
        };
        any_alpha |= a != 255;
        alpha.push(a);
    }
    Ok(Picture {
        width,
        height,
        space: if gray_out { Space::Gray } else { Space::Rgb },
        data: pdf_syntax::deflate_zlib(&colour_out),
        jpeg: false,
        inverted: false,
        alpha: any_alpha.then(|| pdf_syntax::deflate_zlib(&alpha)),
        palette: None,
        key: None,
    })
}

fn unfilter_pass(
    raw: &[u8],
    w: usize,
    h: usize,
    bits: usize,
    depth: u8,
    channels: usize,
) -> Result<(Vec<u16>, usize), ImageError> {
    let stride = (w * bits).div_ceil(8);
    let bpp = bits.div_ceil(8).max(1);
    let mut previous = vec![0_u8; stride];
    let mut current = vec![0_u8; stride];
    let mut samples = Vec::with_capacity(w * h * channels);
    let mut at = 0;
    for _ in 0..h {
        let filter = *raw
            .get(at)
            .ok_or(ImageError::Unreadable("PNG data cut short"))?;
        let line = raw
            .get(at + 1..at + 1 + stride)
            .ok_or(ImageError::Unreadable("PNG data cut short"))?;
        at += 1 + stride;
        for i in 0..stride {
            let a = if i >= bpp { current[i - bpp] } else { 0 };
            let b = previous[i];
            let c = if i >= bpp { previous[i - bpp] } else { 0 };
            let x = line[i];
            current[i] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                _ => return Err(ImageError::Unreadable("PNG filter")),
            };
        }
        let count = w * channels;
        match depth {
            8 => samples.extend(current[..count].iter().map(|&v| u16::from(v))),
            16 => samples.extend(
                current[..count * 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| u16::from_be_bytes([p[0], p[1]])),
            ),
            _ => {
                let d = usize::from(depth);
                let mask = (1_u16 << d) - 1;
                for k in 0..count {
                    let bit = k * d;
                    let byte = current[bit / 8];
                    let shift = 8 - d - (bit % 8);
                    samples.push((u16::from(byte) >> shift) & mask);
                }
            }
        }
        std::mem::swap(&mut previous, &mut current);
    }
    Ok((samples, at))
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn adam7(
    raw: &[u8],
    w: usize,
    h: usize,
    bits: usize,
    depth: u8,
    channels: usize,
) -> Result<Vec<u16>, ImageError> {
    const PASSES: [(usize, usize, usize, usize); 7] = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    let mut out = vec![0_u16; w * h * channels];
    let mut at = 0;
    for (x0, y0, dx, dy) in PASSES {
        if x0 >= w || y0 >= h {
            continue;
        }
        let pw = (w - x0).div_ceil(dx);
        let ph = (h - y0).div_ceil(dy);
        let (samples, used) = unfilter_pass(&raw[at..], pw, ph, bits, depth, channels)?;
        at += used;
        for py in 0..ph {
            for px in 0..pw {
                let (x, y) = (x0 + px * dx, y0 + py * dy);
                let from = (py * pw + px) * channels;
                let to = (y * w + x) * channels;
                out[to..to + channels].copy_from_slice(&samples[from..from + channels]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out
    }

    #[test]
    fn reads_an_rgba_png() {
        let mut ihdr = 2_u32.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&1_u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let raw = [0_u8, 255, 0, 0, 255, 0, 0, 255, 128];
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &pdf_syntax::deflate_zlib(&raw)));
        png.extend(chunk(b"IEND", &[]));
        let picture = read(&png).unwrap();
        assert_eq!((picture.width, picture.height), (2, 1));
        assert_eq!(picture.space, Space::Rgb);
        let colour = pdf_syntax::inflate_zlib(&picture.data, 100).unwrap();
        assert_eq!(colour, [255, 0, 0, 0, 0, 255]);
        let alpha = pdf_syntax::inflate_zlib(&picture.alpha.unwrap(), 100).unwrap();
        assert_eq!(alpha, [255, 128]);
        assert!(matches!(read(b"GIF89a"), Err(ImageError::Unreadable(_))));
        assert!(matches!(read(b"BM"), Err(ImageError::Unknown)));
    }
}
