//! Python reの設定パターン・置換をこの境界に集約する。
use super::{Error, Result};

#[derive(Debug, Clone)]
pub struct Regex {
    pattern: String,
    inner: fancy_regex::Regex,
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Self> {
        let translated = translate_pattern(pattern);
        let tree = fancy_regex::Expr::parse_tree(&translated)
            .map_err(|e| Error::new("PatternError", format!("{pattern:?}: {e}")))?;
        validate_lookbehind(&tree.expr)
            .map_err(|message| Error::new("PatternError", format!("{pattern:?}: {message}")))?;
        let inner = fancy_regex::Regex::new(&translated)
            .map_err(|e| Error::new("PatternError", format!("{pattern:?}: {e}")))?;
        Ok(Self {
            pattern: pattern.into(),
            inner,
        })
    }

    pub fn search(&self, target: &str) -> Result<bool> {
        self.inner.is_match(target).map_err(|e| self.error(e))
    }

    /// Jumeauxのexact_matchはre.fullmatchではなくre.search("^(pattern)$")。
    /// 終端の改行を許容し、ラッパーのキャプチャもPython版と同じにする。
    pub fn exact_match(&self, target: &str) -> Result<bool> {
        Self::new(&format!("^({})$", self.pattern))?.search(target)
    }

    pub fn full_match(&self, target: &str) -> Result<bool> {
        Self::new(&format!(r"\A(?:{})\z", self.pattern))?.search(target)
    }

    /// 設定ロード時に呼び、マッチしない入力でも不正な参照を検出する。
    pub fn validate_replacement(&self, replacement: &str) -> Result<()> {
        self.replacement_parts(replacement).map(|_| ())
    }

    pub fn sub(&self, replacement: &str, target: &str) -> Result<String> {
        let parts = self.replacement_parts(replacement)?;
        let mut output = String::new();
        let mut end = 0;
        let mut position = 0;
        let mut previous_empty = false;
        let nonempty = match fancy_regex::RegexBuilder::new(&translate_pattern(&self.pattern))
            .find_not_empty(true)
            .build()
        {
            Ok(regex) => Some(regex),
            Err(fancy_regex::Error::CompileError(error))
                if matches!(*error, fancy_regex::CompileError::PatternCanNeverMatch) =>
            {
                None
            }
            Err(error) => return Err(self.error(error)),
        };
        loop {
            // Python 3.7以降は空一致の直後に同じ位置から非空一致を試す。
            // fancy-regexのcaptures_iterはこの一致と非空一致直後の空一致を落とす。
            let captures = if previous_empty {
                let captures = nonempty
                    .as_ref()
                    .map(|regex| {
                        regex.captures_input(
                            fancy_regex::RegexInput::new(target)
                                .from_pos(position)
                                .anchored(true),
                        )
                    })
                    .transpose()
                    .map_err(|e| self.error(e))?
                    .flatten();
                if captures.is_none() {
                    let Some(next) = target[position..].chars().next() else {
                        break;
                    };
                    position += next.len_utf8();
                    previous_empty = false;
                    continue;
                }
                captures
            } else {
                self.inner
                    .captures_from_pos(target, position)
                    .map_err(|e| self.error(e))?
            };
            let Some(captures) = captures else {
                break;
            };
            let matched = captures.get(0).expect("group 0");
            output.push_str(&target[end..matched.start()]);
            for part in &parts {
                match part {
                    Part::Text(text) => output.push_str(text),
                    Part::Group(index) => {
                        if let Some(value) = captures.get(*index) {
                            output.push_str(value.as_str());
                        }
                    }
                }
            }
            end = matched.end();
            position = end;
            previous_empty = matched.start() == matched.end();
        }
        output.push_str(&target[end..]);
        Ok(output)
    }

    fn error(&self, error: impl std::fmt::Display) -> Error {
        Error::new("PatternError", format!("{:?}: {error}", self.pattern))
    }

