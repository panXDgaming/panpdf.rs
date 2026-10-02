use std::collections::HashMap;

use super::inlines::{self, Reference};
use super::tree::{Align, Block, Inline, List};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Options {
    pub gfm: bool,
    pub extras: bool,
}

impl Options {
    pub const COMMONMARK: Self = Self {
        gfm: false,
        extras: false,
    };
    pub const GFM: Self = Self {
        gfm: true,
        extras: false,
    };
    pub const ALL: Self = Self {
        gfm: true,
        extras: true,
    };
}

#[must_use]
pub fn blocks(text: &str) -> Vec<Block> {
    blocks_with(text, Options::ALL)
}

#[must_use]
pub fn blocks_with(text: &str, options: Options) -> Vec<Block> {
    let mut parser = Parser::new(options);
    let text = text.replace('\0', "\u{FFFD}");
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    for line in lines {
        if parser.nodes.len() > MOST_NODES {
            break;
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        for piece in line.split('\r') {
            parser.line(piece);
        }
    }
    parser.finish()
}

const MOST_NODES: usize = 200_000;

const MOST_NESTING: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListData {
    ordered: bool,
    bullet: char,
    start: u64,
    delimiter: char,
    padding: usize,
    marker_offset: usize,
}

#[derive(Clone, Debug)]
struct Fence {
    letter: char,
    length: usize,
    offset: usize,
}

#[derive(Clone, Debug)]
enum Kind {
    Document,
    Quote,
    List {
        data: ListData,
        tight: bool,
    },
    Item(ListData),
    FootnoteDefinition(String),
    Heading(u8),
    Break,
    Code {
        fence: Option<Fence>,
        info: String,
    },
    Html(u8),
    Paragraph,
    Table {
        align: Vec<Align>,
        head: Vec<String>,
    },
}

impl Kind {
    const fn is_paragraph(&self) -> bool {
        matches!(self, Self::Paragraph)
    }

    const fn accepts_lines(&self) -> bool {
        matches!(
            self,
            Self::Paragraph | Self::Code { .. } | Self::Html(_) | Self::Table { .. }
        )
    }

    const fn can_contain(&self, child: &Self) -> bool {
        match self {
            Self::Document | Self::Quote | Self::Item(_) | Self::FootnoteDefinition(_) => {
                !matches!(child, Self::Item(_))
            }
            Self::List { .. } => matches!(child, Self::Item(_)),
            _ => false,
        }
    }
}

struct Node {
    kind: Kind,
    parent: usize,
    children: Vec<usize>,
    open: bool,
    content: String,
    last_line_blank: bool,
    last_line_checked: bool,
    start_line: usize,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "the reference parser's own state, flag for flag"
)]
struct Parser {
    options: Options,
    nodes: Vec<Node>,
    tip: usize,
    old_tip: usize,
    line_number: usize,
    line: Vec<char>,
    offset: usize,
    column: usize,
    next_nonspace: usize,
    next_nonspace_column: usize,
    indent: usize,
    indented: bool,
    blank: bool,
    partially_consumed_tab: bool,
    all_closed: bool,
    last_matched_container: usize,
    references: HashMap<String, Reference>,
    footnotes: Vec<(String, usize)>,
    rows: HashMap<usize, Vec<Vec<String>>>,
}

enum Continued {
    Matched,
    Failed,
    Consumed,
}

enum Started {
    None,
    Container,
    Leaf,
}

fn space_or_tab(letter: Option<char>) -> bool {
    matches!(letter, Some(' ' | '\t'))
}

impl Parser {
    fn new(options: Options) -> Self {
        Self {
            options,
            nodes: vec![Node {
                kind: Kind::Document,
                parent: 0,
                children: Vec::new(),
                open: true,
                content: String::new(),
                last_line_blank: false,
                last_line_checked: false,
                start_line: 0,
            }],
            tip: 0,
            old_tip: 0,
            line_number: 0,
            line: Vec::new(),
            offset: 0,
            column: 0,
            next_nonspace: 0,
            next_nonspace_column: 0,
            indent: 0,
            indented: false,
            blank: false,
            partially_consumed_tab: false,
            all_closed: true,
            last_matched_container: 0,
            references: HashMap::new(),
            footnotes: Vec::new(),
            rows: HashMap::new(),
        }
    }

