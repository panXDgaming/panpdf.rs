use crate::inflate::{InflateError, inflate};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipError {
    NotZip,
    Truncated(&'static str),
    Unsupported(String),
    Inflate(InflateError),
    Checksum(String),
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotZip => f.write_str("not a ZIP archive (an Office file is one)"),
            Self::Truncated(what) => write!(f, "the archive is cut short ({what})"),
            Self::Unsupported(what) => write!(f, "unsupported ZIP entry: {what}"),
            Self::Inflate(error) => error.fmt(f),
            Self::Checksum(name) => write!(f, "'{name}' is damaged (CRC-32 mismatch)"),
        }
    }
}

impl std::error::Error for ZipError {}

impl From<InflateError> for ZipError {
    fn from(error: InflateError) -> Self {
        Self::Inflate(error)
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc: u32,
    pub compressed: u64,
    pub size: u64,
    pub local_offset: u64,
}

#[derive(Clone, Debug)]
pub struct Zip<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

fn to_usize(value: u64) -> Result<usize, ZipError> {
    usize::try_from(value).map_err(|_| ZipError::Truncated("offset"))
}

pub const ENTRY_LIMIT: usize = 1 << 30;

impl<'a> Zip<'a> {
    pub fn read(data: &'a [u8]) -> Result<Self, ZipError> {
        let eocd = find_end(data).ok_or(ZipError::NotZip)?;
        let mut count = u64::from(u16_at(data, eocd + 10).ok_or(ZipError::NotZip)?);
        let mut dir_size = u64::from(u32_at(data, eocd + 12).ok_or(ZipError::NotZip)?);
        let mut dir_offset = u64::from(u32_at(data, eocd + 16).ok_or(ZipError::NotZip)?);
        if eocd >= 20 && u32_at(data, eocd - 20) == Some(0x0706_4b50) {
            let record = to_usize(u64_at(data, eocd - 12).unwrap_or(u64::MAX))?;
            if u32_at(data, record) == Some(0x0606_4b50) {
                count = u64_at(data, record + 32).ok_or(ZipError::Truncated("zip64 end"))?;
                dir_size = u64_at(data, record + 40).ok_or(ZipError::Truncated("zip64 end"))?;
                dir_offset = u64_at(data, record + 48).ok_or(ZipError::Truncated("zip64 end"))?;
            }
        }
        let start = to_usize(dir_offset)?;
        let end = start
            .checked_add(to_usize(dir_size)?)
            .filter(|end| *end <= data.len())
            .ok_or(ZipError::Truncated("central directory"))?;
        let mut entries = Vec::with_capacity(usize::try_from(count.min(1 << 16)).unwrap_or(0));
        let mut at = start;
        while at + 46 <= end && u32_at(data, at) == Some(0x0201_4b50) {
            let short = || ZipError::Truncated("central header");
            let flags = u16_at(data, at + 8).ok_or_else(short)?;
            let method = u16_at(data, at + 10).ok_or_else(short)?;
            let crc = u32_at(data, at + 16).ok_or_else(short)?;
            let mut compressed = u64::from(u32_at(data, at + 20).ok_or_else(short)?);
            let mut size = u64::from(u32_at(data, at + 24).ok_or_else(short)?);
            let name_len = usize::from(u16_at(data, at + 28).ok_or_else(short)?);
            let extra_len = usize::from(u16_at(data, at + 30).ok_or_else(short)?);
            let comment_len = usize::from(u16_at(data, at + 32).ok_or_else(short)?);
            let mut local_offset = u64::from(u32_at(data, at + 42).ok_or_else(short)?);
            let name_bytes = data.get(at + 46..at + 46 + name_len).ok_or_else(short)?;
            let name = String::from_utf8(name_bytes.to_vec())
                .unwrap_or_else(|_| name_bytes.iter().map(|&b| char::from(b)).collect());
            let extra = data
                .get(at + 46 + name_len..at + 46 + name_len + extra_len)
                .ok_or_else(short)?;
            zip64_extra(extra, &mut size, &mut compressed, &mut local_offset);
            entries.push(Entry {
                name,
                method,
                flags,
                crc,
                compressed,
                size,
                local_offset,
            });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { data, entries })
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        let name = name.trim_start_matches('/');
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|entry| entry.name.eq_ignore_ascii_case(name))
            })
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<Result<Vec<u8>, ZipError>> {
        self.entry(name).map(|entry| self.read_entry(entry))
    }

    pub fn read_entry(&self, entry: &Entry) -> Result<Vec<u8>, ZipError> {
        if entry.flags & 1 != 0 {
            return Err(ZipError::Unsupported(format!(
                "'{}' is encrypted",
                entry.name
            )));
        }
        let local = to_usize(entry.local_offset)?;
        if u32_at(self.data, local) != Some(0x0403_4b50) {
            return Err(ZipError::Truncated("local header"));
        }
        let name_len = usize::from(u16_at(self.data, local + 26).unwrap_or(0));
        let extra_len = usize::from(u16_at(self.data, local + 28).unwrap_or(0));
        let start = local + 30 + name_len + extra_len;
        let end = start
            .checked_add(to_usize(entry.compressed)?)
            .filter(|end| *end <= self.data.len())
            .ok_or(ZipError::Truncated("entry data"))?;
        let raw = &self.data[start..end];
        let hint = to_usize(entry.size).unwrap_or(0).min(ENTRY_LIMIT);
        let bytes = match entry.method {
            0 => raw.to_vec(),
            8 => inflate(raw, hint, ENTRY_LIMIT)?,
            other => {
                return Err(ZipError::Unsupported(format!(
                    "'{}' uses compression method {other}",
                    entry.name
                )));
            }
        };
        if convert_zip::crc32(&bytes) != entry.crc {
            return Err(ZipError::Checksum(entry.name.clone()));
        }
        Ok(bytes)
    }
}

