use std::collections::HashMap;
use std::fmt::Write as _;

use super::blocks::Options;
use super::tree::Inline;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reference {
    pub to: String,
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct Notes {
    defined: Vec<String>,
    used: Vec<String>,
}

impl Notes {
    #[must_use]
    pub const fn new(defined: Vec<String>) -> Self {
        Self {
            defined,
            used: Vec::new(),
        }
    }

    fn number(&mut self, label: &str) -> Option<usize> {
        let label = normalize_label(label);
        if !self.defined.contains(&label) {
            return None;
        }
        if let Some(at) = self.used.iter().position(|known| *known == label) {
            return Some(at + 1);
        }
        self.used.push(label);
        Some(self.used.len())
    }

    #[must_use]
    pub fn used(&self) -> Vec<String> {
        self.used.clone()
    }
}

#[must_use]
pub fn inlines(text: &str) -> Vec<Inline> {
    inlines_with(text, &HashMap::new(), Options::ALL, &mut Notes::default())
}

#[must_use]
#[expect(clippy::implicit_hasher, reason = "one map type, the reader's own")]
pub fn inlines_with(
    text: &str,
    references: &HashMap<String, Reference>,
    options: Options,
    notes: &mut Notes,
) -> Vec<Inline> {
    let mut parser = InlineParser {
        letters: text.chars().collect(),
        at: 0,
        nodes: vec![INode::new(IKind::Root, String::new())],
        delimiters: Vec::new(),
        top: None,
        brackets: Vec::new(),
        bracket_top: None,
        references,
        options,
        notes,
        budget: MOST_STEPS,
    };
    parser.parse();
    let out = parser.convert(0);
    let out = merge_texts(out);
    if options.gfm { autolink(out) } else { out }
}

const MOST_STEPS: usize = 4_000_000;

#[derive(Clone, Debug, PartialEq)]
enum IKind {
    Root,
    Text,
    Soft,
    Hard,
    Code,
    Html,
    Emphasis,
    Strong,
    Strike,
    Highlight,
    Superscript,
    Subscript,
    Link { to: String, title: String },
    Image { to: String, title: String },
    Note(usize),
}

#[derive(Clone, Debug)]
struct INode {
    kind: IKind,
    literal: String,
    parent: Option<usize>,
    first: Option<usize>,
    last: Option<usize>,
    prev: Option<usize>,
    next: Option<usize>,
}

impl INode {
    const fn new(kind: IKind, literal: String) -> Self {
        Self {
            kind,
            literal,
            parent: None,
            first: None,
            last: None,
            prev: None,
            next: None,
        }
    }
}

#[derive(Clone, Debug)]
struct Delimiter {
    node: usize,
    letter: char,
    count: usize,
    original: usize,
    can_open: bool,
    can_close: bool,
    prev: Option<usize>,
    next: Option<usize>,
}

#[derive(Clone, Debug)]
struct Bracket {
    node: usize,
    prev: Option<usize>,
    delimiter: Option<usize>,
    index: usize,
    image: bool,
    active: bool,
    after: bool,
}

struct InlineParser<'a> {
    letters: Vec<char>,
    at: usize,
    nodes: Vec<INode>,
    delimiters: Vec<Delimiter>,
    top: Option<usize>,
    brackets: Vec<Bracket>,
    bracket_top: Option<usize>,
    references: &'a HashMap<String, Reference>,
    options: Options,
    notes: &'a mut Notes,
    budget: usize,
}

const fn escapable(letter: char) -> bool {
    letter.is_ascii_punctuation()
}

fn whitespace(letter: Option<char>) -> bool {
    letter.is_none_or(char::is_whitespace)
}

fn punctuation(letter: Option<char>) -> bool {
    let Some(letter) = letter else {
        return false;
    };
    if letter.is_ascii() {
        return letter.is_ascii_punctuation();
    }
    let code = u32::from(letter);
    matches!(code,
        0xA1..=0xA9 | 0xAB..=0xAC | 0xAE..=0xB1 | 0xB4 | 0xB6..=0xB8 | 0xBB | 0xBF | 0xD7 | 0xF7
        | 0x2C2..=0x2C5 | 0x2D2..=0x2DF | 0x2E5..=0x2EB | 0x2ED | 0x2EF..=0x2FF | 0x375 | 0x37E
        | 0x384..=0x385 | 0x387 | 0x55A..=0x55F | 0x589..=0x58A | 0x5BE | 0x5C0 | 0x5C3 | 0x5C6
        | 0x5F3..=0x5F4 | 0x606..=0x60F | 0x61B | 0x61D..=0x61F | 0x66A..=0x66D | 0x6D4
        | 0x964..=0x965 | 0x970 | 0xE3F | 0xE4F | 0xE5A..=0xE5B | 0xF04..=0xF12 | 0x104A..=0x104F
        | 0x10FB | 0x1360..=0x1368 | 0x166E | 0x169B..=0x169C | 0x16EB..=0x16ED | 0x17D4..=0x17D6
        | 0x17D8..=0x17DA | 0x1800..=0x180A | 0x2010..=0x2027 | 0x2030..=0x205E | 0x207A..=0x207E
        | 0x208A..=0x208E | 0x20A0..=0x20C0 | 0x2100..=0x2101 | 0x2103..=0x2106 | 0x2108..=0x2109
        | 0x2114 | 0x2116..=0x2118 | 0x211E..=0x2123 | 0x2125 | 0x2127 | 0x2129 | 0x212E
        | 0x213A..=0x213B | 0x2140..=0x2144 | 0x214A..=0x214D | 0x214F | 0x218A..=0x218B
        | 0x2190..=0x2426 | 0x2440..=0x244A | 0x249C..=0x24E9 | 0x2500..=0x2775 | 0x2794..=0x2B73
        | 0x2B76..=0x2B95 | 0x2B97..=0x2BFF | 0x2CE5..=0x2CEA | 0x2CF9..=0x2CFC | 0x2CFE..=0x2CFF
        | 0x2D70 | 0x2E00..=0x2E2E | 0x2E30..=0x2E5D | 0x2E80..=0x2FFB | 0x3001..=0x3004
        | 0x3008..=0x3020 | 0x3030 | 0x3036..=0x3037 | 0x303D..=0x303F | 0x309B..=0x309C | 0x30A0
        | 0x30FB | 0x3190..=0x3191 | 0x3196..=0x319F | 0x31C0..=0x31E3 | 0x3200..=0x321E
        | 0x322A..=0x3247 | 0x3250 | 0x3260..=0x327F | 0x328A..=0x32B0 | 0x32C0..=0x33FF
        | 0xFD3E..=0xFD3F | 0xFE10..=0xFE19 | 0xFE30..=0xFE52 | 0xFE54..=0xFE66 | 0xFE68..=0xFE6B
        | 0xFF01..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65 | 0xFFE0..=0xFFEE
        | 0x1F000..=0x1FAFF)
}

