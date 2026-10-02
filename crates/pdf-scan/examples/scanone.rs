#![allow(clippy::cast_precision_loss)]

use std::path::Path;

use pdf_scan::{Grey, Rgba};

fn main() {
    let input = std::env::args().nth(1).expect("the photograph");
    let out = std::env::args().nth(2).expect("where to write");
    let picture = read_ppm(Path::new(&input));
    let started = std::time::Instant::now();
    let Some(corners) = pdf_scan::corners_in_photo(&picture) else {
        println!("no sheet found");
        return;
    };
    let size = pdf_scan::true_size(&corners, (picture.width, picture.height));
    println!(
        "found in {:?}: {corners:?} size {size:?}",
        started.elapsed()
    );
    let flat = pdf_scan::flatten(&picture, &corners, size);
    write_ppm(
        Path::new(&format!("{out}-colour.ppm")),
        &pdf_scan::enhance(&flat),
    );
    write_pgm(
        Path::new(&format!("{out}-grey.pgm")),
        &pdf_scan::clean(&flat).grey(),
    );
}

fn read_ppm(path: &Path) -> Rgba {
    let bytes = std::fs::read(path).expect("a picture");
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        while bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let start = at;
        while !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
    }
    at += 1;
    let (width, height): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    let pixels = bytes[at..at + width * height * 3]
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2], 255])
        .collect();
    Rgba {
        width,
        height,
        pixels,
    }
}

fn write_pgm(path: &Path, grey: &Grey) {
    let mut out = format!("P5\n{} {}\n255\n", grey.width, grey.height).into_bytes();
    out.extend_from_slice(&grey.pixels);
    std::fs::write(path, out).expect("written");
}

fn write_ppm(path: &Path, picture: &Rgba) {
    let mut out = format!("P6\n{} {}\n255\n", picture.width, picture.height).into_bytes();
    for p in picture.pixels.as_chunks::<4>().0 {
        out.extend_from_slice(&p[..3]);
    }
    std::fs::write(path, out).expect("written");
}
