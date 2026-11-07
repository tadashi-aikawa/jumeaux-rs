use serde_json::Value;

/// パスの要素。数字の辞書キーと配列indexを区別する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

/// 構造化したパスを`root<'items'><0><'id'>`形式にする。
pub fn format_path(segments: &[PathSegment]) -> String {
    let mut path = String::from("root");
    for segment in segments {
        match segment {
            PathSegment::Key(key) => path = key_path(&path, key),
            PathSegment::Index(index) => path.push_str(&format!("<{index}>")),
        }
    }
    path
}

pub(super) fn key_path(parent: &str, key: &str) -> String {
    let quote = if key.contains('\'') { '"' } else { '\'' };
    let mut path = format!("{parent}<{quote}");
    for c in key.chars() {
        match c {
            '\\' => path.push_str("\\\\"),
            '\n' => path.push_str("\\n"),
            '\r' => path.push_str("\\r"),
            '\t' => path.push_str("\\t"),
            c if c == quote => {
                path.push('\\');
                path.push(c);
            }
            c if c.is_control() => path.push_str(&format!("\\u{:04x}", c as u32)),
            c => path.push(c),
        }
    }
    path.push(quote);
    path.push('>');
    path
}

/// パスを解釈して値を借用する。存在しないキー・index・不正な構文は`None`。
/// `root`単独は入力全体、JSON nullは`Some(Value::Null)`となる。
/// 引用符内の`root`・ブラケット・ドットはキーの文字として扱う。
pub fn get_by_diff_key<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut rest = path.strip_prefix("root")?;
    let mut current = value;
    while !rest.is_empty() {
        rest = rest.strip_prefix('<')?;
        if rest.starts_with(['\'', '"']) {
            let (key, remaining) = parse_key(rest)?;
            rest = remaining.strip_prefix('>')?;
            current = current.as_object()?.get(&key)?;
        } else {
            let end = rest.find('>')?;
            let number = &rest[..end];
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let index: usize = number.parse().ok()?;
            current = current.as_array()?.get(index)?;
            rest = &rest[end + 1..];
        }
    }
    Some(current)
}

fn parse_key(input: &str) -> Option<(String, &str)> {
    let mut chars = input.chars();
    let quote = chars.next()?;
    let mut key = String::new();
    while let Some(c) = chars.next() {
        if c == quote {
            return Some((key, chars.as_str()));
        }
        if c != '\\' {
            key.push(c);
            continue;
        }
        key.push(match chars.next()? {
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'b' => '\u{8}',
            'f' => '\u{c}',
            escape @ ('x' | 'u' | 'U') => {
                let digits = match escape {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let mut codepoint = 0;
                for _ in 0..digits {
                    codepoint = codepoint * 16 + chars.next()?.to_digit(16)?;
                }
                char::from_u32(codepoint)?
            }
            _ => return None,
        });
    }
    None
}
