use crate::model::{Block, Document, ListItem, ListKind, Paragraph, Role};

const BULLETS: &[char] = &[
    '•', '●', '○', '◦', '▪', '■', '□', '◆', '◇', '❖', '➢', '►', '▶', '✓', '✔', '·', '\u{F0B7}',
    '\u{F0A7}', '\u{F076}', '–', '—', '-', '*',
];

#[must_use]
pub fn body_size(document: &Document) -> f64 {
    let mut sizes: Vec<(f64, usize)> = Vec::new();
    let mut count = |paragraph: &Paragraph| {
        for run in &paragraph.runs {
            let n = run.text.chars().filter(|c| c.is_alphabetic()).count();
            let size = (run.style.size * 2.0).round() / 2.0;
            match sizes.iter_mut().find(|(s, _)| (*s - size).abs() < 0.01) {
                Some(entry) => entry.1 += n,
                None => sizes.push((size, n)),
            }
        }
    };
    for page in &document.pages {
        for block in &page.blocks {
            match block {
                Block::Paragraph(paragraph) => count(paragraph),
                Block::Table(table) => {
                    for row in &table.rows {
                        for cell in &row.cells {
                            cell.paragraphs.iter().for_each(&mut count);
                        }
                    }
                }
                Block::Picture(_) => {}
            }
        }
    }
    sizes
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.total_cmp(&a.0)))
        .map_or(10.0, |(size, _)| size)
}

fn is_lao_consonant(c: char) -> bool {
    ('\u{0E81}'..='\u{0EAE}').contains(&c)
}

fn is_thai_consonant(c: char) -> bool {
    ('\u{0E01}'..='\u{0E2E}').contains(&c)
}

fn is_roman(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 5
        && (text.chars().all(|c| "ivxl".contains(c)) || text.chars().all(|c| "IVXL".contains(c)))
}

#[must_use]
pub fn list_marker(text: &str) -> Option<(String, ListKind, usize)> {
    let mut chars = text.char_indices();
    let (_, first) = chars.next()?;
    let rest_after = |end: usize| -> Option<usize> {
        let tail = &text[end..];
        let spaces = tail.len() - tail.trim_start().len();
        (spaces > 0 && !tail.trim_start().is_empty()).then_some(end + spaces)
    };
    if BULLETS.contains(&first) {
        let end = first.len_utf8();
        let needs_space = matches!(first, '-' | '*' | '–' | '—' | '·');
        let tail = &text[end..];
        let spaces = tail.len() - tail.trim_start().len();
        if tail.trim_start().is_empty() || (needs_space && spaces == 0) {
            return None;
        }
        return Some((first.to_string(), ListKind::Bullet, end + spaces));
    }
    if first == '(' {
        let close = text.find(')')?;
        let inner = &text[1..close];
        if inner.is_empty() || inner.chars().count() > 3 {
            return None;
        }
        let end = rest_after(close + 1)?;
        let kind = match inner.parse::<u32>() {
            Ok(n) if inner.chars().all(|c| c.is_ascii_digit()) => ListKind::Decimal(n),
            _ if inner.chars().all(|c| {
                c.is_ascii_digit()
                    || ('\u{0ED0}'..='\u{0ED9}').contains(&c)
                    || ('\u{0E50}'..='\u{0E59}').contains(&c)
            }) =>
            {
                ListKind::Kept
            }
            _ if inner.chars().count() == 1
                && inner.chars().all(|c| {
                    is_lao_consonant(c) || is_thai_consonant(c) || c.is_ascii_alphabetic()
                }) =>
            {
                ListKind::Kept
            }
            _ => return None,
        };
        return Some((text[..close + 1].to_owned(), kind, end));
    }
    let close = text.find(['.', ')'])?;
    let label = &text[..close];
    if label.is_empty() || label.chars().count() > 4 {
        return None;
    }
    let end = rest_after(close + 1)?;
    let marker = text[..=close].to_owned();
    let kind = if label.chars().all(|c| c.is_ascii_digit()) {
        ListKind::Decimal(label.parse().ok()?)
    } else if label.chars().count() == 1 && first.is_ascii_lowercase() {
        ListKind::LowerLetter(u32::from(first) - u32::from('a') + 1)
    } else if label.chars().count() == 1 && first.is_ascii_uppercase() {
        ListKind::UpperLetter(u32::from(first) - u32::from('A') + 1)
    } else if label
        .chars()
        .all(|c| ('\u{0ED0}'..='\u{0ED9}').contains(&c) || ('\u{0E50}'..='\u{0E59}').contains(&c))
        || (label.chars().count() == 1 && (is_lao_consonant(first) || is_thai_consonant(first)))
        || is_roman(label)
    {
        ListKind::Kept
    } else {
        return None;
    };
    Some((marker, kind, end))
}

