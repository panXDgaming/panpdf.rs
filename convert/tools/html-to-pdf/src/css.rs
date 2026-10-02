use crate::html::{Dom, NodeId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub property: String,
    pub value: String,
    pub important: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compound {
    pub tag: Option<String>,
    pub classes: Vec<String>,
    pub id: Option<String>,
    pub attrs: Vec<(String, Option<String>)>,
    pub first_child: bool,
    pub last_child: bool,
    pub never: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    Adjacent,
    Sibling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    pub parts: Vec<(Compound, Option<Combinator>)>,
}

impl Selector {
    #[must_use]
    pub fn specificity(&self) -> (u32, u32, u32) {
        let mut spec = (0, 0, 0);
        for (compound, _) in &self.parts {
            spec.0 += u32::from(compound.id.is_some());
            spec.1 += u32::try_from(compound.classes.len() + compound.attrs.len()).unwrap_or(0)
                + u32::from(compound.first_child)
                + u32::from(compound.last_child);
            spec.2 += u32::from(compound.tag.is_some());
        }
        spec
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    pub pages: Vec<(String, Vec<Declaration>)>,
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

#[must_use]
pub fn parse_sheet(text: &str) -> Sheet {
    let mut sheet = Sheet::default();
    read_rules(&strip_comments(text), &mut sheet);
    sheet
}

fn read_rules(text: &str, sheet: &mut Sheet) {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return;
        }
        if let Some(at_rule) = rest.strip_prefix('@') {
            let name_end = at_rule
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .unwrap_or(at_rule.len());
            let name = at_rule[..name_end].to_ascii_lowercase();
            let brace = rest.find('{');
            let semi = rest.find(';');
            if semi.is_some_and(|s| brace.is_none_or(|b| s < b)) {
                rest = &rest[semi.unwrap_or(0) + 1..];
                continue;
            }
            let Some(open) = brace else { return };
            let prelude = rest[1 + name.len()..open].trim().to_ascii_lowercase();
            let close = matching_brace(rest, open);
            let inner = &rest[open + 1..close];
            match name.as_str() {
                "media" => {
                    if prelude.contains("print")
                        || prelude.contains("all")
                        || (prelude.contains("screen") && !prelude.contains("max-width"))
                        || prelude.is_empty()
                    {
                        read_rules(inner, sheet);
                    }
                }
                "page" => {
                    let name = prelude.split(':').next().unwrap_or("").trim().to_owned();
                    sheet.pages.push((name, parse_declarations(inner)));
                }
                "supports" => read_rules(inner, sheet),
                _ => {}
            }
            rest = rest.get(close + 1..).unwrap_or("");
            continue;
        }
        let Some(open) = rest.find('{') else { return };
        let close = matching_brace(rest, open);
        let selectors: Vec<Selector> = rest[..open]
            .split(',')
            .filter_map(|s| parse_selector(s.trim()))
            .collect();
        let declarations = parse_declarations(&rest[open + 1..close]);
        if !selectors.is_empty() && !declarations.is_empty() {
            sheet.rules.push(Rule {
                selectors,
                declarations,
            });
        }
        rest = rest.get(close + 1..).unwrap_or("");
    }
}

fn matching_brace(text: &str, open: usize) -> usize {
    let mut depth = 0;
    for (offset, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return open + offset;
                }
            }
            _ => {}
        }
    }
    text.len()
}

#[must_use]
pub fn parse_declarations(text: &str) -> Vec<Declaration> {
    let mut out = Vec::new();
    for part in split_outside_parens(text, ';') {
        let Some((property, value)) = part.split_once(':') else {
            continue;
        };
        let property = property.trim().to_ascii_lowercase();
        let mut value = value.trim().to_owned();
        let mut important = false;
        if let Some(position) = value.to_ascii_lowercase().find("!important") {
            value.truncate(position);
            value = value.trim().to_owned();
            important = true;
        }
        if !property.is_empty() && !value.is_empty() {
            out.push(Declaration {
                property,
                value,
                important,
            });
        }
    }
    out
}