impl InlineParser<'_> {
    fn spend(&mut self) -> bool {
        if self.budget == 0 {
            return false;
        }
        self.budget -= 1;
        true
    }

    fn peek(&self) -> Option<char> {
        self.letters.get(self.at).copied()
    }

    fn peek_at(&self, at: usize) -> Option<char> {
        self.letters.get(at).copied()
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.letters[from.min(self.letters.len())..to.min(self.letters.len())]
            .iter()
            .collect()
    }

    fn add(&mut self, kind: IKind, literal: String) -> usize {
        let node = self.nodes.len();
        self.nodes.push(INode::new(kind, literal));
        node
    }

    fn append_child(&mut self, parent: usize, child: usize) {
        self.unlink(child);
        self.nodes[child].parent = Some(parent);
        if let Some(last) = self.nodes[parent].last {
            self.nodes[last].next = Some(child);
            self.nodes[child].prev = Some(last);
        } else {
            self.nodes[parent].first = Some(child);
        }
        self.nodes[parent].last = Some(child);
    }

    fn insert_after(&mut self, sibling: usize, node: usize) {
        self.unlink(node);
        let next = self.nodes[sibling].next;
        let parent = self.nodes[sibling].parent;
        self.nodes[node].next = next;
        self.nodes[node].prev = Some(sibling);
        self.nodes[node].parent = parent;
        self.nodes[sibling].next = Some(node);
        if let Some(next) = next {
            self.nodes[next].prev = Some(node);
        } else if let Some(parent) = parent {
            self.nodes[parent].last = Some(node);
        }
    }

    fn unlink(&mut self, node: usize) {
        let (prev, next, parent) = (
            self.nodes[node].prev,
            self.nodes[node].next,
            self.nodes[node].parent,
        );
        if let Some(prev) = prev {
            self.nodes[prev].next = next;
        } else if let Some(parent) = parent {
            self.nodes[parent].first = next;
        }
        if let Some(next) = next {
            self.nodes[next].prev = prev;
        } else if let Some(parent) = parent {
            self.nodes[parent].last = prev;
        }
        self.nodes[node].prev = None;
        self.nodes[node].next = None;
        self.nodes[node].parent = None;
    }

    fn text(&mut self, literal: String) -> usize {
        let node = self.add(IKind::Text, literal);
        self.append_child(0, node);
        node
    }

    fn parse(&mut self) {
        while self.at < self.letters.len() {
            if !self.spend() {
                let rest = self.slice(self.at, self.letters.len());
                self.text(rest);
                self.at = self.letters.len();
                break;
            }
            if !self.one() {
                let letter = self.letters[self.at];
                self.at += 1;
                self.text(letter.to_string());
            }
        }
        self.process_emphasis(None);
    }

    fn one(&mut self) -> bool {
        let Some(letter) = self.peek() else {
            return false;
        };
        match letter {
            '\n' => {
                self.newline();
                true
            }
            '\\' => {
                self.backslash();
                true
            }
            '`' => {
                self.backticks();
                true
            }
            '*' | '_' => {
                self.delimiters_of(letter);
                true
            }
            '~' if self.options.gfm || self.options.extras => {
                if self.options.extras && self.script('~') {
                    return true;
                }
                if self.options.gfm {
                    self.delimiters_of('~');
                    return true;
                }
                false
            }
            '=' if self.options.extras && self.peek_at(self.at + 1) == Some('=') => {
                self.delimiters_of('=');
                true
            }
            '^' if self.options.extras => self.script('^'),
            '[' => {
                if self.options.extras && self.note() {
                    return true;
                }
                self.at += 1;
                let node = self.text("[".to_owned());
                self.add_bracket(node, self.at, false);
                true
            }
            '!' => {
                self.at += 1;
                if self.peek() == Some('[') {
                    self.at += 1;
                    let node = self.text("![".to_owned());
                    self.add_bracket(node, self.at, true);
                } else {
                    self.text("!".to_owned());
                }
                true
            }
            ']' => {
                self.close_bracket();
                true
            }
            '<' => self.autolink() || self.html(),
            '&' => {
                self.entity();
                true
            }
            _ => {
                self.string();
                true
            }
        }
    }

    fn special(&self, letter: char) -> bool {
        matches!(
            letter,
            '\n' | '`' | '[' | ']' | '\\' | '!' | '<' | '&' | '*' | '_'
        ) || (letter == '~' && (self.options.gfm || self.options.extras))
            || (matches!(letter, '=' | '^') && self.options.extras)
    }

    fn string(&mut self) {
        let start = self.at;
        while let Some(letter) = self.peek() {
            if self.special(letter) {
                break;
            }
            self.at += 1;
        }
        if self.at == start {
            self.at += 1;
        }
        let text = self.slice(start, self.at);
        self.text(text);
    }

    fn newline(&mut self) {
        self.at += 1;
        let last = self.nodes[0].last;
        let mut hard = false;
        if let Some(last) = last
            && self.nodes[last].kind == IKind::Text
            && self.nodes[last].literal.ends_with(' ')
        {
            let literal = &self.nodes[last].literal;
            hard = literal.ends_with("  ");
            let trimmed = literal.trim_end_matches(' ').to_owned();
            self.nodes[last].literal = trimmed;
        }
        let kind = if hard { IKind::Hard } else { IKind::Soft };
        let node = self.add(kind, String::new());
        self.append_child(0, node);
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.at += 1;
        }
    }

    fn backslash(&mut self) {
        self.at += 1;
        match self.peek() {
            Some('\n') => {
                self.at += 1;
                let node = self.add(IKind::Hard, String::new());
                self.append_child(0, node);
                while matches!(self.peek(), Some(' ' | '\t')) {
                    self.at += 1;
                }
            }
            Some(letter) if escapable(letter) => {
                self.at += 1;
                self.text(letter.to_string());
            }
            _ => {
                self.text("\\".to_owned());
            }
        }
    }

    fn backticks(&mut self) {
        let start = self.at;
        while self.peek() == Some('`') {
            self.at += 1;
        }
        let count = self.at - start;
        let after_open = self.at;
        let mut at = self.at;
        while at < self.letters.len() {
            if self.letters[at] == '`' {
                let run_start = at;
                while self.peek_at(at) == Some('`') {
                    at += 1;
                }
                if at - run_start == count {
                    let inside: String = self.slice(after_open, run_start).replace('\n', " ");
                    let inside = if inside.len() > 2
                        && inside.starts_with(' ')
                        && inside.ends_with(' ')
                        && !inside.chars().all(|c| c == ' ')
                    {
                        inside[1..inside.len() - 1].to_owned()
                    } else if inside.len() == 2
                        && inside.starts_with(' ')
                        && inside.ends_with(' ')
                        && inside != "  "
                    {
                        String::new()
                    } else {
                        inside
                    };
                    let node = self.add(IKind::Code, inside);
                    self.append_child(0, node);
                    self.at = at;
                    return;
                }
            } else {
                at += 1;
            }
        }
        self.at = after_open;
        let ticks = "`".repeat(count);
        self.text(ticks);
    }

    fn script(&mut self, mark: char) -> bool {
        if self.peek_at(self.at + 1) == Some(mark) {
            return false;
        }
        let start = self.at + 1;
        let mut at = start;
        while let Some(letter) = self.peek_at(at) {
            if letter == mark {
                break;
            }
            if letter.is_whitespace() {
                return false;
            }
            if letter == '\\' {
                at += 1;
            }
            at += 1;
        }
        if self.peek_at(at) != Some(mark) || at == start || self.peek_at(at + 1) == Some(mark) {
            return false;
        }
        let inside = self.slice(start, at);
        let kind = if mark == '^' {
            IKind::Superscript
        } else {
            IKind::Subscript
        };
        let node = self.add(kind, String::new());
        self.append_child(0, node);
        let mut notes = Notes::default();
        for child in inlines_with(&inside, self.references, Options::GFM, &mut notes) {
            let made = self.adopt(child);
            self.append_child(node, made);
        }
        self.at = at + 1;
        true
    }

    fn note(&mut self) -> bool {
        if self.peek_at(self.at + 1) != Some('^') {
            return false;
        }
        let start = self.at + 2;
        let mut at = start;
        while let Some(letter) = self.peek_at(at) {
            if letter == ']' || letter.is_whitespace() || letter == '[' {
                break;
            }
            at += 1;
        }
        if self.peek_at(at) != Some(']') || at == start {
            return false;
        }
        let label = self.slice(start, at);
        let Some(number) = self.notes.number(&label) else {
            return false;
        };
        let node = self.add(IKind::Note(number), label);
        self.append_child(0, node);
        self.at = at + 1;
        true
    }

    fn adopt(&mut self, inline: Inline) -> usize {
        let (kind, literal, inside) = match inline {
            Inline::Text(text) => (IKind::Text, text, Vec::new()),
            Inline::Code(text) => (IKind::Code, text, Vec::new()),
            Inline::Html(text) => (IKind::Html, text, Vec::new()),
            Inline::Soft => (IKind::Soft, String::new(), Vec::new()),
            Inline::Hard => (IKind::Hard, String::new(), Vec::new()),
            Inline::Emphasis(inside) => (IKind::Emphasis, String::new(), inside),
            Inline::Strong(inside) => (IKind::Strong, String::new(), inside),
            Inline::Strike(inside) => (IKind::Strike, String::new(), inside),
            Inline::Highlight(inside) => (IKind::Highlight, String::new(), inside),
            Inline::Superscript(inside) => (IKind::Superscript, String::new(), inside),
            Inline::Subscript(inside) => (IKind::Subscript, String::new(), inside),
            Inline::Link { to, title, text } => (IKind::Link { to, title }, String::new(), text),
            Inline::Image { at, title, text } => {
                (IKind::Image { to: at, title }, String::new(), text)
            }
            Inline::Check(ticked) => (
                IKind::Text,
                if ticked { "[x]" } else { "[ ]" }.to_owned(),
                Vec::new(),
            ),
            Inline::Note(number) => (IKind::Note(number), String::new(), Vec::new()),
        };
        let node = self.add(kind, literal);
        for child in inside {
            let made = self.adopt(child);
            self.append_child(node, made);
        }
        node
    }

    fn delimiters_of(&mut self, letter: char) {
        let start = self.at;
        while self.peek() == Some(letter) {
            self.at += 1;
        }
        let count = self.at - start;
        let before = if start == 0 {
            Some('\n')
        } else {
            self.peek_at(start - 1)
        };
        let after = self.peek().or(Some('\n'));
        let before_white = whitespace(before);
        let after_white = whitespace(after);
        let before_punct = punctuation(before);
        let after_punct = punctuation(after);
        let left = !after_white && (!after_punct || before_white || before_punct);
        let right = !before_white && (!before_punct || after_white || after_punct);
        let (mut can_open, mut can_close) = if letter == '_' {
            (
                left && (!right || before_punct),
                right && (!left || after_punct),
            )
        } else {
            (left, right)
        };
        if (letter == '~' && count > 2) || (letter == '=' && count != 2) {
            can_open = false;
            can_close = false;
        }
        let literal = self.slice(start, self.at);
        let node = self.text(literal);
        if can_open || can_close {
            let delimiter = self.delimiters.len();
            self.delimiters.push(Delimiter {
                node,
                letter,
                count,
                original: count,
                can_open,
                can_close,
                prev: self.top,
                next: None,
            });
            if let Some(top) = self.top {
                self.delimiters[top].next = Some(delimiter);
            }
            self.top = Some(delimiter);
        }
    }

    fn remove_delimiter(&mut self, delimiter: usize) {
        let (prev, next) = (
            self.delimiters[delimiter].prev,
            self.delimiters[delimiter].next,
        );
        if let Some(prev) = prev {
            self.delimiters[prev].next = next;
        }
        if let Some(next) = next {
            self.delimiters[next].prev = prev;
        } else {
            self.top = prev;
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the specification's procedure, step by step"
    )]
    fn process_emphasis(&mut self, bottom: Option<usize>) {
        let mut openers_bottom: HashMap<(char, usize), Option<usize>> = HashMap::new();
        let mut closer = self.top;
        while let Some(candidate) = closer {
            if self.delimiters[candidate].prev == bottom {
                break;
            }
            closer = self.delimiters[candidate].prev;
        }
        while let Some(current) = closer {
            if !self.spend() {
                break;
            }
            let Delimiter {
                letter,
                can_close,
                can_open: closer_opens,
                original: closer_original,
                ..
            } = self.delimiters[current].clone();
            if !can_close {
                closer = self.delimiters[current].next;
                continue;
            }
            let index = if matches!(letter, '*' | '_') {
                usize::from(closer_opens) * 3 + closer_original % 3
            } else {
                0
            };
            let floor = openers_bottom.get(&(letter, index)).copied().flatten();
            let mut opener = self.delimiters[current].prev;
            let mut found = false;
            while let Some(candidate) = opener {
                if Some(candidate) == bottom || Some(candidate) == floor {
                    break;
                }
                let them = &self.delimiters[candidate];
                let odd_match = matches!(letter, '*' | '_')
                    && (closer_opens || them.can_close)
                    && closer_original % 3 != 0
                    && (them.original + closer_original).is_multiple_of(3);
                let same_length =
                    !matches!(letter, '~' | '=') || them.count == self.delimiters[current].count;
                if them.letter == letter && them.can_open && !odd_match && same_length {
                    found = true;
                    break;
                }
                opener = them.prev;
            }
            let old_closer = current;
            if found && let Some(opener) = opener {
                let use_delimiters = match letter {
                    '~' | '=' => self.delimiters[current].count,
                    _ => {
                        if self.delimiters[current].count >= 2 && self.delimiters[opener].count >= 2
                        {
                            2
                        } else {
                            1
                        }
                    }
                };
                let opener_node = self.delimiters[opener].node;
                let closer_node = self.delimiters[current].node;
                self.delimiters[opener].count -= use_delimiters;
                self.delimiters[current].count -= use_delimiters;
                for node in [opener_node, closer_node] {
                    let literal = &mut self.nodes[node].literal;
                    let keep = literal.chars().count() - use_delimiters;
                    *literal = literal.chars().take(keep).collect();
                }
                let kind = match (letter, use_delimiters) {
                    ('~', _) => IKind::Strike,
                    ('=', _) => IKind::Highlight,
                    (_, 2) => IKind::Strong,
                    _ => IKind::Emphasis,
                };
                let wrapper = self.add(kind, String::new());
                let mut child = self.nodes[opener_node].next;
                while let Some(node) = child {
                    if node == closer_node {
                        break;
                    }
                    let next = self.nodes[node].next;
                    self.append_child(wrapper, node);
                    child = next;
                }
                self.insert_after(opener_node, wrapper);
                let mut between = self.delimiters[current].prev;
                while let Some(node) = between {
                    if node == opener {
                        break;
                    }
                    let prev = self.delimiters[node].prev;
                    self.remove_delimiter(node);
                    between = prev;
                }
                if self.delimiters[opener].count == 0 {
                    self.unlink(opener_node);
                    self.remove_delimiter(opener);
                }
                if self.delimiters[current].count == 0 {
                    let next = self.delimiters[current].next;
                    self.unlink(closer_node);
                    self.remove_delimiter(current);
                    closer = next;
                }
            } else {
                closer = self.delimiters[current].next;
            }
            if !found {
                openers_bottom.insert((letter, index), self.delimiters[old_closer].prev);
                if !self.delimiters[old_closer].can_open {
                    self.remove_delimiter(old_closer);
                }
            }
        }
        while let Some(top) = self.top {
            if Some(top) == bottom {
                break;
            }
            self.remove_delimiter(top);
        }
    }

    fn add_bracket(&mut self, node: usize, index: usize, image: bool) {
        if let Some(top) = self.bracket_top {
            self.brackets[top].after = true;
        }
        let bracket = self.brackets.len();
        self.brackets.push(Bracket {
            node,
            prev: self.bracket_top,
            delimiter: self.top,
            index,
            image,
            active: true,
            after: false,
        });
        self.bracket_top = Some(bracket);
    }

    fn remove_bracket(&mut self) {
        if let Some(top) = self.bracket_top {
            self.bracket_top = self.brackets[top].prev;
        }
    }

    fn spnl(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.at += 1;
        }
        if self.peek() == Some('\n') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.at += 1;
        }
    }

    fn close_bracket(&mut self) {
        let start = self.at;
        self.at += 1;
        let Some(opener) = self.bracket_top else {
            self.text("]".to_owned());
            return;
        };
        if !self.brackets[opener].active {
            self.text("]".to_owned());
            self.remove_bracket();
            return;
        }
        let image = self.brackets[opener].image;
        let saved = self.at;
        let mut found: Option<(String, String)> = None;
        if self.peek() == Some('(') {
            self.at += 1;
            self.spnl();
            if let Some(to) = link_destination(&self.letters, &mut self.at) {
                let before_title = self.at;
                self.spnl();
                let mut title = String::new();
                if self.at > before_title
                    && let Some(said) = link_title(&self.letters, &mut self.at)
                {
                    title = said;
                }
                self.spnl();
                if self.peek() == Some(')') {
                    self.at += 1;
                    found = Some((to, title));
                }
            }
            if found.is_none() {
                self.at = saved;
            }
        }
        if found.is_none() {
            let before_label = self.at;
            let used = link_label(&self.letters, self.at);
            let label = if used > 2 {
                Some(self.slice(before_label, before_label + used))
            } else if !self.brackets[opener].after {
                Some(format!(
                    "[{}]",
                    self.slice(self.brackets[opener].index, start)
                ))
            } else {
                None
            };
            if used == 0 {
                self.at = saved;
            } else {
                self.at = before_label + used;
            }
            if let Some(label) = label
                && let Some(reference) = self.references.get(&normalize_label(&label))
            {
                found = Some((reference.to.clone(), reference.title.clone()));
            } else if used > 0 {
                self.at = saved;
            }
        }
        let Some((to, title)) = found else {
            self.remove_bracket();
            self.at = start + 1;
            self.text("]".to_owned());
            return;
        };
        let kind = if image {
            IKind::Image { to, title }
        } else {
            IKind::Link { to, title }
        };
        let link = self.add(kind, String::new());
        let opener_node = self.brackets[opener].node;
        let mut child = self.nodes[opener_node].next;
        while let Some(node) = child {
            let next = self.nodes[node].next;
            self.append_child(link, node);
            child = next;
        }
        self.append_child(0, link);
        self.process_emphasis(self.brackets[opener].delimiter);
        self.remove_bracket();
        self.unlink(opener_node);
        if !image {
            let mut back = self.bracket_top;
            while let Some(bracket) = back {
                if !self.brackets[bracket].image {
                    self.brackets[bracket].active = false;
                }
                back = self.brackets[bracket].prev;
            }
        }
    }

    fn autolink(&mut self) -> bool {
        let start = self.at + 1;
        let mut at = start;
        while let Some(letter) = self.peek_at(at) {
            if letter == '>' || letter == '<' || letter.is_whitespace() || u32::from(letter) < 0x20
            {
                break;
            }
            at += 1;
        }
        if self.peek_at(at) != Some('>') {
            return false;
        }
        let inside = self.slice(start, at);
        let to = if is_uri(&inside) {
            normalize_uri(&inside)
        } else if is_email(&inside) {
            format!("mailto:{}", normalize_uri(&inside))
        } else {
            return false;
        };
        let link = self.add(
            IKind::Link {
                to,
                title: String::new(),
            },
            String::new(),
        );
        self.append_child(0, link);
        let text = self.add(IKind::Text, inside);
        self.append_child(link, text);
        self.at = at + 1;
        true
    }

    fn html(&mut self) -> bool {
        let Some(used) = html_tag(&self.letters, self.at) else {
            return false;
        };
        let literal = self.slice(self.at, self.at + used);
        let node = self.add(IKind::Html, literal);
        self.append_child(0, node);
        self.at += used;
        true
    }

    fn entity(&mut self) {
        if let Some((text, used)) = entity_at(&self.letters, self.at) {
            self.at += used;
            self.text(text);
        } else {
            self.at += 1;
            self.text("&".to_owned());
        }
    }

    fn convert(&self, node: usize) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut child = self.nodes[node].first;
        while let Some(at) = child {
            let it = &self.nodes[at];
            let inside = || self.convert(at);
            let made = match &it.kind {
                IKind::Root => None,
                IKind::Text => (!it.literal.is_empty()).then(|| Inline::Text(it.literal.clone())),
                IKind::Soft => Some(Inline::Soft),
                IKind::Hard => Some(Inline::Hard),
                IKind::Code => Some(Inline::Code(it.literal.clone())),
                IKind::Html => Some(Inline::Html(it.literal.clone())),
                IKind::Emphasis => Some(Inline::Emphasis(inside())),
                IKind::Strong => Some(Inline::Strong(inside())),
                IKind::Strike => Some(Inline::Strike(inside())),
                IKind::Highlight => Some(Inline::Highlight(inside())),
                IKind::Superscript => Some(Inline::Superscript(inside())),
                IKind::Subscript => Some(Inline::Subscript(inside())),
                IKind::Note(number) => Some(Inline::Note(*number)),
                IKind::Link { to, title } => Some(Inline::Link {
                    to: to.clone(),
                    title: title.clone(),
                    text: inside(),
                }),
                IKind::Image { to, title } => Some(Inline::Image {
                    at: to.clone(),
                    title: title.clone(),
                    text: inside(),
                }),
            };
            out.extend(made);
            child = it.next;
        }
        out
    }
}

