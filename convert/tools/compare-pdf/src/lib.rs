use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use convert_pdftool::{Job, Output, Tool};
use convert_structure::model::{Block, Page, Paragraph};
use convert_structure::{FontProvider, Options, Reader};

pub static TOOL: Tool = Tool {
    name: "compare-pdf",
    extension: "html",
    inputs: 2,
    file_options: &[],
    flags: &["no-pictures"],
    help: "  --format html|txt        the report (default html, pictures inside)\n  \
           --no-pictures            text only\n  \
           --dpi N                  the pictures' resolution (default 72)\n  \
           --password PW            the old file's password   --password2 PW  the new file's",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

static FONTS: OnceLock<Option<Arc<dyn FontProvider>>> = OnceLock::new();

pub fn set_fonts(fonts: Option<Arc<dyn FontProvider>>) {
    let _ = FONTS.set(fonts);
}

const OPTIONS: Options = Options {
    pictures: false,
    tables: true,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Same(String),
    Removed(String),
    Added(String),
}

fn paragraph_text(p: &Paragraph) -> String {
    p.text_with_marker().replace('\n', " ")
}

#[must_use]
pub fn page_text(page: &Page) -> String {
    let mut lines: Vec<String> = page.header.iter().map(paragraph_text).collect();
    for block in &page.blocks {
        match block {
            Block::Paragraph(p) => lines.push(paragraph_text(p)),
            Block::Table(table) => {
                for row in &table.rows {
                    let cells: Vec<String> = row
                        .cells
                        .iter()
                        .filter(|c| !c.covered)
                        .map(|c| {
                            c.paragraphs
                                .iter()
                                .map(paragraph_text)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .collect();
                    lines.push(cells.join(" | "));
                }
            }
            Block::Picture(_) => {}
        }
    }
    lines.extend(page.footer.iter().map(paragraph_text));
    lines.retain(|l| !l.trim().is_empty());
    lines.join("\n")
}

fn unspaced(c: char) -> bool {
    matches!(u32::from(c),
        0x0E00..=0x0EFF
        | 0x1000..=0x109F
        | 0x1780..=0x17FF
        | 0x3040..=0x30FF
        | 0x3400..=0x9FFF
        | 0xAC00..=0xD7AF)
}

#[must_use]
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split_whitespace() {
        if !token.chars().any(unspaced) {
            out.push(token.to_owned());
            continue;
        }
        let boundaries: Vec<usize> = token.char_indices().map(|(i, _)| i).collect();
        let mut start = 0;
        for cut in pdf_edit::layout::line_break_opportunities(token, &boundaries) {
            out.push(token[start..cut].to_owned());
            start = cut;
        }
        out.push(token[start..].to_owned());
    }
    out
}

const MOST_WORDS: usize = 4000;

#[must_use]
pub fn diff(old: &[String], new: &[String]) -> Vec<Step> {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let a = &old[head..old.len() - tail];
    let b = &new[head..new.len() - tail];
    let mut steps: Vec<Step> = old[..head].iter().cloned().map(Step::Same).collect();
    if a.len() * b.len() > MOST_WORDS * MOST_WORDS {
        steps.extend(a.iter().cloned().map(Step::Removed));
        steps.extend(b.iter().cloned().map(Step::Added));
    } else {
        let w = b.len() + 1;
        let mut table = vec![0_u32; (a.len() + 1) * w];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                table[i * w + j] = if a[i] == b[j] {
                    table[(i + 1) * w + j + 1] + 1
                } else {
                    table[(i + 1) * w + j].max(table[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                steps.push(Step::Same(a[i].clone()));
                i += 1;
                j += 1;
            } else if i < a.len()
                && (j == b.len() || table[(i + 1) * w + j] >= table[i * w + j + 1])
            {
                steps.push(Step::Removed(a[i].clone()));
                i += 1;
            } else {
                steps.push(Step::Added(b[j].clone()));
                j += 1;
            }
        }
    }
    steps.extend(old[old.len() - tail..].iter().cloned().map(Step::Same));
    steps
}

fn picture(old: &Reader, new: &Reader, page: usize, dpi: f64) -> Result<(Vec<u8>, f64), String> {
    let draw = |reader: &Reader| -> Result<pdf_render::Canvas, String> {
        let view = reader.view(page)?;
        pdf_render::render_page_layers(
            &view.layers(),
            &view.program.geometry,
            pdf_render::RenderOptions {
                scale: dpi / 72.0,
                ..pdf_render::RenderOptions::default()
            },
        )
        .map(|(canvas, _)| canvas)
        .map_err(|e| e.to_string())
    };
    let a = draw(old)?;
    let b = draw(new)?;
    let (w, h) = (a.width.max(b.width), a.height.max(b.height));
    let at = |c: &pdf_render::Canvas, x: u32, y: u32| -> [f32; 3] {
        if x < c.width && y < c.height {
            c.pixels[(y * c.width + x) as usize]
        } else {
            [1.0, 1.0, 1.0]
        }
    };
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    let mut changed = 0_u64;
    for y in 0..h {
        for x in 0..w {
            let p = at(&a, x, y);
            let q = at(&b, x, y);
            let delta = (0..3).map(|k| (p[k] - q[k]).abs()).fold(0.0_f32, f32::max);
            if delta > 0.12 {
                changed += 1;
                let dark_new = q.iter().sum::<f32>() < p.iter().sum::<f32>();
                out.extend_from_slice(if dark_new {
                    &[220, 20, 20]
                } else {
                    &[20, 90, 230]
                });
            } else {
                let g = p.iter().sum::<f32>() / 3.0;
                let faded = (0.75 + 0.25 * g).clamp(0.0, 1.0);
                let v = (faded * 255.0).round() as u8;
                out.extend_from_slice(&[v, v, v]);
            }
        }
    }
    let png = pdf_edit::png::write((w, h), &out, None).map_err(str::to_owned)?;
    Ok((png, changed as f64 / f64::from((w * h).max(1))))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[derive(Debug)]
pub struct PageDiff {
    pub page: usize,
    pub steps: Vec<Step>,
    pub added: usize,
    pub removed: usize,
    pub picture: Option<(Vec<u8>, f64)>,
    pub only_in: Option<&'static str>,
}

pub fn compare(job: &Job) -> Result<Vec<PageDiff>, String> {
    let [old, new] = job.inputs.as_slice() else {
        return Err("give two PDFs: the old one and the new one".into());
    };
    let fonts = FONTS.get().cloned().flatten();
    let old = Reader::open(old.clone(), &job.password(), fonts.clone())
        .map_err(|e| format!("the first file: {e}"))?;
    let second = job.get("password2").unwrap_or_default().as_bytes().to_vec();
    let new =
        Reader::open(new.clone(), &second, fonts).map_err(|e| format!("the second file: {e}"))?;
    let dpi = job.number("dpi")?.unwrap_or(72.0).clamp(20.0, 300.0);
    let pictures = !job.flag("no-pictures") && job.get("format") != Some("txt");
    let mut out = Vec::new();
    for page in 0..old.page_count().max(new.page_count()) {
        let a = (page < old.page_count()).then(|| page_text(&old.read_page(page, &OPTIONS)));
        let b = (page < new.page_count()).then(|| page_text(&new.read_page(page, &OPTIONS)));
        let only_in = match (&a, &b) {
            (Some(_), None) => Some("first"),
            (None, Some(_)) => Some("second"),
            _ => None,
        };
        let wa = words(a.as_deref().unwrap_or_default());
        let wb = words(b.as_deref().unwrap_or_default());
        let steps = diff(&wa, &wb);
        let added = steps.iter().filter(|s| matches!(s, Step::Added(_))).count();
        let removed = steps
            .iter()
            .filter(|s| matches!(s, Step::Removed(_)))
            .count();
        let picture = if pictures && only_in.is_none() {
            picture(&old, &new, page, dpi).ok()
        } else {
            None
        };
        out.push(PageDiff {
            page,
            steps,
            added,
            removed,
            picture,
            only_in,
        });
    }
    Ok(out)
}

fn changed(d: &PageDiff) -> bool {
    d.added > 0
        || d.removed > 0
        || d.only_in.is_some()
        || d.picture.as_ref().is_some_and(|p| p.1 > 0.0)
}

fn text_report(pages: &[PageDiff]) -> String {
    let mut out = String::new();
    for d in pages.iter().filter(|d| changed(d)) {
        let _ = write!(out, "== page {}", d.page + 1);
        if let Some(side) = d.only_in {
            let _ = writeln!(out, ": only in the {side} file");
            continue;
        }
        let _ = write!(out, ": +{} -{} words", d.added, d.removed);
        if let Some((_, share)) = &d.picture {
            let _ = write!(out, ", {:.2}% of pixels changed", share * 100.0);
        }
        out.push('\n');
        for step in &d.steps {
            match step {
                Step::Removed(w) => {
                    let _ = writeln!(out, "- {w}");
                }
                Step::Added(w) => {
                    let _ = writeln!(out, "+ {w}");
                }
                Step::Same(_) => {}
            }
        }
    }
    if out.is_empty() {
        out.push_str("no differences\n");
    }
    out
}

fn html_report(pages: &[PageDiff]) -> String {
    let mut out = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>PDF comparison</title><style>\
         body{font-family:system-ui,'Noto Sans Lao','Noto Sans Thai',sans-serif;max-width:1100px;margin:auto;padding:16px}\
         del{background:#fdd;color:#900}ins{background:#dfd;color:#060;text-decoration:none}\
         table{border-collapse:collapse}td,th{border:1px solid #ccc;padding:2px 8px}\
         .page{display:flex;gap:16px;flex-wrap:wrap;margin:12px 0 32px}.text{flex:1 1 400px;line-height:1.7}\
         img{max-width:100%;border:1px solid #999}</style></head><body><h1>PDF comparison</h1>",
    );
    let total_added: usize = pages.iter().map(|d| d.added).sum();
    let total_removed: usize = pages.iter().map(|d| d.removed).sum();
    let changed_pages: Vec<&PageDiff> = pages.iter().filter(|d| changed(d)).collect();
    let _ = write!(
        out,
        "<p>{} pages compared; {} with differences; {total_added} words added, {total_removed} removed. \
         In the pictures, <b style=color:#d00>red</b> is ink only the second file has, <b style=color:#15e>blue</b> ink only the first had.</p>\
         <table><tr><th>page</th><th>added</th><th>removed</th><th>pixels changed</th></tr>",
        pages.len(),
        changed_pages.len()
    );
    for d in &changed_pages {
        let pixels = match (d.only_in, &d.picture) {
            (Some(side), _) => format!("only in the {side} file"),
            (None, Some((_, share))) => format!("{:.2}%", share * 100.0),
            _ => String::new(),
        };
        let _ = write!(
            out,
            "<tr><td><a href=\"#p{0}\">{0}</a></td><td>{1}</td><td>{2}</td><td>{pixels}</td></tr>",
            d.page + 1,
            d.added,
            d.removed
        );
    }
    out.push_str("</table>");
    for d in changed_pages {
        let _ = write!(
            out,
            "<h2 id=\"p{0}\">Page {0}</h2><div class=\"page\"><div class=\"text\">",
            d.page + 1
        );
        for step in &d.steps {
            match step {
                Step::Same(w) => {
                    let _ = write!(out, "{} ", escape(w));
                }
                Step::Removed(w) => {
                    let _ = write!(out, "<del>{}</del> ", escape(w));
                }
                Step::Added(w) => {
                    let _ = write!(out, "<ins>{}</ins> ", escape(w));
                }
            }
        }
        out.push_str("</div>");
        if let Some((png, _)) = &d.picture {
            let _ = write!(
                out,
                "<div><img alt=\"page {}\" src=\"data:image/png;base64,{}\"></div>",
                d.page + 1,
                base64(png)
            );
        }
        out.push_str("</div>");
    }
    out.push_str("</body></html>\n");
    out
}

pub fn run(job: &Job) -> Result<Output, String> {
    let pages = compare(job)?;
    let differ = pages.iter().filter(|d| changed(d)).count();
    let added: usize = pages.iter().map(|d| d.added).sum();
    let removed: usize = pages.iter().map(|d| d.removed).sum();
    let main = if job.get("format") == Some("txt") {
        text_report(&pages)
    } else {
        html_report(&pages)
    };
    Ok(Output {
        main: main.into_bytes(),
        attachments: Vec::new(),
        notes: vec![format!(
            "{} pages compared, {differ} differ: {added} words added, {removed} removed",
            pages.len()
        )],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str) -> Vec<String> {
        text.split(' ').map(String::from).collect()
    }

    #[test]
    fn a_changed_word_is_one_removal_and_one_addition() {
        let steps = diff(&w("the quick brown fox"), &w("the slow brown fox jumps"));
        assert_eq!(
            steps,
            vec![
                Step::Same("the".into()),
                Step::Removed("quick".into()),
                Step::Added("slow".into()),
                Step::Same("brown".into()),
                Step::Same("fox".into()),
                Step::Added("jumps".into()),
            ]
        );
    }

    #[test]
    fn lao_is_split_into_words() {
        let words = words("ພາສາລາວ ok");
        assert!(words.len() >= 3, "{words:?}");
        assert_eq!(words.concat(), "ພາສາລາວok");
    }

    #[test]
    fn base64_is_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
