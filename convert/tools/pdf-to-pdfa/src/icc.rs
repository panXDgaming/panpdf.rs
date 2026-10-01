fn s15(value: f64) -> [u8; 4] {
    let fixed = (value * 65536.0).round() as i32;
    fixed.to_be_bytes()
}

fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut out = b"XYZ \0\0\0\0".to_vec();
    out.extend_from_slice(&s15(x));
    out.extend_from_slice(&s15(y));
    out.extend_from_slice(&s15(z));
    out
}

fn text(tag: &[u8; 4], ascii: &str) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&[0; 4]);
    if tag == b"desc" {
        let len = u32::try_from(ascii.len() + 1).unwrap_or(0);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(ascii.as_bytes());
        out.push(0);
        out.extend_from_slice(&[0; 4 + 4 + 2 + 1 + 67]);
    } else {
        out.extend_from_slice(ascii.as_bytes());
        out.push(0);
    }
    out
}

fn curve() -> Vec<u8> {
    let mut out = b"curv\0\0\0\0".to_vec();
    let n: u32 = 1024;
    out.extend_from_slice(&n.to_be_bytes());
    for i in 0..n {
        let v = f64::from(i) / f64::from(n - 1);
        let linear = if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        let q = (linear * 65535.0).round() as u16;
        out.extend_from_slice(&q.to_be_bytes());
    }
    out
}

#[must_use]
pub fn srgb() -> Vec<u8> {
    let tags: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", text(b"desc", "sRGB IEC61966-2.1")),
        (*b"cprt", text(b"text", "No copyright, use freely")),
        (*b"wtpt", xyz(0.9642, 1.0, 0.8249)),
        (*b"rXYZ", xyz(0.4361, 0.2225, 0.0139)),
        (*b"gXYZ", xyz(0.3851, 0.7169, 0.0971)),
        (*b"bXYZ", xyz(0.1431, 0.0606, 0.7141)),
        (*b"rTRC", curve()),
    ];
    let count = tags.len() + 2;
    let table_len = 4 + 12 * count;
    let mut offset = 128 + table_len;
    let mut entries = Vec::new();
    let mut data = Vec::new();
    let mut trc = (0, 0);
    for (sig, body) in &tags {
        while !offset.is_multiple_of(4) {
            data.push(0);
            offset += 1;
        }
        entries.push((*sig, offset, body.len()));
        if sig == b"rTRC" {
            trc = (offset, body.len());
        }
        data.extend_from_slice(body);
        offset += body.len();
    }
    entries.push((*b"gTRC", trc.0, trc.1));
    entries.push((*b"bTRC", trc.0, trc.1));
    while !offset.is_multiple_of(4) {
        data.push(0);
        offset += 1;
    }
    let size = u32::try_from(offset).unwrap_or(0);
    let mut out = Vec::with_capacity(offset);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[2, 0x10, 0, 0]);
    out.extend_from_slice(b"mntrRGB XYZ ");
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(b"acsp");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&s15(0.9642));
    out.extend_from_slice(&s15(1.0));
    out.extend_from_slice(&s15(0.8249));
    out.extend_from_slice(&[0; 4]);
    out.resize(128, 0);
    out.extend_from_slice(&u32::try_from(count).unwrap_or(0).to_be_bytes());
    for (sig, at, len) in entries {
        out.extend_from_slice(&sig);
        out.extend_from_slice(&u32::try_from(at).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&u32::try_from(len).unwrap_or(0).to_be_bytes());
    }
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_profile_states_its_own_size_and_signature() {
        let p = super::srgb();
        assert_eq!(
            u32::from_be_bytes(p[0..4].try_into().unwrap()) as usize,
            p.len()
        );
        assert_eq!(&p[36..40], b"acsp");
        assert_eq!(&p[12..20], b"mntrRGB ");
        assert_eq!(p.len() % 4, 0);
    }
}