fn merge_texts(pieces: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for piece in pieces {
        let piece = match piece {
            Inline::Emphasis(inside) => Inline::Emphasis(merge_texts(inside)),
            Inline::Strong(inside) => Inline::Strong(merge_texts(inside)),
            Inline::Strike(inside) => Inline::Strike(merge_texts(inside)),
            Inline::Highlight(inside) => Inline::Highlight(merge_texts(inside)),
            Inline::Superscript(inside) => Inline::Superscript(merge_texts(inside)),
            Inline::Subscript(inside) => Inline::Subscript(merge_texts(inside)),
            Inline::Link { to, title, text } => Inline::Link {
                to,
                title,
                text: merge_texts(text),
            },
            Inline::Image { at, title, text } => Inline::Image {
                at,
                title,
                text: merge_texts(text),
            },
            other => other,
        };
        if let Inline::Text(text) = &piece
            && let Some(Inline::Text(last)) = out.last_mut()
        {
            last.push_str(text);
            continue;
        }
        out.push(piece);
    }
    out
}

fn link_label(letters: &[char], from: usize) -> usize {
    if letters.get(from) != Some(&'[') {
        return 0;
    }
    let mut at = from + 1;
    let mut inside = 0;
    while let Some(letter) = letters.get(at) {
        match letter {
            '\\' if letters.get(at + 1).is_some_and(|next| escapable(*next)) => {
                at += 2;
                inside += 2;
                continue;
            }
            '[' => return 0,
            ']' => {
                if inside > 999 {
                    return 0;
                }
                return at + 1 - from;
            }
            _ => {}
        }
        at += 1;
        inside += 1;
    }
    0
}