fn strip_front(paragraph: &mut Paragraph, mut bytes: usize) {
    while bytes > 0 && !paragraph.runs.is_empty() {
        let run = &mut paragraph.runs[0];
        if run.text.len() <= bytes {
            bytes -= run.text.len();
            paragraph.runs.remove(0);
        } else {
            let mut cut = bytes;
            while !run.text.is_char_boundary(cut) {
                cut += 1;
            }
            run.text.drain(..cut);
            bytes = 0;
        }
    }
}

pub fn mark_lists(paragraph: &mut Paragraph, left: f64, body: f64) {
    if paragraph.role != Role::Body || (body > 0.0 && paragraph.size() < body * 0.85) {
        return;
    }
    let text = paragraph.text();
    let Some((marker, kind, length)) = list_marker(&text) else {
        return;
    };
    let indent = paragraph.frame.x0 - left;
    let level = if indent > 40.0 {
        2
    } else {
        u8::from(indent > 18.0)
    };
    if kind != ListKind::Kept {
        strip_front(paragraph, length);
    }
    paragraph.role = Role::ListItem(ListItem {
        marker,
        kind,
        level,
    });
}

pub fn mark_headings(document: &mut Document) {
    let body = document.body_size;
    let is_candidate = |paragraph: &Paragraph| -> Option<f64> {
        if paragraph.role != Role::Body {
            return None;
        }
        let text = paragraph.text();
        let visible: usize = text.chars().filter(|c| !c.is_whitespace()).count();
        if visible == 0 || visible > 160 || paragraph.lines > 3 {
            return None;
        }
        if text.chars().all(|c| !c.is_alphabetic()) {
            return None;
        }
        let size = paragraph.size();
        if size >= body * 1.15 && size - body >= 1.0 {
            Some(size)
        } else if paragraph.all_bold()
            && size >= body * 0.95
            && paragraph.lines <= 2
            && visible <= 120
        {
            Some(0.0)
        } else {
            None
        }
    };
    let mut found: Vec<(f64, usize)> = Vec::new();
    for page in &document.pages {
        for block in &page.blocks {
            if let Block::Paragraph(paragraph) = block
                && let Some(size) = is_candidate(paragraph)
                && size > 0.0
            {
                let size = size.round();
                match found.iter_mut().find(|(s, _)| *s == size) {
                    Some(entry) => entry.1 += 1,
                    None => found.push((size, 1)),
                }
            }
        }
    }
    let mut sizes: Vec<f64> = found
        .iter()
        .filter(|(_, n)| *n >= 2)
        .map(|(s, _)| *s)
        .collect();
    if sizes.is_empty() {
        sizes = found.iter().map(|(s, _)| *s).collect();
    }
    sizes.sort_by(|a, b| b.total_cmp(a));
    let level_of = |size: f64| -> u8 {
        if size <= 0.0 {
            return u8::try_from((sizes.len() + 1).min(6)).unwrap_or(6);
        }
        let rank = sizes
            .iter()
            .position(|s| *s <= size.round() + 0.25)
            .unwrap_or(sizes.len().saturating_sub(1));
        u8::try_from((rank + 1).min(6)).unwrap_or(6)
    };
    for page in &mut document.pages {
        for block in &mut page.blocks {
            if let Block::Paragraph(paragraph) = block
                && let Some(size) = is_candidate(paragraph)
            {
                paragraph.role = Role::Heading(level_of(size));
            }
        }
    }
}

