//! The GVariant text format for `s` and `as` values, as `gsettings get`
//! prints them and `gsettings set` reads them.

fn parse_quoted(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<String, String> {
    let quote = chars.next().ok_or("empty value")?;
    if quote != '\'' && quote != '"' {
        return Err(format!("expected a string, got {quote:?}"));
    }
    let mut out = String::new();
    loop {
        let c = chars.next().ok_or("unterminated string")?;
        if c == quote {
            return Ok(out);
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let e = chars.next().ok_or("unterminated escape")?;
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'a' => out.push('\u{7}'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            'u' | 'U' => {
                let n = if e == 'u' { 4 } else { 8 };
                let hex: String = (0..n).filter_map(|_| chars.next()).collect();
                let v = u32::from_str_radix(&hex, 16).map_err(|_| format!("bad \\{e} escape"))?;
                out.push(char::from_u32(v).ok_or("bad code point")?);
            }
            other => out.push(other), // \\ \' \" and anything else literally
        }
    }
}

fn skip_ws(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

pub fn parse_string(text: &str) -> Result<String, String> {
    let mut chars = text.trim().chars().peekable();
    let s = parse_quoted(&mut chars)?;
    skip_ws(&mut chars);
    if chars.next().is_some() {
        return Err("trailing text after string".into());
    }
    Ok(s)
}

pub fn parse_string_array(text: &str) -> Result<Vec<String>, String> {
    let t = text.trim();
    let t = t.strip_prefix("@as").map(str::trim_start).unwrap_or(t);
    let mut chars = t.chars().peekable();
    if chars.next() != Some('[') {
        return Err("expected an array".into());
    }
    let mut out = Vec::new();
    loop {
        skip_ws(&mut chars);
        match chars.peek() {
            Some(']') => {
                chars.next();
                break;
            }
            Some(_) => out.push(parse_quoted(&mut chars)?),
            None => return Err("unterminated array".into()),
        }
        skip_ws(&mut chars);
        match chars.next() {
            Some(',') => continue,
            Some(']') => break,
            _ => return Err("expected , or ]".into()),
        }
    }
    Ok(out)
}

pub fn format_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

pub fn format_string_array(v: &[String]) -> String {
    if v.is_empty() {
        return "@as []".into();
    }
    format!("[{}]", v.iter().map(|s| format_string(s)).collect::<Vec<_>>().join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings() {
        assert_eq!(parse_string("'pop-down'\n").unwrap(), "pop-down");
        assert_eq!(parse_string("\"it's\"").unwrap(), "it's");
        assert_eq!(parse_string(r"'a\'b\\c'").unwrap(), "a'b\\c");
        assert_eq!(parse_string(r"'été'").unwrap(), "été");
        assert_eq!(parse_string("''").unwrap(), "");
        assert!(parse_string("pop").is_err());
        assert!(parse_string("'a' x").is_err());
    }

    #[test]
    fn arrays() {
        assert_eq!(parse_string_array("@as []").unwrap(), Vec::<String>::new());
        assert_eq!(parse_string_array("[]").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_string_array("['/home/user/Music/a b.ogg', \"/x/it's.wav\"]").unwrap(),
            vec!["/home/user/Music/a b.ogg".to_string(), "/x/it's.wav".to_string()]
        );
        assert!(parse_string_array("['a',").is_err());
    }

    #[test]
    fn round_trip() {
        let v = vec!["/m/O'Brien \\ x.mp3".to_string(), "é\n".to_string()];
        assert_eq!(parse_string_array(&format_string_array(&v)).unwrap(), v);
        assert_eq!(format_string_array(&[]), "@as []");
    }
}
