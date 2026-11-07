//! Python reの設定パターン・置換をこの境界に集約する。
use super::{Error, Result};
use std::{collections::HashSet, sync::OnceLock};

#[derive(Debug, Clone)]
pub struct Regex {
    pattern: String,
    inner: fancy_regex::Regex,
    translated: String,
    full: OnceLock<Result<fancy_regex::Regex>>,
    nonempty: OnceLock<Result<Option<fancy_regex::Regex>>>,
}

/// `Regex::new_exact`でのみ生成するJumeauxのexact用パターン。
/// 通常のRegexにはexact_matchを公開しない。
///
/// ```compile_fail
/// use jumeaux::compat::regex::Regex;
/// Regex::new(r"(a)\2").unwrap().exact_match("aa");
/// ```
#[derive(Debug, Clone)]
pub struct ExactRegex {
    inner: Regex,
}

impl ExactRegex {
    /// re.fullmatchではなくre.search("^(pattern)$")。末尾改行を許容する。
    pub fn exact_match(&self, target: &str) -> Result<bool> {
        self.inner.search(target)
    }
}

impl Regex {
    /// exact_match用の設定は必ずこの入口で検査する。ラッパーもgroup番号に含む。
    pub fn new_exact(pattern: &str) -> Result<ExactRegex> {
        let mut regex = Self::new(&format!("^({pattern})$"))?;
        regex.pattern = pattern.into();
        Ok(ExactRegex { inner: regex })
    }
    pub fn new(pattern: &str) -> Result<Self> {
        let translated = translate_pattern(pattern)?;
        let tree = fancy_regex::Expr::parse_tree(&translated)
            .map_err(|e| Error::new("PatternError", format!("{pattern:?}: {e}")))?;
        validate_lookbehind(&tree.expr)
            .map_err(|message| Error::new("PatternError", format!("{pattern:?}: {message}")))?;
        let inner = fancy_regex::Regex::new(&translated)
            .map_err(|e| Error::new("PatternError", format!("{pattern:?}: {e}")))?;
        Ok(Self {
            pattern: pattern.into(),
            inner,
            translated,
            full: OnceLock::new(),
            nonempty: OnceLock::new(),
        })
    }

    pub fn search(&self, target: &str) -> Result<bool> {
        self.inner.is_match(target).map_err(|e| self.error(e))
    }

