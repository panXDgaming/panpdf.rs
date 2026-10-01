use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use convert_pdfdoc::{Dict, Doc, Load, Object, Value, shown};
use convert_pdftool::{Job, Output, Tool};
use pdf_bytes::{ByteStore, SourceId};
use pdf_content::{Code, FontProvider};
use pdf_edit::{Command, GlyphChange, RunRewrite, SourceAnchor};
use pdf_paint::{PaintAtomKind, TextShowPaint};
use pdf_session::{PageView, Session};

pub static TOOL: Tool = Tool {
    name: "redact-pdf",
    extension: "pdf",
    inputs: 1,
    file_options: &[],
    flags: &["annotations", "case"],
    help: "  --areas \"P:X0,Y0,X1,Y1; ...\"   page (from 1) and a rectangle in points on the page as shown, from its bottom left\n  \
           --search \"words|other words\"   every place these appear (whitespace ignored); --case to match case\n  \
           --pages 1,3-5                  where to search (default all)\n  \
           --annotations                  apply the file's Redact annotations (the default when nothing else is asked)\n  \
           --color R,G,B                  the boxes' colour (default black)\n  \
           --password PW                  when the file is protected",
    run,
};

convert_pdftool::export_pdf_tool!(crate::TOOL);

static FONTS: OnceLock<Option<Arc<dyn FontProvider>>> = OnceLock::new();

pub fn set_fonts(fonts: Option<Arc<dyn FontProvider>>) {
    let _ = FONTS.set(fonts);
}

type Rect = [f64; 4];

fn overlap(a: Rect, b: Rect) -> f64 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h
}

fn area(a: Rect) -> f64 {
    (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0)
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner[0] >= outer[0] - 0.5
        && inner[1] >= outer[1] - 0.5
        && inner[2] <= outer[2] + 0.5
        && inner[3] <= outer[3] + 0.5
}

fn under(b: Rect, r: Rect) -> bool {
    let cx = (b[0] + b[2]) / 2.0;
    let cy = (b[1] + b[3]) / 2.0;
    (cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3])
        || overlap(b, r) >= area(b).max(1e-6) / 3.0
}

fn glyph_text(text: &TextShowPaint, glyphs: std::ops::Range<usize>) -> String {
    let mut out = String::new();
    for glyph in text.glyphs.get(glyphs).unwrap_or_default() {
        match text.text.text_of(Code {
            value: glyph.code.value,
            byte_len: glyph.code.bytes.len(),
        }) {
            Some(meaning) => out.push_str(&meaning.text),
            None => out.push('\u{fffd}'),
        }
    }
    out
}

struct Piece {
    atom: usize,
    glyphs: std::ops::Range<usize>,
    rect: Rect,
    text: String,
}

fn pieces(view: &PageView) -> Vec<Piece> {
    let mut order: Vec<usize> = Vec::new();
    let mut seen = vec![false; view.index.clusters.len()];
    for block in &view.index.blocks {
        for &line in &block.lines {
            if let Some(line) = view.index.lines.get(line) {
                for &c in &line.clusters {
                    if c < seen.len() && !seen[c] {
                        seen[c] = true;
                        order.push(c);
                    }
                }
            }
        }
    }
    order.extend((0..seen.len()).filter(|&c| !seen[c]));
    order
        .into_iter()
        .filter_map(|c| {
            let cluster = &view.index.clusters[c];
            let atom = view.graph.atoms.get(cluster.atom)?;
            let PaintAtomKind::Text(text) = &atom.kind else {
                return None;
            };
            let rect = cluster.bounds.or(cluster.layout).unwrap_or_else(|| {
                let (x, y, em) = (cluster.baseline.x, cluster.baseline.y, cluster.em);
                [
                    x,
                    y - 0.25 * em,
                    x + cluster.advance.max(0.3 * em),
                    y + 0.85 * em,
                ]
            });
            Some(Piece {
                atom: cluster.atom,
                glyphs: cluster.glyphs.clone(),
                rect,
                text: glyph_text(text, cluster.glyphs.clone()),
            })
        })
        .collect()
}

