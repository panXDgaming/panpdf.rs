#![allow(clippy::cast_precision_loss)]

use std::path::Path;
use std::time::{Duration, Instant};

use convert_raster::Image;

type Corners = [(f64, f64); 4];

fn main() {
    let dir = std::env::args().nth(1).expect("the pictures' directory");
    let dir = Path::new(&dir);
    let early: Option<f32> = std::env::args()
        .nth(2)
        .map(|v| v.parse().expect("a sureness"));
    let truth = std::fs::read_to_string(dir.join("truth.txt")).expect("truth.txt");
    let mut bands = [[0_usize; 3]; 4];
    let (mut old_time, mut new_time) = (Duration::ZERO, Duration::ZERO);
    for line in truth.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let n: Vec<f64> = fields[1..10]
            .iter()
            .map(|v| v.parse().unwrap_or(0.0))
            .collect();
        let want: Corners = [(n[0], n[1]), (n[2], n[3]), (n[4], n[5]), (n[6], n[7])];
        let band = ((n[8] * 4.0) as usize).min(3);
        let photo = read_ppm(&dir.join(format!("{}.ppm", fields[0])));
        let diagonal = f64::from(photo.width).hypot(f64::from(photo.height));

        let started = Instant::now();
        let old = convert_raster::scan::find_page(&photo);
        old_time += started.elapsed();

        let started = Instant::now();
        let rgba = pdf_scan::Rgba {
            width: photo.width as usize,
            height: photo.height as usize,
            pixels: photo
                .data
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
        };
        let new = match early {
            Some(early) => scan_to_pdf::sheet_corners(&rgba, early),
            None => scan_to_pdf::choose_sheet(&photo, &rgba).map(|(c, _)| c),
        }
        .map(|c| c.map(|(x, y)| (f64::from(x), f64::from(y))));
        new_time += started.elapsed();

        let close = |got: Option<Corners>| {
            got.is_some_and(|got| {
                (0..4).any(|shift| {
                    (0..4).all(|i| {
                        let (g, w) = (got[(i + shift) % 4], want[i]);
                        (g.0 - w.0).hypot(g.1 - w.1) / diagonal <= 0.02
                    })
                })
            })
        };
        bands[band][0] += 1;
        bands[band][1] += usize::from(close(old));
        bands[band][2] += usize::from(close(new));
    }
    println!("| difficulty | pictures | before | after |");
    println!("|---|---|---|---|");
    let names = ["0.00-0.25", "0.25-0.50", "0.50-0.75", "0.75-1.00"];
    let mut all = [0; 3];
    for (name, band) in names.iter().zip(bands) {
        println!("| {name} | {} | {} | {} |", band[0], band[1], band[2]);
        for i in 0..3 {
            all[i] += band[i];
        }
    }
    println!("| all | {} | {} | {} |", all[0], all[1], all[2]);
    println!(
        "time per picture: before {:.0} ms, after {:.0} ms",
        old_time.as_secs_f64() * 1000.0 / all[0] as f64,
        new_time.as_secs_f64() * 1000.0 / all[0] as f64
    );
}

fn read_ppm(path: &Path) -> Image {
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
    let (width, height): (u32, u32) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    Image {
        width,
        height,
        channels: 3,
        data: bytes[at..at + (width * height * 3) as usize].to_vec(),
    }
}
