mod icc;

use convert_pdfdoc::{Dict, Doc, Load, Object, Value};
use convert_pdftool::{Job, Output, Tool};

pub static TOOL: Tool = Tool {
    name: "pdf-to-pdfa",
    extension: "pdf",
    inputs: 1,
    file_options: &[],
    flags: &[],
    help: "  --password PW   when the file is protected",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

const FORBIDDEN_ACTIONS: &[&str] = &[
    "JavaScript",
    "Launch",
    "Sound",
    "Movie",
    "ResetForm",
    "ImportData",
    "Hide",
    "SetOCGState",
    "Rendition",
    "Trans",
    "GoTo3DView",
];

const FORBIDDEN_ANNOTATIONS: &[&str] = &[
    "Sound",
    "Movie",
    "Screen",
    "3D",
    "FileAttachment",
    "RichMedia",
];

fn action_forbidden(doc: &Doc, action: &Value) -> bool {
    match doc.resolve(action) {
        Value::Dict(d) => {
            let kind = d.name("S").map(|s| String::from_utf8_lossy(s).into_owned());
            let named_ok = !d.is("S", "Named")
                || ["NextPage", "PrevPage", "FirstPage", "LastPage"]
                    .iter()
                    .any(|n| d.is("N", n));
            kind.is_some_and(|k| FORBIDDEN_ACTIONS.contains(&k.as_str())) || !named_ok
        }
        _ => false,
    }
}

#[derive(Default, Debug)]
pub struct Fixed {
    pub actions: usize,
    pub annotations_removed: usize,
    pub annotations_flagged: usize,
    pub attachments: usize,
    pub images: usize,
    pub states: usize,
    pub other: Vec<String>,
}

fn fix(doc: &mut Doc) -> Fixed {
    let mut fixed = Fixed::default();
    let numbers: Vec<u32> = doc.objects.keys().copied().collect();
    for n in numbers {
        let forbidden_action = doc
            .get(n)
            .and_then(Object::dict)
            .map(|d| {
                (
                    d.get("A").is_some_and(|a| action_forbidden(doc, a)),
                    d.get("Next").is_some_and(|a| action_forbidden(doc, a)),
                )
            })
            .unwrap_or((false, false));
        let Some(object) = doc.get_mut(n) else {
            continue;
        };
        let is_stream = object.stream.is_some();
        let Some(d) = object.dict_mut() else { continue };
        if forbidden_action.0 {
            d.remove("A");
            fixed.actions += 1;
        }
        if forbidden_action.1 {
            d.remove("Next");
            fixed.actions += 1;
        }
        if d.remove("AA").is_some() {
            fixed.actions += 1;
        }
        if is_stream {
            for key in ["F", "FFilter", "FDecodeParms"] {
                d.remove(key);
            }
        }
        if d.is("Subtype", "Image") || d.has("Interpolate") {
            let mut changed = false;
            if matches!(d.get("Interpolate"), Some(Value::Bool(true))) {
                d.set("Interpolate", Value::Bool(false));
                changed = true;
            }
            changed |= d.remove("Alternates").is_some();
            changed |= d.remove("OPI").is_some();
            fixed.images += usize::from(changed);
        }
        if d.is("Type", "ExtGState") || d.has("TR") || d.has("HTP") {
            let mut changed = d.remove("TR").is_some() | d.remove("HTP").is_some();
            if d.get("TR2")
                .is_some_and(|v| !matches!(v, Value::Name(n) if n == b"Default"))
            {
                d.remove("TR2");
                changed = true;
            }
            fixed.states += usize::from(changed);
        }
    }
    let postscript: Vec<u32> = doc
        .objects
        .iter()
        .filter(|(_, o)| {
            o.dict()
                .is_some_and(|d| d.is("Subtype", "PS") || d.is("Subtype2", "PS"))
        })
        .map(|(n, _)| *n)
        .collect();
    if !postscript.is_empty() {
        for object in doc.objects.values_mut() {
            strip_refs(&mut object.value, &postscript);
        }
        fixed
            .other
            .push(format!("{} PostScript objects removed", postscript.len()));
    }

    let names_ref = doc
        .catalog()
        .and_then(|c| c.get("Names"))
        .and_then(Value::as_ref);
    let open_forbidden = doc
        .catalog()
        .and_then(|c| c.get("OpenAction"))
        .is_some_and(|a| action_forbidden(doc, a));
    if let Some(catalog) = doc.catalog_mut() {
        if open_forbidden {
            catalog.remove("OpenAction");
            fixed.actions += 1;
        }
        if catalog.remove("AA").is_some() {
            fixed.actions += 1;
        }
        catalog.remove("Collection");
        if let Some(Value::Dict(names)) = catalog.get_mut("Names") {
            strip_names(names, &mut fixed);
        }
    }
    if let Some(n) = names_ref
        && let Some(names) = doc.get_mut(n).and_then(Object::dict_mut)
    {
        strip_names(names, &mut fixed);
    }
    let form_ref = doc
        .catalog()
        .and_then(|c| c.get("AcroForm"))
        .and_then(Value::as_ref);
    let form = match form_ref {
        Some(n) => doc.get_mut(n).and_then(Object::dict_mut),
        None => doc
            .catalog_mut()
            .and_then(|c| c.get_mut("AcroForm"))
            .and_then(Value::as_dict_mut),
    };
    if let Some(form) = form {
        if form.remove("XFA").is_some() {
            fixed
                .other
                .push("the XFA form removed (the AcroForm stays)".into());
        }
        form.remove("NeedAppearances");
    }
    let oc_ref = doc
        .catalog()
        .and_then(|c| c.get("OCProperties"))
        .and_then(Value::as_ref);
    let oc = match oc_ref {
        Some(n) => doc.get_mut(n).and_then(Object::dict_mut),
        None => doc
            .catalog_mut()
            .and_then(|c| c.get_mut("OCProperties"))
            .and_then(Value::as_dict_mut),
    };
    if let Some(oc) = oc {
        if let Some(Value::Dict(d)) = oc.get_mut("D") {
            d.remove("AS");
            if !d.has("Name") {
                d.set("Name", Value::string("Default"));
            }
        }
        if let Some(Value::Array(configs)) = oc.get_mut("Configs") {
            for (i, c) in configs.iter_mut().enumerate() {
                if let Value::Dict(d) = c {
                    d.remove("AS");
                    if !d.has("Name") {
                        d.set("Name", Value::string(&format!("Configuration {}", i + 1)));
                    }
                }
            }
        }
    }
    for page in doc.pages() {
        let annots: Vec<Value> = doc
            .get(page.number)
            .and_then(Object::dict)
            .and_then(|d| d.get("Annots"))
            .map(|v| doc.resolve(v).clone())
            .and_then(|v| v.as_array().map(<[Value]>::to_vec))
            .unwrap_or_default();
        let mut kept = Vec::new();
        for a in annots {
            let Some(n) = a.as_ref() else {
                kept.push(a);
                continue;
            };
            let Some(d) = doc.get_mut(n).and_then(Object::dict_mut) else {
                continue;
            };
            let subtype = d
                .name("Subtype")
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .unwrap_or_default();
            if FORBIDDEN_ANNOTATIONS.contains(&subtype.as_str()) {
                fixed.annotations_removed += 1;
                continue;
            }
            if subtype != "Popup" {
                let flags = match d.get("F") {
                    Some(Value::Int(f)) => *f,
                    _ => 0,
                };
                let wanted = (flags | 4) & !(1 | 2 | 32 | 256);
                if wanted != flags {
                    d.set("F", Value::Int(wanted));
                    fixed.annotations_flagged += 1;
                }
            }
            if let Some(Value::Dict(ap)) = d.get_mut("AP") {
                ap.remove("D");
                ap.remove("R");
            }
            kept.push(a);
        }
        if let Some(d) = doc.get_mut(page.number).and_then(Object::dict_mut) {
            if kept.is_empty() {
                d.remove("Annots");
            } else {
                d.set("Annots", Value::Array(kept));
            }
            if d.remove("AA").is_some() {
                fixed.actions += 1;
            }
        }
    }
    fixed
}

fn strip_names(names: &mut Dict, fixed: &mut Fixed) {
    if names.remove("JavaScript").is_some() {
        fixed.actions += 1;
    }
    if names.remove("EmbeddedFiles").is_some() {
        fixed.attachments += 1;
    }
}

fn strip_refs(value: &mut Value, gone: &[u32]) {
    match value {
        Value::Dict(d) => {
            d.0.retain(|(_, v)| !matches!(v, Value::Ref(n) if gone.contains(n)));
            d.0.iter_mut().for_each(|(_, v)| strip_refs(v, gone));
        }
        Value::Array(items) => items.iter_mut().for_each(|v| strip_refs(v, gone)),
        _ => {}
    }
}

#[must_use]
pub fn problems(doc: &Doc) -> Vec<String> {
    let mut out = Vec::new();
    let mut unembedded = std::collections::BTreeSet::new();
    let mut lzw = 0;
    let mut cmyk = false;
    let mut no_appearance = 0;
    for object in doc.objects.values() {
        let Some(d) = object.dict() else { continue };
        if d.is("Type", "Font") || (d.has("BaseFont") && d.has("Subtype")) {
            let subtype = d.name("Subtype").unwrap_or_default();
            if subtype != b"Type3"
                && subtype != b"Type0"
                && !d.is("Subtype", "CIDFontType0")
                && !d.is("Subtype", "CIDFontType2")
            {
                let embedded = doc.dict_at(d, "FontDescriptor").is_some_and(|fd| {
                    fd.has("FontFile") || fd.has("FontFile2") || fd.has("FontFile3")
                });
                if !embedded {
                    unembedded.insert(
                        d.name("BaseFont")
                            .map(|n| String::from_utf8_lossy(n).into_owned())
                            .unwrap_or_else(|| "(unnamed)".into()),
                    );
                }
            }
        }
        if object.stream.is_some() {
            let lzw_here = match d.get("Filter").map(|v| doc.resolve(v)) {
                Some(Value::Name(n)) => n == b"LZWDecode" || n == b"LZW",
                Some(Value::Array(items)) => items
                    .iter()
                    .any(|i| matches!(doc.resolve(i), Value::Name(n) if n == b"LZWDecode" || n == b"LZW")),
                _ => false,
            };
            lzw += usize::from(lzw_here);
        }
        if d.is("Subtype", "Widget") || (d.has("Rect") && d.has("Subtype") && !d.is("Type", "Page"))
        {
            let exempt = d.is("Subtype", "Popup") || d.is("Subtype", "Link");
            let flat = doc
                .rect(d, "Rect")
                .is_some_and(|r| (r[2] - r[0]).abs() < 1e-6 || (r[3] - r[1]).abs() < 1e-6);
            if !exempt && !flat && !d.has("AP") {
                no_appearance += 1;
            }
        }
    }
    for object in doc.objects.values() {
        let Some(d) = object.dict() else { continue };
        if d.is("Subtype", "Type0")
            && let Some(Value::Array(kids)) = d.get("DescendantFonts").map(|v| doc.resolve(v))
        {
            for kid in kids {
                if let Value::Dict(k) = doc.resolve(kid) {
                    let embedded = doc.dict_at(k, "FontDescriptor").is_some_and(|fd| {
                        fd.has("FontFile") || fd.has("FontFile2") || fd.has("FontFile3")
                    });
                    if !embedded {
                        unembedded.insert(
                            d.name("BaseFont")
                                .map(|n| String::from_utf8_lossy(n).into_owned())
                                .unwrap_or_default(),
                        );
                    }
                }
            }
        }
    }
    let mut names = Vec::new();
    for object in doc.objects.values() {
        collect_names(&object.value, &mut names);
        if names.iter().any(|n| n == b"DeviceCMYK") {
            cmyk = true;
            break;
        }
    }
    if !cmyk {
        for page in doc.pages() {
            let Some(pd) = doc.get(page.number).and_then(Object::dict) else {
                continue;
            };
            let contents: Vec<u32> = match pd.get("Contents") {
                Some(Value::Ref(n)) => match doc.get(*n).map(|o| &o.value) {
                    Some(Value::Array(items)) => items.iter().filter_map(Value::as_ref).collect(),
                    _ => vec![*n],
                },
                Some(Value::Array(items)) => items.iter().filter_map(Value::as_ref).collect(),
                _ => Vec::new(),
            };
            for c in contents {
                if let Some(bytes) = doc.get(c).and_then(|o| doc.decoded(o))
                    && sets_cmyk(&bytes)
                {
                    cmyk = true;
                }
            }
        }
    }
    if !unembedded.is_empty() {
        let list: Vec<String> = unembedded.into_iter().take(12).collect();
        out.push(format!(
            "fonts not embedded (6.2.11.4): {} -- a PDF/A reader may draw them differently",
            list.join(", ")
        ));
    }
    if cmyk {
        out.push(
            "device CMYK colour is used and the output intent is RGB (6.2.4.3): a CMYK output \
             intent needs a CMYK profile this tool does not carry"
                .into(),
        );
    }
    if lzw > 0 {
        out.push(format!("{lzw} streams compressed with LZW (6.1.7.2)"));
    }
    if no_appearance > 0 {
        out.push(format!(
            "{no_appearance} annotations without an appearance (6.3.3)"
        ));
    }
    out
}

fn collect_names(value: &Value, out: &mut Vec<Vec<u8>>) {
    match value {
        Value::Name(n) => out.push(n.clone()),
        Value::Array(items) => items.iter().for_each(|i| collect_names(i, out)),
        Value::Dict(d) => d.0.iter().for_each(|(_, v)| collect_names(v, out)),
        _ => {}
    }
}

fn sets_cmyk(bytes: &[u8]) -> bool {
    bytes.windows(3).any(|w| {
        (w[1] == b'k' || w[1] == b'K')
            && matches!(w[0], b' ' | b'\n' | b'\r' | b'\t')
            && matches!(w[2], b' ' | b'\n' | b'\r' | b'\t')
    })
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[must_use]
pub fn iso_date(pdf: &str) -> Option<String> {
    let s = pdf.trim().strip_prefix("D:").unwrap_or(pdf.trim());
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return None;
    }
    let part = |from: usize, to: usize, default: &str| -> String {
        digits
            .get(from..to)
            .map_or_else(|| default.to_owned(), str::to_owned)
    };
    let mut out = format!(
        "{}-{}-{}T{}:{}:{}",
        part(0, 4, "0000"),
        part(4, 6, "01"),
        part(6, 8, "01"),
        part(8, 10, "00"),
        part(10, 12, "00"),
        part(12, 14, "00")
    );
    let rest = &s[digits.len()..];
    match rest.chars().next() {
        Some('Z') | None => out.push('Z'),
        Some(sign @ ('+' | '-')) => {
            let tz: String = rest[1..].chars().filter(char::is_ascii_digit).collect();
            let hh = tz.get(0..2).unwrap_or("00");
            let mm = tz.get(2..4).unwrap_or("00");
            out.push_str(&format!("{sign}{hh}:{mm}"));
        }
        Some(_) => out.push('Z'),
    }
    Some(out)
}

fn xmp(info: &Dict) -> String {
    let get = |key: &str| match info.get(key) {
        Some(Value::Str(s)) => Some(convert_pdfdoc::read_text_string(s)),
        _ => None,
    };
    let mut body = String::new();
    if let Some(t) = get("Title") {
        body.push_str(&format!(
            "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>",
            xml(&t)
        ));
    }
    if let Some(a) = get("Author") {
        body.push_str(&format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            xml(&a)
        ));
    }
    if let Some(s) = get("Subject") {
        body.push_str(&format!(
            "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>",
            xml(&s)
        ));
    }
    if let Some(k) = get("Keywords") {
        body.push_str(&format!("<pdf:Keywords>{}</pdf:Keywords>", xml(&k)));
    }
    if let Some(p) = get("Producer") {
        body.push_str(&format!("<pdf:Producer>{}</pdf:Producer>", xml(&p)));
    }
    if let Some(c) = get("Creator") {
        body.push_str(&format!("<xmp:CreatorTool>{}</xmp:CreatorTool>", xml(&c)));
    }
    if let Some(d) = get("CreationDate").and_then(|d| iso_date(&d)) {
        body.push_str(&format!("<xmp:CreateDate>{d}</xmp:CreateDate>"));
    }
    if let Some(d) = get("ModDate").and_then(|d| iso_date(&d)) {
        body.push_str(&format!("<xmp:ModifyDate>{d}</xmp:ModifyDate>"));
    }
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" \
         xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">\n\
         <pdfaid:part>2</pdfaid:part><pdfaid:conformance>B</pdfaid:conformance>{body}\n\
         </rdf:Description></rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

pub fn convert(doc: &mut Doc) -> Result<(Fixed, Vec<String>), String> {
    let mut fixed = fix(doc);
    if doc.version.as_str() > "1.7" || doc.version.is_empty() {
        doc.version = "1.7".into();
    }
    let info_ref = doc.trailer.get("Info").and_then(Value::as_ref);
    let mut info = info_ref
        .and_then(|n| doc.get(n))
        .and_then(Object::dict)
        .cloned()
        .unwrap_or_default();
    info.0.retain(|(k, v)| {
        matches!(
            k.as_slice(),
            b"Title"
                | b"Author"
                | b"Subject"
                | b"Keywords"
                | b"Creator"
                | b"Producer"
                | b"CreationDate"
                | b"ModDate"
        ) && matches!(v, Value::Str(_) | Value::Name(_))
    });
    for key in ["CreationDate", "ModDate"] {
        if let Some(Value::Str(s)) = info.get(key)
            && iso_date(&convert_pdfdoc::read_text_string(s)).is_none()
        {
            info.remove(key);
        }
    }
    let packet = xmp(&info);
    if !info.0.is_empty() {
        match info_ref {
            Some(n) => {
                if let Some(o) = doc.get_mut(n) {
                    o.value = Value::Dict(info);
                }
            }
            None => {
                let n = doc.add(Object::new(Value::Dict(info)));
                doc.trailer.set("Info", Value::Ref(n));
            }
        }
    } else {
        doc.trailer.remove("Info");
    }
    let mut meta = Dict::new();
    meta.set("Type", Value::name("Metadata"));
    meta.set("Subtype", Value::name("XML"));
    let metadata = doc.add(Object::stream(meta, packet.into_bytes()));
    let mut profile = Dict::new();
    profile.set("N", Value::Int(3));
    profile.set("Filter", Value::name("FlateDecode"));
    let profile = doc.add(Object::stream(
        profile,
        convert_pdfdoc::deflate(&icc::srgb()),
    ));
    let mut intent = Dict::new();
    intent.set("Type", Value::name("OutputIntent"));
    intent.set("S", Value::name("GTS_PDFA1"));
    intent.set(
        "OutputConditionIdentifier",
        Value::string("sRGB IEC61966-2.1"),
    );
    intent.set("Info", Value::string("sRGB IEC61966-2.1"));
    intent.set("RegistryName", Value::string("http://www.color.org"));
    intent.set("DestOutputProfile", Value::Ref(profile));
    let catalog = doc.catalog_mut().ok_or("the document has no catalogue")?;
    if catalog.has("OutputIntents") {
        fixed
            .other
            .push("the file's own output intents replaced by sRGB".into());
    }
    catalog.set("OutputIntents", Value::Array(vec![Value::Dict(intent)]));
    catalog.set("Metadata", Value::Ref(metadata));
    catalog.remove("Perms");
    catalog.remove("Requirements");
    let still = problems(doc);
    Ok((fixed, still))
}

pub fn run(job: &Job) -> Result<Output, String> {
    let mut doc = convert_pdfdoc::load(
        job.pdf()?.to_vec(),
        &Load {
            password: job.password(),
            recover: true,
            everything: false,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    if doc.recovered {
        notes.push(format!(
            "the file's cross-reference was damaged and was rebuilt to read it ({} repairs)",
            doc.notes.len()
        ));
    }
    if doc.was_encrypted {
        notes.push("protection removed (PDF/A forbids encryption)".into());
    }
    if convert_pdfdoc::is_signed(&doc) {
        notes.push("the file's digital signatures do not survive the conversion".into());
    }
    let pages = doc.pages().len();
    let (fixed, _) = convert(&mut doc)?;
    let written = convert_pdfdoc::write(&doc, None, true)?;
    convert_pdfdoc::check_written(&written.bytes, b"", pages)?;
    let again =
        convert_pdfdoc::load(written.bytes.clone(), &Load::default()).map_err(|e| e.to_string())?;
    let still = problems(&again);
    notes.push(format!(
        "PDF/A-2b: sRGB output intent and XMP identification written; {} forbidden actions, {} annotations and {} attachments removed, {} annotations set to print, {} images and {} graphics states corrected",
        fixed.actions, fixed.annotations_removed, fixed.attachments, fixed.annotations_flagged, fixed.images, fixed.states
    ));
    notes.extend(fixed.other);
    if still.is_empty() {
        notes.push(
            "no remaining problem found by this tool's own check (not a veraPDF validation)".into(),
        );
    } else {
        notes.push("NOT fully conformant:".into());
        notes.extend(still.into_iter().map(|p| format!("  {p}")));
    }
    Ok(Output {
        main: written.bytes,
        attachments: Vec::new(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_become_iso() {
        assert_eq!(
            iso_date("D:20260924171500+07'00'").as_deref(),
            Some("2026-09-24T17:15:00+07:00")
        );
        assert_eq!(iso_date("D:2026").as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(iso_date("garbage"), None);
    }

    #[test]
    fn cmyk_operators_are_seen() {
        assert!(sets_cmyk(b"0 0 0 1 k\n"));
        assert!(!sets_cmyk(b"/Mk Do"));
    }
}