fn link_destination(letters: &[char], at: &mut usize) -> Option<String> {
    if letters.get(*at) == Some(&'<') {
        let mut end = *at + 1;
        while let Some(letter) = letters.get(end) {
            match letter {
                '\\' if letters.get(end + 1).is_some_and(|next| escapable(*next)) => end += 2,
                '>' => {
                    let inside: String = letters[*at + 1..end].iter().collect();
                    *at = end + 1;
                    return Some(normalize_uri(&unescape(&inside)));
                }
                '<' | '\n' => return None,
                _ => end += 1,
            }
        }
        return None;
    }
    let start = *at;
    let mut depth = 0_usize;
    while let Some(&letter) = letters.get(*at) {
        match letter {
            '\\' if letters.get(*at + 1).is_some_and(|next| escapable(*next)) => *at += 2,
            '(' => {
                depth += 1;
                *at += 1;
            }
            ')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                *at += 1;
            }
            letter if letter.is_ascii_control() || letter == ' ' => break,
            _ => *at += 1,
        }
    }
    if depth != 0 {
        *at = start;
        return None;
    }
    if *at == start && letters.get(*at) != Some(&')') {
        return None;
    }
    let raw: String = letters[start..*at].iter().collect();
    Some(normalize_uri(&unescape(&raw)))
}

