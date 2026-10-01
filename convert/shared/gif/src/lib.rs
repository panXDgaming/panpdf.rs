#![forbid(unsafe_code)]

pub const MOST_PIXELS: u64 = 40_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GifError(pub &'static str);

impl std::fmt::Display for GifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GIF not readable: {}", self.0)
    }
}

impl std::error::Error for GifError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gif {
    pub width: u32,
    pub height: u32,
    pub palette: Vec<u8>,
    pub indices: Vec<u8>,
    pub transparent: Option<u8>,
    pub frames: usize,
}

impl Gif {
    #[must_use]
    pub fn is_gif(bytes: &[u8]) -> bool {
        bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
    }

    #[must_use]
    pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
        if !Self::is_gif(bytes) || bytes.len() < 10 {
            return None;
        }
        let w = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
        let h = u32::from(u16::from_le_bytes([bytes[8], bytes[9]]));
        (w > 0 && h > 0).then_some((w, h))
    }

    pub fn read(bytes: &[u8]) -> Result<Self, GifError> {
        read(bytes)
    }

    #[must_use]
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.indices.len() * 4);
        for &index in &self.indices {
            if Some(index) == self.transparent {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let at = usize::from(index) * 3;
                let rgb = self.palette.get(at..at + 3).unwrap_or(&[0, 0, 0]);
                out.extend_from_slice(rgb);
                out.push(255);
            }
        }
        out
    }

    #[must_use]
    pub fn has_transparency(&self) -> bool {
        self.transparent
            .is_some_and(|key| self.indices.contains(&key))
    }

    #[must_use]
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = self.width.to_be_bytes().to_vec();
        header.extend_from_slice(&self.height.to_be_bytes());
        header.extend_from_slice(&[8, 3, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"PLTE", &self.palette);
        if let Some(key) = self.transparent.filter(|_| self.has_transparency()) {
            let mut alpha = vec![255_u8; usize::from(key) + 1];
            alpha[usize::from(key)] = 0;
            chunk(&mut out, b"tRNS", &alpha);
        }
        let w = self.width as usize;
        let mut raw = Vec::with_capacity(self.indices.len() + self.height as usize);
        for row in self.indices.chunks(w.max(1)) {
            raw.push(0);
            raw.extend_from_slice(row);
        }
        chunk(&mut out, b"IDAT", &stored_zlib(&raw));
        chunk(&mut out, b"IEND", &[]);
        out
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn stored_zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(65_535).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.byte()?, self.byte()?]))
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn sub_blocks(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(len) = self.byte() {
            if len == 0 {
                break;
            }
            let len = usize::from(len);
            let end = (self.at + len).min(self.bytes.len());
            out.extend_from_slice(&self.bytes[self.at..end]);
            self.at = end;
        }
        out
    }

    fn skip_sub_blocks(&mut self) {
        while let Some(len) = self.byte() {
            if len == 0 {
                break;
            }
            self.at = self
                .at
                .saturating_add(usize::from(len))
                .min(self.bytes.len());
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Control {
    transparent: Option<u8>,
}

fn grey_palette() -> Vec<u8> {
    (0..=255_u8).flat_map(|v| [v, v, v]).collect()
}

fn read(bytes: &[u8]) -> Result<Gif, GifError> {
    if !Gif::is_gif(bytes) {
        return Err(GifError("not a GIF"));
    }
    let mut c = Cursor { bytes, at: 6 };
    let width = c.u16().ok_or(GifError("header cut short"))?;
    let height = c.u16().ok_or(GifError("header cut short"))?;
    let packed = c.byte().ok_or(GifError("header cut short"))?;
    let background = c.byte().ok_or(GifError("header cut short"))?;
    let _aspect = c.byte();
    let global = if packed & 0x80 != 0 {
        let n = 3 << ((packed & 7) + 1);
        Some(c.take(n).ok_or(GifError("palette cut short"))?.to_vec())
    } else {
        None
    };
    let mut control = Control::default();
    let mut first: Option<Gif> = None;
    let mut frames = 0;
    while let Some(kind) = c.byte() {
        match kind {
            0x21 => {
                let label = c.byte().unwrap_or(0);
                if label == 0xF9 {
                    let body = c.sub_blocks();
                    if body.len() >= 4 {
                        control.transparent = (body[0] & 1 != 0).then_some(body[3]);
                    }
                } else {
                    c.skip_sub_blocks();
                }
            }
            0x2C => {
                frames += 1;
                if first.is_some() {
                    let _ = c.take(8);
                    let flags = c.byte().unwrap_or(0);
                    if flags & 0x80 != 0 {
                        let _ = c.take(3 << ((flags & 7) + 1));
                    }
                    let _ = c.byte();
                    c.skip_sub_blocks();
                    control = Control::default();
                    continue;
                }
                first = Some(frame(
                    &mut c,
                    (width, height),
                    global.as_deref(),
                    background,
                    control,
                )?);
                control = Control::default();
            }
            0x3B => break,
            _ => {}
        }
    }
    let mut gif = first.ok_or(GifError("no picture in the file"))?;
    gif.frames = frames;
    Ok(gif)
}

fn frame(
    c: &mut Cursor<'_>,
    screen: (u16, u16),
    global: Option<&[u8]>,
    background: u8,
    control: Control,
) -> Result<Gif, GifError> {
    let short = GifError("frame header cut short");
    let left = usize::from(c.u16().ok_or(short)?);
    let top = usize::from(c.u16().ok_or(short)?);
    let fw = usize::from(c.u16().ok_or(short)?);
    let fh = usize::from(c.u16().ok_or(short)?);
    let flags = c.byte().ok_or(short)?;
    let local = if flags & 0x80 != 0 {
        let n = 3 << ((flags & 7) + 1);
        Some(c.take(n).ok_or(GifError("palette cut short"))?.to_vec())
    } else {
        None
    };
    let interlaced = flags & 0x40 != 0;
    let sw = usize::from(screen.0).max(left + fw);
    let sh = usize::from(screen.1).max(top + fh);
    if sw == 0 || sh == 0 {
        return Err(GifError("a picture of no size"));
    }
    if (sw as u64) * (sh as u64) > MOST_PIXELS {
        return Err(GifError("the picture is too large"));
    }
    let min_code = c.byte().ok_or(GifError("image data missing"))?;
    let data = c.sub_blocks();
    let mut pixels = vec![0_u8; fw * fh];
    let decoded = lzw(&data, min_code, &mut pixels);
    let mut palette = local
        .or_else(|| global.map(<[u8]>::to_vec))
        .unwrap_or_else(grey_palette);
    let mut transparent = control.transparent;
    let mut rows = vec![0_usize; fh];
    if interlaced {
        let mut next = 0;
        for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
            let mut y = start;
            while y < fh {
                rows[next] = y;
                next += 1;
                y += step;
            }
        }
    } else {
        for (i, row) in rows.iter_mut().enumerate() {
            *row = i;
        }
    }
    let covered_all = left == 0 && top == 0 && fw == sw && fh == sh && decoded >= fw * fh;
    let fill = if covered_all {
        0
    } else if let Some(key) = transparent {
        key
    } else if palette.len() / 3 < 256 {
        let key = (palette.len() / 3) as u8;
        palette.extend_from_slice(&[0, 0, 0]);
        transparent = Some(key);
        key
    } else {
        background
    };
    let mut indices = vec![fill; sw * sh];
    for (k, &y) in rows.iter().enumerate() {
        let from = k * fw;
        let upto = decoded.min(from + fw);
        if upto <= from {
            continue;
        }
        let to = (top + y) * sw + left;
        indices[to..to + (upto - from)].copy_from_slice(&pixels[from..upto]);
    }
    let most = indices.iter().copied().max().unwrap_or(0);
    let wanted = (usize::from(most) + 1) * 3;
    if palette.len() < wanted {
        palette.resize(wanted, 0);
    }
    palette.truncate(256 * 3);
    Ok(Gif {
        width: sw as u32,
        height: sh as u32,
        palette,
        indices,
        transparent,
        frames: 1,
    })
}

fn lzw(data: &[u8], min_code: u8, out: &mut [u8]) -> usize {
    let min_code = min_code.clamp(2, 11);
    let clear = 1_usize << min_code;
    let end = clear + 1;
    let mut prefix = [0_u16; 4096];
    let mut suffix = [0_u8; 4096];
    let mut first = [0_u8; 4096];
    let mut length = [0_u16; 4096];
    for code in 0..clear {
        suffix[code] = code as u8;
        first[code] = code as u8;
        length[code] = 1;
    }
    let mut next = end + 1;
    let mut width = u32::from(min_code) + 1;
    let mut previous: Option<usize> = None;
    let mut written = 0;
    let mut bits = 0_u32;
    let mut held = 0_u32;
    let mut stack = [0_u8; 4096];
    let mut at = 0;
    loop {
        while held < width {
            let Some(&b) = data.get(at) else {
                return written;
            };
            at += 1;
            bits |= u32::from(b) << held;
            held += 8;
        }
        let code = (bits & ((1 << width) - 1)) as usize;
        bits >>= width;
        held -= width;
        if code == clear {
            next = end + 1;
            width = u32::from(min_code) + 1;
            previous = None;
            continue;
        }
        if code == end {
            return written;
        }
        let entry = if code < next {
            code
        } else if code == next && previous.is_some() {
            usize::MAX
        } else {
            return written;
        };
        if let Some(p) = previous {
            let head = if entry == usize::MAX {
                first[p]
            } else {
                first[entry]
            };
            if next < 4096 {
                prefix[next] = p as u16;
                suffix[next] = head;
                first[next] = first[p];
                length[next] = length[p].saturating_add(1);
                next += 1;
                if next == 1 << width && width < 12 {
                    width += 1;
                }
            }
        }
        let code = if entry == usize::MAX { next - 1 } else { entry };
        let len = usize::from(length[code]);
        let mut k = code;
        for slot in stack[..len].iter_mut().rev() {
            *slot = suffix[k];
            k = usize::from(prefix[k]);
        }
        let room = out.len() - written;
        let n = len.min(room);
        out[written..written + n].copy_from_slice(&stack[..n]);
        written += n;
        if written == out.len() {
            return written;
        }
        previous = Some(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn encode(
        w: u16,
        h: u16,
        palette: &[u8],
        pixels: &[u8],
        transparent: Option<u8>,
        interlaced: bool,
        frames: usize,
    ) -> Vec<u8> {
        let size_bits = {
            let entries = (palette.len() / 3).max(2);
            let mut b = 1;
            while (1 << b) < entries {
                b += 1;
            }
            b
        };
        let mut pal = palette.to_vec();
        pal.resize(3 << size_bits, 0);
        let mut out = b"GIF89a".to_vec();
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.push(0x80 | (size_bits - 1) as u8);
        out.extend([0, 0]);
        out.extend(&pal);
        let min = size_bits.max(2) as u8;
        for _ in 0..frames {
            if let Some(t) = transparent {
                out.extend([0x21, 0xF9, 4, 1, 0, 0, t, 0]);
            }
            out.push(0x2C);
            out.extend([0, 0, 0, 0]);
            out.extend(w.to_le_bytes());
            out.extend(h.to_le_bytes());
            out.push(if interlaced { 0x40 } else { 0 });
            out.push(min);
            let order: Vec<usize> = if interlaced {
                let mut rows = Vec::new();
                for (s, st) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                    rows.extend((s..usize::from(h)).step_by(st));
                }
                rows
            } else {
                (0..usize::from(h)).collect()
            };
            let clear = 1_u32 << min;
            let width = u32::from(min) + 1;
            let mut codes = Vec::new();
            for (i, &y) in order.iter().enumerate() {
                for x in 0..usize::from(w) {
                    if (i * usize::from(w) + x) % 2 == 0 {
                        codes.push(clear);
                    }
                    codes.push(u32::from(pixels[y * usize::from(w) + x]));
                }
            }
            codes.push(clear + 1);
            let mut bytes = Vec::new();
            let (mut acc, mut n) = (0_u32, 0);
            for code in codes {
                acc |= code << n;
                n += width;
                while n >= 8 {
                    bytes.push(acc as u8);
                    acc >>= 8;
                    n -= 8;
                }
            }
            if n > 0 {
                bytes.push(acc as u8);
            }
            for chunk in bytes.chunks(255) {
                out.push(chunk.len() as u8);
                out.extend(chunk);
            }
            out.push(0);
        }
        out.push(0x3B);
        out
    }

    #[test]
    fn reads_a_small_gif_with_transparency() {
        let pal = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let px: Vec<u8> = (0..12).map(|i| (i % 4) as u8).collect();
        let gif = Gif::read(&encode(4, 3, &pal, &px, Some(3), false, 1)).unwrap();
        assert_eq!((gif.width, gif.height), (4, 3));
        assert_eq!(gif.indices, px);
        assert_eq!(gif.transparent, Some(3));
        assert!(gif.has_transparency());
        assert_eq!(&gif.rgba()[..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(&gif.rgba()[12..16], &[0, 0, 0, 0]);
    }

    #[test]
    fn the_png_copy_has_checked_chunks() {
        let pal = [255, 0, 0, 0, 255, 0, 0, 0, 255];
        let px: Vec<u8> = (0..300 * 300).map(|i| (i % 3) as u8).collect();
        let gif = Gif::read(&encode(300, 300, &pal, &px, Some(2), false, 1)).unwrap();
        let png = gif.to_png();
        assert!(png.starts_with(b"\x89PNG"));
        let mut at = 8;
        let mut kinds = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(&png[at + 4..at + 8 + len]));
            kinds.push(String::from_utf8_lossy(&png[at + 4..at + 8]).into_owned());
            at += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "PLTE", "tRNS", "IDAT", "IEND"]);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn interlaced_rows_come_back_in_order() {
        let pal = [0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 255];
        let px: Vec<u8> = (0..9 * 10).map(|i| ((i / 9) % 4) as u8).collect();
        let gif = Gif::read(&encode(9, 10, &pal, &px, None, true, 1)).unwrap();
        assert_eq!(gif.indices, px);
    }

    #[test]
    fn an_animation_gives_its_first_frame() {
        let pal = [0, 0, 0, 255, 255, 255];
        let px = vec![1_u8; 6];
        let gif = Gif::read(&encode(3, 2, &pal, &px, None, false, 3)).unwrap();
        assert_eq!(gif.frames, 3);
        assert_eq!(gif.indices, px);
    }

    #[test]
    fn a_real_lzw_stream_decodes() {
        let data = [
            0x8C, 0x2D, 0x99, 0x87, 0x2A, 0x1C, 0xDC, 0x33, 0xA0, 0x02, 0x75, 0xEC, 0x95, 0xFA,
            0xA8, 0xDE, 0x60, 0x8C, 0x04, 0x91, 0x4C, 0x01,
        ];
        let mut out = vec![0; 100];
        let n = lzw(&data, 2, &mut out);
        assert_eq!(n, 100);
        let expect_row0 = [1, 1, 1, 1, 1, 2, 2, 2, 2, 2];
        assert_eq!(&out[..10], &expect_row0);
        let expect_row4 = [1, 1, 1, 0, 0, 0, 0, 2, 2, 2];
        assert_eq!(&out[40..50], &expect_row4);
    }

    #[test]
    fn damaged_input_never_panics() {
        let pal = [0, 0, 0, 255, 255, 255, 9, 9, 9];
        let px: Vec<u8> = (0..64).map(|i| (i % 3) as u8).collect();
        let good = encode(8, 8, &pal, &px, Some(2), true, 2);
        for cut in 0..good.len() {
            let _ = Gif::read(&good[..cut]);
        }
        let mut seed = 0x1234_5678_u32;
        let rounds = std::env::var("FUZZ_ROUNDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        for round in 0..rounds {
            let mut bytes = good.clone();
            for _ in 0..(round % 7 + 1) {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let at = (seed as usize >> 8) % bytes.len();
                bytes[at] = (seed >> 24) as u8;
            }
            if let Ok(gif) = Gif::read(&bytes) {
                assert_eq!(gif.indices.len(), (gif.width * gif.height) as usize);
                assert!(gif.palette.len() / 3 > usize::from(*gif.indices.iter().max().unwrap()));
            }
        }
        assert!(Gif::read(b"GIF89a\xff\xff\xff\xff\x00\x00\x00,").is_err());
    }
}
