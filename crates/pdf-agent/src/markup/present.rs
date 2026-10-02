use super::tree::{Block, Inline, List};

#[must_use]
pub fn present(blocks: Vec<Block>) -> Vec<Block> {
    let mut out = Vec::new();
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => out.push(Block::Heading {
                level,
                inlines: heading_id_off(inlines_shown(inlines)),
            }),
            Block::Paragraph { inlines } => out.extend(definitions(inlines_shown(inlines))),
            Block::Quote { blocks } => out.push(Block::Quote {
                blocks: alert(present(blocks)),
            }),
            Block::List(list) => out.push(Block::List(List {
                items: list.items.into_iter().map(present).collect(),
                ..list
            })),
            Block::Table { head, rows, align } => out.push(Block::Table {
                head: head.into_iter().map(inlines_shown).collect(),
                rows: rows
                    .into_iter()
                    .map(|row| row.into_iter().map(inlines_shown).collect())
                    .collect(),
                align,
            }),
            Block::Html(html) => out.extend(html_block(&html)),
            Block::Footnotes(notes) => {
                out.push(Block::Break);
                out.push(Block::List(List {
                    first: Some(1),
                    loose: false,
                    items: notes.into_iter().map(present).collect(),
                }));
            }
            other => out.push(other),
        }
    }
    out
}

pub fn task_items(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::List(list) => {
                for item in &mut list.items {
                    if let Some(Block::Paragraph { inlines }) = item.first_mut()
                        && let more = inlines.len() > 1
                        && let Some(Inline::Text(text)) = inlines.first_mut()
                    {
                        let ticked = if text.starts_with("[ ]") {
                            Some(false)
                        } else if text.starts_with("[x]") || text.starts_with("[X]") {
                            Some(true)
                        } else {
                            None
                        };
                        let followed = text[3.min(text.len())..].starts_with([' ', '\t'])
                            || (text.len() == 3 && more);
                        if let Some(ticked) = ticked
                            && followed
                        {
                            let rest = text[3..].trim_start_matches([' ', '\t']).to_owned();
                            let empty = rest.is_empty();
                            *text = rest;
                            if empty {
                                inlines.remove(0);
                            }
                            inlines.insert(0, Inline::Check(ticked));
                        }
                    }
                    task_items(item);
                }
            }
            Block::Quote { blocks } => task_items(blocks),
            _ => {}
        }
    }
}

#[must_use]
pub fn superscript(text: &str) -> Option<String> {
    text.chars()
        .map(|letter| {
            Some(match letter {
                '0' => '\u{2070}',
                '1' => '\u{B9}',
                '2' => '\u{B2}',
                '3' => '\u{B3}',
                '4' => '\u{2074}',
                '5' => '\u{2075}',
                '6' => '\u{2076}',
                '7' => '\u{2077}',
                '8' => '\u{2078}',
                '9' => '\u{2079}',
                '+' => '\u{207A}',
                '-' | '\u{2212}' => '\u{207B}',
                '=' => '\u{207C}',
                '(' => '\u{207D}',
                ')' => '\u{207E}',
                'n' => '\u{207F}',
                'i' => '\u{2071}',
                'a' => '\u{1D43}',
                'b' => '\u{1D47}',
                'c' => '\u{1D9C}',
                'd' => '\u{1D48}',
                'e' => '\u{1D49}',
                'f' => '\u{1DA0}',
                'g' => '\u{1D4D}',
                'h' => '\u{2B0}',
                'j' => '\u{2B2}',
                'k' => '\u{1D4F}',
                'l' => '\u{2E1}',
                'm' => '\u{1D50}',
                'o' => '\u{1D52}',
                'p' => '\u{1D56}',
                'r' => '\u{2B3}',
                's' => '\u{2E2}',
                't' => '\u{1D57}',
                'u' => '\u{1D58}',
                'v' => '\u{1D5B}',
                'w' => '\u{2B7}',
                'x' => '\u{2E3}',
                'y' => '\u{2B8}',
                'z' => '\u{1DBB}',
                _ => return None,
            })
        })
        .collect()
}

