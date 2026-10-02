#![allow(clippy::cast_precision_loss)]

use std::fmt::Write as _;
use std::path::Path;

use pdf_scan::{Corners, Grey, Rgba};

fn main() {
    let dir = std::env::args().nth(1).expect("the pictures' directory");
    let dir = Path::new(&dir);
    let net = std::env::args().nth(2).map(|path| {
        pdf_scan::net::SheetNet::read(&std::fs::read(path).expect("weights")).expect("a network")
    });
    let truth = std::fs::read_to_string(dir.join("truth.txt")).expect("truth.txt");
    let mut report = String::new();
    let (mut found, mut total) = (0_usize, 0_usize);
    let mut by_band = [(0_usize, 0_usize); 4];
    let mut errors: Vec<f32> = Vec::new();
    for line in truth.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let name = fields[0];
        let numbers: Vec<f32> = fields[1..9]
            .iter()
            .map(|v| v.parse().unwrap_or(0.0))
            .collect();
        let difficulty: f32 = fields[9].parse().unwrap_or(0.0);
        let want: Corners = [
            (numbers[0], numbers[1]),
            (numbers[2], numbers[3]),
            (numbers[4], numbers[5]),
            (numbers[6], numbers[7]),
        ];
        let picture = read_ppm(&dir.join(format!("{name}.ppm")));
        let got = match &net {
            Some(net) => pdf_scan::net::find_sheet_with(net, &picture),
            None => find_like_the_phone(&picture),
        };
        let diagonal = (picture.width as f32).hypot(picture.height as f32);
        let error = got.map(|got| {
            got.iter()
                .zip(want)
                .map(|(g, w)| (g.0 - w.0).hypot(g.1 - w.1) / diagonal)
                .fold(0.0_f32, f32::max)
        });
        let ok = error.is_some_and(|e| e < 0.02);
        if let Some(e) = error.filter(|_| ok) {
            errors.push(e);
        }
        total += 1;
        found += usize::from(ok);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let band = ((difficulty * 4.0) as usize).min(3);
        by_band[band].1 += 1;
        by_band[band].0 += usize::from(ok);
        let _ = writeln!(
            report,
            "{name} difficulty {difficulty:.2} {}",
            error.map_or_else(
                || "not found".to_owned(),
                |e| format!("corner error {:.1}%", e * 100.0)
            )
        );
        let corners = got.unwrap_or(want);
        let size = pdf_scan::true_size(&corners, (picture.width, picture.height));
        let flat = pdf_scan::flatten(&picture, &corners, size);
        write_pgm(
            &dir.join(format!("{name}-grey.pgm")),
            &pdf_scan::clean(&flat).grey(),
        );
        write_ppm(
            &dir.join(format!("{name}-colour.ppm")),
            &pdf_scan::enhance(&flat),
        );
    }
    print!("{report}");
    println!("found {found} of {total}");
    if !errors.is_empty() {
        errors.sort_by(f32::total_cmp);
        println!(
            "corner error of those found: median {:.2}%, mean {:.2}%",
            errors[errors.len() / 2] * 100.0,
            errors.iter().sum::<f32>() / errors.len() as f32 * 100.0
        );
    }
    for (band, (ok, all)) in by_band.iter().enumerate() {
        println!(
            "difficulty {:.2}-{:.2}: {ok} of {all}",
            band as f32 / 4.0,
            (band + 1) as f32 / 4.0
        );
    }
}

fn find_like_the_phone(picture: &Rgba) -> Option<Corners> {
    if std::env::var_os("NO_SNAP").is_none() {
        return pdf_scan::corners_in_photo(picture);
    }
    let step = picture
        .width
        .max(picture.height)
        .div_ceil(pdf_scan::FOUND_IN)
        .max(1);
    #[allow(clippy::cast_precision_loss)]
    pdf_scan::find_sheet_best(&picture.shrunk(step))
        .map(|c| c.map(|(x, y)| (x * step as f32, y * step as f32)))
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
