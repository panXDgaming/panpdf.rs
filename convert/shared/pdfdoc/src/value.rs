use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
    Str(Vec<u8>),
    Array(Vec<Value>),
    Dict(Dict),
    Ref(u32),
}

pub type StringMap<'a, E> = dyn FnMut(&[u8]) -> Result<Vec<u8>, E> + 'a;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dict(pub Vec<(Vec<u8>, Value)>);

impl Dict {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.as_slice() == key.as_bytes())
            .map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0
            .iter_mut()
            .find(|(k, _)| k.as_slice() == key.as_bytes())
            .map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(slot) = self.get_mut(key) {
            *slot = value;
        } else {
            self.0.push((key.as_bytes().to_vec(), value));
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self
            .0
            .iter()
            .position(|(k, _)| k.as_slice() == key.as_bytes())?;
        Some(self.0.remove(at).1)
    }

    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    #[must_use]
    pub fn name(&self, key: &str) -> Option<&[u8]> {
        match self.get(key) {
            Some(Value::Name(name)) => Some(name),
            _ => None,
        }
    }

    #[must_use]
    pub fn is(&self, key: &str, name: &str) -> bool {
        self.name(key) == Some(name.as_bytes())
    }
}

impl Value {
    #[must_use]
    pub fn name(name: &str) -> Self {
        Self::Name(name.as_bytes().to_vec())
    }

    #[must_use]
    pub fn string(text: &str) -> Self {
        Self::Str(text.as_bytes().to_vec())
    }

    #[must_use]
    pub const fn as_dict(&self) -> Option<&Dict> {
        match self {
            Self::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    pub const fn as_dict_mut(&mut self) -> Option<&mut Dict> {
        match self {
            Self::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "PDF numbers are small")]
    pub const fn as_number(&self) -> Option<f64> {
        match self {
            Self::Int(n) => Some(*n as f64),
            Self::Real(n) => Some(*n),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_ref(&self) -> Option<u32> {
        match self {
            Self::Ref(n) => Some(*n),
            _ => None,
        }
    }

    pub fn references(&self, out: &mut Vec<u32>) {
        match self {
            Self::Ref(n) => out.push(*n),
            Self::Array(items) => items.iter().for_each(|item| item.references(out)),
            Self::Dict(dict) => dict.0.iter().for_each(|(_, v)| v.references(out)),
            _ => {}
        }
    }

    pub fn renumber(&mut self, map: &dyn Fn(u32) -> Option<u32>) {
        match self {
            Self::Ref(n) => match map(*n) {
                Some(to) => *n = to,
                None => *self = Self::Null,
            },
            Self::Array(items) => items.iter_mut().for_each(|item| item.renumber(map)),
            Self::Dict(dict) => dict.0.iter_mut().for_each(|(_, v)| v.renumber(map)),
            _ => {}
        }
    }

    pub fn map_strings<E>(&mut self, f: &mut StringMap<'_, E>) -> Result<(), E> {
        match self {
            Self::Str(bytes) => {
                *bytes = f(bytes)?;
                Ok(())
            }
            Self::Array(items) => items.iter_mut().try_for_each(|item| item.map_strings(f)),
            Self::Dict(dict) => dict.0.iter_mut().try_for_each(|(_, v)| v.map_strings(f)),
            _ => Ok(()),
        }
    }

    pub fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Null => token(out, b"null"),
            Self::Bool(b) => token(out, if *b { b"true" } else { b"false" }),
            Self::Int(n) => token(out, n.to_string().as_bytes()),
            Self::Real(n) => token(out, real(*n).as_bytes()),
            Self::Name(name) => write_name(name, out),
            Self::Str(bytes) => write_string(bytes, out),
            Self::Array(items) => {
                out.push(b'[');
                for item in items {
                    item.write(out);
                }
                out.push(b']');
            }
            Self::Dict(dict) => {
                out.extend_from_slice(b"<<");
                for (key, value) in &dict.0 {
                    write_name(key, out);
                    value.write(out);
                }
                out.extend_from_slice(b">>");
            }
            Self::Ref(n) => token(out, format!("{n} 0 R").as_bytes()),
        }
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }
}

#[must_use]
pub fn real(n: f64) -> String {
    if !n.is_finite() {
        return "0".to_owned();
    }
    let mut text = format!("{n:.6}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_owned();
    }
    text
}

const fn ends_token(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')'
            | b'<'
            | b'>'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'/'
            | b'%'
            | b' '
            | b'\n'
            | b'\r'
            | b'\t'
            | 0
            | 0x0c
    )
}

fn token(out: &mut Vec<u8>, text: &[u8]) {
    if let (Some(&last), Some(&next)) = (out.last(), text.first())
        && !ends_token(last)
        && !ends_token(next)
    {
        out.push(b' ');
    }
    out.extend_from_slice(text);
}

fn write_name(name: &[u8], out: &mut Vec<u8>) {
    out.push(b'/');
    for &byte in name {
        let plain = (0x21..=0x7e).contains(&byte) && !b"#()<>[]{}/%".contains(&byte);
        if plain {
            out.push(byte);
        } else {
            let mut text = String::new();
            let _ = write!(text, "#{byte:02X}");
            out.extend_from_slice(text.as_bytes());
        }
    }
}

fn write_string(bytes: &[u8], out: &mut Vec<u8>) {
    let printable = bytes
        .iter()
        .all(|&b| (0x20..0x7f).contains(&b) || b == b'\n' || b == b'\r' || b == b'\t');
    if printable {
        out.push(b'(');
        for &byte in bytes {
            match byte {
                b'(' | b')' | b'\\' => {
                    out.push(b'\\');
                    out.push(byte);
                }
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                _ => out.push(byte),
            }
        }
        out.push(b')');
    } else {
        out.push(b'<');
        for &byte in bytes {
            out.extend_from_slice(format!("{byte:02X}").as_bytes());
        }
        out.push(b'>');
    }
}

#[must_use]
pub fn text_string(text: &str) -> Vec<u8> {
    if text.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return text.as_bytes().to_vec();
    }
    let mut out = vec![0xfe, 0xff];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

#[must_use]
pub fn read_text_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    bytes.iter().map(|&b| char::from(b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_written_as_pdf() {
        let mut dict = Dict::new();
        dict.set("Type", Value::name("Page"));
        dict.set("A b", Value::Str(b"x(y)\\".to_vec()));
        dict.set("K", Value::Array(vec![Value::Ref(3), Value::Real(0.5)]));
        dict.set("H", Value::Str(vec![0, 255]));
        dict.set("N", Value::Null);
        dict.set("T", Value::Array(vec![Value::Bool(true), Value::Int(1)]));
        let text = String::from_utf8(Value::Dict(dict).to_bytes()).unwrap();
        assert_eq!(
            text,
            "<</Type/Page/A#20b(x\\(y\\)\\\\)/K[3 0 R 0.5]/H<00FF>/N null/T[true 1]>>"
        );
    }

    #[test]
    fn reals_have_no_exponent() {
        assert_eq!(real(1.0), "1");
        assert_eq!(real(-0.000_000_1), "0");
        assert_eq!(real(12.25), "12.25");
        assert_eq!(real(1e20), "100000000000000000000");
    }

    #[test]
    fn text_strings_round_trip() {
        for text in ["plain", "ລາວ ไทย"] {
            assert_eq!(read_text_string(&text_string(text)), text);
        }
    }
}