#[must_use]
pub fn subscript(text: &str) -> Option<String> {
    text.chars()
        .map(|letter| {
            Some(match letter {
                '0'..='9' => char::from_u32(0x2080 + (u32::from(letter) - u32::from('0')))?,
                '+' => '\u{208A}',
                '-' | '\u{2212}' => '\u{208B}',
                '=' => '\u{208C}',
                '(' => '\u{208D}',
                ')' => '\u{208E}',
                'a' => '\u{2090}',
                'e' => '\u{2091}',
                'o' => '\u{2092}',
                'x' => '\u{2093}',
                'h' => '\u{2095}',
                'k' => '\u{2096}',
                'l' => '\u{2097}',
                'm' => '\u{2098}',
                'n' => '\u{2099}',
                'p' => '\u{209A}',
                's' => '\u{209B}',
                't' => '\u{209C}',
                'i' => '\u{1D62}',
                'r' => '\u{1D63}',
                'u' => '\u{1D64}',
                'v' => '\u{1D65}',
                'j' => '\u{2C7C}',
                _ => return None,
            })
        })
        .collect()
}

fn inlines_shown(inlines: Vec<Inline>) -> Vec<Inline> {
    let folded = fold_html(inlines);
    folded
        .into_iter()
        .map(|inline| match inline {
            Inline::Text(text) => Inline::Text(shortcodes(&text)),
            Inline::Emphasis(inside) => Inline::Emphasis(inlines_shown(inside)),
            Inline::Strong(inside) => Inline::Strong(inlines_shown(inside)),
            Inline::Strike(inside) => Inline::Strike(inlines_shown(inside)),
            Inline::Highlight(inside) => Inline::Highlight(inlines_shown(inside)),
            Inline::Superscript(inside) => Inline::Superscript(inlines_shown(inside)),
            Inline::Subscript(inside) => Inline::Subscript(inlines_shown(inside)),
            Inline::Link { to, title, text } => Inline::Link {
                to,
                title,
                text: inlines_shown(text),
            },
            Inline::Image { at, title, text } => Inline::Image {
                at,
                title,
                text: inlines_shown(text),
            },
            other => other,
        })
        .collect()
}

fn tag_of(html: &str) -> Option<(String, bool)> {
    let inside = html.strip_prefix('<')?;
    let (closing, inside) = match inside.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inside),
    };
    let name: String = inside
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    (!name.is_empty()).then(|| (name.to_ascii_lowercase(), closing))
}

fn attribute(html: &str, name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &html[at..];
    let value = match rest.chars().next()? {
        quote @ ('"' | '\'') => rest[1..].split(quote).next()?.to_owned(),
        _ => rest
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()?
            .to_owned(),
    };
    Some(super::inlines::unescape(&value))
}

fn wrap_for(name: &str) -> Option<fn(Vec<Inline>) -> Inline> {
    Some(match name {
        "b" | "strong" => Inline::Strong,
        "i" | "em" | "cite" | "var" | "dfn" | "u" | "ins" => Inline::Emphasis,
        "s" | "del" | "strike" => Inline::Strike,
        "mark" => Inline::Highlight,
        "sup" => Inline::Superscript,
        "sub" => Inline::Subscript,
        _ => return None,
    })
}

fn fold_html(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    let mut pieces = inlines.into_iter().peekable();
    while let Some(piece) = pieces.next() {
        let Inline::Html(html) = piece else {
            out.push(piece);
            continue;
        };
        let Some((name, closing)) = tag_of(&html) else {
            continue;
        };
        if closing {
            continue;
        }
        match name.as_str() {
            "br" | "wbr" => {
                out.push(Inline::Hard);
                continue;
            }
            "img" => {
                if let Some(alt) = attribute(&html, "alt").filter(|alt| !alt.trim().is_empty()) {
                    out.push(Inline::Text(alt));
                }
                continue;
            }
            "code" | "kbd" | "samp" | "tt" => {
                let mut text = String::new();
                for next in pieces.by_ref() {
                    if let Inline::Html(close) = &next
                        && tag_of(close) == Some((name.clone(), true))
                    {
                        break;
                    }
                    text.push_str(&next.plain());
                }
                out.push(Inline::Code(text));
                continue;
            }
            "script" | "style" => {
                for next in pieces.by_ref() {
                    if let Inline::Html(close) = &next
                        && tag_of(close) == Some((name.clone(), true))
                    {
                        break;
                    }
                }
                continue;
            }
            _ => {}
        }
        let Some(wrap) = wrap_for(&name) else {
            continue;
        };
        let mut inside = Vec::new();
        let mut depth = 0;
        for next in pieces.by_ref() {
            if let Inline::Html(tag) = &next
                && let Some((other, closes)) = tag_of(tag)
                && other == name
            {
                if closes {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                } else {
                    depth += 1;
                }
            }
            inside.push(next);
        }
        out.push(wrap(fold_html(inside)));
    }
    out
}

