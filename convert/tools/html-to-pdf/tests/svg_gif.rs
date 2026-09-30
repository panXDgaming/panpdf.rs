use std::sync::Arc;

use convert_structure::bytes_tool::{FontFile, Fonts, Input, Settings};

fn package() -> Vec<FontFile> {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../fonts/packaged"
    );
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ttf"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| FontFile {
            family: p.file_stem().unwrap().to_string_lossy().into_owned(),
            bytes: Arc::from(std::fs::read(p).unwrap().as_slice()),
        })
        .collect()
}

const GIF: &str = "R0lGODlhAQABAIAAAP8AAAAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

#[test]
fn svg_and_gif_pictures_are_drawn() {
    let files = package();
    if files.is_empty() {
        return;
    }
    let page = format!(
        r##"<html><head><style>.bar {{ fill: #1f5fbf }}</style></head><body>
        <p>Before <svg width="200" height="100" viewBox="0 0 200 100">
          <defs><linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs>
          <rect class="bar" width="50" height="50"/><circle cx="120" cy="50" r="40" fill="url(#g)" filter="url(#f)"/>
          <text x="10" y="90">ສະບາຍດີ</text></svg> after.</p>
        <p><img src="pics/chart.svg"> <img src="data:image/gif;base64,{GIF}" width="20"></p>
        </body></html>"##
    );
    let chart = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><path d="M0 0 A 25 25 0 0 1 50 50" stroke="green" fill="none"/></svg>"#;
    let inputs = vec![
        Input {
            name: "page.html".into(),
            bytes: page.into_bytes(),
        },
        Input {
            name: "pics/chart.svg".into(),
            bytes: chart.to_vec(),
        },
    ];
    let fonts = Fonts {
        provider: None,
        files,
    };
    let made = html_to_pdf::run(&inputs, &Settings::parse(""), &fonts, &mut |_, _| true).unwrap();
    let pdf = &made.files[0].1;
    let text = String::from_utf8_lossy(pdf);
    assert!(text.contains("/PatternType 2"), "no shading pattern");
    assert!(text.contains("/Indexed /DeviceRGB"), "GIF not indexed");
    assert!(text.contains("/Mask [0 0]"), "GIF transparency lost");
    assert!(
        made.notes.iter().any(|n| n.contains("filters")),
        "{:?}",
        made.notes
    );
    assert!(
        !made.notes.iter().any(|n| n.contains("not read")),
        "{:?}",
        made.notes
    );
}
