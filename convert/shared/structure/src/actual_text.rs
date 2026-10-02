use std::collections::HashMap;

use pdf_bytes::ByteStore;
use pdf_paint::MarkedProperties;
use pdf_session::PageView;

pub type ActualTexts = HashMap<usize, (String, String)>;

fn pdf_string(bytes: &[u8]) -> Option<Vec<u8>> {
    let bytes = bytes.trim_ascii_start();
    match bytes.first()? {
        b'<' => {
            let end = bytes.iter().position(|b| *b == b'>')?;
            let digits: Vec<u8> = bytes[1..end]
                .iter()
                .copied()
                .filter(u8::is_ascii_hexdigit)
                .collect();
            let mut out = Vec::with_capacity(digits.len() / 2 + 1);
            for pair in digits.chunks(2) {
                let hi = char::from(pair[0]).to_digit(16)?;
                let lo = pair
                    .get(1)
                    .map_or(Some(0), |b| char::from(*b).to_digit(16))?;
                out.push(u8::try_from(hi * 16 + lo).ok()?);
            }
            Some(out)
        }
        b'(' => {
            let mut out = Vec::new();
            let mut depth = 0_usize;
            let mut at = 1;
            while at < bytes.len() {
                let b = bytes[at];
                match b {
                    b'\\' => {
                        at += 1;
                        let next = *bytes.get(at)?;
                        match next {
                            b'n' => out.push(b'\n'),
                            b'r' => out.push(b'\r'),
                            b't' => out.push(b'\t'),
                            b'b' => out.push(8),
                            b'f' => out.push(12),
                            b'0'..=b'7' => {
                                let mut value = u32::from(next - b'0');
                                for _ in 0..2 {
                                    match bytes.get(at + 1) {
                                        Some(d @ b'0'..=b'7') => {
                                            value = value * 8 + u32::from(d - b'0');
                                            at += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                out.push(u8::try_from(value & 0xFF).unwrap_or(0));
                            }
                            b'\r' | b'\n' => {}
                            other => out.push(other),
                        }
                    }
                    b'(' => {
                        depth += 1;
                        out.push(b);
                    }
                    b')' => {
                        if depth == 0 {
                            return Some(out);
                        }
                        depth -= 1;
                        out.push(b);
                    }
                    _ => out.push(b),
                }
                at += 1;
            }
            None
        }
        _ => None,
    }
}

fn text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    bytes.iter().map(|b| char::from(*b)).collect()
}

#[must_use]
pub fn actual_text_in(dictionary: &[u8]) -> Option<String> {
    let at = dictionary
        .windows(11)
        .position(|window| window == b"/ActualText")?;
    let value = pdf_string(&dictionary[at + 11..])?;
    Some(text_string(&value))
}

fn bytes_of<'a>(
    view: &'a PageView,
    file: Option<&'a ByteStore>,
    span: pdf_bytes::SourceSpan,
) -> Option<&'a [u8]> {
    view.program
        .streams
        .iter()
        .find_map(|stream| stream.bytes.resolve(span).ok())
        .or_else(|| file.and_then(|file| file.resolve(span).ok()))
}

#[must_use]
pub fn read(view: &PageView, file: Option<&ByteStore>) -> ActualTexts {
    let mut out = ActualTexts::new();
    for (index, atom) in view.graph.atoms.iter().enumerate() {
        if !matches!(atom.kind, pdf_paint::PaintAtomKind::Text(_)) {
            continue;
        }
        for mark in atom.marks.iter().rev() {
            let span = match &mark.properties {
                Some(MarkedProperties::Inline(span)) => *span,
                Some(MarkedProperties::Resource {
                    dictionary_span, ..
                }) => *dictionary_span,
                None => continue,
            };
            let Some(bytes) = bytes_of(view, file, span) else {
                continue;
            };
            if let Some(text) = actual_text_in(bytes) {
                out.insert(index, (format!("{:?}", mark.operator_span), text));
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_literal_strings() {
        assert_eq!(
            actual_text_in(b"<< /ActualText <FEFF0E970EBB> >>"),
            Some("ທົ".to_owned())
        );
        assert_eq!(
            actual_text_in(b"<</ActualText (fi)>>"),
            Some("fi".to_owned())
        );
        assert_eq!(
            actual_text_in(b"<</ActualText (\\376\\377\\000A\\000\\(\\000B)>>"),
            Some("A(B".to_owned())
        );
        assert_eq!(actual_text_in(b"<</MCID 3>>"), None);
    }
}