fn find_end(data: &[u8]) -> Option<usize> {
    if data.len() < 22 {
        return None;
    }
    let lowest = data.len().saturating_sub(22 + 0xFFFF);
    (lowest..=data.len() - 22)
        .rev()
        .find(|&at| u32_at(data, at) == Some(0x0605_4b50))
}

fn zip64_extra(extra: &[u8], size: &mut u64, compressed: &mut u64, offset: &mut u64) {
    let mut at = 0;
    while at + 4 <= extra.len() {
        let id = u16_at(extra, at).unwrap_or(0);
        let len = usize::from(u16_at(extra, at + 2).unwrap_or(0));
        if id == 1 {
            let mut field = at + 4;
            for value in [&mut *size, &mut *compressed, &mut *offset] {
                if *value == 0xFFFF_FFFF {
                    if let Some(wide) = u64_at(extra, field) {
                        *value = wide;
                    }
                    field += 8;
                }
            }
        }
        at += 4 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_writer_wrote() {
        let mut writer = convert_zip::ZipWriter::new();
        writer
            .add(
                "[Content_Types].xml",
                b"<Types/>",
                convert_zip::Method::Stored,
            )
            .unwrap();
        writer
            .add(
                "xl/workbook.xml",
                "ພາສາລາວ".repeat(50).as_bytes(),
                convert_zip::Method::Stored,
            )
            .unwrap();
        let bytes = writer.finish().unwrap();
        let zip = Zip::read(&bytes).unwrap();
        assert_eq!(
            zip.names().collect::<Vec<_>>(),
            ["[Content_Types].xml", "xl/workbook.xml"]
        );
        assert_eq!(
            zip.get("/XL/Workbook.xml").unwrap().unwrap(),
            "ພາສາລາວ".repeat(50).as_bytes()
        );
        assert!(zip.get("missing").is_none());
        assert_eq!(
            Zip::read(b"not a zip at all, sorry").unwrap_err(),
            ZipError::NotZip
        );
    }
}
