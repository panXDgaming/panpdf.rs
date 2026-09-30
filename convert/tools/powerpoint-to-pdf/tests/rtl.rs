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
        ("DejaVu Sans", "DejaVuSans.ttf"),
    ] {
        let bytes = std::fs::read(path(&format!("../../../panpdf.rs/fonts/packaged/{file}")))
            .expect("the engine's font package");
        held.add(family, bytes).expect(file);
    }
    held
}

const LINES: [&str; 14] = [
    "مرحبا بكم في PanPDF",
    "שלום עולם, ברוכים הבאים",
    "البند الأول",
    "البند الثاني",
    "البند الثالث",
    "الخطوة الأولى",
    "The word سلام means peace",
    "نص على اليسار",
    "عام 2026 هو عام جديد",
    "العمود الأول يبدأ هنا",
    "والسطر الأخير",
    "الاسم",
    "الكمية",
    "25,000",
];

const ARABIC_RIGHT: f32 = 36.0 + 400.0 - 7.2;
const LEFT_BOX_LEFT: f32 = 480.0 + 7.2;

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
fn right_to_left_slides_are_printed() {
    let bytes = std::fs::read(path("tests/data/rtl.pptx")).unwrap();
    let mut notes = Vec::new();
    let pdf = powerpoint_to_pdf::convert(
        &bytes,
        "rtl.pptx",
        &Settings::parse(""),
        Some(Arc::new(fonts())),
        &mut notes,
    )
    .unwrap();
    if let Ok(keep) = std::env::var("RTL_TEST_OUT") {
        std::fs::write(keep, &pdf).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("pptx-rtl-{}", std::process::id()));
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
        "rtl.pptx: runs in logical order {}/{}",
        wanted.len() - missing.len(),
        wanted.len()
    );
    assert!(missing.is_empty(), "missing {missing:?}\n{text}");

    let pages = words(&String::from_utf8_lossy(&boxes.stdout));
    let slide = &pages[0];
    let hello = one(slide, "مرحبا");
    let pan = one(slide, "PanPDF");
    assert!((hello.x1 - ARABIC_RIGHT).abs() < 1.5, "{hello:?}");
    assert!(pan.x1 < hello.x0, "{pan:?} {hello:?}");
    let hebrew = one(slide, "שלום");
    assert!((hebrew.x1 - ARABIC_RIGHT).abs() < 1.5, "{hebrew:?}");
    let left = one(slide, "اليسار");
    assert!((left.x0 - LEFT_BOX_LEFT).abs() < 1.5, "{left:?}");
    let (the, peace, means) = (one(slide, "The"), one(slide, "سلام"), one(slide, "means"));
    assert!(
        the.x1 < peace.x0 && peace.x1 < means.x0,
        "{the:?} {peace:?} {means:?}"
    );
    let first = find(slide, "الأول")
        .into_iter()
        .find(|w| w.x1 < 440.0)
        .expect("the first item");
    let bullet = slide
        .iter()
        .filter(|w| w.text == "\u{2022}" && (w.y0 - first.y0).abs() < 4.0)
        .min_by(|a, b| (a.y0 - first.y0).abs().total_cmp(&(b.y0 - first.y0).abs()))
        .expect("the bullet");
    assert!(bullet.x0 > first.x1, "{bullet:?} {first:?}");
    let in_columns = |w: &&Word| w.x0 > 480.0 && w.y0 > 190.0;
    let column_one = find(slide, "يبدأ")
        .into_iter()
        .find(in_columns)
        .expect("column one");
    let column_two = find(slide, "الأخير")
        .into_iter()
        .find(in_columns)
        .expect("column two");
    assert!(
        column_two.x1 < column_one.x0,
        "{column_one:?} {column_two:?}"
    );

    let table = &pages[1];
    let (a, b, c) = (
        one(table, "FirstCol"),
        one(table, "SecondCol"),
        one(table, "ThirdCol"),
    );
    assert!(a.x0 > b.x0 && b.x0 > c.x0, "{a:?} {b:?} {c:?}");
}
