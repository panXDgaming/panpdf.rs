use crate::unicode_data::{BIDI_CLASS, BRACKETS, MIRROR};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    L,
    R,
    AL,
    EN,
    ES,
    ET,
    AN,
    CS,
    NSM,
    BN,
    B,
    S,
    WS,
    ON,
    LRE,
    LRO,
    RLE,
    RLO,
    PDF,
    LRI,
    RLI,
    FSI,
    PDI,
}

const CLASSES: [Class; 23] = [
    Class::L,
    Class::R,
    Class::AL,
    Class::EN,
    Class::ES,
    Class::ET,
    Class::AN,
    Class::CS,
    Class::NSM,
    Class::BN,
    Class::B,
    Class::S,
    Class::WS,
    Class::ON,
    Class::LRE,
    Class::LRO,
    Class::RLE,
    Class::RLO,
    Class::PDF,
    Class::LRI,
    Class::RLI,
    Class::FSI,
    Class::PDI,
];

#[must_use]
pub fn class(c: char) -> Class {
    let key = u32::from(c) << 5 | 31;
    let at = BIDI_CLASS.partition_point(|&entry| entry <= key);
    let entry = BIDI_CLASS[at.saturating_sub(1)];
    CLASSES[(entry & 31) as usize]
}

#[must_use]
pub fn is_rtl(c: char) -> bool {
    matches!(class(c), Class::R | Class::AL)
}

#[must_use]
pub fn mirror(c: char) -> Option<char> {
    let code = u32::from(c);
    MIRROR
        .binary_search_by_key(&code, |&(from, _)| from)
        .ok()
        .and_then(|at| char::from_u32(MIRROR[at].1))
}

fn bracket(c: char) -> Option<(char, bool)> {
    let code = u32::from(c);
    let at = BRACKETS
        .binary_search_by_key(&code, |&(from, _, _)| from)
        .ok()?;
    let (_, pair, opens) = BRACKETS[at];
    Some((char::from_u32(pair)?, opens))
}

fn canonical(c: char) -> char {
    match c {
        '\u{2329}' => '\u{3008}',
        '\u{232A}' => '\u{3009}',
        _ => c,
    }
}

const MAX_DEPTH: u8 = 125;

fn removed(class: Class) -> bool {
    matches!(
        class,
        Class::RLE | Class::LRE | Class::RLO | Class::LRO | Class::PDF | Class::BN
    )
}

fn isolate_initiator(class: Class) -> bool {
    matches!(class, Class::LRI | Class::RLI | Class::FSI)
}