    fn peek(&self, at: usize) -> Option<char> {
        self.line.get(at).copied()
    }

    fn rest(&self, from: usize) -> String {
        self.line
            .get(from..)
            .map_or_else(String::new, |rest| rest.iter().collect())
    }

    fn depth_of(&self, mut node: usize) -> usize {
        let mut depth = 0;
        while node != 0 {
            node = self.nodes[node].parent;
            depth += 1;
        }
        depth
    }

    fn last_child(&self, node: usize) -> Option<usize> {
        self.nodes[node].children.last().copied()
    }

    fn find_next_nonspace(&mut self) {
        let mut at = self.offset;
        let mut columns = self.column;
        while let Some(letter) = self.peek(at) {
            match letter {
                ' ' => {
                    at += 1;
                    columns += 1;
                }
                '\t' => {
                    at += 1;
                    columns += 4 - columns % 4;
                }
                _ => break,
            }
        }
        self.blank = self.peek(at).is_none();
        self.next_nonspace = at;
        self.next_nonspace_column = columns;
        self.indent = columns - self.column;
        self.indented = self.indent >= 4;
    }

    fn advance_next_nonspace(&mut self) {
        self.offset = self.next_nonspace;
        self.column = self.next_nonspace_column;
        self.partially_consumed_tab = false;
    }

    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        while count > 0 {
            let Some(letter) = self.peek(self.offset) else {
                break;
            };
            if letter == '\t' {
                let to_tab = 4 - self.column % 4;
                if columns {
                    self.partially_consumed_tab = to_tab > count;
                    let advance = to_tab.min(count);
                    self.column += advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += 1;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn add_line(&mut self) {
        if self.partially_consumed_tab {
            self.offset += 1;
            let to_tab = 4 - self.column % 4;
            self.nodes[self.tip].content.push_str(&" ".repeat(to_tab));
        }
        let rest = self.rest(self.offset);
        let tip = &mut self.nodes[self.tip];
        tip.content.push_str(&rest);
        tip.content.push('\n');
    }

    fn add_child(&mut self, kind: Kind) -> usize {
        while !self.nodes[self.tip].kind.can_contain(&kind) {
            self.finalize(self.tip);
        }
        let node = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: self.tip,
            children: Vec::new(),
            open: true,
            content: String::new(),
            last_line_blank: false,
            last_line_checked: false,
            start_line: self.line_number,
        });
        self.nodes[self.tip].children.push(node);
        self.tip = node;
        node
    }

    fn unlink(&mut self, node: usize) {
        let parent = self.nodes[node].parent;
        self.nodes[parent].children.retain(|child| *child != node);
    }

    fn close_unmatched_blocks(&mut self) {
        if !self.all_closed {
            while self.old_tip != self.last_matched_container {
                let parent = self.nodes[self.old_tip].parent;
                self.finalize(self.old_tip);
                self.old_tip = parent;
            }
            self.all_closed = true;
        }
    }

    fn line(&mut self, text: &str) {
        self.line = text.chars().collect();
        self.line_number += 1;
        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;
        self.old_tip = self.tip;
        let mut container = 0;
        while let Some(last) = self.last_child(container) {
            if !self.nodes[last].open {
                break;
            }
            container = last;
            self.find_next_nonspace();
            match self.continues(container) {
                Continued::Matched => {}
                Continued::Failed => {
                    container = self.nodes[container].parent;
                    break;
                }
                Continued::Consumed => return,
            }
        }
        self.all_closed = container == self.old_tip;
        self.last_matched_container = container;
        let mut matched_leaf = {
            let kind = &self.nodes[container].kind;
            !matches!(kind, Kind::Paragraph | Kind::Table { .. }) && kind.accepts_lines()
        };
        while !matched_leaf {
            self.find_next_nonspace();
            if self.depth_of(container) >= MOST_NESTING {
                self.advance_next_nonspace();
                break;
            }
            match self.start(container) {
                Started::None => {
                    self.advance_next_nonspace();
                    break;
                }
                Started::Container => container = self.tip,
                Started::Leaf => {
                    container = self.tip;
                    matched_leaf = true;
                }
            }
        }
        if !self.all_closed && !self.blank && self.nodes[self.tip].kind.is_paragraph() {
            self.add_line();
            return;
        }
        self.close_unmatched_blocks();
        if self.blank
            && let Some(last) = self.last_child(container)
        {
            self.nodes[last].last_line_blank = true;
        }
        let kind = self.nodes[container].kind.clone();
        let last_line_blank = self.blank
            && !(matches!(kind, Kind::Quote)
                || matches!(kind, Kind::Code { fence: Some(_), .. })
                || (matches!(kind, Kind::Item(_))
                    && self.nodes[container].children.is_empty()
                    && self.nodes[container].start_line == self.line_number));
        let mut climbing = container;
        loop {
            self.nodes[climbing].last_line_blank = last_line_blank;
            if climbing == 0 {
                break;
            }
            climbing = self.nodes[climbing].parent;
        }
        match kind {
            Kind::Table { .. } => {
                if self.nodes[container].start_line != self.line_number && !self.blank {
                    let row = cells(&self.rest(self.next_nonspace));
                    self.rows.entry(container).or_default().push(row);
                }
            }
            kind if kind.accepts_lines() => {
                self.add_line();
                if let Kind::Html(html) = kind
                    && (1..=5).contains(&html)
                    && html_block_ends(html, &self.rest(self.offset))
                {
                    self.finalize(container);
                }
            }
            _ => {
                if self.offset < self.line.len() && !self.blank {
                    self.add_child(Kind::Paragraph);
                    self.advance_next_nonspace();
                    self.add_line();
                }
            }
        }
    }

    fn continues(&mut self, container: usize) -> Continued {
        let kind = self.nodes[container].kind.clone();
        match kind {
            Kind::Document | Kind::List { .. } => Continued::Matched,
            Kind::Quote => {
                if !self.indented && self.peek(self.next_nonspace) == Some('>') {
                    self.advance_next_nonspace();
                    self.advance_offset(1, false);
                    if space_or_tab(self.peek(self.offset)) {
                        self.advance_offset(1, true);
                    }
                    Continued::Matched
                } else {
                    Continued::Failed
                }
            }
            Kind::Item(data) => {
                if self.blank {
                    if self.nodes[container].children.is_empty() {
                        return Continued::Failed;
                    }
                    self.advance_next_nonspace();
                } else if self.indent >= data.marker_offset + data.padding {
                    self.advance_offset(data.marker_offset + data.padding, true);
                } else {
                    return Continued::Failed;
                }
                Continued::Matched
            }
            Kind::FootnoteDefinition(_) => {
                if self.blank {
                    if self.nodes[container].children.is_empty() {
                        return Continued::Failed;
                    }
                    self.advance_next_nonspace();
                } else if self.indent >= 4 {
                    self.advance_offset(4, true);
                } else {
                    return Continued::Failed;
                }
                Continued::Matched
            }
            Kind::Heading(_) | Kind::Break => Continued::Failed,
            Kind::Code { fence, .. } => {
                if let Some(fence) = fence {
                    let closes = self.indent <= 3
                        && self.peek(self.next_nonspace) == Some(fence.letter)
                        && {
                            let rest = self.rest(self.next_nonspace);
                            let run = rest.chars().take_while(|c| *c == fence.letter).count();
                            run >= fence.length
                                && rest[run * fence.letter.len_utf8()..]
                                    .chars()
                                    .all(|c| c == ' ' || c == '\t')
                        };
                    if closes {
                        self.last_line_length_close(container);
                        return Continued::Consumed;
                    }
                    let mut skip = fence.offset;
                    while skip > 0 && space_or_tab(self.peek(self.offset)) {
                        self.advance_offset(1, true);
                        skip -= 1;
                    }
                } else if self.indent >= 4 {
                    self.advance_offset(4, true);
                } else if self.blank {
                    self.advance_next_nonspace();
                } else {
                    return Continued::Failed;
                }
                Continued::Matched
            }
            Kind::Html(html) => {
                if self.blank && (html == 6 || html == 7) {
                    Continued::Failed
                } else {
                    Continued::Matched
                }
            }
            Kind::Paragraph | Kind::Table { .. } => {
                if self.blank {
                    Continued::Failed
                } else {
                    Continued::Matched
                }
            }
        }
    }

    fn last_line_length_close(&mut self, container: usize) {
        self.finalize(container);
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per kind of block, as the spec lists them"
    )]
    fn start(&mut self, container: usize) -> Started {
        let first = self.peek(self.next_nonspace);
        let rest = self.rest(self.next_nonspace);
        let container_is_paragraph = self.nodes[container].kind.is_paragraph();
        if !self.indented && first == Some('>') {
            self.advance_next_nonspace();
            self.advance_offset(1, false);
            if space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            self.close_unmatched_blocks();
            self.add_child(Kind::Quote);
            return Started::Container;
        }
        if !self.indented && first == Some('#') {
            let hashes = rest.chars().take_while(|c| *c == '#').count();
            let after = rest.chars().nth(hashes);
            if (1..=6).contains(&hashes) && (after.is_none() || matches!(after, Some(' ' | '\t'))) {
                self.advance_next_nonspace();
                self.advance_offset(hashes, false);
                self.close_unmatched_blocks();
                let node = self.add_child(Kind::Heading(u8::try_from(hashes).unwrap_or(6)));
                let text = self.rest(self.offset);
                self.nodes[node].content = atx_content(&text);
                self.offset = self.line.len();
                return Started::Leaf;
            }
        }
        if !self.indented && matches!(first, Some('`' | '~')) {
            let letter = first.unwrap_or('`');
            let length = rest.chars().take_while(|c| *c == letter).count();
            let info: String = rest.chars().skip(length).collect();
            if length >= 3 && !(letter == '`' && info.contains('`')) {
                self.close_unmatched_blocks();
                let offset = self.indent;
                self.add_child(Kind::Code {
                    fence: Some(Fence {
                        letter,
                        length,
                        offset,
                    }),
                    info: String::new(),
                });
                self.advance_next_nonspace();
                self.advance_offset(length, false);
                return Started::Leaf;
            }
        }
        if !self.indented && first == Some('<') {
            for kind in 1..=7_u8 {
                let lazy =
                    !self.all_closed && !self.blank && self.nodes[self.tip].kind.is_paragraph();
                if html_block_starts(kind, &rest)
                    && (kind < 7 || (!container_is_paragraph && !lazy))
                {
                    self.close_unmatched_blocks();
                    self.add_child(Kind::Html(kind));
                    return Started::Leaf;
                }
            }
        }
        if !self.indented && container_is_paragraph && matches!(first, Some('=' | '-')) {
            let letter = first.unwrap_or('=');
            let trimmed = rest.trim_end_matches([' ', '\t']);
            if trimmed.chars().all(|c| c == letter) {
                self.close_unmatched_blocks();
                self.take_references(container);
                if !self.nodes[container].content.is_empty() {
                    let content = std::mem::take(&mut self.nodes[container].content);
                    let parent = self.nodes[container].parent;
                    let node = self.nodes.len();
                    self.nodes.push(Node {
                        kind: Kind::Heading(if letter == '=' { 1 } else { 2 }),
                        parent,
                        children: Vec::new(),
                        open: true,
                        content,
                        last_line_blank: false,
                        last_line_checked: false,
                        start_line: self.nodes[container].start_line,
                    });
                    let children = &mut self.nodes[parent].children;
                    if let Some(at) = children.iter().position(|child| *child == container) {
                        children[at] = node;
                    }
                    self.tip = node;
                    self.offset = self.line.len();
                    return Started::Leaf;
                }
            }
        }
        if !self.indented && thematic_break(&rest) {
            self.close_unmatched_blocks();
            self.add_child(Kind::Break);
            self.offset = self.line.len();
            return Started::Leaf;
        }
        if self.options.extras
            && !self.indented
            && first == Some('[')
            && let Some((label, used)) = footnote_label(&rest)
        {
            self.close_unmatched_blocks();
            self.add_child(Kind::FootnoteDefinition(label));
            self.advance_next_nonspace();
            self.advance_offset(used, false);
            if space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
            return Started::Container;
        }
        if (!self.indented || matches!(self.nodes[container].kind, Kind::List { .. }))
            && let Some(data) = self.list_marker(container)
        {
            self.close_unmatched_blocks();
            let same_list = match &self.nodes[self.tip].kind {
                Kind::List { data: open, .. } => lists_match(open, &data),
                _ => false,
            };
            if !same_list {
                self.add_child(Kind::List { data, tight: true });
            }
            self.add_child(Kind::Item(data));
            return Started::Container;
        }
        if self.indented && !self.nodes[self.tip].kind.is_paragraph() && !self.blank {
            self.advance_offset(4, true);
            self.close_unmatched_blocks();
            self.add_child(Kind::Code {
                fence: None,
                info: String::new(),
            });
            return Started::Leaf;
        }
        if self.options.gfm
            && !self.indented
            && container_is_paragraph
            && let Some(align) = delimiter_row(&rest)
        {
            let content = self.nodes[container].content.clone();
            let body = content.strip_suffix('\n').unwrap_or(&content);
            let (before, head_line) = body.rsplit_once('\n').map_or(("", body), |(a, b)| (a, b));
            let head = cells(head_line);
            if head.len() == align.len() && (head_line.contains('|') || rest.contains('|')) {
                self.close_unmatched_blocks();
                let before = before.to_owned();
                let parent = self.nodes[container].parent;
                if before.trim().is_empty() {
                    self.unlink(container);
                    self.tip = parent;
                } else {
                    self.nodes[container].content = format!("{before}\n");
                    self.finalize(container);
                    self.tip = parent;
                }
                self.add_child(Kind::Table { align, head });
                self.offset = self.line.len();
                return Started::Leaf;
            }
        }
        Started::None
    }