#[must_use]
pub fn split_outside_parens(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, _) if c == separator && depth <= 0 => {
                parts.push(&text[start..at]);
                start = at + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

fn parse_selector(text: &str) -> Option<Selector> {
    if text.is_empty() {
        return None;
    }
    let mut parts: Vec<(Compound, Option<Combinator>)> = Vec::new();
    let mut pending: Option<Combinator> = None;
    let mut rest = text;
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        let had_space = trimmed.len() < rest.len();
        rest = trimmed;
        if rest.is_empty() {
            break;
        }
        let c = rest.chars().next()?;
        if matches!(c, '>' | '+' | '~') {
            pending = Some(match c {
                '>' => Combinator::Child,
                '+' => Combinator::Adjacent,
                _ => Combinator::Sibling,
            });
            rest = &rest[1..];
            continue;
        }
        if had_space && !parts.is_empty() && pending.is_none() {
            pending = Some(Combinator::Descendant);
        }
        let (compound, used) = parse_compound(rest)?;
        rest = &rest[used..];
        if let Some(last) = parts.last_mut() {
            last.1 = pending.take();
        }
        parts.push((compound, None));
    }
    if parts.is_empty() {
        return None;
    }
    let count = parts.len();
    let mut reversed = Vec::with_capacity(count);
    for index in (0..count).rev() {
        let combinator = if index == 0 { None } else { parts[index - 1].1 };
        reversed.push((parts[index].0.clone(), combinator));
    }
    Some(Selector { parts: reversed })
}

fn parse_compound(text: &str) -> Option<(Compound, usize)> {
    let mut compound = Compound::default();
    let bytes = text.as_bytes();
    let mut at = 0;
    let ident = |from: usize| -> usize {
        let mut end = from;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric()
                || bytes[end] == b'-'
                || bytes[end] == b'_'
                || bytes[end] >= 0x80)
        {
            end += 1;
        }
        end
    };
    if at < bytes.len() && bytes[at] == b'*' {
        at += 1;
    } else {
        let end = ident(at);
        if end > at {
            compound.tag = Some(text[at..end].to_ascii_lowercase());
            at = end;
        }
    }
    while at < bytes.len() {
        match bytes[at] {
            b'.' => {
                let end = ident(at + 1);
                compound.classes.push(text[at + 1..end].to_owned());
                at = end;
            }
            b'#' => {
                let end = ident(at + 1);
                compound.id = Some(text[at + 1..end].to_owned());
                at = end;
            }
            b'[' => {
                let end = text[at..].find(']').map_or(text.len(), |e| at + e);
                let inner = &text[at + 1..end];
                let (name, value) = match inner.split_once('=') {
                    Some((name, value)) => (
                        name.trim_end_matches(['~', '|', '^', '$', '*']).trim(),
                        Some(value.trim().trim_matches(['"', '\'']).to_owned()),
                    ),
                    None => (inner.trim(), None),
                };
                compound.attrs.push((name.to_ascii_lowercase(), value));
                at = (end + 1).min(text.len());
            }
            b':' => {
                let double = bytes.get(at + 1) == Some(&b':');
                let from = at + 1 + usize::from(double);
                let end = ident(from);
                let name = text[from..end].to_ascii_lowercase();
                at = end;
                if bytes.get(at) == Some(&b'(') {
                    at = text[at..].find(')').map_or(text.len(), |e| at + e + 1);
                    compound.never = true;
                    continue;
                }
                match name.as_str() {
                    "first-child" => compound.first_child = true,
                    "last-child" => compound.last_child = true,
                    "link" | "root" => {}
                    _ => compound.never = true,
                }
            }
            _ => break,
        }
    }
    (at > 0).then_some((compound, at))
}

fn element_children(dom: &Dom, parent: NodeId) -> Vec<NodeId> {
    dom.children(parent)
        .iter()
        .copied()
        .filter(|id| dom.tag(*id).is_some())
        .collect()
}

fn matches_compound(dom: &Dom, node: NodeId, compound: &Compound) -> bool {
    if compound.never {
        return false;
    }
    let Some(tag) = dom.tag(node) else {
        return false;
    };
    if compound.tag.as_deref().is_some_and(|want| want != tag) {
        return false;
    }
    if let Some(id) = &compound.id
        && dom.attr(node, "id") != Some(id.as_str())
    {
        return false;
    }
    if !compound.classes.is_empty() {
        let classes: Vec<&str> = dom
            .attr(node, "class")
            .unwrap_or("")
            .split_ascii_whitespace()
            .collect();
        if !compound
            .classes
            .iter()
            .all(|c| classes.contains(&c.as_str()))
        {
            return false;
        }
    }
    for (name, value) in &compound.attrs {
        match (dom.attr(node, name), value) {
            (None, _) => return false,
            (Some(have), Some(want)) if have != want => return false,
            _ => {}
        }
    }
    if compound.first_child || compound.last_child {
        let Some(parent) = dom.parent(node) else {
            return false;
        };
        let siblings = element_children(dom, parent);
        if compound.first_child && siblings.first() != Some(&node) {
            return false;
        }
        if compound.last_child && siblings.last() != Some(&node) {
            return false;
        }
    }
    true
}