fn html_block(html: &str) -> Vec<Block> {
    let mut text = String::new();
    let mut letters = html.char_indices().peekable();
    let lower = html.to_ascii_lowercase();
    let mut skip_until: Option<String> = None;
    while let Some((at, letter)) = letters.next() {
        if let Some(end) = &skip_until {
            if lower[at..].starts_with(end.as_str()) {
                let end = end.clone();
                for _ in 1..end.len() {
                    letters.next();
                }
                skip_until = None;
            }
            continue;
        }
        if letter == '<' {
            let rest = &lower[at..];
            if rest.starts_with("<!--") {
                skip_until = Some("-->".to_owned());
                continue;
            }
            if rest.starts_with("<script") || rest.starts_with("<style") {
                let name = if rest.starts_with("<script") {
                    "script"
                } else {
                    "style"
                };
                skip_until = Some(format!("</{name}>"));
                continue;
            }
            let Some(close) = html[at..].find('>') else {
                text.push(letter);
                continue;
            };
            let tag = &html[at..=at + close];
            if let Some((name, closing)) = tag_of(tag) {
                if name == "img"
                    && let Some(alt) = attribute(tag, "alt")
                {
                    text.push_str(&alt);
                }
                if matches!(
                    name.as_str(),
                    "br" | "p"
                        | "div"
                        | "li"
                        | "tr"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "summary"
                        | "details"
                        | "section"
                        | "blockquote"
                        | "table"
                        | "ul"
                        | "ol"
                ) && (closing || name == "br")
                {
                    text.push('\n');
                }
            }
            for _ in 0..close {
                letters.next();
            }
            continue;
        }
        text.push(letter);
    }
    super::inlines::unescape(&text)
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| Block::Paragraph {
            inlines: vec![Inline::Text(line.to_owned())],
        })
        .collect()
}

fn heading_id_off(mut inlines: Vec<Inline>) -> Vec<Inline> {
    if let Some(Inline::Text(text)) = inlines.last_mut() {
        let trimmed = text.trim_end();
        if trimmed.ends_with('}')
            && let Some(open) = trimmed.rfind('{')
            && trimmed[open + 1..].starts_with(['#', '.'])
            && !trimmed[open..].contains(' ')
        {
            *text = trimmed[..open].trim_end().to_owned();
        }
    }
    inlines
}

fn alert(mut blocks: Vec<Block>) -> Vec<Block> {
    if let Some(Block::Paragraph { inlines }) = blocks.first_mut()
        && let Some(Inline::Text(text)) = inlines.first_mut()
    {
        let kinds = [
            ("[!NOTE]", "Note"),
            ("[!TIP]", "Tip"),
            ("[!IMPORTANT]", "Important"),
            ("[!WARNING]", "Warning"),
            ("[!CAUTION]", "Caution"),
        ];
        let upper = text.to_ascii_uppercase();
        if let Some((mark, word)) = kinds.iter().find(|(mark, _)| upper.starts_with(mark)) {
            let rest = text[mark.len()..].trim_start().to_owned();
            if rest.is_empty() {
                inlines.remove(0);
                if matches!(inlines.first(), Some(Inline::Soft | Inline::Hard)) {
                    inlines.remove(0);
                }
            } else {
                *text = rest;
            }
            inlines.insert(0, Inline::Hard);
            inlines.insert(0, Inline::Strong(vec![Inline::Text((*word).to_owned())]));
            if inlines.len() == 2 {
                inlines.pop();
            }
        }
    }
    blocks
}

