use std::sync::{Arc, OnceLock};

use convert_pdfdoc::shown;
use convert_pdftool::{Job, Output, Tool};
use pdf_bytes::{ByteStore, SourceId};
use pdf_content::FontProvider;
use pdf_edit::{Command, ParagraphLayout, PenStep, PenStroke};
use pdf_paint::Matrix;
use pdf_session::Session;

pub static TOOL: Tool = Tool {
    name: "sign-pdf",
    extension: "pdf",
    inputs: 1,
    file_options: &["image"],
    flags: &["flatten"],
    help: "  one of: --image SIG.png|.jpg   --draw \"x,y x,y ...; x,y ...\" (strokes in a 0..1 box, y up)   --text \"Name\"\n  \
           --pages 1,3-5|last|all (default: last)\n  \
           --at X,Y,W,H           points on the page as shown, from its bottom left\n  \
           --position bottom-right|bottom-left|bottom-centre|top-right|top-left|centre (default bottom-right)\n  \
           --width PT (default 160)  --height PT  --margin PT (default 36)\n  \
           --color R,G,B (0..1, default dark blue)  --pen PT (stroke width, default 1.6)  --font FAMILY\n  \
           --flatten              one revision: the unsigned document is not left inside the file\n  \
           --password PW          when the file is protected",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

static FONTS: OnceLock<Option<Arc<dyn FontProvider>>> = OnceLock::new();

pub fn set_fonts(fonts: Option<Arc<dyn FontProvider>>) {
    let _ = FONTS.set(fonts);
}

#[derive(Clone, Debug)]
pub enum Mark {
    Picture(Arc<[u8]>),
    Strokes(Vec<Vec<(f64, f64)>>),
    Typed(String),
}

#[must_use]
pub fn picture_size(file: &[u8]) -> Option<(u32, u32)> {
    if file.starts_with(b"\x89PNG\r\n\x1a\n") && file.len() >= 24 {
        let w = u32::from_be_bytes(file[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(file[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if !file.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut at = 2;
    while at + 9 < file.len() {
        if file[at] != 0xff {
            at += 1;
            continue;
        }
        let marker = file[at + 1];
        if marker == 0xff {
            at += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([file[at + 2], file[at + 3]]));
        let frame = (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker);
        if frame {
            let h = u32::from(u16::from_be_bytes([file[at + 5], file[at + 6]]));
            let w = u32::from(u16::from_be_bytes([file[at + 7], file[at + 8]]));
            return Some((w, h));
        }
        at += 2 + len;
    }
    None
}

pub fn strokes(text: &str) -> Result<Vec<Vec<(f64, f64)>>, String> {
    let mut out = Vec::new();
    for stroke in text.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let mut points = Vec::new();
        for point in stroke.split_whitespace() {
            let (x, y) = point
                .split_once(',')
                .ok_or_else(|| format!("--draw: '{point}' is not x,y"))?;
            let x: f64 = x
                .parse()
                .map_err(|_| format!("--draw: '{point}' is not x,y"))?;
            let y: f64 = y
                .parse()
                .map_err(|_| format!("--draw: '{point}' is not x,y"))?;
            points.push((x, y));
        }
        if points.len() == 1 {
            let (x, y) = points[0];
            points.push((x + 0.002, y));
        }
        out.push(points);
    }
    if out.is_empty() {
        return Err("--draw has no stroke".into());
    }
    Ok(out)
}

fn numbers(text: &str, key: &str, count: usize) -> Result<Vec<f64>, String> {
    let values: Vec<f64> = text
        .split([',', ' '])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("--{key}: '{text}' is not {count} numbers"))?;
    if values.len() != count {
        return Err(format!("--{key}: '{text}' is not {count} numbers"));
    }
    Ok(values)
}

fn place(job: &Job, page: (f64, f64), aspect: f64) -> Result<[f64; 4], String> {
    if let Some(at) = job.get("at") {
        let v = numbers(at, "at", 4)?;
        if v[2] <= 0.0 || v[3] <= 0.0 {
            return Err("--at: the width and height must be positive".into());
        }
        return Ok([v[0], v[1], v[2], v[3]]);
    }
    let margin = job.number("margin")?.unwrap_or(36.0);
    let mut w = job.number("width")?.unwrap_or(160.0);
    let mut h = job.number("height")?.unwrap_or(w * aspect);
    let room = (page.0 - 2.0 * margin).max(10.0);
    if w > room {
        h *= room / w;
        w = room;
    }
    let (x, y) = match job.get("position").unwrap_or("bottom-right") {
        "bottom-right" => (page.0 - margin - w, margin),
        "bottom-left" => (margin, margin),
        "bottom-centre" | "bottom-center" => ((page.0 - w) / 2.0, margin),
        "top-right" => (page.0 - margin - w, page.1 - margin - h),
        "top-left" => (margin, page.1 - margin - h),
        "centre" | "center" => ((page.0 - w) / 2.0, (page.1 - h) / 2.0),
        other => return Err(format!("--position: '{other}' is not a place this knows")),
    };
    Ok([x, y, w, h])
}

fn colour(job: &Job) -> Result<[f64; 3], String> {
    match job.get("color").or_else(|| job.get("colour")) {
        None => Ok([0.05, 0.12, 0.45]),
        Some(text) => {
            let v = numbers(text, "color", 3)?;
            let scale = if v.iter().any(|&c| c > 1.0) {
                255.0
            } else {
                1.0
            };
            Ok([v[0] / scale, v[1] / scale, v[2] / scale])
        }
    }
}

fn commands(
    job: &Job,
    mark: &Mark,
    page_index: usize,
    crop: [f64; 4],
    rotate: i64,
    aspect: f64,
) -> Result<Vec<Command>, String> {
    let to_user = shown::shown_to_user(crop, rotate);
    let size = shown::shown_size(crop, rotate);
    let [x, y, w, h] = place(job, size, aspect)?;
    let unit = [w, 0.0, 0.0, h, x, y];
    let placement = shown::compose(unit, to_user);
    Ok(match mark {
        Mark::Picture(file) => vec![Command::PlaceNewImage {
            page_index,
            placement: Matrix {
                a: placement[0],
                b: placement[1],
                c: placement[2],
                d: placement[3],
                e: placement[4],
                f: placement[5],
            },
            file: Arc::clone(file),
        }],
        Mark::Strokes(strokes) => {
            let pen = PenStroke::pen(colour(job)?, job.number("pen")?.unwrap_or(1.6));
            strokes
                .iter()
                .map(|stroke| {
                    let steps = stroke
                        .iter()
                        .enumerate()
                        .map(|(at, &point)| {
                            let p = shown::apply(placement, point);
                            if at == 0 {
                                PenStep::Move(p)
                            } else {
                                PenStep::Line(p)
                            }
                        })
                        .collect();
                    Command::DrawPath {
                        page_index,
                        steps,
                        closed: false,
                        stroke: Some(pen),
                        fill: None,
                    }
                })
                .collect()
        }
        Mark::Typed(text) => {
            let frame = shown::rect_to_user(to_user, [x, y, x + w, y + h]);
            let chars = text.chars().count().max(1) as f64;
            let size = (h * 0.6).min(w / (0.7 * chars)).max(4.0);
            vec![Command::PlaceNewText {
                page_index,
                frame,
                text: text.clone(),
                family: job.get("font").unwrap_or("DejaVu Serif").to_owned(),
                size,
                bold: false,
                italic: true,
                fill: Some(colour(job)?),
                paragraph: ParagraphLayout {
                    alignment: Some(pdf_edit::layout::Alignment::Start),
                    flow_round: false,
                },
            }]
        }
    })
}

pub fn run(job: &Job) -> Result<Output, String> {
    convert_pdfdoc::seed_if_given(&job.random)?;
    let mark = match (job.files.get("image"), job.get("draw"), job.get("text")) {
        (Some(file), None, None) => Mark::Picture(Arc::from(file.as_slice())),
        (None, Some(draw), None) => Mark::Strokes(strokes(draw)?),
        (None, None, Some(text)) if !text.trim().is_empty() => Mark::Typed(text.trim().to_owned()),
        (None, None, None) => {
            return Err("give the signature: --image FILE, --draw STROKES or --text NAME".into());
        }
        _ => return Err("give one signature: --image, --draw or --text".into()),
    };
    let aspect = match &mark {
        Mark::Picture(file) => {
            let (w, h) = picture_size(file)
                .ok_or("the signature picture is not a PNG or a JPEG this can read")?;
            f64::from(h.max(1)) / f64::from(w.max(1))
        }
        Mark::Strokes(strokes) => {
            let (mut x0, mut y0, mut x1, mut y1) = (1.0_f64, 1.0_f64, 0.0_f64, 0.0_f64);
            for &(x, y) in strokes.iter().flatten() {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
            ((y1 - y0).max(0.05) / (x1 - x0).max(0.05)).clamp(0.1, 2.0)
        }
        Mark::Typed(_) => 0.3,
    };
    let password = job.password();
    let (plain, kept) = convert_pdfdoc::plain(job.pdf()?.to_vec(), &password)?;
    let source = ByteStore::owning(SourceId::next_document(), plain);
    let fonts = FONTS.get().cloned().flatten();
    let mut session = Session::with_fonts(source, b"", fonts);
    let geometries = session.page_geometries().map_err(|e| e.to_string())?;
    let count = geometries.len();
    let pages = match job.get("pages") {
        None => vec![count.saturating_sub(1)],
        Some(_) => job.pages("pages", count)?,
    };
    let mut done = 0;
    let mut left = Vec::new();
    for &page in &pages {
        let geometry = &geometries[page];
        let marks = commands(
            job,
            &mark,
            page,
            geometry.crop_box,
            i64::from(geometry.rotate),
            aspect,
        )?;
        match session.apply_each(&marks) {
            Ok(()) => done += 1,
            Err(e) => left.push(format!("page {}: {e}", page + 1)),
        }
    }
    if done == 0 {
        return Err(format!(
            "the signature could not be written: {}",
            left.join("; ")
        ));
    }
    let signed = session.source().to_vec();
    let mut notes = vec![format!(
        "signed {} page{} ({})",
        done,
        if done == 1 { "" } else { "s" },
        match &mark {
            Mark::Picture(_) => "picture",
            Mark::Strokes(_) => "drawn",
            Mark::Typed(_) => "typed",
        }
    )];
    for why in &left {
        notes.push(format!("left unsigned, {why}"));
    }
    if kept.as_ref().is_some_and(convert_pdfdoc::Kept::restricted) {
        notes.push(
            "the file's permissions say it may not be changed; they were set aside as unlock-pdf \
             does, and the signed file keeps them"
                .into(),
        );
    }
    let flat = if job.flag("flatten") {
        let doc = convert_pdfdoc::load(signed, &convert_pdfdoc::Load::default())
            .map_err(|e| e.to_string())?;
        if convert_pdfdoc::is_signed(&doc) {
            notes.push("the file's digital signatures do not survive --flatten".into());
        }
        notes.push("flattened: one revision, the unsigned document is not inside the file".into());
        convert_pdfdoc::write(&doc, None, true)?.bytes
    } else {
        signed
    };
    let main = match &kept {
        Some(kept) => {
            notes.push(
                "written again under the file's own protection (same passwords and permissions), as one revision"
                    .into(),
            );
            convert_pdfdoc::relock(flat, kept)?
        }
        None => flat,
    };
    convert_pdfdoc::check_written(&main, &password, count)?;
    notes.push("a visible signature; no digital (certificate) signature was made".into());
    Ok(Output {
        main,
        attachments: Vec::new(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawings_parse() {
        let s = strokes("0,0 0.5,1 1,0; 0.2,0.5").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].len(), 2);
        assert!(strokes("0,0 x").is_err());
    }

    fn pdf_of(pages: &[&str]) -> Vec<u8> {
        let n = pages.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
            "<< /Type /Font /Subtype /TrueType /BaseFont /NoFaceHasThisName \
             /FirstChar 65 /LastChar 65 /Widths [600] >>"
                .to_owned(),
        ];
        for (i, content) in pages.iter().enumerate() {
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents {} 0 R \
                 /Resources << /Font << /F1 3 0 R >> >> >>",
                5 + 2 * i
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ));
        }
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn a_page_that_cannot_be_signed_leaves_the_others_signed() {
        let pdf = pdf_of(&[
            "0 0 1 rg 10 10 20 20 re f",
            "BT /F1 12 Tf 7 Tr 50 50 Td (A) Tj ET 0 0 1 rg 10 10 20 20 re f",
        ]);
        let mut job = Job {
            inputs: vec![pdf],
            ..Job::default()
        };
        job.options.insert("draw".into(), "0,0 1,1".into());
        job.options.insert("pages".into(), "1-2".into());
        let out = run(&job).expect("page 1 is signed");
        assert!(
            out.notes
                .iter()
                .any(|note| note.starts_with("signed 1 page ")),
            "{:?}",
            out.notes
        );
        assert!(
            out.notes
                .iter()
                .any(|note| note.starts_with("left unsigned, page 2:")),
            "{:?}",
            out.notes
        );
        let mut job = Job {
            inputs: vec![pdf_of(&[
                "BT /F1 12 Tf 7 Tr 50 50 Td (A) Tj ET 0 0 1 rg 10 10 20 20 re f",
            ])],
            ..Job::default()
        };
        job.options.insert("draw".into(), "0,0 1,1".into());
        job.options.insert("pages".into(), "1".into());
        let refused = run(&job).unwrap_err();
        assert!(
            refused.starts_with("the signature could not be written: page 1:"),
            "{refused}"
        );
    }

    #[test]
    fn picture_sizes_come_from_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&300_u32.to_be_bytes());
        png.extend_from_slice(&100_u32.to_be_bytes());
        assert_eq!(picture_size(&png), Some((300, 100)));
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0, 50, 0, 120, 3, 0, 0, 0,
        ];
        assert_eq!(picture_size(&jpeg), Some((120, 50)));
    }
}
