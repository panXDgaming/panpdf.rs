use convert_files::{Done, Input, Job, Outcome, Settings};
use pdf_bytes::{ByteStore, SourceId};
use pdf_edit::text_layer::{LayerWord, TextLayer};
use pdf_paint::PaintAtomKind;

pub fn asked(inputs: &[Input], settings: &Settings) -> bool {
    settings.flag("survey")
        || settings.flag("draw")
        || settings.flag("judge")
        || inputs
            .iter()
            .any(|input| is_words(input) || table_page(input).is_some())
}

fn is_words(input: &Input) -> bool {
    input.name.to_ascii_lowercase().ends_with(".tsv") && table_page(input).is_none()
}

fn table_page(input: &Input) -> Option<usize> {
    let name = input.name.to_ascii_lowercase();
    let stem = name
        .strip_suffix(".tsv")
        .or_else(|| name.strip_suffix(".txt"))?;
    stem.strip_prefix("page-")?
        .parse()
        .ok()
        .filter(|&page| page > 0)
}

fn open(input: Input, password: &[u8]) -> Result<(String, ByteStore, usize), String> {
    let stem = input.stem().to_owned();
    let source = ByteStore::owning(SourceId::next_document(), input.bytes);
    let count = pdf_content::count_pages_recovering(
        &source,
        pdf_content::PageContentLimits::default(),
        pdf_content::RecoverLimits::default(),
        password,
    )
    .map_err(|e| format!("the file cannot be read as a PDF: {e}"))?
    .into_parts()
    .0;
    Ok((stem, source, count))
}

pub fn start(mut inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
    let password = settings.text("password", "").as_bytes().to_vec();
    if settings.flag("judge") {
        return judge(&inputs);
    }
    let tables: Vec<Input> = {
        let (tables, rest): (Vec<Input>, Vec<Input>) = inputs
            .into_iter()
            .partition(|input| table_page(input).is_some());
        inputs = rest;
        tables
    };
    if !tables.is_empty() {
        if inputs.len() != 1 {
            return Err("ocr-pdf takes one PDF at a time".to_owned());
        }
        let (stem, source, count) = open(inputs.remove(0), &password)?;
        let outcome = write_tables(&stem, &source, &password, count, &tables)?;
        return Ok(Box::new(Done(outcome)));
    }
    if settings.flag("draw") {
        if inputs.len() != 1 {
            return Err("ocr-pdf takes one PDF at a time".to_owned());
        }
        let (_, source, count) = open(inputs.remove(0), &password)?;
        let page = match settings.pages()?.as_slice() {
            [page] if *page < count => *page,
            [page] => {
                return Err(format!(
                    "page {} was asked for and the file has {count} pages",
                    page + 1
                ));
            }
            _ => return Err("draw takes one page: pages=N".to_owned()),
        };
        let view = pdf_session::interpret_page_for_display(&source, page, &password, None, None)
            .map_err(|e| format!("page {}: {e}", page + 1))?;
        let pgm = page_pgm(&view.layers(), &view.program.geometry)
            .map_err(|e| format!("page {}: {e}", page + 1))?;
        return Ok(Box::new(Done(Outcome {
            files: vec![(format!("page-{}.pgm", page + 1), pgm)],
            notes: Vec::new(),
        })));
    }
    let words = inputs.iter().position(is_words).map(|at| inputs.remove(at));
    if inputs.len() != 1 {
        return Err("ocr-pdf takes one PDF at a time".to_owned());
    }
    let (stem, source, count) = open(inputs.remove(0), &password)?;
    match words {
        Some(words) => {
            let layers = read_words(&words.bytes, count)?;
            let outcome = crate::write(&stem, &source, &password, layers, Vec::new())?;
            Ok(Box::new(Done(outcome)))
        }
        None => {
            let asked = settings.pages()?;
            if let Some(p) = asked.iter().find(|&&p| p >= count) {
                return Err(format!(
                    "page {} was asked for and the file has {count} pages",
                    p + 1
                ));
            }
            let pages: Vec<usize> = if asked.is_empty() {
                (0..count).collect()
            } else {
                asked
            };
            let max = settings.number("max", 0.0)?;
            if max >= 1.0 && pages.len() as f64 > max {
                return Err(format!(
                    "too many pages: {} chosen of {count}, at most {max} can be read at once",
                    pages.len()
                ));
            }
            Ok(Box::new(Survey {
                source,
                password,
                count,
                pages,
                next: 0,
                lines: Vec::new(),
            }))
        }
    }
}

