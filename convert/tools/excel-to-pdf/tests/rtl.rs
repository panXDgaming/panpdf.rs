use std::sync::Arc;

use convert_structure::bytes_tool::Settings;
use convert_structure::held_fonts::HeldFonts;

fn path(rest: &str) -> String {
    format!("{}/{rest}", env!("CARGO_MANIFEST_DIR"))
}

fn fonts() -> HeldFonts {
    let mut held = HeldFonts::new();
    for (family, file) in [
        ("Liberation Sans", "LiberationSans-Regular.ttf"),
        ("Liberation Sans", "LiberationSans-Bold.ttf"),
        ("DejaVu Sans", "DejaVuSans.ttf"),
    ] {
        let bytes = std::fs::read(path(&format!("../../../fonts/packaged/{file}")))
            .expect("the engine's font package");
        held.add(family, bytes).expect(file);
    }
    held
}

const LINES: [&str; 12] = [
    "مرحبا بالعالم",
    "שלום עולם",
    "السعر 250 دولار",
    "نظام Windows الجديد",
    "(مرحبا)",
    "هذه فقرة عربية طويلة تلتف داخل الخلية لتختبر تقسيم الأسطر من اليمين إلى اليسار",
    "Total: 42 items",
    "مرحبا PanPDF",
    "المبيعات الشهرية",
    "يناير",
    "العمود أ",
    "نص مدمج في خليتين",
];

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !('\u{200B}'..='\u{202E}').contains(c))
        .collect()
}

fn runs(line: &str) -> Vec<String> {
    let rtl = |c: char| ('\u{0590}'..='\u{08FF}').contains(&c);
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in line.chars() {
        let kind = if c.is_whitespace() {
            out.last().is_some_and(|(k, _)| *k)
        } else {
            rtl(c)
        };
        match out.last_mut() {
            Some((k, s)) if *k == kind => s.push(c),
            _ => out.push((kind, c.to_string())),
        }
    }
    out.into_iter()
        .map(|(_, s)| squash(&s))
        .filter(|s| !s.is_empty())
        .collect()
}

#[derive(Debug, Clone)]
struct Word {
    x0: f32,
    y0: f32,
    x1: f32,
    text: String,
}

fn words(xml: &str) -> Vec<Vec<Word>> {
    let mut pages = Vec::new();
    for page in xml.split("<page ").skip(1) {
        let mut list = Vec::new();
        for line in page
            .lines()
            .filter(|l| l.trim_start().starts_with("<word "))
        {
            let attr = |name: &str| -> f32 {
                let key = format!("{name}=\"");
                let at = line.find(&key).map_or(0, |a| a + key.len());
                line[at..]
                    .split('"')
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0)
            };
            let text = line
                .split('>')
                .nth(1)
                .and_then(|t| t.split('<').next())
                .unwrap_or("")
                .to_owned();
            list.push(Word {
                x0: attr("xMin"),
                y0: attr("yMin"),
                x1: attr("xMax"),
                text,
            });
        }
        pages.push(list);
    }
    pages
}

fn find<'a>(page: &'a [Word], text: &str) -> Vec<&'a Word> {
    let reversed: String = text.chars().rev().collect();
    page.iter()
        .filter(|w| w.text == text || w.text == reversed)
        .collect()
}

fn one<'a>(page: &'a [Word], text: &str) -> &'a Word {
    find(page, text)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{text} not on the page: {page:?}"))
}

#[test]
fn right_to_left_cells_are_printed() {
    let bytes = std::fs::read(path("tests/data/rtl.xlsx")).unwrap();
    let mut notes = Vec::new();
    let pdf = excel_to_pdf::convert(
        &bytes,
        "rtl.xlsx",
        &Settings::parse(""),
        Some(Arc::new(fonts())),
        &mut notes,
    )
    .unwrap();
    if let Ok(keep) = std::env::var("RTL_TEST_OUT") {
        std::fs::write(keep, &pdf).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("excel-rtl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rtl.pdf");
    std::fs::write(&file, &pdf).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("pdftotext")
            .args(args)
            .arg(&file)
            .arg("-")
            .output()
    };
    let (Ok(plain), Ok(boxes)) = (run(&["-enc", "UTF-8"]), run(&["-enc", "UTF-8", "-bbox"])) else {
        let _ = std::fs::remove_dir_all(dir);
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    let text = squash(&String::from_utf8_lossy(&plain.stdout));
    let wanted: Vec<String> = LINES.iter().flat_map(|l| runs(l)).collect();
    let missing: Vec<&String> = wanted.iter().filter(|w| !text.contains(*w)).collect();
    println!(
        "rtl.xlsx: runs in logical order {}/{}",
        wanted.len() - missing.len(),
        wanted.len()
    );
    assert!(missing.is_empty(), "missing {missing:?}\n{text}");

    let pages = words(&String::from_utf8_lossy(&boxes.stdout));
    let cells = &pages[0];
    let left = one(cells, "LEFT");
    let arabic = one(cells, "بالعالم");
    assert!(arabic.x1 > left.x1 + 100.0, "{arabic:?} {left:?}");
    let total = one(cells, "Total:");
    assert!(total.x0 > left.x1 + 100.0, "{total:?}");
    let mut pan = find(cells, "PanPDF");
    pan.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let hello: Vec<&Word> = find(cells, "مرحبا")
        .into_iter()
        .filter(|w| pan.iter().any(|p| (p.y0 - w.y0).abs() < 3.0))
        .collect();
    let on_line = |p: &Word| {
        hello
            .iter()
            .find(|w| (w.y0 - p.y0).abs() < 3.0)
            .copied()
            .unwrap_or_else(|| panic!("no Arabic word beside {p:?}"))
    };
    assert_eq!(pan.len(), 2, "{pan:?}");
    let (ltr, context) = (pan[0], pan[1]);
    assert!(on_line(ltr).x1 < ltr.x0, "{ltr:?} {:?}", on_line(ltr));
    assert!((on_line(ltr).x0 - left.x0).abs() < 1.0);
    assert!(on_line(context).x0 > context.x1, "{context:?}");
    assert!(context.x0 > left.x1 + 50.0);

    let mirrored = pages
        .iter()
        .find(|p| !find(p, "ColA").is_empty())
        .expect("the right-to-left sheet");
    let (a, b, c) = (
        one(mirrored, "ColA"),
        one(mirrored, "ColB"),
        one(mirrored, "ColC"),
    );
    assert!(a.x0 > b.x0 && b.x0 > c.x0, "{a:?} {b:?} {c:?}");
    let n = one(mirrored, "42");
    assert!(n.x0 > a.x1, "{n:?} {a:?}");
}