    fn replacement_parts(&self, replacement: &str) -> Result<Vec<Part>> {
        let mut chars = replacement.chars().peekable();
        let mut parts = Vec::new();
        while let Some(c) = chars.next() {
            if c != '\\' {
                parts.push(Part::Text(c.to_string()));
                continue;
            }
            let c = chars
                .next()
                .ok_or_else(|| self.error("bad escape at end of replacement"))?;
            let group = match c {
                'g' => {
                    if chars.next() != Some('<') {
                        return Err(self.error("missing < in group reference"));
                    }
                    let mut name = String::new();
                    loop {
                        match chars.next() {
                            Some('>') => break,
                            Some(c) => name.push(c),
                            None => return Err(self.error("unterminated group reference")),
                        }
                    }
                    if name.is_empty() {
                        return Err(self.error("empty group reference"));
                    }
                    if name.chars().all(|c| c.is_ascii_digit()) {
                        name.parse::<usize>().map_err(|e| self.error(e))?
                    } else {
                        self.inner
                            .capture_names()
                            .position(|n| n == Some(&name))
                            .ok_or_else(|| {
                                Error::new(
                                    "IndexError",
                                    format!("{:?}: unknown group {name}", self.pattern),
                                )
                            })?
                    }
                }
                '0'..='9' => {
                    let mut digits = String::from(c);
                    if c != '0' && chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                        digits.push(chars.next().unwrap());
                    }
                    let octal = c == '0'
                        || (digits.len() == 2
                            && digits.chars().all(|c| ('0'..='7').contains(&c))
                            && chars.peek().is_some_and(|c| ('0'..='7').contains(c)));
                    if octal {
                        while digits.len() < 3
                            && chars.peek().is_some_and(|c| ('0'..='7').contains(c))
                        {
                            digits.push(chars.next().unwrap());
                        }
                        let value = u32::from_str_radix(&digits, 8).map_err(|e| self.error(e))?;
                        if value > 255 {
                            return Err(self.error("octal escape outside 0..377"));
                        }
                        parts.push(Part::Text(char::from_u32(value).unwrap().to_string()));
                        continue;
                    }
                    digits.parse::<usize>().map_err(|e| self.error(e))?
                }
                _ => {
                    let escaped = match c {
                        '\\' => "\\".into(),
                        'n' => "\n".into(),
                        'r' => "\r".into(),
                        't' => "\t".into(),
                        'a' => "\x07".into(),
                        'b' => "\x08".into(),
                        'f' => "\x0c".into(),
                        'v' => "\x0b".into(),
                        c if c.is_ascii_alphabetic() => {
                            return Err(self.error(format!("bad escape \\{c}")));
                        }
                        c => format!("\\{c}"),
                    };
                    parts.push(Part::Text(escaped));
                    continue;
                }
            };
            if group >= self.inner.captures_len() {
                return Err(self.error(format!("invalid group reference {group}")));
            }
            parts.push(Part::Group(group));
        }
        Ok(parts)
    }
}

enum Part {
    Text(String),
    Group(usize),
}

fn validate_lookbehind(expr: &fancy_regex::Expr) -> std::result::Result<(), &'static str> {
    use fancy_regex::{Expr, LookAround};
    if let Expr::LookAround(child, LookAround::LookBehind | LookAround::LookBehindNeg) = expr
        && fixed_width(child).is_none()
    {
        return Err("look-behind requires fixed-width pattern");
    }
    for child in expr.children_iter() {
        validate_lookbehind(child)?;
    }
    Ok(())
}

fn fixed_width(expr: &fancy_regex::Expr) -> Option<usize> {
    use fancy_regex::Expr;
    match expr {
        Expr::Empty | Expr::Assertion(_) | Expr::LookAround(_, _) => Some(0),
        Expr::Any { .. } | Expr::Delegate { .. } => Some(1),
        Expr::Literal { val, .. } => Some(val.chars().count()),
        Expr::Group(child) => fixed_width(child),
        Expr::AtomicGroup(child) => fixed_width(child),
        Expr::Concat(children) => children
            .iter()
            .try_fold(0usize, |sum, child| sum.checked_add(fixed_width(child)?)),
        Expr::Alt(children) => {
            let width = fixed_width(children.first()?)?;
            children
                .iter()
                .all(|child| fixed_width(child) == Some(width))
                .then_some(width)
        }
        Expr::Repeat { child, lo, hi, .. } if lo == hi => fixed_width(child)?.checked_mul(*lo),
        _ => None,
    }
}

fn translate_pattern(pattern: &str) -> String {
    let mut result = String::new();
    let mut chars = pattern.chars();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                result.push('\\');
                if let Some(c) = chars.next() {
                    result.push(if c == 'Z' && !in_class { 'z' } else { c });
                }
            }
            '[' => {
                in_class = true;
                result.push(c);
            }
            ']' => {
                in_class = false;
                result.push(c);
            }
            // Rustの$は通常、末尾改行の直前を受理しない。
            '$' if !in_class => result.push_str(r"(?:$|(?=\n\z))"),
            c => result.push(c),
        }
    }
    result
}

pub fn exact_match(target: &str, pattern: &str) -> Result<bool> {
    Regex::new(pattern)?.exact_match(target)
}