    fn list_marker(&mut self, container: usize) -> Option<ListData> {
        if self.indent >= 4 {
            return None;
        }
        let rest: Vec<char> = self.line[self.next_nonspace..].to_vec();
        let first = *rest.first()?;
        let container_is_paragraph = self.nodes[container].kind.is_paragraph();
        let (mut data, length) = if matches!(first, '*' | '+' | '-') {
            (
                ListData {
                    ordered: false,
                    bullet: first,
                    start: 1,
                    delimiter: ' ',
                    padding: 0,
                    marker_offset: self.indent,
                },
                1,
            )
        } else {
            let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            let delimiter = *rest.get(digits)?;
            if delimiter != '.' && delimiter != ')' {
                return None;
            }
            let start: u64 = rest[..digits].iter().collect::<String>().parse().ok()?;
            if container_is_paragraph && start != 1 {
                return None;
            }
            (
                ListData {
                    ordered: true,
                    bullet: ' ',
                    start,
                    delimiter,
                    padding: 0,
                    marker_offset: self.indent,
                },
                digits + 1,
            )
        };
        let next = rest.get(length).copied();
        if !(next.is_none() || matches!(next, Some(' ' | '\t'))) {
            return None;
        }
        if container_is_paragraph && rest[length..].iter().all(|c| *c == ' ' || *c == '\t') {
            return None;
        }
        self.advance_next_nonspace();
        self.advance_offset(length, true);
        let spaces_start_column = self.column;
        let spaces_start_offset = self.offset;
        loop {
            self.advance_offset(1, true);
            let next = self.peek(self.offset);
            if !(self.column - spaces_start_column < 5 && space_or_tab(next)) {
                break;
            }
        }
        let blank_item = self.peek(self.offset).is_none();
        let spaces_after_marker = self.column - spaces_start_column;
        if !(1..5).contains(&spaces_after_marker) || blank_item {
            data.padding = length + 1;
            self.column = spaces_start_column;
            self.offset = spaces_start_offset;
            self.partially_consumed_tab = false;
            if space_or_tab(self.peek(self.offset)) {
                self.advance_offset(1, true);
            }
        } else {
            data.padding = length + spaces_after_marker;
        }
        Some(data)
    }