fn neutral(class: Class) -> bool {
    matches!(
        class,
        Class::B
            | Class::S
            | Class::WS
            | Class::ON
            | Class::LRI
            | Class::RLI
            | Class::FSI
            | Class::PDI
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    pub level: u8,
    pub levels: Vec<u8>,
    pub classes: Vec<Class>,
}

impl Paragraph {
    #[must_use]
    pub fn line_levels(&self, range: std::ops::Range<usize>) -> Vec<u8> {
        let mut out = self.levels[range.clone()].to_vec();
        let classes = &self.classes[range];
        let trailing = |c: Class| {
            matches!(
                c,
                Class::WS | Class::FSI | Class::LRI | Class::RLI | Class::PDI
            ) || removed(c)
        };
        for index in 0..out.len() {
            if matches!(classes[index], Class::S | Class::B) {
                out[index] = self.level;
                let mut k = index;
                while k > 0 && trailing(classes[k - 1]) {
                    k -= 1;
                    out[k] = self.level;
                }
            }
        }
        let mut k = out.len();
        while k > 0 && trailing(classes[k - 1]) {
            k -= 1;
            out[k] = self.level;
        }
        out
    }
}

fn first_strong(
    classes: &[Class],
    matching: &[Option<usize>],
    from: usize,
    to: usize,
) -> Option<u8> {
    let mut i = from;
    while i < to {
        match classes[i] {
            Class::L => return Some(0),
            Class::R | Class::AL => return Some(1),
            Class::B => return None,
            c if isolate_initiator(c) => i = matching[i]?,
            _ => {}
        }
        i += 1;
    }
    None
}

fn matching_pdis(classes: &[Class]) -> Vec<Option<usize>> {
    let mut out = vec![None; classes.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, &c) in classes.iter().enumerate() {
        if isolate_initiator(c) {
            open.push(i);
        } else if c == Class::PDI {
            if let Some(start) = open.pop() {
                out[start] = Some(i);
            }
        } else if c == Class::B {
            open.clear();
        }
    }
    out
}

#[must_use]
pub fn paragraph(chars: &[char], base: Option<u8>) -> Paragraph {
    let original: Vec<Class> = chars.iter().map(|&c| class(c)).collect();
    let n = chars.len();
    let matching = matching_pdis(&original);
    let level = base
        .map(|b| b.min(1))
        .unwrap_or_else(|| first_strong(&original, &matching, 0, n).unwrap_or(0));
    let mut classes = original.clone();
    let mut levels = explicit(&mut classes, &matching, level);

    let explicit_levels = levels.clone();
    let kept: Vec<usize> = (0..n).filter(|&i| !removed(original[i])).collect();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for &i in &kept {
        match runs.last_mut() {
            Some(run) if levels[*run.last().unwrap_or(&i)] == levels[i] => run.push(i),
            _ => runs.push(vec![i]),
        }
    }
    let mut run_of = vec![usize::MAX; n];
    for (r, run) in runs.iter().enumerate() {
        for &i in run {
            run_of[i] = r;
        }
    }
    let matched_pdi: Vec<bool> = {
        let mut out = vec![false; n];
        for pdi in matching.iter().flatten() {
            out[*pdi] = true;
        }
        out
    };
    for run in &runs {
        let first = run[0];
        if original[first] == Class::PDI && matched_pdi[first] {
            continue;
        }
        let mut sequence: Vec<usize> = run.clone();
        loop {
            let last = *sequence.last().unwrap_or(&first);
            let next = if isolate_initiator(original[last]) {
                matching[last]
            } else {
                None
            };
            match next {
                Some(pdi) if run_of[pdi] != usize::MAX => {
                    sequence.extend(runs[run_of[pdi]].iter().copied());
                }
                _ => break,
            }
        }
        resolve_sequence(
            &sequence,
            chars,
            &original,
            &mut classes,
            &explicit_levels,
            level,
            n,
        );
        implicit(&sequence, &classes, &mut levels);
    }
    let mut previous = level;
    for i in 0..n {
        if removed(original[i]) {
            levels[i] = previous;
        } else {
            previous = levels[i];
        }
    }
    Paragraph {
        level,
        levels,
        classes: original,
    }
}

#[derive(Clone, Copy)]
struct Status {
    level: u8,
    over: Option<Class>,
    isolate: bool,
}

fn explicit(classes: &mut [Class], matching: &[Option<usize>], level: u8) -> Vec<u8> {
    let n = classes.len();
    let mut levels = vec![level; n];
    let mut stack = vec![Status {
        level,
        over: None,
        isolate: false,
    }];
    let mut overflow_isolates = 0_usize;
    let mut overflow_embeddings = 0_usize;
    let mut valid_isolates = 0_usize;
    let odd_above = |l: u8| if l.is_multiple_of(2) { l + 1 } else { l + 2 };
    let even_above = |l: u8| if l.is_multiple_of(2) { l + 2 } else { l + 1 };
    for i in 0..n {
        let top = *stack.last().unwrap_or(&Status {
            level,
            over: None,
            isolate: false,
        });
        match classes[i] {
            c @ (Class::RLE | Class::LRE | Class::RLO | Class::LRO) => {
                levels[i] = top.level;
                let next = if matches!(c, Class::RLE | Class::RLO) {
                    odd_above(top.level)
                } else {
                    even_above(top.level)
                };
                if next <= MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0 {
                    stack.push(Status {
                        level: next,
                        over: match c {
                            Class::RLO => Some(Class::R),
                            Class::LRO => Some(Class::L),
                            _ => None,
                        },
                        isolate: false,
                    });
                } else if overflow_isolates == 0 {
                    overflow_embeddings += 1;
                }
            }
            c @ (Class::RLI | Class::LRI | Class::FSI) => {
                levels[i] = top.level;
                if let Some(over) = top.over {
                    classes[i] = over;
                }
                let rtl = match c {
                    Class::RLI => true,
                    Class::LRI => false,
                    _ => {
                        first_strong(classes, matching, i + 1, matching[i].unwrap_or(n)) == Some(1)
                    }
                };
                let next = if rtl {
                    odd_above(top.level)
                } else {
                    even_above(top.level)
                };
                if next <= MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0 {
                    valid_isolates += 1;
                    stack.push(Status {
                        level: next,
                        over: None,
                        isolate: true,
                    });
                } else {
                    overflow_isolates += 1;
                }
            }
            Class::PDI => {
                if overflow_isolates > 0 {
                    overflow_isolates -= 1;
                } else if valid_isolates > 0 {
                    overflow_embeddings = 0;
                    while stack.last().is_some_and(|s| !s.isolate) {
                        stack.pop();
                    }
                    stack.pop();
                    valid_isolates -= 1;
                }
                let top = *stack.last().unwrap_or(&top);
                levels[i] = top.level;
                if let Some(over) = top.over {
                    classes[i] = over;
                }
            }
            Class::PDF => {
                levels[i] = top.level;
                if overflow_isolates > 0 {
                } else if overflow_embeddings > 0 {
                    overflow_embeddings -= 1;
                } else if !top.isolate && stack.len() >= 2 {
                    stack.pop();
                }
            }
            Class::B => levels[i] = level,
            Class::BN => levels[i] = top.level,
            _ => {
                levels[i] = top.level;
                if let Some(over) = top.over {
                    classes[i] = over;
                }
            }
        }
    }
    levels
}

fn direction_of(level: u8) -> Class {
    if level.is_multiple_of(2) {
        Class::L
    } else {
        Class::R
    }
}

fn strong(class: Class) -> Option<Class> {
    match class {
        Class::L => Some(Class::L),
        Class::R | Class::AL | Class::EN | Class::AN => Some(Class::R),
        _ => None,
    }
}

#[allow(clippy::too_many_lines)]
fn resolve_sequence(
    sequence: &[usize],
    chars: &[char],
    original: &[Class],
    classes: &mut [Class],
    levels: &[u8],
    paragraph_level: u8,
    n: usize,
) {
    let first = sequence[0];
    let last = *sequence.last().unwrap_or(&first);
    let level = levels[first];
    let before = (0..first)
        .rev()
        .find(|&i| !removed(original[i]))
        .map_or(paragraph_level, |i| levels[i]);
    let after = if isolate_initiator(original[last]) {
        paragraph_level
    } else {
        (last + 1..n)
            .find(|&i| !removed(original[i]))
            .map_or(paragraph_level, |i| levels[i])
    };
    let sos = direction_of(level.max(before));
    let eos = direction_of(level.max(after));
    let mut t: Vec<Class> = sequence.iter().map(|&i| classes[i]).collect();
    let len = t.len();

    for k in 0..len {
        if t[k] == Class::NSM {
            t[k] = if k == 0 {
                sos
            } else if isolate_initiator(t[k - 1]) || t[k - 1] == Class::PDI {
                Class::ON
            } else {
                t[k - 1]
            };
        }
    }
    let mut last_strong = sos;
    for class in &mut t {
        match *class {
            Class::L | Class::R | Class::AL => last_strong = *class,
            Class::EN if last_strong == Class::AL => *class = Class::AN,
            _ => {}
        }
    }
    for class in &mut t {
        if *class == Class::AL {
            *class = Class::R;
        }
    }
    for k in 1..len.saturating_sub(1) {
        let (a, b) = (t[k - 1], t[k + 1]);
        if t[k] == Class::ES && a == Class::EN && b == Class::EN {
            t[k] = Class::EN;
        } else if t[k] == Class::CS && a == b && matches!(a, Class::EN | Class::AN) {
            t[k] = a;
        }
    }
    let mut k = 0;
    while k < len {
        if t[k] == Class::ET {
            let start = k;
            while k < len && t[k] == Class::ET {
                k += 1;
            }
            let touches =
                (start > 0 && t[start - 1] == Class::EN) || (k < len && t[k] == Class::EN);
            if touches {
                for class in &mut t[start..k] {
                    *class = Class::EN;
                }
            }
        } else {
            k += 1;
        }
    }
    for class in &mut t {
        if matches!(*class, Class::ES | Class::ET | Class::CS) {
            *class = Class::ON;
        }
    }
    let mut last_strong = sos;
    for class in &mut t {
        match *class {
            Class::L | Class::R => last_strong = *class,
            Class::EN if last_strong == Class::L => *class = Class::L,
            _ => {}
        }
    }

    let embedding = direction_of(level);
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut open: Vec<(char, usize)> = Vec::new();
    for k in 0..len {
        if t[k] != Class::ON {
            continue;
        }
        let c = chars[sequence[k]];
        let Some((pair, opens)) = bracket(c) else {
            continue;
        };
        if opens {
            if open.len() == 63 {
                break;
            }
            open.push((canonical(pair), k));
        } else if let Some(at) = open.iter().rposition(|&(want, _)| want == canonical(c)) {
            pairs.push((open[at].1, k));
            open.truncate(at);
        }
    }
    pairs.sort_unstable();
    for (a, b) in pairs {
        let mut found_embedding = false;
        let mut found_opposite = false;
        for class in &t[a + 1..b] {
            match strong(*class) {
                Some(s) if s == embedding => found_embedding = true,
                Some(_) => found_opposite = true,
                None => {}
            }
        }
        let set = if found_embedding {
            Some(embedding)
        } else if found_opposite {
            let preceding = t[..a].iter().rev().find_map(|c| strong(*c)).unwrap_or(sos);
            Some(if preceding == embedding {
                embedding
            } else {
                preceding
            })
        } else {
            None
        };
        if let Some(dir) = set {
            for at in [a, b] {
                t[at] = dir;
                let mut k = at + 1;
                while k < len && original[sequence[k]] == Class::NSM {
                    t[k] = dir;
                    k += 1;
                }
            }
        }
    }

    let mut k = 0;
    while k < len {
        if !neutral(t[k]) {
            k += 1;
            continue;
        }
        let start = k;
        while k < len && neutral(t[k]) {
            k += 1;
        }
        let left = if start == 0 {
            sos
        } else {
            strong(t[start - 1]).unwrap_or(embedding)
        };
        let right = if k == len {
            eos
        } else {
            strong(t[k]).unwrap_or(embedding)
        };
        let dir = if left == right { left } else { embedding };
        for class in &mut t[start..k] {
            *class = dir;
        }
    }
    for (k, &i) in sequence.iter().enumerate() {
        classes[i] = t[k];
    }
}

fn implicit(sequence: &[usize], classes: &[Class], levels: &mut [u8]) {
    for &i in sequence {
        let level = levels[i];
        levels[i] = match (level % 2, classes[i]) {
            (0, Class::R) => level + 1,
            (0, Class::AN | Class::EN) => level + 2,
            (1, Class::L | Class::EN | Class::AN) => level + 1,
            _ => level,
        };
    }
}

#[must_use]
pub fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    let Some(&highest) = levels.iter().max() else {
        return order;
    };
    let lowest_odd = levels
        .iter()
        .copied()
        .filter(|l| l % 2 == 1)
        .min()
        .unwrap_or(highest + 1);
    let mut level = highest;
    while level >= lowest_odd && level > 0 {
        let mut k = 0;
        while k < order.len() {
            if levels[order[k]] >= level {
                let start = k;
                while k < order.len() && levels[order[k]] >= level {
                    k += 1;
                }
                order[start..k].reverse();
            } else {
                k += 1;
            }
        }
        level -= 1;
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_from_the_tables() {
        assert_eq!(class('a'), Class::L);
        assert_eq!(class('\u{05D0}'), Class::R);
        assert_eq!(class('\u{0627}'), Class::AL);
        assert_eq!(class('1'), Class::EN);
        assert_eq!(class('\u{0661}'), Class::AN);
        assert_eq!(class(' '), Class::WS);
        assert_eq!(class('\u{064B}'), Class::NSM);
        assert_eq!(class('\u{060C}'), Class::CS);
        assert_eq!(mirror('('), Some(')'));
        assert_eq!(mirror('a'), None);
    }

    fn visual(text: &str, base: Option<u8>) -> String {
        let chars: Vec<char> = text.chars().collect();
        let p = paragraph(&chars, base);
        let levels = p.line_levels(0..chars.len());
        visual_order(&levels)
            .into_iter()
            .map(|i| {
                let c = chars[i];
                if levels[i] % 2 == 1 {
                    mirror(c).unwrap_or(c)
                } else {
                    c
                }
            })
            .collect()
    }

    #[test]
    fn reorders_lines() {
        assert_eq!(
            visual("abc \u{05D0}\u{05D1}\u{05D2} def", None),
            "abc \u{05D2}\u{05D1}\u{05D0} def"
        );
        assert_eq!(
            visual("\u{05D0}\u{05D1} 12 \u{05D2}", None),
            "\u{05D2} 12 \u{05D1}\u{05D0}"
        );
        assert_eq!(
            visual("\u{05D0} (b) \u{05D1}", None),
            "\u{05D1} (b) \u{05D0}"
        );
        assert_eq!(visual("\u{05D0}(\u{05D1})", None), "(\u{05D1})\u{05D0}");
        assert_eq!(
            visual("D03.8 \u{05D0}\u{05D1}", Some(1)),
            "\u{05D1}\u{05D0} D03.8"
        );
    }

    #[test]
    #[ignore = "reads the Unicode conformance files from BIDI_TEST_DIR"]
    fn conformance() {
        let dir = std::path::PathBuf::from(std::env::var("BIDI_TEST_DIR").expect("BIDI_TEST_DIR"));
        let (pass, fail) = bidi_test(&dir.join("BidiTest.txt"));
        let (pass2, fail2) = character_test(&dir.join("BidiCharacterTest.txt"));
        eprintln!(
            "BidiTest: {pass} pass, {fail} fail; BidiCharacterTest: {pass2} pass, {fail2} fail"
        );
        assert_eq!((fail, fail2), (0, 0));
    }

    fn check(
        chars: &[char],
        base: Option<u8>,
        want_level: Option<u8>,
        want: &[Option<u8>],
        order: &[usize],
    ) -> bool {
        let p = paragraph(chars, base);
        if want_level.is_some_and(|l| l != p.level) {
            return false;
        }
        let levels = p.line_levels(0..chars.len());
        for (k, w) in want.iter().enumerate() {
            if let Some(w) = w
                && levels[k] != *w
            {
                return false;
            }
        }
        let shown: Vec<usize> = visual_order(&levels)
            .into_iter()
            .filter(|&i| want[i].is_some())
            .collect();
        shown == order
    }

    fn bidi_test(path: &std::path::Path) -> (usize, usize) {
        let text = std::fs::read_to_string(path).expect("BidiTest.txt");
        let sample = |c: Class| -> char {
            match c {
                Class::L => 'a',
                Class::R => '\u{05D0}',
                Class::AL => '\u{0627}',
                Class::EN => '1',
                Class::ES => '+',
                Class::ET => '$',
                Class::AN => '\u{0661}',
                Class::CS => ',',
                Class::NSM => '\u{0300}',
                Class::BN => '\u{00AD}',
                Class::B => '\u{2029}',
                Class::S => '\t',
                Class::WS => ' ',
                Class::ON => '!',
                Class::LRE => '\u{202A}',
                Class::LRO => '\u{202D}',
                Class::RLE => '\u{202B}',
                Class::RLO => '\u{202E}',
                Class::PDF => '\u{202C}',
                Class::LRI => '\u{2066}',
                Class::RLI => '\u{2067}',
                Class::FSI => '\u{2068}',
                Class::PDI => '\u{2069}',
            }
        };
        let name = |s: &str| CLASSES.iter().copied().find(|c| format!("{c:?}") == s);
        let (mut pass, mut fail) = (0, 0);
        let mut want: Vec<Option<u8>> = Vec::new();
        let mut order: Vec<usize> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("@Levels:") {
                want = rest.split_whitespace().map(|v| v.parse().ok()).collect();
                continue;
            }
            if let Some(rest) = line.strip_prefix("@Reorder:") {
                order = rest
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                continue;
            }
            if line.starts_with('@') {
                continue;
            }
            let Some((input, bits)) = line.split_once(';') else {
                continue;
            };
            let chars: Vec<char> = input
                .split_whitespace()
                .filter_map(name)
                .map(sample)
                .collect();
            let bits: u32 = bits.trim().parse().unwrap_or(0);
            for (bit, base) in [(1, None), (2, Some(0)), (4, Some(1))] {
                if bits & bit == 0 {
                    continue;
                }
                if check(&chars, base, None, &want, &order) {
                    pass += 1;
                } else {
                    if fail < 12 {
                        eprintln!("fails: {line} (paragraph level {base:?})");
                    }
                    fail += 1;
                }
            }
        }
        (pass, fail)
    }

    fn character_test(path: &std::path::Path) -> (usize, usize) {
        let text = std::fs::read_to_string(path).expect("BidiCharacterTest.txt");
        let (mut pass, mut fail) = (0, 0);
        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split(';').collect();
            if fields.len() < 5 {
                continue;
            }
            let chars: Vec<char> = fields[0]
                .split_whitespace()
                .filter_map(|h| u32::from_str_radix(h, 16).ok().and_then(char::from_u32))
                .collect();
            let base = match fields[1].trim() {
                "0" => Some(0),
                "1" => Some(1),
                _ => None,
            };
            let level: Option<u8> = fields[2].trim().parse().ok();
            let want: Vec<Option<u8>> = fields[3]
                .split_whitespace()
                .map(|v| v.parse().ok())
                .collect();
            let order: Vec<usize> = fields[4]
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            if check(&chars, base, level, &want, &order) {
                pass += 1;
            } else {
                if fail < 12 {
                    eprintln!("fails: {line}");
                }
                fail += 1;
            }
        }
        (pass, fail)
    }
}