fn fold(text: &str, case: bool) -> Vec<char> {
    let chars = text.chars().filter(|c| !c.is_whitespace());
    if case {
        chars.collect()
    } else {
        chars.flat_map(char::to_lowercase).collect()
    }
}

fn found(view: &PageView, words: &[String], case: bool) -> (Vec<Rect>, usize) {
    let pieces = pieces(view);
    let mut chars: Vec<(char, usize)> = Vec::new();
    for (at, piece) in pieces.iter().enumerate() {
        for c in fold(&piece.text, case) {
            chars.push((c, at));
        }
    }
    let mut rects = Vec::new();
    let mut count = 0;
    for word in words {
        let want = fold(word, case);
        if want.is_empty() {
            continue;
        }
        let mut at = 0;
        while at + want.len() <= chars.len() {
            if chars[at..at + want.len()]
                .iter()
                .map(|(c, _)| *c)
                .eq(want.iter().copied())
            {
                count += 1;
                let mut hit: Vec<usize> =
                    chars[at..at + want.len()].iter().map(|(_, p)| *p).collect();
                hit.dedup();
                let mut current: Option<Rect> = None;
                for p in hit {
                    let r = pieces[p].rect;
                    current = match current {
                        Some(c) if r[1] < c[3] && r[3] > c[1] => Some([
                            c[0].min(r[0]),
                            c[1].min(r[1]),
                            c[2].max(r[2]),
                            c[3].max(r[3]),
                        ]),
                        Some(c) => {
                            rects.push(c);
                            Some(r)
                        }
                        None => Some(r),
                    };
                }
                rects.extend(current);
                at += want.len();
            } else {
                at += 1;
            }
        }
    }
    let grown = rects
        .into_iter()
        .map(|r| [r[0] - 0.5, r[1] - 0.5, r[2] + 0.5, r[3] + 0.5])
        .collect();
    (grown, count)
}

fn removals(
    view: &PageView,
    areas: &[Rect],
    object_areas: &[Rect],
) -> BTreeMap<(u32, u16), (Vec<RunRewrite>, Vec<SourceAnchor>)> {
    let mut by_stream: BTreeMap<(u32, u16), (Vec<RunRewrite>, Vec<SourceAnchor>)> = BTreeMap::new();
    let mut ranges: BTreeMap<usize, std::ops::Range<usize>> = BTreeMap::new();
    let mut closed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut all = pieces(view);
    all.sort_by_key(|p| (p.atom, p.glyphs.start));
    for piece in &all {
        let hit = areas.iter().any(|&a| under(piece.rect, a));
        if !hit {
            if ranges.contains_key(&piece.atom) {
                closed.insert(piece.atom);
            }
            continue;
        }
        if closed.contains(&piece.atom) {
            continue;
        }
        ranges
            .entry(piece.atom)
            .and_modify(|r| {
                if piece.glyphs.start == r.end {
                    r.end = piece.glyphs.end;
                } else {
                    closed.insert(piece.atom);
                }
            })
            .or_insert(piece.glyphs.clone());
    }
    for (atom, glyphs) in ranges {
        let id = &view.graph.atoms[atom].id;
        let anchor = SourceAnchor::of(id);
        let key = (anchor.stream.object_number(), anchor.stream.generation());
        by_stream.entry(key).or_default().0.push(RunRewrite {
            anchor,
            glyphs: Some(GlyphChange::Remove {
                glyphs,
                close_gap: false,
            }),
            displace: (0.0, 0.0),
        });
    }
    for atom in &view.graph.atoms {
        let Some(bounds) = atom.kind.user_bounds() else {
            continue;
        };
        let take = match &atom.kind {
            PaintAtomKind::Text(_) => false,
            PaintAtomKind::Image(_) => object_areas.iter().any(|&a| overlap(bounds, a) > 0.0),
            _ => object_areas.iter().any(|&a| inside(bounds, a)),
        };
        if take {
            let anchor = SourceAnchor::of(&atom.id);
            let key = (anchor.stream.object_number(), anchor.stream.generation());
            by_stream.entry(key).or_default().1.push(anchor);
        }
    }
    by_stream
}