    fn take_references(&mut self, paragraph: usize) {
        let content = std::mem::take(&mut self.nodes[paragraph].content);
        if !content.starts_with('[') {
            self.nodes[paragraph].content = content;
            return;
        }
        let letters: Vec<char> = content.chars().collect();
        let mut at = 0;
        while letters.get(at) == Some(&'[') {
            let used = inlines::reference_at(&letters, at, &mut self.references);
            if used == 0 {
                break;
            }
            at += used;
        }
        let mut rest: String = letters[at..].iter().collect();
        if rest.trim_matches([' ', '\t', '\n']).is_empty() {
            rest.clear();
        }
        self.nodes[paragraph].content = rest;
    }

    fn finalize(&mut self, block: usize) {
        let parent = self.nodes[block].parent;
        self.nodes[block].open = false;
        match self.nodes[block].kind.clone() {
            Kind::Paragraph => {
                self.take_references(block);
                if self.nodes[block].content.is_empty() {
                    self.unlink(block);
                }
            }
            Kind::Code { fence, .. } => {
                let content = std::mem::take(&mut self.nodes[block].content);
                if fence.is_some() {
                    let (first, rest) = content.split_once('\n').unwrap_or((&content, ""));
                    let info = inlines::unescape(first.trim_matches([' ', '\t']));
                    self.nodes[block].kind = Kind::Code { fence, info };
                    rest.clone_into(&mut self.nodes[block].content);
                } else {
                    let mut lines: Vec<&str> = content.split('\n').collect();
                    while lines
                        .last()
                        .is_some_and(|line| line.trim_matches([' ', '\t']).is_empty())
                    {
                        lines.pop();
                    }
                    let mut text = lines.join("\n");
                    text.push('\n');
                    self.nodes[block].content = text;
                }
            }
            Kind::Html(_) => {
                let content = &mut self.nodes[block].content;
                while content.ends_with('\n') {
                    content.pop();
                }
            }
            Kind::List { data, .. } => {
                let tight = self.tight(block);
                self.nodes[block].kind = Kind::List { data, tight };
            }
            Kind::FootnoteDefinition(label) => {
                let normal = inlines::normalize_label(&label);
                if !self.footnotes.iter().any(|(known, _)| *known == normal) {
                    self.footnotes.push((normal, block));
                }
                self.unlink(block);
            }
            _ => {}
        }
        self.tip = parent;
    }

