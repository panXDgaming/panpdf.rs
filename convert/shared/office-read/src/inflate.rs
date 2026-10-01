#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InflateError {
    Truncated,
    Malformed(&'static str),
    TooLarge,
}

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => f.write_str("compressed data ends too early"),
            Self::Malformed(why) => write!(f, "compressed data is malformed: {why}"),
            Self::TooLarge => f.write_str("compressed data expands past the limit"),
        }
    }
}

impl std::error::Error for InflateError {}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit_buf: u32,
    bit_count: u32,
}

impl Bits<'_> {
    fn need(&mut self, count: u32) -> Result<u32, InflateError> {
        while self.bit_count < count {
            let byte = *self.data.get(self.pos).ok_or(InflateError::Truncated)?;
            self.pos += 1;
            self.bit_buf |= u32::from(byte) << self.bit_count;
            self.bit_count += 8;
        }
        let value = if count == 0 {
            0
        } else {
            self.bit_buf & ((1_u32 << count) - 1)
        };
        self.bit_buf = if count >= 32 {
            0
        } else {
            self.bit_buf >> count
        };
        self.bit_count -= count;
        Ok(value)
    }

    fn align(&mut self) {
        self.bit_buf = 0;
        self.bit_count = 0;
    }
}

const MAX_BITS: usize = 15;

struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, InflateError> {
        let mut counts = [0_u16; MAX_BITS + 1];
        for &length in lengths {
            counts[usize::from(length)] += 1;
        }
        let mut left: i32 = 1;
        for &count in &counts[1..] {
            left = (left << 1) - i32::from(count);
            if left < 0 {
                return Err(InflateError::Malformed("over-subscribed code"));
            }
        }
        let mut offsets = [0_u16; MAX_BITS + 2];
        for length in 1..=MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0_u16; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                let slot = &mut offsets[usize::from(length)];
                symbols[usize::from(*slot)] =
                    u16::try_from(symbol).map_err(|_| InflateError::Malformed("table"))?;
                *slot += 1;
            }
        }
        counts[0] = 0;
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, InflateError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for length in 1..=MAX_BITS {
            code |= i32::try_from(bits.need(1)?).unwrap_or(0);
            let count = i32::from(self.counts[length]);
            if code - count < first {
                let at = usize::try_from(index + (code - first))
                    .map_err(|_| InflateError::Malformed("code"))?;
                return self
                    .symbols
                    .get(at)
                    .copied()
                    .ok_or(InflateError::Malformed("code"));
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(InflateError::Malformed("no such code"))
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

pub fn inflate(data: &[u8], size_hint: usize, limit: usize) -> Result<Vec<u8>, InflateError> {
    let mut out = Vec::with_capacity(size_hint.min(limit));
    let mut bits = Bits {
        data,
        pos: 0,
        bit_buf: 0,
        bit_count: 0,
    };
    let fixed = fixed_tables()?;
    loop {
        let last = bits.need(1)? == 1;
        match bits.need(2)? {
            0 => stored(&mut bits, &mut out, limit)?,
            1 => codes(&mut bits, &mut out, &fixed.0, &fixed.1, limit)?,
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err(InflateError::Malformed("block type 3")),
        }
        if last {
            return Ok(out);
        }
    }
}

fn stored(bits: &mut Bits<'_>, out: &mut Vec<u8>, limit: usize) -> Result<(), InflateError> {
    bits.align();
    let header = bits
        .data
        .get(bits.pos..bits.pos + 4)
        .ok_or(InflateError::Truncated)?;
    let len = usize::from(u16::from_le_bytes([header[0], header[1]]));
    let nlen = u16::from_le_bytes([header[2], header[3]]);
    if usize::from(!nlen) != len {
        return Err(InflateError::Malformed("stored length check"));
    }
    bits.pos += 4;
    let body = bits
        .data
        .get(bits.pos..bits.pos + len)
        .ok_or(InflateError::Truncated)?;
    if out.len() + len > limit {
        return Err(InflateError::TooLarge);
    }
    out.extend_from_slice(body);
    bits.pos += len;
    Ok(())
}

fn fixed_tables() -> Result<(Huffman, Huffman), InflateError> {
    let mut lengths = [0_u8; 288];
    for (symbol, length) in lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5; 30])?))
}