#[derive(Default, Debug)]
struct PageDone {
    edits: usize,
    pictures: usize,
    refused: Option<String>,
}

fn redact_page(
    session: &mut Session,
    page: usize,
    areas: &[Rect],
    object_areas: &[Rect],
) -> PageDone {
    let mut done = PageDone::default();
    for _ in 0..200 {
        let view = match session.page(page) {
            Ok(view) => view,
            Err(e) => {
                done.refused = Some(e.to_string());
                return done;
            }
        };
        let groups = removals(&view, areas, object_areas);
        let Some((_, (runs, objects))) = groups
            .into_iter()
            .max_by_key(|(_, (r, o))| r.len() + o.len())
        else {
            return done;
        };
        let pictures = objects
            .iter()
            .filter(|anchor| {
                view.graph.atoms.iter().any(|a| {
                    matches!(a.kind, PaintAtomKind::Image(_)) && SourceAnchor::of(&a.id) == **anchor
                })
            })
            .count();
        let command = Command::DeleteGroup {
            page_index: page,
            runs,
            objects,
        };
        let applied = session
            .plan(&command)
            .map_err(|e| e.to_string())
            .and_then(|plan| session.apply(plan).map_err(|e| e.to_string()));
        if let Err(why) = applied {
            done.refused = Some(why);
            return done;
        }
        done.edits += 1;
        done.pictures += pictures;
    }
    done.refused = Some("too many pieces to remove one by one".into());
    done
}

pub fn parse_areas(text: &str) -> Result<Vec<(usize, Rect)>, String> {
    let mut out = Vec::new();
    for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let bad = || format!("--areas: '{part}' is not PAGE:X0,Y0,X1,Y1");
        let (page, rect) = part.split_once(':').ok_or_else(bad)?;
        let page: usize = page.trim().parse().map_err(|_| bad())?;
        let v: Vec<f64> = rect
            .split(',')
            .map(|n| n.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| bad())?;
        if page == 0 || v.len() != 4 {
            return Err(bad());
        }
        out.push((
            page - 1,
            [
                v[0].min(v[2]),
                v[1].min(v[3]),
                v[0].max(v[2]),
                v[1].max(v[3]),
            ],
        ));
    }
    Ok(out)
}

fn redact_annotations(doc: &Doc) -> (BTreeMap<usize, Vec<Rect>>, Vec<u32>) {
    let mut areas: BTreeMap<usize, Vec<Rect>> = BTreeMap::new();
    let mut numbers = Vec::new();
    for (index, page) in doc.pages().iter().enumerate() {
        let Some(dict) = doc.get(page.number).and_then(Object::dict) else {
            continue;
        };
        let Some(annots) = dict
            .get("Annots")
            .map(|v| doc.resolve(v))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for annot in annots {
            let Some(number) = annot.as_ref() else {
                continue;
            };
            let Some(a) = doc.get(number).and_then(Object::dict) else {
                continue;
            };
            if !a.is("Subtype", "Redact") {
                continue;
            }
            numbers.push(number);
            let quads: Vec<f64> = a
                .get("QuadPoints")
                .map(|v| doc.resolve(v))
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| doc.resolve(i).as_number())
                        .collect()
                })
                .unwrap_or_default();
            if quads.len() >= 8 {
                for quad in quads.as_chunks::<8>().0 {
                    let xs = [quad[0], quad[2], quad[4], quad[6]];
                    let ys = [quad[1], quad[3], quad[5], quad[7]];
                    areas.entry(index).or_default().push([
                        xs.iter().copied().fold(f64::INFINITY, f64::min),
                        ys.iter().copied().fold(f64::INFINITY, f64::min),
                        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    ]);
                }
            } else if let Some(r) = doc.rect(a, "Rect") {
                areas.entry(index).or_default().push(r);
            }
        }
    }
    (areas, numbers)
}