    fn ends_with_blank_line(&mut self, mut block: usize) -> bool {
        loop {
            if self.nodes[block].last_line_blank {
                return true;
            }
            let is_list = matches!(self.nodes[block].kind, Kind::List { .. } | Kind::Item(_));
            if !self.nodes[block].last_line_checked && is_list {
                self.nodes[block].last_line_checked = true;
                match self.last_child(block) {
                    Some(last) => block = last,
                    None => return false,
                }
            } else {
                self.nodes[block].last_line_checked = true;
                return false;
            }
        }
    }

    fn tight(&mut self, list: usize) -> bool {
        let items = self.nodes[list].children.clone();
        for (at, item) in items.iter().enumerate() {
            let has_next = at + 1 < items.len();
            if has_next && self.ends_with_blank_line(*item) {
                return false;
            }
            let children = self.nodes[*item].children.clone();
            for (sub, child) in children.iter().enumerate() {
                let child_has_next = sub + 1 < children.len();
                if child_has_next && self.ends_with_blank_line(*child) {
                    return false;
                }
            }
        }
        true
    }

    fn finish(mut self) -> Vec<Block> {
        while self.tip != 0 {
            self.finalize(self.tip);
        }
        self.finalize(0);
        let mut notes = inlines::Notes::new(
            self.footnotes
                .iter()
                .map(|(label, _)| label.clone())
                .collect(),
        );
        let mut out = self.convert_children(0, &mut notes);
        if self.options.gfm {
            super::present::task_items(&mut out);
        }
        let used = notes.used();
        if !used.is_empty() {
            let mut bodies = Vec::new();
            for label in used {
                if let Some((_, node)) = self.footnotes.iter().find(|(known, _)| *known == label) {
                    let node = *node;
                    bodies.push(self.convert_children(node, &mut notes));
                }
            }
            out.push(Block::Footnotes(bodies));
        }
        out
    }