fn link_title(letters: &[char], at: &mut usize) -> Option<String> {
    let open = *letters.get(*at)?;
    let close = match open {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => return None,
    };
    let mut end = *at + 1;
    while let Some(&letter) = letters.get(end) {
        if letter == '\\' && letters.get(end + 1).is_some_and(|next| escapable(*next)) {
            end += 2;
            continue;
        }
        if letter == close {
            let inside: String = letters[*at + 1..end].iter().collect();
            *at = end + 1;
            return Some(unescape(&inside));
        }
        if open == '(' && letter == '(' {
            return None;
        }
        end += 1;
    }
    None
}

#[expect(clippy::implicit_hasher, reason = "one map type, the reader's own")]
pub fn reference(text: &str, references: &mut HashMap<String, Reference>) -> usize {
    let letters: Vec<char> = text.chars().collect();
    let used = reference_at(&letters, 0, references);
    letters[..used].iter().map(|c| c.len_utf8()).sum()
}

#[expect(clippy::implicit_hasher, reason = "one map type, the reader's own")]
pub fn reference_at(
    letters: &[char],
    from: usize,
    references: &mut HashMap<String, Reference>,
) -> usize {
    let used = link_label(letters, from);
    if used == 0 {
        return 0;
    }
    let label: String = letters[from..from + used].iter().collect();
    let mut at = from + used;
    if letters.get(at) != Some(&':') {
        return 0;
    }
    at += 1;
    let skip = |at: &mut usize| {
        while matches!(letters.get(*at), Some(' ' | '\t')) {
            *at += 1;
        }
        if letters.get(*at) == Some(&'\n') {
            *at += 1;
        }
        while matches!(letters.get(*at), Some(' ' | '\t')) {
            *at += 1;
        }
    };
    skip(&mut at);
    let destination_start = at;
    let Some(to) = link_destination(letters, &mut at) else {
        return 0;
    };
    if at == destination_start && letters.get(destination_start) != Some(&'<') {
        return 0;
    }
    let before_title = at;
    skip(&mut at);
    let mut title = None;
    if at > before_title {
        title = link_title(letters, &mut at);
    }
    if title.is_none() {
        at = before_title;
    }
    let at_line_end = |at: usize| {
        let mut end = at;
        while matches!(letters.get(end), Some(' ' | '\t')) {
            end += 1;
        }
        matches!(letters.get(end), None | Some('\n')).then_some(end)
    };
    let end = match at_line_end(at) {
        Some(end) => end,
        None if title.is_some() => {
            title = None;
            at = before_title;
            match at_line_end(at) {
                Some(end) => end,
                None => return 0,
            }
        }
        None => return 0,
    };
    let end = if letters.get(end) == Some(&'\n') {
        end + 1
    } else {
        end
    };
    let normal = normalize_label(&label);
    if normal.is_empty() {
        return 0;
    }
    references.entry(normal).or_insert(Reference {
        to,
        title: title.unwrap_or_default(),
    });
    end - from
}