fn colour(job: &Job) -> Result<[f64; 3], String> {
    match job.get("color").or_else(|| job.get("colour")) {
        None => Ok([0.0, 0.0, 0.0]),
        Some(text) => {
            let v: Vec<f64> = text
                .split(',')
                .map(|n| n.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("--color: '{text}' is not R,G,B"))?;
            if v.len() != 3 {
                return Err(format!("--color: '{text}' is not R,G,B"));
            }
            let scale = if v.iter().any(|&c| c > 1.0) {
                255.0
            } else {
                1.0
            };
            Ok([v[0] / scale, v[1] / scale, v[2] / scale])
        }
    }
}

fn rasterise(
    source: &ByteStore,
    page: usize,
    areas: &[Rect],
    fill: [f64; 3],
    fonts: Option<Arc<dyn FontProvider>>,
) -> Result<(Vec<u8>, u32, u32, [f64; 6]), String> {
    let view = pdf_session::interpret_page_for_display(source, page, b"", None, fonts)
        .map_err(|e| e.to_string())?;
    let geometry = &view.program.geometry;
    let scale = 150.0 / 72.0;
    let (canvas, _) = pdf_render::render_page_layers(
        &[&view.graph],
        geometry,
        pdf_render::RenderOptions {
            scale,
            ..pdf_render::RenderOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let to_user = shown::shown_to_user(geometry.crop_box, i64::from(geometry.rotate));
    let to_shown = shown::invert(to_user);
    let (width, height) = (canvas.width, canvas.height);
    let mut pixels = Vec::with_capacity(canvas.pixels.len() * 3);
    for p in &canvas.pixels {
        for c in p {
            pixels.push((c.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    let (sw, sh) = shown::shown_size(geometry.crop_box, i64::from(geometry.rotate));
    let fill = fill.map(|c| (c * 255.0).round() as u8);
    for &a in areas {
        let r = shown::rect_to_user(to_shown, a);
        let x0 = ((r[0] / sw) * f64::from(width)).floor().max(0.0) as u32;
        let x1 = ((r[2] / sw) * f64::from(width))
            .ceil()
            .min(f64::from(width)) as u32;
        let y0 = (((sh - r[3]) / sh) * f64::from(height)).floor().max(0.0) as u32;
        let y1 = (((sh - r[1]) / sh) * f64::from(height))
            .ceil()
            .min(f64::from(height)) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let at = ((y * width + x) * 3) as usize;
                pixels[at..at + 3].copy_from_slice(&fill);
            }
        }
    }
    let placement = shown::compose([sw, 0.0, 0.0, sh, 0.0, 0.0], to_user);
    Ok((pixels, width, height, placement))
}

fn replace_with_picture(
    doc: &mut Doc,
    page_number: u32,
    (pixels, width, height, m): (Vec<u8>, u32, u32, [f64; 6]),
) {
    let mut image = Dict::new();
    image.set("Type", Value::name("XObject"));
    image.set("Subtype", Value::name("Image"));
    image.set("Width", Value::Int(i64::from(width)));
    image.set("Height", Value::Int(i64::from(height)));
    image.set("ColorSpace", Value::name("DeviceRGB"));
    image.set("BitsPerComponent", Value::Int(8));
    image.set("Filter", Value::name("FlateDecode"));
    let image = doc.add(Object::stream(image, convert_pdfdoc::deflate(&pixels)));
    let content = format!(
        "q {} {} {} {} {} {} cm /Redacted Do Q\n",
        convert_pdfdoc::real(m[0]),
        convert_pdfdoc::real(m[1]),
        convert_pdfdoc::real(m[2]),
        convert_pdfdoc::real(m[3]),
        convert_pdfdoc::real(m[4]),
        convert_pdfdoc::real(m[5])
    );
    let content = doc.add(Object::stream(Dict::new(), content.into_bytes()));
    if let Some(page) = doc.get_mut(page_number).and_then(Object::dict_mut) {
        let mut xobjects = Dict::new();
        xobjects.set("Redacted", Value::Ref(image));
        let mut resources = Dict::new();
        resources.set("XObject", Value::Dict(xobjects));
        page.set("Resources", Value::Dict(resources));
        page.set("Contents", Value::Ref(content));
        page.remove("Group");
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the steps are listed at the top of the file"
)]
pub fn run(job: &Job) -> Result<Output, String> {
    convert_pdfdoc::seed_if_given(&job.random)?;
    let password = job.password();
    let (plain, kept) = convert_pdfdoc::plain(job.pdf()?.to_vec(), &password)?;
    let fonts = FONTS.get().cloned().flatten();
    let fill = colour(job)?;
    let case = job.flag("case");
    let words: Vec<String> = job
        .get("search")
        .map(|s| {
            s.split('|')
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    let doc = convert_pdfdoc::load(plain.clone(), &Load::default()).map_err(|e| e.to_string())?;
    let page_refs = doc.pages();
    let count = page_refs.len();
    let mut areas: BTreeMap<usize, Vec<Rect>> = BTreeMap::new();
    if let Some(text) = job.get("areas") {
        for (page, rect) in parse_areas(text)? {
            let p = page_refs.get(page).ok_or_else(|| {
                format!(
                    "--areas: page {} is not in the file ({count} pages)",
                    page + 1
                )
            })?;
            let m = shown::shown_to_user(p.crop_box, p.rotate);
            areas
                .entry(page)
                .or_default()
                .push(shown::rect_to_user(m, rect));
        }
    }
    let (annotated, redact_annots) = redact_annotations(&doc);
    let use_annotations =
        job.flag("annotations") || (job.get("areas").is_none() && words.is_empty());
    let mut notes = Vec::new();
    if kept.as_ref().is_some_and(convert_pdfdoc::Kept::restricted) {
        notes.push(
            "the file's permissions say it may not be changed; they were set aside as unlock-pdf \
             does, and the redacted file keeps them"
                .into(),
        );
    }
    if use_annotations {
        for (page, rects) in &annotated {
            areas.entry(*page).or_default().extend(rects);
        }
        notes.push(format!(
            "{} Redact annotations applied",
            redact_annots.len()
        ));
    }
    let object_areas = areas.clone();
    let source = ByteStore::owning(SourceId::next_document(), plain);
    let mut session = Session::with_fonts(source, b"", fonts.clone());
    let mut matches = 0;
    let searched = if words.is_empty() {
        Vec::new()
    } else {
        job.pages("pages", count)?
    };
    if !words.is_empty() {
        let mut unread = Vec::new();
        for page in searched.clone() {
            let view = match session.page(page) {
                Ok(view) => view,
                Err(e) => {
                    unread.push(format!("{} ({e})", page + 1));
                    continue;
                }
            };
            let (rects, n) = found(&view, &words, case);
            matches += n;
            if !rects.is_empty() {
                areas.entry(page).or_default().extend(rects);
            }
        }
        notes.push(format!("{matches} matches of the searched words"));
        if !unread.is_empty() {
            notes.insert(
                0,
                format!(
                    "WARNING: {} pages could not be read and were NOT searched: {}",
                    unread.len(),
                    unread.join(", ")
                ),
            );
        }
    }
    if areas.is_empty() {
        return Err(if words.is_empty() {
            "nothing to redact: give --areas or --search (the file has no Redact annotation)".into()
        } else {
            "the searched words were not found; nothing was changed".into()
        });
    }

    let mut rasterise_pages = Vec::new();
    let mut edits = 0;
    let mut pictures = 0;
    for (&page, rects) in &areas {
        let objects = object_areas.get(&page).map_or(&[][..], Vec::as_slice);
        let done = redact_page(&mut session, page, rects, objects);
        edits += done.edits;
        pictures += done.pictures;
        if let Some(why) = done.refused {
            notes.push(format!(
                "page {}: the engine would not remove the text piece by piece ({why}); the page was replaced by a picture of itself",
                page + 1
            ));
            rasterise_pages.push(page);
            continue;
        }
        let boxes: Vec<Command> = rects
            .iter()
            .map(|r| Command::DrawPath {
                page_index: page,
                steps: vec![
                    pdf_edit::PenStep::Move((r[0], r[1])),
                    pdf_edit::PenStep::Line((r[2], r[1])),
                    pdf_edit::PenStep::Line((r[2], r[3])),
                    pdf_edit::PenStep::Line((r[0], r[3])),
                ],
                closed: true,
                stroke: None,
                fill: Some(fill),
            })
            .collect();
        if let Err(why) = session.apply_each(&boxes) {
            notes.push(format!("page {}: the boxes could not be drawn ({why}); the page was replaced by a picture of itself", page + 1));
            rasterise_pages.push(page);
        }
    }
    let edited = session.source().clone();

    let mut doc =
        convert_pdfdoc::load(edited.to_vec(), &Load::default()).map_err(|e| e.to_string())?;
    let pages_now = doc.pages();
    let scrubbed = scrub(&mut doc, &areas, &words, case, use_annotations);
    for &page in &rasterise_pages {
        let picture = rasterise(&edited, page, &areas[&page], fill, fonts.clone())?;
        if let Some(number) = pages_now.get(page).map(|p| p.number) {
            replace_with_picture(&mut doc, number, picture);
        }
    }
    let compress = convert_pdfdoc::uses_object_streams(job.pdf()?);
    let written = convert_pdfdoc::write(&doc, None, compress)?;

    let check = ByteStore::owning(SourceId::next_document(), written.bytes.clone());
    let mut again = Session::with_fonts(check, b"", fonts);
    let mut left = 0;
    let mut left_words = 0;
    for (&page, rects) in &areas {
        let view = again
            .page(page)
            .map_err(|e| format!("the redacted file does not read back: {e}"))?;
        left += pieces(&view)
            .iter()
            .filter(|p| !p.text.trim().is_empty() && rects.iter().any(|&a| under(p.rect, a)))
            .count();
    }
    if !words.is_empty() {
        for &page in &searched {
            if let Ok(view) = again.page(page) {
                left_words += found(&view, &words, case).1;
            }
        }
    }
    if left > 0 || left_words > 0 {
        return Err(format!(
            "the redaction did not take everything: {left} letters still under the areas, {left_words} searched words still in the text; nothing was written"
        ));
    }
    let mut elsewhere = 0;
    if !words.is_empty() {
        let doc = convert_pdfdoc::load(written.bytes.clone(), &Load::default())
            .map_err(|e| e.to_string())?;
        for object in doc.objects.values() {
            let mut texts = Vec::new();
            collect_strings(&object.value, &mut texts);
            if object.dict().is_some_and(|d| d.is("Type", "Metadata"))
                && let Some(xml) = doc.decoded(object)
            {
                texts.push(String::from_utf8_lossy(&xml).into_owned());
            }
            for text in texts {
                let hay = fold(&text, case);
                elsewhere += words
                    .iter()
                    .filter(|w| {
                        let w = fold(w, case);
                        !w.is_empty() && hay.windows(w.len()).any(|x| x == w.as_slice())
                    })
                    .count();
            }
        }
    }
    notes.push(format!(
        "{} pages redacted: {edits} removals, {pictures} pictures removed whole, {} pages made pictures",
        areas.len(),
        rasterise_pages.len()
    ));
    notes.push(format!(
        "{} annotations and {} form fields under the areas or holding the words removed; {} strings (bookmarks, document information) and {} metadata streams holding the words cleared",
        scrubbed.annotations, scrubbed.fields, scrubbed.strings, scrubbed.metadata
    ));
    if elsewhere > 0 {
        notes.push(format!(
            "WARNING: the searched words still appear {elsewhere} times outside the page content (bookmarks, form fields, annotations or metadata); those are not redacted yet"
        ));
    }
    notes.push("checked: no text left under any area".into());
    let main = match &kept {
        Some(kept) => convert_pdfdoc::relock(written.bytes, kept)?,
        None => written.bytes,
    };
    Ok(Output {
        main,
        attachments: Vec::new(),
        notes,
    })
}

#[derive(Default)]
struct Scrubbed {
    annotations: usize,
    fields: usize,
    strings: usize,
    metadata: usize,
}

fn holds(text: &str, words: &[String], case: bool) -> bool {
    let hay = fold(text, case);
    words.iter().any(|w| {
        let w = fold(w, case);
        !w.is_empty() && hay.windows(w.len()).any(|x| x == w.as_slice())
    })
}

fn annotation_text(doc: &Doc, d: &Dict) -> String {
    let mut out = String::new();
    for key in ["Contents", "V", "RC", "TU"] {
        if let Some(Value::Str(s)) = d.get(key).map(|v| doc.resolve(v)) {
            out.push_str(&convert_pdfdoc::read_text_string(s));
            out.push(' ');
        }
    }
    let mut parent = d.get("Parent").and_then(Value::as_ref);
    for _ in 0..16 {
        let Some(p) = parent.and_then(|n| doc.get(n)).and_then(Object::dict) else {
            break;
        };
        if let Some(Value::Str(s)) = p.get("V").map(|v| doc.resolve(v)) {
            out.push_str(&convert_pdfdoc::read_text_string(s));
        }
        parent = p.get("Parent").and_then(Value::as_ref);
    }
    out
}

fn scrub(
    doc: &mut Doc,
    areas: &BTreeMap<usize, Vec<Rect>>,
    words: &[String],
    case: bool,
    use_annotations: bool,
) -> Scrubbed {
    let mut done = Scrubbed::default();
    let mut removed: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for (index, page) in doc.pages().iter().enumerate() {
        let rects = areas.get(&index).cloned().unwrap_or_default();
        let annots: Vec<Value> = doc
            .get(page.number)
            .and_then(Object::dict)
            .and_then(|d| d.get("Annots"))
            .map(|v| doc.resolve(v).clone())
            .and_then(|v| v.as_array().map(<[Value]>::to_vec))
            .unwrap_or_default();
        if annots.is_empty() {
            continue;
        }
        let kept: Vec<Value> = annots
            .into_iter()
            .filter(|a| {
                let Some(n) = a.as_ref() else { return true };
                let Some(d) = doc.get(n).and_then(Object::dict) else {
                    return true;
                };
                let applied = d.is("Subtype", "Redact") && use_annotations;
                let touches = doc
                    .rect(d, "Rect")
                    .is_some_and(|r| rects.iter().any(|&a| overlap(r, a) > 0.0));
                let says = !words.is_empty() && holds(&annotation_text(doc, d), words, case);
                let gone = applied || touches || says;
                if gone {
                    removed.insert(n);
                }
                !gone
            })
            .collect();
        if let Some(dict) = doc.get_mut(page.number).and_then(Object::dict_mut) {
            if kept.is_empty() {
                dict.remove("Annots");
            } else {
                dict.set("Annots", Value::Array(kept));
            }
        }
    }
    done.annotations = removed.len();
    let fields: Vec<Value> = doc
        .catalog()
        .and_then(|c| doc.dict_at(c, "AcroForm"))
        .and_then(|f| f.get("Fields"))
        .map(|v| doc.resolve(v).clone())
        .and_then(|v| v.as_array().map(<[Value]>::to_vec))
        .unwrap_or_default();
    if !fields.is_empty() {
        let before = fields.len();
        let kept: Vec<Value> = fields
            .into_iter()
            .filter(|f| f.as_ref().is_none_or(|n| prune(doc, n, &removed, 0)))
            .collect();
        done.fields = before - kept.len();
        let form = doc
            .catalog()
            .and_then(|c| c.get("AcroForm"))
            .and_then(Value::as_ref);
        let target = match form {
            Some(n) => doc.get_mut(n).and_then(Object::dict_mut),
            None => doc
                .catalog_mut()
                .and_then(|c| c.get_mut("AcroForm"))
                .and_then(Value::as_dict_mut),
        };
        if let Some(form) = target {
            form.set("Fields", Value::Array(kept));
        }
    }
    if words.is_empty() {
        return done;
    }
    let numbers: Vec<u32> = doc.objects.keys().copied().collect();
    for n in numbers {
        let object = &doc.objects[&n];
        if object.dict().is_some_and(|d| d.is("Type", "Metadata"))
            && doc
                .decoded(object)
                .is_some_and(|xml| holds(&String::from_utf8_lossy(&xml), words, case))
        {
            doc.objects.remove(&n);
            done.metadata += 1;
            continue;
        }
        let mut value = object.value.clone();
        let _ = value.map_strings::<()>(&mut |bytes| {
            let text = convert_pdfdoc::read_text_string(bytes);
            if holds(&text, words, case) {
                done.strings += 1;
                Ok(b"[redacted]".to_vec())
            } else {
                Ok(bytes.to_vec())
            }
        });
        if let Some(object) = doc.objects.get_mut(&n) {
            object.value = value;
        }
    }
    done
}

fn prune(
    doc: &mut Doc,
    field: u32,
    removed: &std::collections::BTreeSet<u32>,
    depth: usize,
) -> bool {
    if removed.contains(&field) || depth > 32 {
        return !removed.contains(&field);
    }
    let kids: Option<Vec<Value>> = doc
        .get(field)
        .and_then(Object::dict)
        .and_then(|d| d.get("Kids"))
        .map(|v| doc.resolve(v).clone())
        .and_then(|v| v.as_array().map(<[Value]>::to_vec));
    let Some(kids) = kids else {
        return true;
    };
    let kept: Vec<Value> = kids
        .into_iter()
        .filter(|k| k.as_ref().is_none_or(|n| prune(doc, n, removed, depth + 1)))
        .collect();
    if kept.is_empty() {
        return false;
    }
    if let Some(d) = doc.get_mut(field).and_then(Object::dict_mut) {
        d.set("Kids", Value::Array(kept));
    }
    true
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Str(bytes) => out.push(convert_pdfdoc::read_text_string(bytes)),
        Value::Array(items) => items.iter().for_each(|i| collect_strings(i, out)),
        Value::Dict(d) => d.0.iter().for_each(|(_, v)| collect_strings(v, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_parse() {
        let a = parse_areas("1:10,20,30,40; 3: 50,60,5,6").unwrap();
        assert_eq!(
            a,
            vec![(0, [10.0, 20.0, 30.0, 40.0]), (2, [5.0, 6.0, 50.0, 60.0])]
        );
        assert!(parse_areas("0:1,2,3,4").is_err());
        assert!(parse_areas("1:1,2,3").is_err());
    }

    #[test]
    fn a_cluster_is_under_an_area_by_its_centre_or_a_third() {
        assert!(under([0.0, 0.0, 10.0, 10.0], [4.0, 4.0, 6.0, 6.0]));
        assert!(under([0.0, 0.0, 10.0, 10.0], [0.0, 0.0, 4.0, 10.0]));
        assert!(!under([0.0, 0.0, 10.0, 10.0], [0.0, 0.0, 2.0, 10.0]));
    }
}
