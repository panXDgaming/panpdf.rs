fn scan_page() -> Vec<u8> {
    let samples = vec![200u8; 16 * 16];
    let content = b"q 595 0 0 842 0 0 cm /Im1 Do Q";
    let mut pdf: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    };
    object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut pdf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    object(&mut pdf, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>");
    let mut body = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(b"\nendstream");
    object(&mut pdf, &body);
    let mut body = format!("<< /Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n", samples.len()).into_bytes();
    body.extend_from_slice(&samples);
    body.extend_from_slice(b"\nendstream");
    object(&mut pdf, &body);
    let start = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for o in &offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

#[test]
fn a_scanned_page_gets_an_invisible_text_layer() {
    let fake = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake-tesseract.sh");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ocr-fake");
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("scan.pdf");
    let output = dir.join("scan-ocr.pdf");
    std::fs::write(&input, scan_page()).unwrap();
    let _ = std::fs::remove_file(&output);
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_ocr-pdf"))
        .arg(&input)
        .args(["-o"])
        .arg(&output)
        .args(["--languages", "lao+eng", "--quiet"])
        .env("PANPDF_TESSERACT", fake)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let pdf = std::fs::read(&output).unwrap();
    let source = pdf_bytes::ByteStore::owning(pdf_bytes::SourceId::next_document(), pdf);
    let view = pdf_session::interpret_page_for_display(&source, 0, b"", None, None).unwrap();
    let texts = view
        .graph
        .atoms
        .iter()
        .filter(|a| matches!(a.kind, pdf_paint::PaintAtomKind::Text(_)))
        .count();
    assert!(texts >= 3, "{texts} text atoms");
    let again = std::process::Command::new(env!("CARGO_BIN_EXE_ocr-pdf"))
        .arg(&output)
        .args(["-o"])
        .arg(dir.join("twice.pdf"))
        .args(["--quiet"])
        .env("PANPDF_TESSERACT", fake)
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already had text"));
}

fn given(
    inputs: Vec<(&str, Vec<u8>)>,
    settings: &str,
) -> Result<ocr_pdf::convert_files::Outcome, String> {
    let inputs = inputs
        .into_iter()
        .map(|(name, bytes)| ocr_pdf::convert_files::Input {
            name: name.to_owned(),
            bytes,
        })
        .collect();
    ocr_pdf::convert_files::run_to_end(
        ocr_pdf::start,
        inputs,
        &ocr_pdf::convert_files::Settings::parse(settings),
    )
}

#[test]
fn words_read_elsewhere_are_written_and_the_survey_sees_them() {
    let survey = given(vec![("scan.pdf", scan_page())], "survey=true\nmax=10\n").unwrap();
    assert_eq!(survey.files[0].0, "pages.tsv");
    assert_eq!(
        String::from_utf8(survey.files[0].1.clone()).unwrap(),
        "pages\t1\n1\tscan\t595.00\t842.00\n"
    );
    let words =
        "1\t72\t700\t160\t716\tຍິນດີ \n1\t160\t700\t260\t716\tຕ້ອນຮັບ\n1\t72\t660\t200\t676\tWelcome\n";
    let out = given(
        vec![
            ("scan.pdf", scan_page()),
            ("words.tsv", words.as_bytes().to_vec()),
        ],
        "",
    )
    .unwrap();
    assert_eq!(out.files[0].0, "scan-ocr.pdf");
    assert_eq!(out.notes, ["1 pages made searchable"]);
    let source =
        pdf_bytes::ByteStore::owning(pdf_bytes::SourceId::next_document(), out.files[0].1.clone());
    let view = pdf_session::interpret_page_for_display(&source, 0, b"", None, None).unwrap();
    let texts = view
        .graph
        .atoms
        .iter()
        .filter(|a| matches!(a.kind, pdf_paint::PaintAtomKind::Text(_)))
        .count();
    assert!(texts >= 3, "{texts} text atoms");
    let again = given(
        vec![("scan-ocr.pdf", out.files[0].1.clone())],
        "survey=true\n",
    )
    .unwrap();
    assert!(
        String::from_utf8(again.files[0].1.clone())
            .unwrap()
            .contains("1\ttext\t")
    );
}

#[test]
fn a_survey_refuses_pages_the_file_does_not_have() {
    let err = given(vec![("scan.pdf", scan_page())], "survey=true\npages=2\n").unwrap_err();
    assert!(err.contains("page 2"), "{err}");
    let err = given(
        vec![
            ("scan.pdf", scan_page()),
            ("words.tsv", b"2\t1\t1\t9\t9\tx\n".to_vec()),
        ],
        "",
    )
    .unwrap_err();
    assert!(err.contains("page 2, and the file has 1 pages"), "{err}");
}