fn dynamic_tables(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), InflateError> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let nlen = bits.need(5)? as usize + 257;
    let ndist = bits.need(5)? as usize + 1;
    let ncode = bits.need(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(InflateError::Malformed("too many codes"));
    }
    let mut code_lengths = [0_u8; 19];
    for &slot in ORDER.iter().take(ncode) {
        code_lengths[slot] = u8::try_from(bits.need(3)?).unwrap_or(0);
    }
    let code_code = Huffman::new(&code_lengths)?;
    let mut lengths = vec![0_u8; nlen + ndist];
    let mut index = 0;
    while index < nlen + ndist {
        let symbol = code_code.decode(bits)?;
        if symbol < 16 {
            lengths[index] = u8::try_from(symbol).unwrap_or(0);
            index += 1;
            continue;
        }
        let (value, repeat) = match symbol {
            16 => {
                let previous = *index
                    .checked_sub(1)
                    .and_then(|at| lengths.get(at))
                    .ok_or(InflateError::Malformed("repeat with no length"))?;
                (previous, 3 + bits.need(2)? as usize)
            }
            17 => (0, 3 + bits.need(3)? as usize),
            _ => (0, 11 + bits.need(7)? as usize),
        };
        if index + repeat > nlen + ndist {
            return Err(InflateError::Malformed("too many lengths"));
        }
        lengths[index..index + repeat].fill(value);
        index += repeat;
    }
    if lengths[256] == 0 {
        return Err(InflateError::Malformed("no end-of-block code"));
    }
    Ok((
        Huffman::new(&lengths[..nlen])?,
        Huffman::new(&lengths[nlen..])?,
    ))
}

fn codes(
    bits: &mut Bits<'_>,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    limit: usize,
) -> Result<(), InflateError> {
    loop {
        let symbol = lit.decode(bits)?;
        match symbol {
            0..=255 => {
                if out.len() >= limit {
                    return Err(InflateError::TooLarge);
                }
                out.push(u8::try_from(symbol).unwrap_or(0));
            }
            256 => return Ok(()),
            _ => {
                let at = usize::from(symbol - 257);
                if at >= 29 {
                    return Err(InflateError::Malformed("length code"));
                }
                let length =
                    usize::from(LENGTH_BASE[at]) + bits.need(u32::from(LENGTH_EXTRA[at]))? as usize;
                let code = usize::from(dist.decode(bits)?);
                if code >= 30 {
                    return Err(InflateError::Malformed("distance code"));
                }
                let distance =
                    usize::from(DIST_BASE[code]) + bits.need(u32::from(DIST_EXTRA[code]))? as usize;
                if distance > out.len() {
                    return Err(InflateError::Malformed("distance before the start"));
                }
                if out.len() + length > limit {
                    return Err(InflateError::TooLarge);
                }
                let start = out.len() - distance;
                if distance >= length {
                    out.extend_from_within(start..start + length);
                } else {
                    for k in 0..length {
                        let byte = out[start + k];
                        out.push(byte);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(bytes: &[u8]) -> Vec<u8> {
        let zlib = pdf_syntax::deflate_zlib(bytes);
        zlib[2..zlib.len() - 4].to_vec()
    }

    #[test]
    fn round_trips_the_engine_deflater() {
        let mut text = Vec::new();
        for n in 0..5000_u32 {
            text.extend_from_slice(format!("<c r=\"A{n}\"><v>{}</v></c>", n * 7 % 13).as_bytes());
        }
        text.extend_from_slice("ພາສາລາວ ภาษาไทย".as_bytes());
        assert_eq!(inflate(&raw(&text), 0, usize::MAX).unwrap(), text);
        assert_eq!(inflate(&raw(b""), 0, 10).unwrap(), b"");
        let noise: Vec<u8> = (0..70_000_u32)
            .map(|n| (n.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        assert_eq!(inflate(&raw(&noise), 0, usize::MAX).unwrap(), noise);
    }

    #[test]
    fn stored_blocks_and_limits() {
        let stored = [1, 3, 0, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(inflate(&stored, 3, 3).unwrap(), b"abc");
        assert_eq!(inflate(&stored, 3, 2), Err(InflateError::TooLarge));
        assert_eq!(inflate(&stored[..6], 3, 9), Err(InflateError::Truncated));
        assert!(inflate(&[0xFF], 0, 9).is_err());
    }
}
