pub(crate) fn decode_concat(input: &str) -> String {
    let Some(start) = input.find("(concat") else {
        return input.to_owned();
    };
    let input = &input[start + "(concat".len()..];
    let mut output = Vec::new();
    let mut quoted = false;
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                quoted = !quoted;
                index += 1;
            }
            b'\\' if quoted => {
                if index + 3 < bytes.len()
                    && bytes[index + 1].is_ascii_digit()
                    && bytes[index + 2].is_ascii_digit()
                    && bytes[index + 3].is_ascii_digit()
                {
                    let value = (bytes[index + 1] - b'0') * 64
                        + (bytes[index + 2] - b'0') * 8
                        + bytes[index + 3]
                        - b'0';
                    output.push(value);
                    index += 4;
                } else if index + 1 < bytes.len() {
                    output.push(match bytes[index + 1] {
                        b'n' => b'\n',
                        b't' => b'\t',
                        c => c,
                    });
                    index += 2;
                } else {
                    index += 1;
                }
            }
            byte if quoted => {
                output.push(byte);
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub(crate) fn encode_candidate(candidate: &str) -> String {
    if !candidate
        .chars()
        .any(|ch| matches!(ch, '/' | ';' | '\\' | '"'))
    {
        return candidate.to_owned();
    }
    let mut output = String::from("(concat \"");
    for ch in candidate.chars() {
        match ch {
            '/' => output.push_str("\\057"),
            ';' => output.push_str("\\073"),
            '\\' => output.push_str("\\134"),
            '"' => output.push_str("\\042"),
            _ => output.push(ch),
        }
    }
    output.push_str("\")");
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn octal_concat_round_trip() {
        let text = "slash/ semi; slash\\ quote\"";
        assert_eq!(decode_concat(&encode_candidate(text)), text);
        assert_eq!(
            decode_concat("(concat \"x\\057y\\073z\\134q\\042\")"),
            "x/y;z\\q\""
        );
    }
}