    fn inline(&self, text: &str, notes: &mut inlines::Notes) -> Vec<Inline> {
        inlines::inlines_with(text, &self.references, self.options, notes)
    }

    fn convert_children(&self, node: usize, notes: &mut inlines::Notes) -> Vec<Block> {
        self.nodes[node]
            .children
            .iter()
            .filter_map(|child| self.convert(*child, notes))
            .collect()
    }

    fn convert(&self, node: usize, notes: &mut inlines::Notes) -> Option<Block> {
        let content = &self.nodes[node].content;
        Some(match &self.nodes[node].kind {
            Kind::Document | Kind::Item(_) | Kind::FootnoteDefinition(_) => return None,
            Kind::Quote => Block::Quote {
                blocks: self.convert_children(node, notes),
            },
            Kind::List { data, tight } => Block::List(List {
                first: data.ordered.then_some(data.start),
                loose: !tight,
                items: self.nodes[node]
                    .children
                    .iter()
                    .map(|item| self.convert_children(*item, notes))
                    .collect(),
            }),
            Kind::Heading(level) => Block::Heading {
                level: *level,
                inlines: self.inline(content.trim_matches([' ', '\t', '\n']), notes),
            },
            Kind::Break => Block::Break,
            Kind::Code { info, .. } => Block::Code {
                info: info.clone(),
                text: content.clone(),
            },
            Kind::Html(_) => Block::Html(content.clone()),
            Kind::Paragraph => Block::Paragraph {
                inlines: self.inline(content.trim_matches([' ', '\t', '\n']), notes),
            },
            Kind::Table { align, head } => Block::Table {
                head: head.iter().map(|cell| self.inline(cell, notes)).collect(),
                rows: self
                    .rows
                    .get(&node)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .map(|row| {
                        let mut row: Vec<Vec<Inline>> = row
                            .iter()
                            .take(head.len())
                            .map(|cell| self.inline(cell, notes))
                            .collect();
                        row.resize(head.len(), Vec::new());
                        row
                    })
                    .collect(),
                align: align.clone(),
            },
        })
    }
}