#[must_use]
pub fn normalize_label(label: &str) -> String {
    let inside = label
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(label);
    let folded = inside.to_lowercase().to_uppercase().to_lowercase();
    let folded = folded.replace(['\u{1E9E}', 'ß'], "ss");
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[must_use]
pub fn unescape(text: &str) -> String {
    let letters: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < letters.len() {
        match letters[at] {
            '\\' if letters.get(at + 1).is_some_and(|next| escapable(*next)) => {
                out.push(letters[at + 1]);
                at += 2;
            }
            '&' => {
                if let Some((said, used)) = entity_at(&letters, at) {
                    out.push_str(&said);
                    at += used;
                } else {
                    out.push('&');
                    at += 1;
                }
            }
            letter => {
                out.push(letter);
                at += 1;
            }
        }
    }
    out
}

fn entity_at(letters: &[char], at: usize) -> Option<(String, usize)> {
    if letters.get(at) != Some(&'&') {
        return None;
    }
    let end = (at + 1..letters.len().min(at + 40)).find(|end| letters[*end] == ';')?;
    let name: String = letters[at + 1..end].iter().collect();
    let used = end + 1 - at;
    if let Some(number) = name.strip_prefix('#') {
        let value = if let Some(hex) = number.strip_prefix(['x', 'X']) {
            if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(hex, 16).ok()?
        } else {
            if number.is_empty() || number.len() > 7 || !number.chars().all(|c| c.is_ascii_digit())
            {
                return None;
            }
            number.parse().ok()?
        };
        let letter = if value == 0 {
            '\u{FFFD}'
        } else {
            char::from_u32(value).unwrap_or('\u{FFFD}')
        };
        return Some((letter.to_string(), used));
    }
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    super::entities::named(&name).map(|said| (said.to_owned(), used))
}

#[must_use]
pub fn normalize_uri(uri: &str) -> String {
    let bytes = uri.as_bytes();
    let mut out = String::with_capacity(uri.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'%'
            && bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(at + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            out.push('%');
            at += 1;
            continue;
        }
        if byte.is_ascii_alphanumeric() || b";/?:@&=+$,-_.!~*'()#".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
        at += 1;
    }
    out
}

fn is_uri(inside: &str) -> bool {
    let Some((scheme, rest)) = inside.split_once(':') else {
        return false;
    };
    let _ = rest;
    (2..=32).contains(&scheme.len())
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && !inside
            .chars()
            .any(|c| c == ' ' || c == '<' || c == '>' || c.is_ascii_control())
}

fn is_email(inside: &str) -> bool {
    let Some((local, domain)) = inside.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c))
        && !domain.is_empty()
        && domain.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !part.starts_with('-')
                && !part.ends_with('-')
        })
}

