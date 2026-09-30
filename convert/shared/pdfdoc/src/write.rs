use pdf_security::AuthenticatedSecurity;
use pdf_syntax::Reference;

use crate::value::{Dict, Value};
use crate::{Doc, deflate};

pub struct Protection<'a> {
    pub dictionary: &'a [u8],
    pub security: &'a AuthenticatedSecurity,
}

#[derive(Clone, Debug)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub objects: usize,
    pub dropped: usize,
}

const PER_OBJECT_STREAM: usize = 200;

pub fn write(
    doc: &Doc,
    protection: Option<&Protection<'_>>,
    compress: bool,
) -> Result<Written, String> {
    if doc.root().is_none() {
        return Err("the document has no catalogue".into());
    }
    let order = doc.reachable();
    let mut new_number = std::collections::HashMap::new();
    for (at, old) in order.iter().enumerate() {
        new_number.insert(*old, u32::try_from(at + 1).map_err(|_| "too many objects")?);
    }
    let map = |old: u32| new_number.get(&old).copied();

    let revision_6 = protection.is_some_and(|p| {
        p.dictionary.windows(4).any(|w| w == b"/R 6") || p.security.revision() >= 6
    });
    let version = if revision_6 {
        "2.0".to_owned()
    } else if compress && version_below(&doc.version, 1, 5) {
        "1.5".to_owned()
    } else if doc.version.is_empty() {
        "1.4".to_owned()
    } else {
        doc.version.clone()
    };
    let mut out = format!("%PDF-{version}\n").into_bytes();
    out.extend_from_slice(b"%\xe2\xe3\xcf\xd3\n");

    let count = order.len();
    let mut offsets: Vec<Place> = vec![Place::Free; count + 1];
    let mut packed: Vec<(u32, Vec<u8>)> = Vec::new();

    for (at, old) in order.iter().enumerate() {
        let number = u32::try_from(at + 1).map_err(|_| "too many objects")?;
        let object = &doc.objects[old];
        let mut value = object.value.clone();
        value.renumber(&map);
        let reference = Reference::new(number, 0);
        if object.stream.is_none() && compress {
            packed.push((number, value.to_bytes()));
            continue;
        }
        if let Some(protection) = protection {
            value
                .map_strings(&mut |bytes| {
                    protection
                        .security
                        .encrypt_string(reference, bytes)
                        .map_err(|e| e.to_string())
                })
                .map_err(|e| format!("object {number}: {e}"))?;
        }
        offsets[at + 1] = Place::At(out.len());
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        match &object.stream {
            Some(data) => {
                let data = match protection {
                    Some(protection) => protection
                        .security
                        .encrypt_stream(reference, data)
                        .map_err(|e| format!("stream {number}: {e}"))?,
                    None => data.clone(),
                };
                let mut dict = value.as_dict().cloned().unwrap_or_default();
                dict.set("Length", Value::Int(len(data.len())));
                Value::Dict(dict).write(&mut out);
                out.extend_from_slice(b"\nstream\n");
                out.extend_from_slice(&data);
                out.extend_from_slice(b"\nendstream\nendobj\n");
            }
            None => {
                value.write(&mut out);
                out.extend_from_slice(b"\nendobj\n");
            }
        }
    }

    let mut next = u32::try_from(count + 1).map_err(|_| "too many objects")?;
    for group in packed.chunks(PER_OBJECT_STREAM) {
        let stream_number = next;
        next += 1;
        let mut head = String::new();
        let mut body = Vec::new();
        for (index, (number, bytes)) in group.iter().enumerate() {
            head.push_str(&format!("{number} {} ", body.len()));
            body.extend_from_slice(bytes);
            body.push(b'\n');
            offsets[*number as usize] = Place::InStream(
                stream_number,
                u32::try_from(index).map_err(|_| "too many objects")?,
            );
        }
        let mut data = head.into_bytes();
        let first = data.len();
        data.extend_from_slice(&body);
        let mut data = deflate(&data);
        if let Some(protection) = protection {
            data = protection
                .security
                .encrypt_stream(Reference::new(stream_number, 0), &data)
                .map_err(|e| format!("object stream: {e}"))?;
        }
        let mut dict = Dict::new();
        dict.set("Type", Value::name("ObjStm"));
        dict.set("N", Value::Int(len(group.len())));
        dict.set("First", Value::Int(len(first)));
        dict.set("Filter", Value::name("FlateDecode"));
        dict.set("Length", Value::Int(len(data.len())));
        offsets.push(Place::At(out.len()));
        out.extend_from_slice(format!("{stream_number} 0 obj\n").as_bytes());
        Value::Dict(dict).write(&mut out);
        out.extend_from_slice(b"\nstream\n");
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    let encrypt = match protection {
        Some(protection) => {
            let number = next;
            next += 1;
            offsets.push(Place::At(out.len()));
            out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
            out.extend_from_slice(protection.dictionary);
            out.extend_from_slice(b"\nendobj\n");
            Some(number)
        }
        None => None,
    };

    let mut trailer = Dict::new();
    trailer.set("Size", Value::Int(i64::from(next)));
    if let Some(root) = doc.root().and_then(&map) {
        trailer.set("Root", Value::Ref(root));
    }
    if let Some(info) = doc
        .trailer
        .get("Info")
        .and_then(Value::as_ref)
        .and_then(&map)
    {
        trailer.set("Info", Value::Ref(info));
    }
    if let Some(number) = encrypt {
        trailer.set("Encrypt", Value::Ref(number));
    }
    let id = match doc.trailer.get("ID") {
        Some(Value::Array(items))
            if items.len() == 2 && items.iter().all(|i| matches!(i, Value::Str(_))) =>
        {
            Value::Array(items.clone())
        }
        _ => {
            let hash = identifier(&out);
            Value::Array(vec![Value::Str(hash.clone()), Value::Str(hash)])
        }
    };
    trailer.set("ID", id);

    if compress {
        let xref_number = next;
        offsets.push(Place::At(out.len()));
        let mut rows = Vec::with_capacity(offsets.len() * 7);
        for place in &offsets {
            match place {
                Place::Free => rows.extend_from_slice(&[0, 0, 0, 0, 0, 0xff, 0xff]),
                Place::At(offset) => {
                    rows.push(1);
                    rows.extend_from_slice(
                        &u32::try_from(*offset)
                            .map_err(|_| "the file is over 4 GB")?
                            .to_be_bytes(),
                    );
                    rows.extend_from_slice(&[0, 0]);
                }
                Place::InStream(stream, index) => {
                    rows.push(2);
                    rows.extend_from_slice(&stream.to_be_bytes());
                    rows.extend_from_slice(
                        &u16::try_from(*index)
                            .map_err(|_| "object stream too large")?
                            .to_be_bytes(),
                    );
                }
            }
        }
        let data = deflate(&rows);
        let mut dict = trailer;
        dict.set("Size", Value::Int(i64::from(xref_number) + 1));
        dict.set("Type", Value::name("XRef"));
        dict.set(
            "W",
            Value::Array(vec![Value::Int(1), Value::Int(4), Value::Int(2)]),
        );
        dict.set("Filter", Value::name("FlateDecode"));
        dict.set("Length", Value::Int(len(data.len())));
        let start = out.len();
        out.extend_from_slice(format!("{xref_number} 0 obj\n").as_bytes());
        Value::Dict(dict).write(&mut out);
        out.extend_from_slice(b"\nstream\n");
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        out.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
    } else {
        let start = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", offsets.len()).as_bytes());
        for place in &offsets {
            match place {
                Place::At(offset) => {
                    out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
                }
                _ => out.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        out.extend_from_slice(b"trailer\n");
        Value::Dict(trailer).write(&mut out);
        out.extend_from_slice(format!("\nstartxref\n{start}\n%%EOF\n").as_bytes());
    }
    Ok(Written {
        bytes: out,
        objects: count,
        dropped: doc.objects.len().saturating_sub(count),
    })
}

#[derive(Clone, Copy)]
enum Place {
    Free,
    At(usize),
    InStream(u32, u32),
}

fn len(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn version_below(version: &str, major: u32, minor: u32) -> bool {
    let mut parts = version.split('.');
    let a: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(1);
    let b: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(4);
    (a, b) < (major, minor)
}

#[must_use]
pub fn identifier(bytes: &[u8]) -> Vec<u8> {
    let mut a: u64 = 0xcbf2_9ce4_8422_2325;
    let mut b: u64 = 0x6c62_272e_07bb_0142;
    for &byte in bytes {
        a = (a ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        b = (b ^ u64::from(byte))
            .wrapping_mul(0x0000_0100_0000_01b3)
            .rotate_left(5);
    }
    let mut out = a.to_be_bytes().to_vec();
    out.extend_from_slice(&b.to_be_bytes());
    out
}
