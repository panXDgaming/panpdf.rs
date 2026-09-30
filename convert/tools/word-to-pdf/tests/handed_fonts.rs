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
        "/../../../convert-team/testkit/out/everything.docx"
    );
    let Ok(bytes) = std::fs::read(kit) else {
        return;
    };
    let fonts = Fonts {
        provider: None,
        files: package(),
    };
    let input = Input {
        name: "everything.docx".into(),
        bytes,
    };
    let made = word_to_pdf::run(&[input], &Settings::parse(""), &fonts, &mut |_, _| true).unwrap();
    let (name, pdf) = &made.files[0];
    assert_eq!(name, "everything.pdf");
    assert!(pdf.starts_with(b"%PDF"));
    if let Ok(keep) = std::env::var("WORD_TO_PDF_HANDED_OUT") {
        std::fs::write(keep, pdf).unwrap();
    }
}
