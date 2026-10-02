#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = &args[1];
    for path in &args[2..] {
        let photo =
            convert_raster::decode(&std::fs::read(path).expect("a picture")).expect("pixels");
        let picture = pdf_scan::Rgba {
            width: photo.width as usize,
            height: photo.height as usize,
            pixels: photo
                .data
                .chunks_exact(photo.channels)
                .flat_map(|p| {
                    if p.len() >= 3 {
                        [p[0], p[1], p[2], 255]
                    } else {
                        [p[0], p[0], p[0], 255]
                    }
                })
                .collect(),
        };
        let (found, step) = scan_to_pdf::candidates(&photo, &picture);
        let chosen = scan_to_pdf::choose_sheet(&photo, &picture).map_or("the picture", |(_, f)| f);
        let name = path.rsplit('/').next().unwrap_or(path);
        let mut line = format!("{name:<34} -> {chosen:<11}");
        let mut small = picture.shrunk(step);
        for (support, corners, finder) in &found {
            let _ = write!(line, " {finder} {support:.2}");
            let colour = match *finder {
                "DocQuadNet" => [255, 0, 0],
                "network" => [0, 80, 255],
                _ => [0, 200, 0],
            };
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                for k in 0..=400 {
                    let t = k as f32 / 400.0;
                    let (x, y) = (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (px, py) = (x as usize + dx, y as usize + dy);
                        if px < small.width && py < small.height {
                            let at = (py * small.width + px) * 4;
                            small.pixels[at..at + 3].copy_from_slice(&colour);
                        }
                    }
                }
            }
        }
        println!("{line}");
        let mut ppm = format!("P6 {} {} 255\n", small.width, small.height).into_bytes();
        ppm.extend(
            small
                .pixels
                .chunks_exact(4)
                .flat_map(|p| [p[0], p[1], p[2]]),
        );
        std::fs::write(format!("{out}/{name}.ppm"), ppm).expect("written");
    }
}