fn previous_element(dom: &Dom, node: NodeId) -> Option<NodeId> {
    let parent = dom.parent(node)?;
    let siblings = element_children(dom, parent);
    let position = siblings.iter().position(|id| *id == node)?;
    position.checked_sub(1).map(|p| siblings[p])
}

#[must_use]
pub fn matches(dom: &Dom, node: NodeId, selector: &Selector) -> bool {
    matches_from(dom, node, &selector.parts)
}

fn matches_from(dom: &Dom, node: NodeId, parts: &[(Compound, Option<Combinator>)]) -> bool {
    let Some(((compound, combinator), rest)) = parts.split_first() else {
        return true;
    };
    if !matches_compound(dom, node, compound) {
        return false;
    }
    let Some(combinator) = combinator else {
        return rest.is_empty();
    };
    match combinator {
        Combinator::Child => dom
            .parent(node)
            .is_some_and(|parent| matches_from(dom, parent, rest)),
        Combinator::Descendant => {
            let mut up = dom.parent(node);
            while let Some(ancestor) = up {
                if matches_from(dom, ancestor, rest) {
                    return true;
                }
                up = dom.parent(ancestor);
            }
            false
        }
        Combinator::Adjacent => {
            previous_element(dom, node).is_some_and(|before| matches_from(dom, before, rest))
        }
        Combinator::Sibling => {
            let mut before = previous_element(dom, node);
            while let Some(sibling) = before {
                if matches_from(dom, sibling, rest) {
                    return true;
                }
                before = previous_element(dom, sibling);
            }
            false
        }
    }
}

#[must_use]
pub fn cascade(dom: &Dom, node: NodeId, sheets: &[Sheet]) -> Vec<Declaration> {
    let mut found: Vec<((u32, u32, u32), usize, &Declaration)> = Vec::new();
    let mut order = 0;
    for sheet in sheets {
        for rule in &sheet.rules {
            order += 1;
            let best = rule
                .selectors
                .iter()
                .filter(|selector| matches(dom, node, selector))
                .map(Selector::specificity)
                .max();
            if let Some(specificity) = best {
                for declaration in &rule.declarations {
                    found.push((specificity, order, declaration));
                }
            }
        }
    }
    found.sort_by_key(|(specificity, order, _)| (*specificity, *order));
    let inline = dom
        .attr(node, "style")
        .map(parse_declarations)
        .unwrap_or_default();
    let mut out: Vec<Declaration> = found
        .iter()
        .filter(|(_, _, d)| !d.important)
        .map(|(_, _, d)| (*d).clone())
        .collect();
    out.extend(inline.iter().filter(|d| !d.important).cloned());
    out.extend(
        found
            .iter()
            .filter(|(_, _, d)| d.important)
            .map(|(_, _, d)| (*d).clone()),
    );
    out.extend(inline.into_iter().filter(|d| d.important));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::parse;

    #[test]
    fn specificity_and_order_decide_and_inline_wins() {
        let dom = parse(r#"<div class="a"><p id="x" class="b" style="color: green">t</p></div>"#);
        let sheet = parse_sheet(
            "/* c */ p { color: red } div > p.b { color: blue; font-size: 2em } \
             @media screen and (max-width: 10px) { p { color: pink } } #x { font-weight: bold }",
        );
        let p = dom.find_all("p")[0];
        let got = cascade(&dom, p, &[sheet]);
        let colours: Vec<_> = got
            .iter()
            .filter(|d| d.property == "color")
            .map(|d| d.value.as_str())
            .collect();
        assert_eq!(colours, ["red", "blue", "green"]);
        assert!(got.iter().any(|d| d.property == "font-weight"));
    }

    #[test]
    fn descendant_and_child_selectors() {
        let dom = parse("<table><tr><td><b>x</b></td></tr></table><b>y</b>");
        let sheet = parse_sheet("table b { color: red } td > b { x: y }");
        let bolds = dom.find_all("b");
        assert_eq!(
            cascade(&dom, bolds[0], std::slice::from_ref(&sheet)).len(),
            2
        );
        assert!(cascade(&dom, bolds[1], &[sheet]).is_empty());
    }
}
