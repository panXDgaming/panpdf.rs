use super::*;

const CONTENT: &str = "BT /F1 12 Tf 10 50 Td (Hello) Tj ET\n";

pub fn sample() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
            CONTENT.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "(orphan secret)".to_owned(),
        "<< /Title (T\\(1\\)) /Producer (test) >>".to_owned(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (at, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", at + 1).as_bytes());
    }
    let start = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{start}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn strict() -> Load {
    Load::default()
}

#[test]
fn a_document_reads_and_writes_back() {
    let doc = load(sample(), &strict()).unwrap();
    assert_eq!(doc.pages().len(), 1);
    assert_eq!(doc.pages()[0].media_box, [0.0, 0.0, 200.0, 100.0]);
    assert!(
        doc.objects
            .values()
            .all(|o| o.value != Value::string("orphan secret"))
    );
    for compress in [false, true] {
        let written = write(&doc, None, compress).unwrap();
        let again = load(written.bytes.clone(), &strict()).unwrap();
        assert_eq!(again.pages().len(), 1);
        let info = again.trailer.get("Info").and_then(Value::as_ref).unwrap();
        let title = again
            .get(info)
            .unwrap()
            .dict()
            .unwrap()
            .get("Title")
            .unwrap();
        assert_eq!(title, &Value::string("T(1)"));
        let page = again.get(again.pages()[0].number).unwrap().dict().unwrap();
        let contents = again
            .get(page.get("Contents").unwrap().as_ref().unwrap())
            .unwrap();
        assert_eq!(
            contents.stream.as_deref(),
            Some(&b"BT /F1 12 Tf 10 50 Td (Hello) Tj ET\n"[..])
        );
    }
}

#[test]
fn a_broken_cross_reference_is_rebuilt_by_scanning() {
    let mut bytes = sample();
    let at = bytes.windows(9).rposition(|w| w == b"startxref").unwrap();
    bytes.truncate(at);
    bytes.extend_from_slice(b"startxref\n999999\n%%EOF\n");
    assert!(matches!(
        load(bytes.clone(), &strict()),
        Err(LoadError::Damaged(_))
    ));
    let doc = load(
        bytes,
        &Load {
            recover: true,
            ..Load::default()
        },
    )
    .unwrap();
    assert_eq!(doc.pages().len(), 1);
    assert!(!doc.notes.is_empty());
    let written = write(&doc, None, false).unwrap();
    assert_eq!(load(written.bytes, &strict()).unwrap().pages().len(), 1);
}

#[test]
fn a_file_with_no_trailer_finds_its_catalogue() {
    let bytes = sample();
    let at = bytes.windows(4).rposition(|w| w == b"xref").unwrap();
    let cut = bytes[..at].to_vec();
    let doc = load(
        cut,
        &Load {
            recover: true,
            ..Load::default()
        },
    )
    .unwrap();
    assert_eq!(doc.pages().len(), 1);
}

#[test]
fn objects_alike_are_merged_and_pages_are_not() {
    let mut doc = load(sample(), &strict()).unwrap();
    let font = doc.add(Object::new(Value::Dict(Dict(vec![
        (b"Type".to_vec(), Value::name("Font")),
        (b"Subtype".to_vec(), Value::name("Type1")),
        (b"BaseFont".to_vec(), Value::name("Helvetica")),
    ]))));
    let first = doc.pages()[0].number;
    let mut page = doc.get(first).unwrap().clone();
    let mut fonts = Dict::new();
    fonts.set("F1", Value::Ref(font));
    let mut resources = Dict::new();
    resources.set("Font", Value::Dict(fonts));
    page.dict_mut()
        .unwrap()
        .set("Resources", Value::Dict(resources));
    let second = doc.add(page);
    let tree = doc
        .catalog()
        .unwrap()
        .get("Pages")
        .unwrap()
        .as_ref()
        .unwrap();
    let kids = doc.get_mut(tree).unwrap().dict_mut().unwrap();
    kids.set(
        "Kids",
        Value::Array(vec![Value::Ref(first), Value::Ref(second)]),
    );
    kids.set("Count", Value::Int(2));
    assert!(doc.can_protect().is_ok());
    assert_eq!(doc.dedupe(), 1);
    assert_eq!(doc.pages().len(), 2);
    for compress in [false, true] {
        let written = write(&doc, None, compress).unwrap();
        let again = load(written.bytes, &strict()).unwrap();
        assert_eq!(again.pages().len(), 2);
        let fonts = again
            .objects
            .values()
            .filter(|o| o.dict().is_some_and(|d| d.is("Type", "Font")))
            .count();
        assert_eq!(fonts, 1, "compress {compress}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protection_round_trips() {
    let doc = load(sample(), &strict()).unwrap();
    let made = pdf_security::make_protection(&pdf_security::Wanted {
        user: b"open".to_vec(),
        owner: b"owner".to_vec(),
        allowed: pdf_security::Allowed::default(),
    })
    .unwrap();
    let protection = Protection {
        dictionary: &made.dictionary,
        security: &made.security,
    };
    for compress in [false, true] {
        let written = write(&doc, Some(&protection), compress).unwrap();
        assert!(!written.bytes.windows(5).any(|w| w == b"Hello"));
        assert_eq!(
            load(written.bytes.clone(), &strict()).unwrap_err(),
            LoadError::Password
        );
        for password in [&b"open"[..], b"owner"] {
            let again = load(
                written.bytes.clone(),
                &Load {
                    password: password.to_vec(),
                    ..Load::default()
                },
            )
            .unwrap();
            assert!(again.was_encrypted);
            let page = again.get(again.pages()[0].number).unwrap().dict().unwrap();
            let contents = again
                .get(page.get("Contents").unwrap().as_ref().unwrap())
                .unwrap();
            assert!(contents.stream.as_ref().unwrap().starts_with(b"BT /F1"));
            let info = again.trailer.get("Info").and_then(Value::as_ref).unwrap();
            let title = again
                .get(info)
                .unwrap()
                .dict()
                .unwrap()
                .get("Title")
                .unwrap();
            assert_eq!(title, &Value::string("T(1)"));
        }
    }
}
