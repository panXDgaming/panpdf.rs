use convert_pdf_canvas::{Canvas, Rgb, TextStyle};

fn font(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../../fonts/packaged/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("the engine's font package")
}

const LINES: [&str; 5] = [
    "ສະບາຍດີ ນ້ຳໃຈ ປະເທດລາວ ຂໍ້ມູນ ກ່ຽວກັບ ໂຮງຮຽນ",
    "ภาษาไทย น้ำใจ ที่นี่ ผู้ใหญ่ กำลัง สำคัญ",
    "Hello, world! Café 12,345.67",
    "ຄຳ ນຳ ທຳ ກຳລັງ",
    "ຫຼັກສູດ ໜັງສື ພຣະ",
];

#[test]
fn writes_text_that_reads_back() {
    let mut canvas = Canvas::new();
    canvas.set_title("ທົດສອບ test");
    let lao = canvas.add_font(font("NotoSansLao-Regular.ttf"), 0).unwrap();
    let thai = canvas
        .add_font(font("NotoSansThai-Regular.ttf"), 0)
        .unwrap();
    let latin = canvas
        .add_font(font("LiberationSans-Regular.ttf"), 0)
        .unwrap();
    let page = canvas.add_page(595.0, 842.0);
    let style = TextStyle::new(14.0, Rgb::BLACK);
    for (n, line) in LINES.iter().enumerate() {
        let face = match n {
            1 => thai,
            2 => latin,
            _ => lao,
        };
        let shaped = canvas.shape(face, line);
        assert!(!shaped.any_missing(), "{line}");
        let y = 780.0 - n as f32 * 30.0;
        canvas
            .page(page)
            .show(&shaped, 0..shaped.clusters.len(), &style, 72.0, y, 0.0);
    }
    canvas
        .page(page)
        .fill_rect(72.0, 500.0, 100.0, 40.0, Rgb::from_u8(200, 230, 255))
        .stroke_line(72.0, 490.0, 300.0, 490.0, 1.0, Rgb(1.0, 0.0, 0.0))
        .save()
        .set_alpha(0.5, 1.0)
        .set_fill(Rgb(0.0, 0.5, 0.0))
        .ellipse(200.0, 500.0, 80.0, 40.0)
        .fill()
        .restore()
        .link(72.0, 500.0, 100.0, 40.0, "https://panpdf.org/");
    let pdf = canvas.finish().unwrap();
    let dir = std::env::temp_dir().join(format!("pdf-canvas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("canvas.pdf");
    std::fs::write(&path, &pdf).unwrap();
    if let Ok(keep) = std::env::var("CANVAS_TEST_OUT") {
        std::fs::write(keep, &pdf).unwrap();
    }
    let Ok(out) = std::process::Command::new("pdftotext")
        .args(["-layout", "-enc", "UTF-8"])
        .arg(&path)
        .arg("-")
        .output()
    else {
        return;
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    for line in LINES {
        assert!(text.contains(line), "missing {line:?} in {text:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_calibri_paragraph_falls_back_by_script_and_wraps() {
    use convert_pdf_canvas::{FontBook, Span, layout};
    use std::sync::Arc;
    let package = format!("{}/../../../fonts/packaged", env!("CARGO_MANIFEST_DIR"));
    let provider = pdf_font::system_fonts::SystemFontProvider::discover_in(&[package.into()]);
    let mut book = FontBook::new(Some(Arc::new(provider)));
    let mut canvas = Canvas::new();
    let text = "Budget ງົບປະມານ ປີ 2026 งบประมาณ total";
    let runs = book.runs(&mut canvas, text, "Calibri", true, false);
    let fonts: std::collections::BTreeSet<_> = runs.iter().map(|(_, p)| p.font).collect();
    assert!(fonts.len() >= 3, "{runs:?}");
    assert_eq!(book.missing(), 0);
    let spans = [Span::new(text, "Calibri", 11.0)];
    let lines = layout(&mut canvas, &mut book, &spans, Some(90.0));
    assert!(lines.len() >= 3, "{}", lines.len());
    assert!(
        lines.iter().all(|l| l.width <= 90.5),
        "{:?}",
        lines.iter().map(|l| l.width).collect::<Vec<_>>()
    );
    let page = canvas.add_page(200.0, 200.0);
    let mut y = 180.0;
    for line in &lines {
        y -= line.ascent;
        line.draw(canvas.page(page), 10.0, y, 0.0);
        y -= line.descent + line.gap;
    }
    let pdf = canvas.finish().unwrap();
    if let Ok(keep) = std::env::var("CANVAS_TEST_OUT2") {
        std::fs::write(keep, &pdf).unwrap();
    }
}

#[test]
fn lao_prefers_document_faces() {
    use convert_pdf_canvas::FontBook;
    use std::sync::Arc;
    let package = format!("{}/../../../fonts/packaged", env!("CARGO_MANIFEST_DIR"));
    let provider = pdf_font::system_fonts::SystemFontProvider::discover_in(&[package.into()]);
    let mut book = FontBook::new(Some(Arc::new(provider)));
    let mut canvas = Canvas::new();
    for bold in [false, true] {
        let runs = book.runs(&mut canvas, "ເຂົ້າ Rice", "Calibri", bold, false);
        let family = canvas.font_metrics(runs[0].1.font).family;
        assert_eq!(family, "Saysettha OT", "bold {bold}");
    }
}

#[test]
fn styles_read_from_os2() {
    let mut canvas = Canvas::new();
    let bold = canvas.add_font(font("LiberationSans-Bold.ttf"), 0).unwrap();
    let italic = canvas
        .add_font(font("LiberationSerif-Italic.ttf"), 0)
        .unwrap();
    let (b, i) = (canvas.font_metrics(bold), canvas.font_metrics(italic));
    assert!(b.bold && !b.italic);
    assert!(i.italic && !i.bold);
    let lao = canvas.add_font(font("SaysetthaOT-Regular.ttf"), 0).unwrap();
    let m = canvas.font_metrics(lao);
    assert_eq!((m.win_ascent, m.win_descent), (2427, 1153));
}

#[test]
fn cff_faces_embed_and_read_back() {
    const CJK: &str = "日本語のテキスト 中文字体 한국어";
    let mut canvas = Canvas::new();
    let cjk = canvas
        .add_font(font("NotoSansCJKsc-Regular.otf"), 0)
        .unwrap();
    let latin = canvas
        .add_font(font("LiberationSans-Regular.ttf"), 0)
        .unwrap();
    let page = canvas.add_page(595.0, 842.0);
    let style = TextStyle::new(14.0, Rgb::BLACK);
    let shaped = canvas.shape(cjk, CJK);
    assert!(!shaped.any_missing());
    canvas
        .page(page)
        .show(&shaped, 0..shaped.clusters.len(), &style, 72.0, 780.0, 0.0);
    let shaped = canvas.shape(latin, "Latin beside it");
    canvas
        .page(page)
        .show(&shaped, 0..shaped.clusters.len(), &style, 72.0, 750.0, 0.0);
    let pdf = canvas.finish().unwrap();
    assert!(pdf.len() < 400_000, "{} bytes", pdf.len());
    assert!(pdf.windows(12).any(|w| w == b"CIDFontType0"));
    let dir = std::env::temp_dir().join(format!("pdf-canvas-cff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cff.pdf");
    std::fs::write(&path, &pdf).unwrap();
    let Ok(out) = std::process::Command::new("pdftotext")
        .args(["-enc", "UTF-8"])
        .arg(&path)
        .arg("-")
        .output()
    else {
        return;
    };
    let text = String::from_utf8(out.stdout).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(text.contains(CJK), "{text:?}");
    assert!(text.contains("Latin beside it"), "{text:?}");
}