fn shape_of(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        let digit = c.is_numeric()
            || ('\u{0ED0}'..='\u{0ED9}').contains(&c)
            || ('\u{0E50}'..='\u{0E59}').contains(&c);
        if digit {
            if !out.ends_with('#') {
                out.push('#');
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn take_running_heads(document: &mut Document) {
    let pages = document
        .pages
        .iter()
        .filter(|page| page.refused.is_none())
        .count();
    if pages < 3 {
        return;
    }
    let mut seen: Vec<(String, bool, f64, usize)> = Vec::new();
    for page in &document.pages {
        let mut here: Vec<(String, bool, f64)> = Vec::new();
        for block in &page.blocks {
            let Block::Paragraph(paragraph) = block else {
                continue;
            };
            let top = paragraph.frame.y1 <= page.height * 0.1;
            let bottom = paragraph.frame.y0 >= page.height * 0.9;
            if !(top || bottom) || paragraph.lines > 2 {
                continue;
            }
            let shape = shape_of(&paragraph.text());
            if shape.is_empty() || here.iter().any(|(s, t, _)| *s == shape && *t == top) {
                continue;
            }
            here.push((shape, top, paragraph.frame.y0));
        }
        for (shape, top, y) in here {
            match seen
                .iter_mut()
                .find(|(s, t, at, _)| *s == shape && *t == top && (*at - y).abs() <= 6.0)
            {
                Some(entry) => entry.3 += 1,
                None => seen.push((shape, top, y, 1)),
            }
        }
    }
    let repeated: Vec<(String, bool, f64)> = seen
        .into_iter()
        .filter(|(_, _, _, count)| *count * 2 >= pages)
        .map(|(shape, top, y, _)| (shape, top, y))
        .collect();
    if repeated.is_empty() {
        return;
    }
    for page in &mut document.pages {
        let height = page.height;
        let mut kept = Vec::with_capacity(page.blocks.len());
        for block in std::mem::take(&mut page.blocks) {
            if let Block::Paragraph(paragraph) = &block {
                let top = paragraph.frame.y1 <= height * 0.1;
                let bottom = paragraph.frame.y0 >= height * 0.9;
                if top || bottom {
                    let shape = shape_of(&paragraph.text());
                    if repeated.iter().any(|(s, t, y)| {
                        *s == shape && *t == top && (*y - paragraph.frame.y0).abs() <= 6.0
                    }) {
                        if let Block::Paragraph(paragraph) = block {
                            if top {
                                page.header.push(paragraph);
                            } else {
                                page.footer.push(paragraph);
                            }
                        }
                        continue;
                    }
                }
            }
            kept.push(block);
        }
        page.blocks = kept;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers() {
        assert_eq!(list_marker("• ພາສາ").map(|m| m.1), Some(ListKind::Bullet));
        assert_eq!(list_marker("•ພາສາ").map(|m| m.2), Some(3));
        assert_eq!(
            list_marker("12. Twelve").map(|m| m.1),
            Some(ListKind::Decimal(12))
        );
        assert_eq!(
            list_marker("b) second").map(|m| m.1),
            Some(ListKind::LowerLetter(2))
        );
        assert_eq!(list_marker("ກ. ຂໍ້ທຳອິດ").map(|m| m.1), Some(ListKind::Kept));
        assert_eq!(list_marker("໑. ໜຶ່ງ").map(|m| m.1), Some(ListKind::Kept));
        assert_eq!(
            list_marker("(3) three").map(|m| (m.0, m.1)),
            Some(("(3)".to_owned(), ListKind::Decimal(3)))
        );
        assert_eq!(list_marker("iv. four").map(|m| m.1), Some(ListKind::Kept));
        assert!(list_marker("3.14 is pi").is_none());
        assert!(list_marker("1.2 Section").is_none());
        assert!(list_marker("-5 degrees").is_none());
        assert!(list_marker("Hello. World").is_none());
        assert!(list_marker("12.").is_none());
    }

    #[test]
    fn stripping_crosses_runs() {
        let mut paragraph = Paragraph {
            runs: vec![
                crate::model::Run {
                    text: "1.".into(),
                    style: crate::model::Style::default(),
                },
                crate::model::Run {
                    text: " ພາສາ".into(),
                    style: crate::model::Style::default(),
                },
            ],
            ..Paragraph::default()
        };
        mark_lists(&mut paragraph, 0.0, 0.0);
        assert_eq!(paragraph.text(), "ພາສາ");
        assert!(matches!(
            paragraph.role,
            Role::ListItem(ListItem {
                kind: ListKind::Decimal(1),
                ..
            })
        ));
    }
}