fn definitions(inlines: Vec<Inline>) -> Vec<Block> {
    let mut lines: Vec<Vec<Inline>> = vec![Vec::new()];
    for inline in inlines.iter().cloned() {
        if matches!(inline, Inline::Soft) {
            lines.push(Vec::new());
        } else if let Some(line) = lines.last_mut() {
            line.push(inline);
        }
    }
    let opens = |line: &Vec<Inline>| matches!(line.first(), Some(Inline::Text(text)) if text.starts_with(": ") || text.starts_with(":\t"));
    if lines.len() < 2 || opens(&lines[0]) || !lines[1..].iter().any(opens) || !opens(&lines[1]) {
        return vec![Block::Paragraph { inlines }];
    }
    let mut out = Vec::new();
    let mut term: Vec<Inline> = Vec::new();
    let mut meanings: Vec<Vec<Block>> = Vec::new();
    let finish = |term: &mut Vec<Inline>, meanings: &mut Vec<Vec<Block>>, out: &mut Vec<Block>| {
        if !term.is_empty() {
            out.push(Block::Paragraph {
                inlines: vec![Inline::Strong(std::mem::take(term))],
            });
        }
        if !meanings.is_empty() {
            out.push(Block::List(List {
                first: None,
                loose: false,
                items: std::mem::take(meanings),
            }));
        }
    };
    for mut line in lines {
        if opens(&line) {
            if let Some(Inline::Text(text)) = line.first_mut() {
                *text = text[1..].trim_start().to_owned();
            }
            meanings.push(vec![Block::Paragraph { inlines: line }]);
        } else {
            finish(&mut term, &mut meanings, &mut out);
            term = line;
        }
    }
    finish(&mut term, &mut meanings, &mut out);
    out
}

