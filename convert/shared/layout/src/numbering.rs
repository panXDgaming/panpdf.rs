#[must_use]
pub fn format(number: i64, kind: &str) -> String {
    match kind {
        "lower-alpha" | "lower-latin" | "lowerLetter" => alpha(number, b'a'),
        "upper-alpha" | "upper-latin" | "upperLetter" => alpha(number, b'A'),
        "lower-roman" | "lowerRoman" => roman(number).to_lowercase(),
        "upper-roman" | "upperRoman" => roman(number),
        "lao" | "laoNumbers" => digits(number, '\u{0ED0}'),
        "thai" | "thaiNumbers" | "thaiCounting" => digits(number, '\u{0E50}'),
        "thai-letters" | "thaiLetters" => letters(number, THAI_LETTERS),
        "lao-letters" | "laoLetters" => letters(number, LAO_LETTERS),
        "decimal-leading-zero" | "decimalZero" => format!("{number:02}"),
        _ => number.to_string(),
    }
}

const THAI_LETTERS: &str = "กขคงจฉชซฌญฎฏฐฑฒณดตถทธนบปผฝพฟภมยรลวศษสหฬอฮ";
const LAO_LETTERS: &str = "ກຂຄງຈສຊຍດຕຖທນບປຜຝພຟມຢຣລວຫອຮ";

fn letters(number: i64, set: &str) -> String {
    let set: Vec<char> = set.chars().collect();
    let Ok(index) = usize::try_from(number - 1) else {
        return number.to_string();
    };
    set.get(index % set.len())
        .map_or_else(|| number.to_string(), char::to_string)
}

fn digits(number: i64, zero: char) -> String {
    number
        .to_string()
        .chars()
        .map(|c| match c.to_digit(10) {
            Some(d) => char::from_u32(u32::from(zero) + d).unwrap_or(c),
            None => c,
        })
        .collect()
}

fn alpha(number: i64, base: u8) -> String {
    if number < 1 {
        return number.to_string();
    }
    let mut n = number;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(base + u8::try_from(n % 26).unwrap_or(0));
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn roman(number: i64) -> String {
    if !(1..4000).contains(&number) {
        return number.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut n = number;
    let mut out = String::new();
    for (value, text) in table {
        while n >= value {
            out.push_str(text);
            n -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format;

    #[test]
    fn styles() {
        assert_eq!(format(28, "lower-alpha"), "ab");
        assert_eq!(format(14, "upper-roman"), "XIV");
        assert_eq!(format(12, "lao"), "໑໒");
        assert_eq!(format(3, "thai"), "๓");
        assert_eq!(format(2, "lao-letters"), "ຂ");
    }
}
