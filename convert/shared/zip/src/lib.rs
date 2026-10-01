#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let table = crc_table();
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in bytes {
        crc = table[usize::from((crc as u8) ^ byte)] ^ (crc >> 8);
    }
    !crc
}

fn crc_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0_u32; 256];
        for (n, slot) in (0_u32..).zip(table.iter_mut()) {
            let mut c = n;
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        table
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Stored,
    Deflated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ZipError {
    TooLarge,
    TooManyEntries,
    DuplicateName(String),
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => {
                f.write_str("the archive would reach 4 GiB, which this writer does not do")
            }
            Self::TooManyEntries => f.write_str("more than 65535 files in one archive"),
            Self::DuplicateName(name) => write!(f, "{name} written twice"),
        }
    }
}

impl std::error::Error for ZipError {}

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

#[derive(Default)]
pub struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Entry>,
}

const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1;
const UTF8_NAMES: u16 = 1 << 11;

fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "deflate")]
fn deflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let zlib = pdf_syntax::deflate_zlib(bytes);
    if zlib.len() < 6 || zlib[1] & 0x20 != 0 {
        return None;
    }
    Some(zlib[2..zlib.len() - 4].to_vec())
}

#[cfg(not(feature = "deflate"))]
fn deflate(_bytes: &[u8]) -> Option<Vec<u8>> {
    None
}

impl ZipWriter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, name: &str, bytes: &[u8], method: Method) -> Result<(), ZipError> {
        if self.entries.iter().any(|entry| entry.name == name) {
            return Err(ZipError::DuplicateName(name.to_owned()));
        }
        if self.entries.len() >= usize::from(u16::MAX) {
            return Err(ZipError::TooManyEntries);
        }
        let size = u32::try_from(bytes.len()).map_err(|_| ZipError::TooLarge)?;
        let crc = crc32(bytes);
        let packed = match method {
            Method::Deflated => deflate(bytes).filter(|packed| packed.len() < bytes.len()),
            Method::Stored => None,
        };
        let (method, data): (u16, &[u8]) = match &packed {
            Some(packed) => (8, packed),
            None => (0, bytes),
        };
        let compressed = u32::try_from(data.len()).map_err(|_| ZipError::TooLarge)?;
        let offset = u32::try_from(self.out.len()).map_err(|_| ZipError::TooLarge)?;
        let name_len = u16::try_from(name.len()).map_err(|_| ZipError::TooLarge)?;
        let out = &mut self.out;
        push32(out, 0x0403_4b50);
        push16(out, 20);
        push16(out, UTF8_NAMES);
        push16(out, method);
        push16(out, DOS_TIME);
        push16(out, DOS_DATE);
        push32(out, crc);
        push32(out, compressed);
        push32(out, size);
        push16(out, name_len);
        push16(out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        if u32::try_from(out.len()).is_err() {
            return Err(ZipError::TooLarge);
        }
        self.entries.push(Entry {
            name: name.to_owned(),
            method,
            crc,
            compressed,
            size,
            offset,
        });
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>, ZipError> {
        let start = u32::try_from(self.out.len()).map_err(|_| ZipError::TooLarge)?;
        let count = u16::try_from(self.entries.len()).map_err(|_| ZipError::TooManyEntries)?;
        let out = &mut self.out;
        for entry in &self.entries {
            push32(out, 0x0201_4b50);
            push16(out, 20);
            push16(out, 20);
            push16(out, UTF8_NAMES);
            push16(out, entry.method);
            push16(out, DOS_TIME);
            push16(out, DOS_DATE);
            push32(out, entry.crc);
            push32(out, entry.compressed);
            push32(out, entry.size);
            push16(
                out,
                u16::try_from(entry.name.len()).map_err(|_| ZipError::TooLarge)?,
            );
            push16(out, 0);
            push16(out, 0);
            push16(out, 0);
            push16(out, 0);
            push32(out, 0);
            push32(out, entry.offset);
            out.extend_from_slice(entry.name.as_bytes());
        }
        let size = u32::try_from(out.len()).map_err(|_| ZipError::TooLarge)? - start;
        push32(out, 0x0605_4b50);
        push16(out, 0);
        push16(out, 0);
        push16(out, count);
        push16(out, count);
        push32(out, size);
        push32(out, start);
        push16(out, 0);
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_answers() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn stored_archive_layout() {
        let mut zip = ZipWriter::new();
        zip.add("a.txt", b"hello", Method::Stored).unwrap();
        zip.add("ພາສາ/b.txt", b"", Method::Stored).unwrap();
        let bytes = zip.finish().unwrap();
        assert_eq!(u32_at(&bytes, 0), 0x0403_4b50);
        assert_eq!(u32_at(&bytes, 14), crc32(b"hello"));
        assert_eq!(&bytes[30..35], b"a.txt");
        assert_eq!(&bytes[35..40], b"hello");
        let end = bytes.len() - 22;
        assert_eq!(u32_at(&bytes, end), 0x0605_4b50);
        assert_eq!(u16_at(&bytes, end + 10), 2);
        let directory = u32_at(&bytes, end + 16) as usize;
        assert_eq!(u32_at(&bytes, directory), 0x0201_4b50);
        assert_eq!(u16_at(&bytes, directory + 8) & UTF8_NAMES, UTF8_NAMES);
    }

    #[test]
    fn refuses_a_name_twice() {
        let mut zip = ZipWriter::new();
        zip.add("x", b"1", Method::Stored).unwrap();
        assert!(matches!(
            zip.add("x", b"2", Method::Stored),
            Err(ZipError::DuplicateName(_))
        ));
    }

    #[cfg(feature = "deflate")]
    #[test]
    fn deflates_what_shrinks_and_inflates_back() {
        let text = "ພາສາລາວ ".repeat(500);
        let mut zip = ZipWriter::new();
        zip.add("t", text.as_bytes(), Method::Deflated).unwrap();
        let bytes = zip.finish().unwrap();
        assert_eq!(u16_at(&bytes, 8), 8, "deflated");
        let compressed = u32_at(&bytes, 18) as usize;
        assert!(compressed < text.len() / 10);
        let raw = &bytes[31..31 + compressed];
        let mut zlib = vec![0x78, 0x9C];
        zlib.extend_from_slice(raw);
        zlib.extend_from_slice(&adler32(text.as_bytes()).to_be_bytes());
        let back = pdf_syntax::inflate_zlib(&zlib, 1 << 20).unwrap();
        assert_eq!(back, text.as_bytes());
    }

    #[cfg(feature = "deflate")]
    fn adler32(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1_u32, 0_u32);
        for &byte in bytes {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }
}
