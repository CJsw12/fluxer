//! Minimal ICU MessageFormat renderer (arguments, plural, selectordinal, select, `#`).
use std::borrow::Cow;

pub fn format<'a>(msg: &'a str, args: &[(&str, &str)], locale: &str) -> Cow<'a, str> {
    if !msg.contains(['{', '\'']) {
        return Cow::Borrowed(msg);
    }
    let lang = locale.split('-').next().unwrap_or("en");
    let mut out = String::with_capacity(msg.len() + 16);
    render(msg, args, locale, lang, None, &mut out);
    Cow::Owned(out)
}

/// Index of the `}` matching an already-consumed `{`; `s` starts right after that `{`.
fn matching_brace(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let (mut depth, mut i) = (1usize, 0);
    while i < b.len() {
        match b[i] {
            b'\'' if matches!(b.get(i + 1), Some(b'{') | Some(b'}')) => {
                i += 2;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn render(msg: &str, args: &[(&str, &str)], locale: &str, lang: &str, hash: Option<&str>, out: &mut String) {
    let mut i = 0;
    while i < msg.len() {
        let rest = &msg[i..];
        let Some(c) = rest.chars().next() else { break };
        match c {
            '\'' => {
                let next = rest[1..].chars().next();
                if next == Some('\'') {
                    out.push('\'');
                    i += 2;
                } else if matches!(next, Some('{') | Some('}')) || (hash.is_some() && next == Some('#')) {
                    let body = &rest[1..];
                    let end = body[1..].find('\'').map(|p| p + 1).unwrap_or(body.len());
                    out.push_str(&body[..end]);
                    i += 1 + end + usize::from(end < body.len());
                } else {
                    out.push('\'');
                    i += 1;
                }
            }
            '{' => match matching_brace(&rest[1..]) {
                Some(end) => {
                    argument(&rest[1..1 + end], args, locale, lang, hash, out);
                    i += end + 2;
                }
                None => {
                    out.push('{');
                    i += 1;
                }
            },
            '#' if hash.is_some() => {
                out.push_str(hash.unwrap_or(""));
                i += 1;
            }
            _ => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
}

fn lookup<'a>(args: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    args.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

fn argument(inner: &str, args: &[(&str, &str)], locale: &str, lang: &str, hash: Option<&str>, out: &mut String) {
    let mut parts = inner.splitn(3, ',');
    let name = parts.next().unwrap_or("").trim();
    let kind = parts.next().map(str::trim);
    let value = lookup(args, name);
    let Some(kind) = kind else {
        match value {
            Some(v) => out.push_str(v),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        return;
    };
    let options = parts.next().unwrap_or("");
    let known = matches!(kind, "plural" | "selectordinal" | "select");
    let Some(value) = value.filter(|_| known).or_else(|| if known { None } else { value }) else {
        out.push('{');
        out.push_str(inner);
        out.push('}');
        return;
    };
    if !known {
        out.push_str(value);
        return;
    }
    let mut choices: Vec<(&str, &str)> = Vec::new();
    let mut offset = 0i64;
    let mut s = options.trim_start();
    while !s.is_empty() {
        let Some(open) = s.find('{') else { break };
        let mut selector = s[..open].trim();
        if let Some(o) = selector.strip_prefix("offset:") {
            let o = o.trim_start();
            let split = o.find(char::is_whitespace).unwrap_or(o.len());
            offset = o[..split].parse().unwrap_or(0);
            selector = o[split..].trim();
        }
        let Some(end) = matching_brace(&s[open + 1..]) else { break };
        choices.push((selector, &s[open + 1..open + 1 + end]));
        s = s[open + end + 2..].trim_start();
    }
    let find = |sel: &str| choices.iter().find(|(k, _)| *k == sel).map(|(_, b)| *b);
    if kind == "select" {
        if let Some(b) = find(value.trim()).or_else(|| find("other")) {
            render(b, args, locale, lang, hash, out);
        }
        return;
    }
    let n: f64 = value.trim().parse().unwrap_or(0.0);
    let shown = n - offset as f64;
    let exact = find(&format!("={}", value.trim())).or_else(|| find(&format!("={n}")));
    let integral = shown.fract() == 0.0;
    let cat = if !integral {
        "other"
    } else if kind == "plural" {
        cardinal(locale, lang, shown.abs() as u64)
    } else {
        ordinal(lang, shown.abs() as u64)
    };
    let text = if integral { format!("{}", shown as i64) } else { shown.to_string() };
    if let Some(b) = exact.or_else(|| find(cat)).or_else(|| find("other")) {
        render(b, args, locale, lang, Some(&text), out);
    }
}

fn ordinal(lang: &str, n: u64) -> &'static str {
    if lang != "en" {
        return "other";
    }
    match (n % 10, n % 100) {
        (1, r) if r != 11 => "one",
        (2, r) if r != 12 => "two",
        (3, r) if r != 13 => "few",
        _ => "other",
    }
}

fn cardinal(locale: &str, lang: &str, n: u64) -> &'static str {
    let (m10, m100) = (n % 10, n % 100);
    let slavic_few = (2..=4).contains(&m10) && !(12..=14).contains(&m100);
    let east_one = m10 == 1 && m100 != 11;
    match lang {
        "ja" | "ko" | "zh" | "th" | "vi" | "id" | "tok" => "other",
        "fr" | "hi" | "bn" => {
            if n <= 1 {
                "one"
            } else {
                "other"
            }
        }
        "pt" if locale != "pt-PT" => {
            if n <= 1 {
                "one"
            } else {
                "other"
            }
        }
        "fil" => {
            if (1..=3).contains(&n) || !matches!(m10, 4 | 6 | 9) {
                "one"
            } else {
                "other"
            }
        }
        "ru" | "uk" | "be" => {
            if east_one {
                "one"
            } else if slavic_few {
                "few"
            } else {
                "many"
            }
        }
        "pl" => {
            if n == 1 {
                "one"
            } else if slavic_few {
                "few"
            } else {
                "many"
            }
        }
        "cs" | "sk" => {
            if n == 1 {
                "one"
            } else if (2..=4).contains(&n) {
                "few"
            } else {
                "other"
            }
        }
        "sl" => match m100 {
            1 => "one",
            2 => "two",
            3 | 4 => "few",
            _ => "other",
        },
        "lt" => {
            if m10 == 1 && !(11..=19).contains(&m100) {
                "one"
            } else if (2..=9).contains(&m10) && !(11..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "lv" => {
            if m10 == 0 || (11..=19).contains(&m100) {
                "zero"
            } else if east_one {
                "one"
            } else {
                "other"
            }
        }
        "hr" => {
            if east_one {
                "one"
            } else if slavic_few {
                "few"
            } else {
                "other"
            }
        }
        "ro" => {
            if n == 1 {
                "one"
            } else if n == 0 || (2..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "he" => match n {
            1 => "one",
            2 => "two",
            _ => "other",
        },
        "ar" => match n {
            0 => "zero",
            1 => "one",
            2 => "two",
            _ if (3..=10).contains(&m100) => "few",
            _ if (11..=99).contains(&m100) => "many",
            _ => "other",
        },
        _ => {
            if n == 1 {
                "one"
            } else {
                "other"
            }
        }
    }
}
