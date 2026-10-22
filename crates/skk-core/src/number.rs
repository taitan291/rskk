/// Replace ASCII digit runs in an SKK lookup key with `#` and return the digits.
pub fn convert_number_key(key: &str) -> (String, Option<String>) {
    let mut converted = String::with_capacity(key.len());
    let mut digits = String::new();
    let mut in_run = false;
    for ch in key.chars() {
        if ch.is_ascii_digit() {
            if !in_run {
                converted.push('#');
            }
            digits.push(ch);
            in_run = true;
        } else {
            if in_run {
                in_run = false;
            }
            converted.push(ch);
        }
    }
    if digits.is_empty() {
        (key.to_owned(), None)
    } else {
        (converted, Some(digits))
    }
}

/// Expand #0, #1, #2, #3, and #8 in a dictionary candidate.
pub fn convert_number_candidate(candidate: &str, digits: &str) -> String {
    let grouped = add_grouping(digits);
    let full = digits
        .chars()
        .map(|d| match d {
            '0'..='9' => char::from_u32(d as u32 + 0xfee0).unwrap_or(d),
            _ => d,
        })
        .collect::<String>();
    let ungrouped = digits
        .chars()
        .map(|d| match d {
            '0' => '〇',
            '1' => '一',
            '2' => '二',
            '3' => '三',
            '4' => '四',
            '5' => '五',
            '6' => '六',
            '7' => '七',
            '8' => '八',
            '9' => '九',
            _ => d,
        })
        .collect::<String>();
    let positional = kanji_positional(digits);
    candidate
        .replace("#0", digits)
        .replace("#1", &full)
        .replace("#2", &ungrouped)
        .replace("#3", &positional)
        .replace("#8", &grouped)
}

fn add_grouping(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn kanji_positional(digits: &str) -> String {
    let parsed = digits.parse::<u128>().unwrap_or(0);
    if parsed == 0 {
        return if digits.chars().all(|c| c == '0') {
            "零".into()
        } else {
            "〇".repeat(digits.len())
        };
    }
    if digits.len() > 16 {
        return digits.to_owned();
    }
    let small = ["", "十", "百", "千"];
    let large = ["", "万", "億", "兆", "京"];
    let mut n = parsed;
    let mut groups = Vec::new();
    while n > 0 {
        groups.push((n % 10_000) as usize);
        n /= 10_000;
    }
    let mut out = String::new();
    for (group_index, group) in groups.iter().enumerate().rev() {
        if *group == 0 {
            continue;
        }
        let mut part = String::new();
        for position in (0..4).rev() {
            let power = 10_usize.pow(position as u32);
            let digit = (*group / power) % 10;
            if digit == 0 {
                continue;
            }
            if !(digit == 1 && position > 0) {
                part.push(kanji_digit(digit));
            }
            part.push_str(small[position]);
        }
        if group_index < large.len() {
            part.push_str(large[group_index]);
        }
        out.push_str(&part);
    }
    out
}

fn kanji_digit(digit: usize) -> char {
    ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'][digit]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_placeholders() {
        assert_eq!(
            convert_number_key("第12回"),
            ("第#回".into(), Some("12".into()))
        );
        assert_eq!(
            convert_number_candidate("#0/#1/#2/#3/#8", "12345"),
            "12345/１２３４５/一二三四五/一万二千三百四十五/12,345"
        );
    }
}