#[must_use]
pub fn open_or_closing_tag(text: &str) -> Option<usize> {
    let letters: Vec<char> = text.chars().collect();
    let used = if letters.get(1) == Some(&'/') {
        closing_tag(&letters, 0)?
    } else {
        open_tag(&letters, 0)?
    };
    Some(letters[..used].iter().map(|c| c.len_utf8()).sum())
}

fn tag_name(letters: &[char], mut at: usize) -> Option<usize> {
    if !letters.get(at)?.is_ascii_alphabetic() {
        return None;
    }
    while letters
        .get(at)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '-')
    {
        at += 1;
    }
    Some(at)
}

fn skip_white(letters: &[char], mut at: usize) -> usize {
    while letters
        .get(at)
        .is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
    {
        at += 1;
    }
    at
}

fn open_tag(letters: &[char], from: usize) -> Option<usize> {
    if letters.get(from) != Some(&'<') {
        return None;
    }
    let mut at = tag_name(letters, from + 1)?;
    loop {
        let after_white = skip_white(letters, at);
        if after_white == at {
            break;
        }
        let Some(first) = letters.get(after_white) else {
            break;
        };
        if !(first.is_ascii_alphabetic() || *first == '_' || *first == ':') {
            break;
        }
        let mut name_end = after_white + 1;
        while letters
            .get(name_end)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
        {
            name_end += 1;
        }
        at = name_end;
        let before_equals = skip_white(letters, name_end);
        if letters.get(before_equals) == Some(&'=') {
            let value_start = skip_white(letters, before_equals + 1);
            match letters.get(value_start) {
                Some(&quote @ ('"' | '\'')) => {
                    let close =
                        (value_start + 1..letters.len()).find(|at| letters[*at] == quote)?;
                    at = close + 1;
                }
                Some(_) => {
                    let mut end = value_start;
                    while letters.get(end).is_some_and(|c| {
                        !matches!(c, '"' | '\'' | '=' | '<' | '>' | '`')
                            && !c.is_whitespace()
                            && !c.is_ascii_control()
                    }) {
                        end += 1;
                    }
                    if end == value_start {
                        return None;
                    }
                    at = end;
                }
                None => return None,
            }
        }
    }
    at = skip_white(letters, at);
    if letters.get(at) == Some(&'/') {
        at += 1;
    }
    (letters.get(at) == Some(&'>')).then_some(at + 1 - from)
}

fn closing_tag(letters: &[char], from: usize) -> Option<usize> {
    if letters.get(from) != Some(&'<') || letters.get(from + 1) != Some(&'/') {
        return None;
    }
    let at = skip_white(letters, tag_name(letters, from + 2)?);
    (letters.get(at) == Some(&'>')).then_some(at + 1 - from)
}

fn find(letters: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    (from..=letters.len().saturating_sub(needle.len()))
        .find(|at| letters[*at..].starts_with(&needle))
}