#[test]
fn a_file_with_a_header_strict_parsing_refuses_is_written_all_the_same() {
    let mut pdf = scan_page();
    pdf.splice(8..8, *b" ");
    let words = "1\t72\t700\t160\t716\tHello\n";
    let out = given(
        vec![("scan.pdf", pdf), ("words.tsv", words.as_bytes().to_vec())],
        "",
    )
    .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("written again")),
        "{:?}",
        out.notes
    );
    assert_eq!(out.notes.last().unwrap(), "1 pages made searchable");
    let source =
        pdf_bytes::ByteStore::owning(pdf_bytes::SourceId::next_document(), out.files[0].1.clone());
    let view = pdf_session::interpret_page_for_display(&source, 0, b"", None, None).unwrap();
    assert!(
        view.graph
            .atoms
            .iter()
            .any(|a| matches!(a.kind, pdf_paint::PaintAtomKind::Text(_)))
    );
}

fn tesseract_table(words: &[&str], conf: u32) -> String {
    let mut rows = vec![
        "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext"
            .to_owned(),
        "1\t1\t0\t0\t0\t0\t0\t0\t2479\t3508\t-1\t".to_owned(),
        format!("4\t1\t1\t1\t1\t0\t300\t300\t{}\t60\t-1\t", 100 * words.len()),
    ];
    for (n, word) in words.iter().enumerate() {
        rows.push(format!(
            "5\t1\t1\t1\t1\t{}\t{}\t300\t90\t60\t{conf}\t{word}",
            n + 1,
            300 + 100 * n
        ));
    }
    rows.join("\n") + "\n"
}

#[test]
fn a_page_is_drawn_for_the_browser_as_the_desktop_draws_it() {
    let out = given(vec![("scan.pdf", scan_page())], "draw=true\npages=1\n").unwrap();
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].0, "page-1.pgm");
    assert!(out.files[0].1.starts_with(b"P5\n2480 3509\n255\n"));
    let header = b"P5\n2480 3509\n255\n".len();
    assert_eq!(out.files[0].1.len(), header + 2480 * 3509);
    assert!(
        given(vec![("scan.pdf", scan_page())], "draw=true\npages=2\n")
            .unwrap_err()
            .contains("page 2")
    );
    assert!(
        given(vec![("scan.pdf", scan_page())], "draw=true\n")
            .unwrap_err()
            .contains("one page")
    );
}

#[test]
fn a_page_read_as_lao_is_judged_by_the_desktops_rule() {
    let choice = |conf: u32| {
        let table = tesseract_table(&["ສະບາຍດີ", "Hello"], conf);
        let out = given(
            vec![
                ("lao.tsv", table.into_bytes()),
                ("lao.txt", "ສະບາຍດີ Hello\n".as_bytes().to_vec()),
            ],
            "judge=true\n",
        )
        .unwrap();
        String::from_utf8(out.files[0].1.clone()).unwrap()
    };
    assert_eq!(choice(90), "lao");
    assert_eq!(choice(26), "lao");
    assert_eq!(choice(24), "tha");
}

#[test]
fn a_tesseract_table_from_the_browser_is_written_as_the_desktop_writes_it() {
    let out = given(
        vec![
            ("scan.pdf", scan_page()),
            (
                "page-1.tsv",
                tesseract_table(&["ຍິນດີ", "Welcome"], 90).into_bytes(),
            ),
            ("page-1.txt", "ຍິນດີ Welcome\n".as_bytes().to_vec()),
        ],
        "",
    )
    .unwrap();
    assert_eq!(out.files[0].0, "scan-ocr.pdf");
    assert_eq!(out.notes, ["1 pages made searchable, mean confidence 90%"]);
    let source =
        pdf_bytes::ByteStore::owning(pdf_bytes::SourceId::next_document(), out.files[0].1.clone());
    let view = pdf_session::interpret_page_for_display(&source, 0, b"", None, None).unwrap();
    assert!(
        view.graph
            .atoms
            .iter()
            .any(|a| matches!(a.kind, pdf_paint::PaintAtomKind::Text(_)))
    );
    let again = given(
        vec![("scan-ocr.pdf", out.files[0].1.clone())],
        "survey=true\n",
    )
    .unwrap();
    assert!(
        String::from_utf8(again.files[0].1.clone())
            .unwrap()
            .contains("1\ttext\t")
    );
    let err = given(
        vec![
            ("scan.pdf", scan_page()),
            ("page-2.tsv", tesseract_table(&["x"], 90).into_bytes()),
            ("page-2.txt", b"x\n".to_vec()),
        ],
        "",
    )
    .unwrap_err();
    assert!(err.contains("page 2"), "{err}");
    let err = given(
        vec![
            ("scan.pdf", scan_page()),
            ("page-1.tsv", tesseract_table(&["x"], 90).into_bytes()),
        ],
        "",
    )
    .unwrap_err();
    assert!(err.contains("page-1.txt"), "{err}");
}