struct Survey {
    source: ByteStore,
    password: Vec<u8>,
    count: usize,
    pages: Vec<usize>,
    next: usize,
    lines: Vec<String>,
}

impl Job for Survey {
    fn total(&self) -> usize {
        self.pages.len()
    }

    fn step(&mut self) -> Result<(), String> {
        let Some(&index) = self.pages.get(self.next) else {
            return Ok(());
        };
        self.next += 1;
        let view = pdf_session::interpret_page_for_display(
            &self.source,
            index,
            &self.password,
            None,
            None,
        )
        .map_err(|e| format!("page {}: {e}", index + 1))?;
        let has_text = view
            .graph
            .atoms
            .iter()
            .any(|a| matches!(a.kind, PaintAtomKind::Text(_)));
        let (width, height) = view.program.geometry.rotated_size();
        self.lines.push(format!(
            "{}\t{}\t{width:.2}\t{height:.2}",
            index + 1,
            if has_text { "text" } else { "scan" }
        ));
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Outcome, String> {
        let mut text = format!("pages\t{}\n", self.count);
        for line in &self.lines {
            text.push_str(line);
            text.push('\n');
        }
        Ok(Outcome {
            files: vec![("pages.tsv".to_owned(), text.into_bytes())],
            notes: Vec::new(),
        })
    }
}

fn judge(inputs: &[Input]) -> Result<Box<dyn Job>, String> {
    let find = |suffix: &str| {
        inputs
            .iter()
            .find(|input| input.name.to_ascii_lowercase().ends_with(suffix))
            .ok_or_else(|| format!("judge takes a {suffix} and its text"))
            .and_then(|input| {
                std::str::from_utf8(&input.bytes)
                    .map(str::to_owned)
                    .map_err(|_| format!("the {suffix} is not UTF-8"))
            })
    };
    let lines = pdf_ocr::tsv::read(&find(".tsv")?, &find(".txt")?).map_err(|e| e.to_string())?;
    let choice = if pdf_ocr::is_not_lao(&lines) {
        "tha"
    } else {
        "lao"
    };
    Ok(Box::new(Done(Outcome {
        files: vec![("choice.txt".to_owned(), choice.as_bytes().to_vec())],
        notes: Vec::new(),
    })))
}

fn write_tables(
    stem: &str,
    source: &ByteStore,
    password: &[u8],
    count: usize,
    tables: &[Input],
) -> Result<Outcome, String> {
    let mut pages: Vec<usize> = tables.iter().filter_map(table_page).collect();
    pages.sort_unstable();
    pages.dedup();
    let text_of = |page: usize, suffix: &str| -> Result<String, String> {
        let name = format!("page-{page}.{suffix}");
        let input = tables
            .iter()
            .find(|input| input.name.eq_ignore_ascii_case(&name))
            .ok_or_else(|| format!("{name} is missing"))?;
        String::from_utf8(input.bytes.clone()).map_err(|_| format!("{name} is not UTF-8"))
    };
    let mut layers = Vec::new();
    let mut notes = Vec::new();
    let mut confidence = Vec::new();
    for page in pages {
        if page > count {
            return Err(format!(
                "page {page} was read and the file has {count} pages"
            ));
        }
        let lines = pdf_ocr::tsv::read(&text_of(page, "tsv")?, &text_of(page, "txt")?)
            .map_err(|e| format!("page {page}: {e}"))?;
        let view = pdf_session::interpret_page_for_display(source, page - 1, password, None, None)
            .map_err(|e| format!("page {page}: {e}"))?;
        let (_, shown_height) = view.program.geometry.rotated_size();
        let reading = pdf_ocr::reading(&lines, f64::from(pdf_ocr::DPI) / 72.0, shown_height);
        if reading.layer.words.is_empty() {
            notes.push(format!("page {page}: no words were recognised"));
            continue;
        }
        if let Some(sure) = reading.confidence {
            confidence.push(sure);
        }
        layers.push((page - 1, reading.layer));
    }
    if layers.is_empty() {
        notes.push("no words were recognised on any page".to_owned());
        return Err(notes.join("; "));
    }
    let mut outcome = crate::write(stem, source, password, layers, notes)?;
    if !confidence.is_empty()
        && let Some(last) = outcome.notes.last_mut()
    {
        #[expect(clippy::cast_precision_loss, reason = "a handful of pages")]
        let mean = confidence.iter().sum::<f32>() / confidence.len() as f32;
        last.push_str(&format!(", mean confidence {mean:.0}%"));
    }
    Ok(outcome)
}

fn read_words(bytes: &[u8], count: usize) -> Result<Vec<(usize, TextLayer)>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the words file is not UTF-8".to_owned())?;
    let mut layers: Vec<(usize, TextLayer)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bad = || format!("words line {}: not a page, four numbers and a text", n + 1);
        let mut parts = line.splitn(6, '\t');
        let page: usize = parts
            .next()
            .and_then(|p| p.trim().parse().ok())
            .ok_or_else(bad)?;
        let mut frame = [0.0f64; 4];
        for value in &mut frame {
            *value = parts
                .next()
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .ok_or_else(bad)?;
        }
        let word = parts.next().ok_or_else(bad)?;
        if page == 0 || page > count {
            return Err(format!(
                "words line {}: page {page}, and the file has {count} pages",
                n + 1
            ));
        }
        if frame[2] <= frame[0] || frame[3] <= frame[1] {
            return Err(format!("words line {}: an empty box", n + 1));
        }
        if word.trim().is_empty() {
            continue;
        }
        let index = page - 1;
        let at = match layers.iter().position(|(p, _)| *p == index) {
            Some(at) => at,
            None => {
                layers.push((index, TextLayer::default()));
                layers.len() - 1
            }
        };
        layers[at].1.words.push(LayerWord {
            text: word.to_owned(),
            frame,
        });
    }
    if layers.is_empty() {
        return Err("no words were recognised on any page".to_owned());
    }
    Ok(layers)
}

