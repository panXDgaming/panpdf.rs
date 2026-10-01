pub mod build;
pub mod css;
pub mod html;
pub mod style;

pub fn run(
    inputs: &[convert_structure::bytes_tool::Input],
    _settings: &convert_structure::bytes_tool::Settings,
    fonts: &convert_structure::bytes_tool::Fonts,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<convert_structure::bytes_tool::Files, String> {
    let provider = fonts.layout_provider();
    let is_page = |name: &str| {
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".html") || lower.ends_with(".htm") || lower.ends_with(".xhtml")
    };
    let assets = |wanted: &str| -> Option<Vec<u8>> {
        let wanted = percent_decode(wanted.split(['?', '#']).next().unwrap_or(wanted));
        let wanted = wanted.trim_start_matches("./");
        let last = wanted.rsplit('/').next().unwrap_or(wanted);
        inputs
            .iter()
            .find(|i| i.name == wanted)
            .or_else(|| {
                inputs
                    .iter()
                    .find(|i| i.name.rsplit(['/', '\\']).next() == Some(last))
            })
            .map(|i| i.bytes.clone())
    };
    let pages: Vec<&convert_structure::bytes_tool::Input> =
        inputs.iter().filter(|i| is_page(&i.name)).collect();
    if pages.is_empty() {
        return Err("no .html file among the inputs".into());
    }
    let mut made = convert_structure::bytes_tool::Files::default();
    let mut failures = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        if !progress(index, pages.len()) {
            return Err("cancelled".into());
        }
        match convert(&page.bytes, &assets, provider.clone()) {
            Ok((pdf, notes)) => {
                let base = page.name.rsplit(['/', '\\']).next().unwrap_or(&page.name);
                let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
                made.files.push((format!("{stem}.pdf"), pdf));
                made.notes
                    .extend(notes.into_iter().map(|n| format!("{}: {n}", page.name)));
            }
            Err(why) => failures.push(format!("{}: {why}", page.name)),
        }
    }
    progress(pages.len(), pages.len());
    if made.files.is_empty() && !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    made.notes.extend(failures);
    Ok(made)
}

convert_wasm::export_bytes_tool!(crate::run);

pub fn convert(
    page: &[u8],
    assets: &dyn Fn(&str) -> Option<Vec<u8>>,
    fonts: Option<std::sync::Arc<dyn convert_structure::FontProvider>>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let text = decode(page);
    let dom = html::parse(&text);
    let (document, notes) = build::build(&dom, assets);
    let book = convert_layout::font_book(fonts);
    let pdf = convert_layout::render(&document, book)?;
    Ok((pdf, notes))
}

#[must_use]
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let little = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    if head.contains("tis-620") || head.contains("windows-874") || head.contains("iso-8859-11") {
        return bytes
            .iter()
            .map(|b| match b {
                0xA1..=0xFB => char::from_u32(0x0E01 + u32::from(b - 0xA1)).unwrap_or('\u{FFFD}'),
                _ => char::from(*b),
            })
            .collect();
    }
    bytes.iter().map(|b| char::from(*b)).collect()
}

#[must_use]
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && at + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&text[at + 1..at + 3], 16)
        {
            out.push(v);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