    pub fn full_match(&self, target: &str) -> Result<bool> {
        self.full
            .get_or_init(|| {
                fancy_regex::Regex::new(&format!(r"\A(?:{})\z", self.translated))
                    .map_err(|e| self.error(e))
            })
            .as_ref()
            .map_err(Clone::clone)?
            .is_match(target)
            .map_err(|e| self.error(e))
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
        let nonempty = self
            .nonempty
            .get_or_init(|| {
                match fancy_regex::RegexBuilder::new(&self.translated)
                    .find_not_empty(true)
                    .build()
                {
                    Ok(regex) => Ok(Some(regex)),
                    Err(fancy_regex::Error::CompileError(error))
                        if matches!(*error, fancy_regex::CompileError::PatternCanNeverMatch) =>
                    {
                        Ok(None)
                    }
                    Err(error) => Err(self.error(error)),
                }
            })
            .as_ref()
            .map_err(Clone::clone)?;
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

fn translate_pattern(pattern: &str) -> Result<String> {
    let mut result = String::new();
    let chars: Vec<_> = pattern.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    let mut ascii = false;
    let mut scopes = Vec::new();
    let mut groups = Vec::new();
    let mut capture_count = 0usize;
    let mut closed = HashSet::new();
    let mut flags_allowed = true;
    let mut can_repeat = false;
    let mut quantified = false;
    let mut names = HashSet::new();
    let error = |message| Error::new("PatternError", format!("{pattern:?}: {message}"));
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c != '(' {
            flags_allowed = false;
        }
        match c {
            '\\' => {
                let Some(&escaped) = chars.get(i) else {
                    return Err(error("bad escape at end of pattern"));
                };
                i += 1;
                if matches!(
                    escaped,
                    'p' | 'P' | 'h' | 'H' | 'e' | 'k' | 'N' | 'G' | 'K' | 'g'
                ) || (escaped == 'x' && chars.get(i) == Some(&'{'))
                {
                    return Err(error("unsupported Python pattern escape"));
                }
                if ('0'..='7').contains(&escaped)
                    && (escaped == '0'
                        || in_class
                        || chars
                            .get(i..i + 2)
                            .is_some_and(|s| s.iter().all(|c| ('0'..='7').contains(c))))
                {
                    let mut digits = escaped.to_string();
                    while digits.len() < 3 && chars.get(i).is_some_and(|c| ('0'..='7').contains(c))
                    {
                        digits.push(chars[i]);
                        i += 1;
                    }
                    let value = u32::from_str_radix(&digits, 8).unwrap();
                    if value > 255 {
                        return Err(error("octal escape outside 0..377"));
                    }
                    result.push_str(&format!(r"\x{{{value:x}}}"));
                } else if !in_class && ('1'..='9').contains(&escaped) {
                    let mut digits = escaped.to_string();
                    if chars.get(i).is_some_and(char::is_ascii_digit) {
                        digits.push(chars[i]);
                        i += 1;
                    }
                    let group: usize = digits.parse().unwrap();
                    if !closed.contains(&group) {
                        return Err(error("invalid reference or reference to an open group"));
                    }
                    result.push_str(&format!(r"\{digits}"));
                } else if ascii && matches!(escaped, 'w' | 'W' | 'd' | 'D' | 's' | 'S') {
                    let body = match escaped {
                        'w' | 'W' => "a-zA-Z0-9_",
                        'd' | 'D' => "0-9",
                        _ => r"\t\n\r\x0b\x0c ",
                    };
                    if !in_class || escaped.is_uppercase() {
                        result.push('[');
                    }
                    if escaped.is_uppercase() {
                        result.push('^');
                    }
                    result.push_str(body);
                    if !in_class || escaped.is_uppercase() {
                        result.push(']');
                    }
                } else if ascii && !in_class && matches!(escaped, 'b' | 'B') {
                    result.push_str(&format!(r"(?-u:\{escaped})"));
                } else if in_class && escaped == 'b' {
                    result.push_str(r"\x08");
                } else {
                    result.push('\\');
                    result.push(if escaped == 'Z' && !in_class {
                        'z'
                    } else {
                        escaped
                    });
                }
                class_start = false;
                can_repeat = true;
                quantified = false;
            }
            '[' => {
                if in_class {
                    if chars.get(i) == Some(&':') {
                        return Err(error("POSIX classes are unsupported"));
                    }
                    result.push_str(r"\[");
                    class_start = false;
                } else {
                    in_class = true;
                    class_start = true;
                    result.push(c);
                }
            }
            ']' if in_class && !class_start => {
                in_class = false;
                result.push(c);
                can_repeat = true;
                quantified = false;
            }
            '&' | '-' | '~' if in_class && chars.get(i) == Some(&c) => {
                return Err(error("set operations are unsupported"));
            }
            '(' if !in_class => {
                let rest: String = chars[i..].iter().collect();
                if rest.starts_with("?(")
                    || rest.starts_with("?'")
                    || rest.starts_with('*')
                    || rest.starts_with("?R")
                    || rest.starts_with("?U")
                    || (rest.starts_with("?<")
                        && !rest.starts_with("?<=")
                        && !rest.starts_with("?<!"))
                {
                    return Err(error("unsupported group syntax"));
                }
                if let Some(tail) = rest.strip_prefix("?P<") {
                    let name = tail.split('>').next().unwrap();
                    if name.is_empty()
                        || !name.starts_with(|c: char| c == '_' || c.is_alphabetic())
                        || !name.chars().all(|c| c == '_' || c.is_alphanumeric())
                        || !names.insert(name.to_string())
                    {
                        return Err(error("invalid or duplicate group name"));
                    }
                }
                if let Some(flags) = rest.strip_prefix('?') {
                    let count = flags
                        .chars()
                        .take_while(|c| "aiLmsux-".contains(*c))
                        .count();
                    let end = flags.chars().nth(count);
                    if count > 0 && matches!(end, Some(')' | ':')) {
                        let flags: String = flags.chars().take(count).collect();
                        if end == Some(')') && !flags_allowed {
                            return Err(error("global flags not at the start of the expression"));
                        }
                        if flags.contains('L') {
                            return Err(error("LOCALE is invalid for a string pattern"));
                        }
                        if end == Some(':') {
                            scopes.push(ascii);
                            groups.push(None);
                            flags_allowed = false;
                        }
                        if flags.contains('a') {
                            ascii = true;
                        }
                        let flags = flags.replace('a', "");
                        if !flags.is_empty() || end == Some(':') {
                            result.push_str(&format!("(?{flags}{}", end.unwrap()));
                        }
                        i += count + 2;
                        can_repeat = false;
                        quantified = false;
                        continue;
                    }
                }
                flags_allowed = false;
                scopes.push(ascii);
                let capture = !rest.starts_with('?') || rest.starts_with("?P<");
                groups.push(if capture {
                    capture_count += 1;
                    Some(capture_count)
                } else {
                    None
                });
                result.push(c);
                can_repeat = false;
                quantified = false;
            }
            ')' if !in_class => {
                ascii = scopes.pop().unwrap_or(ascii);
                if let Some(Some(group)) = groups.pop() {
                    closed.insert(group);
                }
                result.push(c);
                can_repeat = true;
                quantified = false;
            }
            '{' if !in_class => {
                let end = chars[i..].iter().position(|c| *c == '}').map(|n| i + n);
                if let Some(end) = end {
                    let body: String = chars[i..end].iter().collect();
                    let parts: Vec<_> = body.split(',').collect();
                    if parts.len() <= 2
                        && parts.iter().all(|s| s.chars().all(|c| c.is_ascii_digit()))
                        && !body.is_empty()
                    {
                        if !can_repeat || quantified {
                            return Err(error("nothing to repeat or multiple repeat"));
                        }
                        let lo = parts[0].parse::<usize>().unwrap_or(0);
                        let hi = if parts.len() == 1 {
                            lo
                        } else {
                            parts[1].parse::<usize>().unwrap_or(usize::MAX)
                        };
                        if lo > hi {
                            return Err(error("min repeat greater than max repeat"));
                        }
                        result.push('{');
                        result.push_str(&body);
                        result.push('}');
                        i = end + 1;
                        quantified = true;
                        continue;
                    }
                }
                result.push(c);
                can_repeat = true;
                quantified = false;
            }
            '*' | '+' | '?' if !in_class => {
                result.push(c);
                quantified = true;
            }
            '^' | '|' if !in_class => {
                result.push(c);
                can_repeat = false;
                quantified = false;
            }
            // Rustの$は通常、末尾改行の直前を受理しない。
            '$' if !in_class => {
                result.push_str(r"(?:$|(?=\n\z))");
                can_repeat = false;
                quantified = false;
            }
            c => {
                result.push(c);
                if c != '^' || !class_start {
                    class_start = false;
                }
                if !in_class {
                    can_repeat = true;
                    quantified = false;
                }
            }
        }
    }
    Ok(result)
}

pub fn exact_match(target: &str, pattern: &str) -> Result<bool> {
    Regex::new_exact(pattern)?.exact_match(target)
}