fn page_pgm(
    layers: &[&pdf_paint::PaintGraph],
    geometry: &pdf_content::PageGeometry,
) -> Result<Vec<u8>, String> {
    let options = pdf_render::RenderOptions {
        scale: f64::from(pdf_ocr::DPI) / 72.0,
        ..pdf_render::RenderOptions::default()
    };
    let (canvas, _) =
        pdf_render::render_page_layers(layers, geometry, options).map_err(|e| e.to_string())?;
    let mut bytes = format!("P5\n{} {}\n255\n", canvas.width, canvas.height).into_bytes();
    bytes.extend(canvas.to_rgb8().as_chunks::<3>().0.iter().map(|pixel| {
        let sum = 299 * u32::from(pixel[0]) + 587 * u32::from(pixel[1]) + 114 * u32::from(pixel[2]);
        u8::try_from(sum / 1000).unwrap_or(u8::MAX)
    }));
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::read_words;

    #[test]
    fn words_are_read_page_by_page_and_bad_lines_named() {
        let layers = read_words(
            "2\t10\t700\t80\t712\tຍິນດີ \n1\t10\t700\t50\t712\tHello \n2\t82\t700\t120\t712\tຕ້ອນຮັບ\n2\t1\t1\t2\t2\t  \n".as_bytes(),
            2,
        )
        .unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].0, 1);
        assert_eq!(layers[0].1.words.len(), 2);
        assert_eq!(layers[0].1.words[1].text, "ຕ້ອນຮັບ");
        assert_eq!(layers[1].1.words[0].frame, [10.0, 700.0, 50.0, 712.0]);
        assert!(
            read_words(b"3\t1\t1\t2\t2\tx\n", 2)
                .unwrap_err()
                .contains("page 3")
        );
        assert!(
            read_words(b"1\t1\tx\t2\t2\tx\n", 2)
                .unwrap_err()
                .contains("line 1")
        );
        assert!(
            read_words(b"1\t5\t1\t2\t2\tx\n", 2)
                .unwrap_err()
                .contains("empty box")
        );
        assert!(
            read_words(b"\n1\t1\t1\t2\t2\t \n", 2)
                .unwrap_err()
                .contains("no words")
        );
    }
}