fn shortcodes(text: &str) -> String {
    if !text.contains(':') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find(':') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-'))
            .unwrap_or(after.len());
        if name_end > 0
            && after[name_end..].starts_with(':')
            && let Some(symbol) = symbol_for(&after[..name_end])
        {
            out.push_str(symbol);
            rest = &after[name_end + 1..];
        } else {
            out.push(':');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn symbol_for(name: &str) -> Option<&'static str> {
    Some(match name {
        "smile" | "slightly_smiling_face" | "relaxed" | "blush" => "\u{263A}",
        "frowning_face" | "disappointed" => "\u{2639}",
        "heart" | "hearts" | "red_heart" => "\u{2665}",
        "star" | "star2" | "glowing_star" => "\u{2605}",
        "white_check_mark"
        | "heavy_check_mark"
        | "check"
        | "check_mark"
        | "ballot_box_with_check" => "\u{2713}",
        "x" | "cross_mark" | "heavy_multiplication_x" | "negative_squared_cross_mark" => "\u{2717}",
        "warning" => "\u{26A0}",
        "zap" | "high_voltage" => "\u{26A1}",
        "sunny" | "sun" => "\u{2600}",
        "cloud" => "\u{2601}",
        "umbrella" => "\u{2602}",
        "snowflake" => "\u{2744}",
        "phone" | "telephone" => "\u{260E}",
        "email" | "envelope" => "\u{2709}",
        "pencil2" | "pencil" => "\u{270F}",
        "scissors" => "\u{2702}",
        "airplane" => "\u{2708}",
        "arrow_right" => "\u{2192}",
        "arrow_left" => "\u{2190}",
        "arrow_up" => "\u{2191}",
        "arrow_down" => "\u{2193}",
        "point_right" => "\u{261E}",
        "point_left" => "\u{261C}",
        "copyright" => "\u{A9}",
        "registered" => "\u{AE}",
        "tm" => "\u{2122}",
        "information_source" | "info" => "\u{2139}",
        "heavy_plus_sign" => "+",
        "heavy_minus_sign" => "\u{2212}",
        "heavy_division_sign" => "\u{F7}",
        "spades" => "\u{2660}",
        "clubs" => "\u{2663}",
        "diamonds" => "\u{2666}",
        "musical_note" => "\u{266A}",
        "hourglass" => "\u{231B}",
        "watch" => "\u{231A}",
        "recycle" => "\u{267B}",
        "peace_symbol" => "\u{262E}",
        "yin_yang" => "\u{262F}",
        "radioactive" => "\u{2622}",
        "coffee" => "\u{2615}",
        "black_circle" => "\u{25CF}",
        "white_circle" => "\u{25CB}",
        "black_square" | "black_large_square" => "\u{25A0}",
        "white_square" | "white_large_square" => "\u{25A1}",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markup::document;

    fn text_of(blocks: &[Block]) -> String {
        blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph { inlines } | Block::Heading { inlines, .. } => {
                    crate::markup::plain(inlines)
                }
                Block::List(list) => list
                    .items
                    .iter()
                    .map(|item| text_of(item))
                    .collect::<Vec<_>>()
                    .join("|"),
                Block::Quote { blocks } => text_of(blocks),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn html_in_a_line_is_obeyed_and_never_shown() {
        let read = document(
            "H<sub>2</sub>O is <b>water</b><br>and <span class=x>this</span> &amp; that <!-- no -->",
        );
        let Block::Paragraph { inlines } = &read[0] else {
            panic!("{read:?}")
        };
        assert!(inlines.contains(&Inline::Subscript(vec![Inline::Text("2".into())])));
        assert!(inlines.contains(&Inline::Strong(vec![Inline::Text("water".into())])));
        assert!(inlines.contains(&Inline::Hard));
        let shown = crate::markup::plain(inlines);
        assert_eq!(shown, "H\u{2082}O is water\nand this & that ");
    }

    #[test]
    fn an_html_block_is_its_words() {
        let read = document(
            "<div align=\"center\">\n<img src=\"a.png\" alt=\"Logo\">\n<p>Hello <b>there</b></p>\n</div>\n",
        );
        assert_eq!(text_of(&read), "Logo\nHello there");
    }

    #[test]
    fn task_items_are_boxes() {
        let read = document("- [ ] wash\n- [x] dry\n- [link](u)\n");
        let Block::List(list) = &read[0] else {
            panic!()
        };
        let first = |at: usize| match &list.items[at][0] {
            Block::Paragraph { inlines } => inlines[0].clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(first(0), Inline::Check(false));
        assert_eq!(first(1), Inline::Check(true));
        assert!(matches!(first(2), Inline::Link { .. }));
    }

    #[test]
    fn footnotes_are_numbered_where_they_are_pointed_at() {
        let read = document("Second[^b] and first[^a].\n\n[^a]: Note A.\n[^b]: Note B.\n");
        assert_eq!(text_of(&read[..1]), "Second\u{B9} and first\u{B2}.");
        assert!(matches!(read[1], Block::Break));
        assert_eq!(text_of(&read[2..]), "Note B.|Note A.");
    }

    #[test]
    fn alerts_definitions_ids_scripts_and_shortcodes() {
        assert_eq!(
            text_of(&document("> [!WARNING]\n> Hot.\n")),
            "Warning\nHot."
        );
        assert_eq!(text_of(&document("## Setup {#setup}\n")), "Setup");
        assert_eq!(
            text_of(&document("Apple\n: A fruit.\n: A company.\n")),
            "Apple\nA fruit.|A company."
        );
        assert_eq!(
            text_of(&document("E = mc^2^ and x~i~, ==marked==")),
            "E = mc\u{B2} and x\u{1D62}, marked"
        );
        assert_eq!(
            text_of(&document("Done :white_check_mark: at 10:30:")),
            "Done \u{2713} at 10:30:"
        );
        assert_eq!(
            text_of(&document("~~gone~~ but ~ fine ~")),
            "gone but ~ fine ~"
        );
    }

    #[test]
    fn references_and_entities_are_resolved() {
        let read = document(
            "See [the guide][g] &copy; 2026 &#x1F600;.\n\n[g]: https://example.com \"Guide\"\n",
        );
        let Block::Paragraph { inlines } = &read[0] else {
            panic!()
        };
        assert!(
            matches!(&inlines[1], Inline::Link { to, title, .. } if to == "https://example.com" && title == "Guide"),
            "{inlines:?}"
        );
        assert_eq!(
            crate::markup::plain(inlines),
            "See the guide \u{A9} 2026 \u{1F600}."
        );
    }
}
