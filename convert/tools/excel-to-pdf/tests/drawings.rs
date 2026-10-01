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
        ("Liberation Sans", "LiberationSans-Italic.ttf"),
        ("Saysettha OT", "SaysetthaOT-Regular.ttf"),
        ("Noto Sans Lao", "NotoSansLao-Regular.ttf"),
        ("Noto Sans Thai", "NotoSansThai-Regular.ttf"),
        ("Noto Sans Thai", "NotoSansThai-Bold.ttf"),
    ] {
        let bytes = std::fs::read(path(&format!("../../../fonts/packaged/{file}")))
            .expect("the engine's font package");
        held.add(family, bytes).expect(file);
    }
    held
}

const PRINTED: [&str; 37] = [
    "Sales and cost 2026",
    "Jan",
    "Jun",
    "Sales",
    "Cost",
    "250",
    "210",
    "ກຳໄລ ລາຍເດືອນ (Profit)",
    "Kip (thousand)",
    "Month",
    "สัดส่วนยอดขาย ตามภาค",
    "ภาคเหนือ",
    "ວຽງຈັນ",
    "35%",
    "Total: 1,234 kip",
    "ສະບາຍດີ",
    "ນີ້ແມ່ນກ່ອງຂໍ້ຄວາມ",
    "สวัสดี",
    "นี่คือกล่องข้อความภาษาไทย",
    "Red run in the text box",
    "Group label",
    "Tasks by team (stacked)",
    "North",
    "Done",
    "Late",
    "Across the break",
    "Visitors (stacked area)",
    "1,000",
    "Growth (scatter)",
    "Crops (doughnut)",
    "Coffee",
    "Skills (radar, not drawn)",
    "Share by year (100%)",
    "Chart sheet Share",
    "First page header",
    "Page 6 of 5",
    "--",
];

const NOT_PRINTED: [&str; 6] = [
    "HIDDEN-ROW-SHAPE",
    "HIDDEN-FLAG-SHAPE",
    "NOPRINT-SHAPE",
    "Open",
    "#DIV/0!",
    "Page 5 of 5",
];

fn squash(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn drawings_are_printed() {
    let bytes = std::fs::read(path("tests/data/drawings.xlsx")).unwrap();
    let mut notes = Vec::new();
    let pdf = excel_to_pdf::convert(
        &bytes,
        "drawings.xlsx",
        &Settings::parse(""),
        Some(Arc::new(fonts())),
        &mut notes,
    )
    .unwrap();
    let raw = String::from_utf8_lossy(&pdf);
    let pages = raw.matches("/Type /Page ").count();
    assert_eq!(pages, 7, "{notes:?}");
    assert!(raw.contains("/Subtype /Image"), "the picture is embedded");
    let undrawn: Vec<&String> = notes.iter().filter(|n| n.contains("not drawn")).collect();
    assert_eq!(undrawn.len(), 1, "{notes:?}");
    assert!(undrawn[0].contains("radarChart"), "{notes:?}");
    if let Ok(keep) = std::env::var("DRAWINGS_TEST_OUT") {
        std::fs::write(keep, &pdf).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("excel-drawings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("drawings.pdf");
    std::fs::write(&file, &pdf).unwrap();
    let Ok(out) = std::process::Command::new("pdftotext")
        .args(["-enc", "UTF-8"])
        .arg(&file)
        .arg("-")
        .output()
    else {
        let _ = std::fs::remove_dir_all(dir);
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    let text = squash(&String::from_utf8_lossy(&out.stdout));
    let missing: Vec<&str> = PRINTED
        .iter()
        .copied()
        .filter(|w| !text.contains(&squash(w)))
        .collect();
    let leaked: Vec<&str> = NOT_PRINTED
        .iter()
        .copied()
        .filter(|w| text.contains(&squash(w)))
        .collect();
    println!(
        "drawings.xlsx: printed {}/{}, leaked {}/{}",
        PRINTED.len() - missing.len(),
        PRINTED.len(),
        leaked.len(),
        NOT_PRINTED.len()
    );
    assert!(missing.is_empty(), "missing {missing:?}");
    assert!(leaked.is_empty(), "leaked {leaked:?}");
}