fn html_tag(letters: &[char], from: usize) -> Option<usize> {
    if letters.get(from) != Some(&'<') {
        return None;
    }
    match letters.get(from + 1)? {
        '/' => closing_tag(letters, from),
        '?' => find(letters, from + 2, "?>").map(|end| end + 2 - from),
        '!' => {
            let rest: String = letters[from..letters.len().min(from + 9)].iter().collect();
            if rest.starts_with("<!-->") {
                return Some(5);
            }
            if rest.starts_with("<!--->") {
                return Some(6);
            }
            if rest.starts_with("<!--") {
                return find(letters, from + 4, "-->").map(|end| end + 3 - from);
            }
            if rest.starts_with("<![CDATA[") {
                return find(letters, from + 9, "]]>").map(|end| end + 3 - from);
            }
            if letters.get(from + 2).is_some_and(char::is_ascii_alphabetic) {
                return find(letters, from + 2, ">").map(|end| end + 1 - from);
            }
            None
        }
        _ => open_tag(letters, from),
    }
}

fn autolink(pieces: Vec<Inline>) -> Vec<Inline> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            Inline::Text(text) => out.extend(link_text(&text)),
            Inline::Emphasis(inside) => out.push(Inline::Emphasis(autolink(inside))),
            Inline::Strong(inside) => out.push(Inline::Strong(autolink(inside))),
            Inline::Strike(inside) => out.push(Inline::Strike(autolink(inside))),
            Inline::Highlight(inside) => out.push(Inline::Highlight(autolink(inside))),
            other => out.push(other),
        }
    }
    out
}

fn link_text(text: &str) -> Vec<Inline> {
    let letters: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut at = 0;
    while at < letters.len() {
        let boundary =
            at == 0 || matches!(letters[at - 1], ' ' | '\t' | '\n' | '*' | '_' | '~' | '(');
        let rest: String = letters[at..letters.len().min(at + 8)].iter().collect();
        let lower = rest.to_ascii_lowercase();
        let (prefix, scheme) = if lower.starts_with("www.") {
            (4, "http://")
        } else if lower.starts_with("https://") {
            (8, "")
        } else if lower.starts_with("http://") {
            (7, "")
        } else if lower.starts_with("ftp://") {
            (6, "")
        } else {
            (0, "")
        };
        if boundary
            && prefix > 0
            && let Some(end) = url_end(&letters, at, prefix)
        {
            {
                let shown: String = letters[at..end].iter().collect();
                if !plain.is_empty() {
                    out.push(Inline::Text(std::mem::take(&mut plain)));
                }
                out.push(Inline::Link {
                    to: normalize_uri(&format!("{scheme}{shown}")),
                    title: String::new(),
                    text: vec![Inline::Text(shown)],
                });
                at = end;
                continue;
            }
        }
        if letters[at] == '@'
            && let Some((start, end)) = email_around(&letters, at)
        {
            let local_len = at - start;
            if plain.chars().count() >= local_len {
                let keep: String = plain
                    .chars()
                    .take(plain.chars().count() - local_len)
                    .collect();
                let shown: String = letters[start..end].iter().collect();
                plain = keep;
                if !plain.is_empty() {
                    out.push(Inline::Text(std::mem::take(&mut plain)));
                }
                out.push(Inline::Link {
                    to: format!("mailto:{shown}"),
                    title: String::new(),
                    text: vec![Inline::Text(shown)],
                });
                at = end;
                continue;
            }
        }
        plain.push(letters[at]);
        at += 1;
    }
    if !plain.is_empty() {
        out.push(Inline::Text(plain));
    }
    out
}

fn url_end(letters: &[char], from: usize, prefix: usize) -> Option<usize> {
    let mut at = from + prefix;
    let domain_start = at;
    let mut segments = 0;
    let mut last_segment_underscore = false;
    let mut second_last_underscore = false;
    loop {
        let seg_start = at;
        let mut underscore = false;
        while letters
            .get(at)
            .is_some_and(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        {
            if letters[at] == '_' {
                underscore = true;
            }
            at += 1;
        }
        if at == seg_start {
            break;
        }
        segments += 1;
        second_last_underscore = last_segment_underscore;
        last_segment_underscore = underscore;
        if letters.get(at) == Some(&'.')
            && letters
                .get(at + 1)
                .is_some_and(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        {
            at += 1;
            continue;
        }
        break;
    }
    if at == domain_start || last_segment_underscore || second_last_underscore {
        return None;
    }
    if prefix == 4 && segments < 2 {
        return None;
    }
    while letters
        .get(at)
        .is_some_and(|c| !c.is_whitespace() && *c != '<')
    {
        at += 1;
    }
    while let Some(&last) = letters.get(at.wrapping_sub(1)) {
        if at <= from + prefix {
            break;
        }
        if matches!(
            last,
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"'
        ) {
            at -= 1;
            continue;
        }
        if last == ')' {
            let opens = letters[from..at].iter().filter(|c| **c == '(').count();
            let closes = letters[from..at].iter().filter(|c| **c == ')').count();
            if closes > opens {
                at -= 1;
                continue;
            }
        }
        if last == ';' {
            let mut back = at - 1;
            while back > from && letters[back - 1].is_ascii_alphanumeric() {
                back -= 1;
            }
            if back > from && letters[back - 1] == '&' {
                at = back - 1;
                continue;
            }
        }
        break;
    }
    Some(at)
}

fn email_around(letters: &[char], at: usize) -> Option<(usize, usize)> {
    let mut start = at;
    while start > 0
        && letters
            .get(start - 1)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        start -= 1;
    }
    if start == at {
        return None;
    }
    let mut end = at + 1;
    let mut dots = 0;
    while letters
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        if letters[end] == '.' {
            dots += 1;
        }
        end += 1;
    }
    while end > at + 1 && letters[end - 1] == '.' {
        end -= 1;
        dots -= 1;
    }
    if dots == 0 || end == at + 1 {
        return None;
    }
    let last = letters[end - 1];
    if last == '-' || last == '_' {
        return None;
    }
    Some((start, end))
}