fn lists_match(one: &ListData, other: &ListData) -> bool {
    one.ordered == other.ordered && one.delimiter == other.delimiter && one.bullet == other.bullet
}

fn atx_content(text: &str) -> String {
    let trimmed = text.trim_end_matches([' ', '\t']);
    let without = trimmed.trim_end_matches('#');
    if without.len() == trimmed.len() {
        return trimmed.trim_start_matches([' ', '\t']).to_owned();
    }
    if without.trim_matches([' ', '\t']).is_empty() {
        return String::new();
    }
    if without.ends_with([' ', '\t']) {
        return without.trim_matches([' ', '\t']).to_owned();
    }
    trimmed.trim_start_matches([' ', '\t']).to_owned()
}

fn thematic_break(rest: &str) -> bool {
    let mut letter = None;
    let mut count = 0;
    for c in rest.chars() {
        match c {
            ' ' | '\t' => {}
            '*' | '-' | '_' => {
                if letter.is_some_and(|l| l != c) {
                    return false;
                }
                letter = Some(c);
                count += 1;
            }
            _ => return false,
        }
    }
    count >= 3
}

fn footnote_label(rest: &str) -> Option<(String, usize)> {
    let inside = rest.strip_prefix("[^")?;
    let close = inside.find(']')?;
    let label = &inside[..close];
    if label.is_empty() || label.chars().any(char::is_whitespace) || label.contains('[') {
        return None;
    }
    if !inside[close + 1..].starts_with(':') {
        return None;
    }
    Some((label.to_owned(), 2 + label.chars().count() + 2))
}

