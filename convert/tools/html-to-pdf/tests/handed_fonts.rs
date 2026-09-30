use std::sync::Arc;

use convert_structure::bytes_tool::{FontFile, Fonts, Input, Settings};

fn package() -> Vec<FontFile> {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../panpdf.rs/fonts/packaged"
    );
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("the engine's font package")
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

#[test]
fn converts_in_faces_handed_over() {
    let kit = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../convert-team/testkit/out/html"
    );
    let Ok(entries) = std::fs::read_dir(kit) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|n| (n != "everything.html", n.clone()));
    let inputs: Vec<Input> = names
        .iter()
        .map(|n| Input {
            name: n.clone(),
            bytes: std::fs::read(format!("{kit}/{n}")).unwrap(),
        })
        .collect();
    let fonts = Fonts {
        provider: None,
        files: package(),
    };
    let made = html_to_pdf::run(&inputs, &Settings::parse(""), &fonts, &mut |_, _| true).unwrap();
    let (name, pdf) = made
        .files
        .iter()
        .find(|(n, _)| n == "everything.pdf")
        .expect("the page");
    assert_eq!(name, "everything.pdf");
    assert!(pdf.starts_with(b"%PDF"));
    if let Ok(keep) = std::env::var("HTML_TO_PDF_HANDED_OUT") {
        std::fs::write(keep, pdf).unwrap();
    }
}