const BLOCK_TAGS: [&str; 62] = [
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

fn html_block_starts(kind: u8, rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    match kind {
        1 => ["<script", "<pre", "<style", "<textarea"]
            .iter()
            .any(|tag| {
                lower.starts_with(tag)
                    && matches!(
                        lower[tag.len()..].chars().next(),
                        None | Some(' ' | '\t' | '>')
                    )
            }),
        2 => rest.starts_with("<!--"),
        3 => rest.starts_with("<?"),
        4 => {
            rest.starts_with("<!")
                && rest[2..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
        }
        5 => rest.starts_with("<![CDATA["),
        6 => {
            let name_start = if lower.starts_with("</") { 2 } else { 1 };
            let name: String = lower[name_start..]
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            let after = &lower[name_start + name.len()..];
            BLOCK_TAGS.contains(&name.as_str())
                && (after.is_empty()
                    || after.starts_with([' ', '\t', '>'])
                    || after.starts_with("/>"))
        }
        7 => {
            let Some(used) = inlines::open_or_closing_tag(rest) else {
                return false;
            };
            let name: String = lower
                .trim_start_matches(['<', '/'])
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            !["script", "style", "pre", "textarea"].contains(&name.as_str())
                && rest[used..].chars().all(|c| c == ' ' || c == '\t')
        }
        _ => false,
    }
}

fn html_block_ends(kind: u8, rest: &str) -> bool {
    let lower = rest.to_ascii_lowercase();
    match kind {
        1 => ["</script>", "</pre>", "</style>", "</textarea>"]
            .iter()
            .any(|end| lower.contains(end)),
        2 => rest.contains("-->"),
        3 => rest.contains("?>"),
        4 => rest.contains('>'),
        5 => rest.contains("]]>"),
        _ => false,
    }
}

fn cells(line: &str) -> Vec<String> {
    let mut bare = line.trim_matches([' ', '\t']);
    if let Some(rest) = bare.strip_prefix('|') {
        bare = rest;
    }
    if bare.ends_with('|') && !bare.ends_with("\\|") {
        bare = &bare[..bare.len() - 1];
    }
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut letters = bare.chars().peekable();
    while let Some(letter) = letters.next() {
        match letter {
            '\\' if letters.peek() == Some(&'|') => {
                cell.push('|');
                letters.next();
            }
            '|' => out.push(
                std::mem::take(&mut cell)
                    .trim_matches([' ', '\t'])
                    .to_owned(),
            ),
            letter => cell.push(letter),
        }
    }
    out.push(cell.trim_matches([' ', '\t']).to_owned());
    out
}

fn delimiter_row(line: &str) -> Option<Vec<Align>> {
    let trimmed = line.trim_matches([' ', '\t']);
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|c| matches!(c, '|' | ':' | '-' | ' ' | '\t'))
    {
        return None;
    }
    if !trimmed.contains('-') {
        return None;
    }
    let mut out = Vec::new();
    for cell in cells(trimmed) {
        let bare = cell.trim();
        let left = bare.starts_with(':');
        let right = bare.len() > 1 && bare.ends_with(':');
        let middle = bare.trim_matches(':');
        if middle.is_empty() || !middle.chars().all(|letter| letter == '-') {
            return None;
        }
        out.push(match (left, right) {
            (true, true) => Align::Middle,
            (false, true) => Align::End,
            (true, false) => Align::Left,
            (false, false) => Align::Start,
        });
    }
    (!out.is_empty()).then_some(out)
}
