//! xmltodictとElementTree→minidomの互換処理。

use std::collections::{BTreeMap, HashMap};

use quick_xml::{Reader, events::Event};
use serde_json::{Map, Value};

use super::{Error, MAX_DEPTH, escape_text, insert_repeated};

#[derive(Debug)]
enum Child {
    Element(Element),
    Text(String),
}

#[derive(Debug)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
}

impl Element {
    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(Child::Text(last)) = self.children.last_mut() {
            last.push_str(text);
        } else {
            self.children.push(Child::Text(text.to_string()));
        }
    }

    fn dict(&self) -> Value {
        let mut value = Map::new();
        for (name, text) in &self.attrs {
            value.insert(format!("@{name}"), Value::String(text.clone()));
        }
        let mut text = String::new();
        for child in &self.children {
            match child {
                Child::Element(child) => {
                    insert_repeated(&mut value, child.name.clone(), child.dict())
                }
                Child::Text(part) => text.push_str(part),
            }
        }
        let text = text.trim();
        if value.is_empty() {
            if text.is_empty() {
                Value::Null
            } else {
                Value::String(text.to_string())
            }
        } else {
            if !text.is_empty() {
                value.insert("#text".into(), Value::String(text.to_string()));
            }
            Value::Object(value)
        }
    }
}

// 外部エンティティはNone。どの経路でもネットワーク・ファイルを参照しない。
type Entities = HashMap<String, Option<String>>;

fn entities_from_doctype(doctype: &str) -> Result<Entities, Error> {
    let mut entities = Entities::new();
    let mut remaining = doctype;
    while let Some(start) = remaining.find("<!ENTITY") {
        remaining = &remaining[start + 8..];
        let declaration = remaining.trim_start();
        let end = declaration
            .find(char::is_whitespace)
            .ok_or(Error::InvalidXml)?;
        let name = &declaration[..end];
        let value = declaration[end..].trim_start();
        if name == "%" {
            return Err(Error::InvalidXml);
        }
        if let Some(quote) = value.chars().next().filter(|c| matches!(c, '\'' | '"')) {
            let end = value[1..].find(quote).ok_or(Error::InvalidXml)? + 1;
            entities
                .entry(name.into())
                .or_insert_with(|| Some(value[1..end].into()));
            remaining = &value[end + 1..];
        } else if value.starts_with("SYSTEM") || value.starts_with("PUBLIC") {
            entities.entry(name.into()).or_insert(None);
            remaining = value;
        } else {
            return Err(Error::InvalidXml);
        }
    }
    Ok(entities)
}

fn reference(name: &str, entities: &Entities, expand: bool, depth: usize) -> Result<String, Error> {
    if depth > 32 {
        return Err(Error::LimitExceeded);
    }
    if let Some(number) = name.strip_prefix('#') {
        let value = if let Some(hex) = name.strip_prefix("#x") {
            u32::from_str_radix(hex, 16)
        } else {
            number.parse::<u32>()
        }
        .map_err(|_| Error::InvalidXml)?;
        return char::from_u32(value)
            .filter(|c| valid_char(*c))
            .map(|c| c.to_string())
            .ok_or(Error::InvalidXml);
    }
    if let Some(value) = quick_xml::escape::resolve_xml_entity(name) {
        return Ok(value.into());
    }
    match entities.get(name) {
        Some(_) if !expand => Ok(String::new()),
        Some(Some(value)) => unescape(value, entities, depth + 1),
        _ => Err(Error::InvalidXml),
    }
}

fn unescape(input: &str, entities: &Entities, depth: usize) -> Result<String, Error> {
    let mut remaining = input;
    let mut output = String::new();
    while let Some(start) = remaining.find('&') {
        output.push_str(&remaining[..start]);
        let end = remaining[start..].find(';').ok_or(Error::InvalidXml)? + start;
        output.push_str(&reference(
            &remaining[start + 1..end],
            entities,
            true,
            depth,
        )?);
        if output.len() > 1_000_000 {
            return Err(Error::LimitExceeded);
        }
        remaining = &remaining[end + 1..];
    }
    output.push_str(remaining);
    Ok(output)
}

fn valid_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

fn valid_name(name: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c, ':' | '_' | 'a'..='z' | 'A'..='Z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
    }
    let mut chars = name.chars();
    chars.next().is_some_and(start) && chars.all(|c| start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}'))
}

fn parse(input: &str, expand_entities: bool) -> Result<Element, Error> {
    if !input.chars().all(valid_char) {
        return Err(Error::InvalidXml);
    }
    let mut reader = Reader::from_str(input);
    reader.config_mut().enable_all_checks(true);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    let mut entities = Entities::new();
    let mut doctype_seen = false;
    let mut declaration_seen = false;
    let mut expanded_bytes = 0;
    loop {
        match reader.read_event().map_err(|_| Error::InvalidXml)? {
            Event::Start(start) | Event::Empty(start) => {
                // read_eventの直後の入力末尾で空要素を判定する。属性内の/>には影響されない。
                let empty = input[..reader.buffer_position() as usize].ends_with("/>");
                if stack.len() >= MAX_DEPTH {
                    return Err(Error::LimitExceeded);
                }
                if stack.is_empty() && root.is_some() {
                    return Err(Error::InvalidXml);
                }
                let name = std::str::from_utf8(start.name().as_ref())
                    .map_err(|_| Error::InvalidXml)?
                    .to_string();
                if !valid_name(&name) {
                    return Err(Error::InvalidXml);
                }
                let mut attrs = Vec::new();
                for attr in start.attributes() {
                    let attr = attr.map_err(|_| Error::InvalidXml)?;
                    let name =
                        std::str::from_utf8(attr.key.as_ref()).map_err(|_| Error::InvalidXml)?;
                    if !valid_name(name) {
                        return Err(Error::InvalidXml);
                    }
                    let raw = std::str::from_utf8(&attr.value).map_err(|_| Error::InvalidXml)?;
                    if raw.contains('<') {
                        return Err(Error::InvalidXml);
                    }
                    let normalized = raw.replace("\r\n", " ").replace(['\r', '\n', '\t'], " ");
                    let value = unescape(&normalized, &entities, 0)?;
                    attrs.push((name.into(), value));
                }
                let element = Element {
                    name,
                    attrs,
                    children: Vec::new(),
                };
                if empty {
                    attach(element, &mut stack, &mut root)?;
                } else {
                    stack.push(element);
                }
            }
            Event::End(_) => {
                let element = stack.pop().ok_or(Error::InvalidXml)?;
                attach(element, &mut stack, &mut root)?;
            }
            Event::Text(text) => {
                let text = text.xml10_content().map_err(|_| Error::InvalidXml)?;
                if text.contains("]]>") {
                    return Err(Error::InvalidXml);
                }
                if let Some(element) = stack.last_mut() {
                    element.text(&text);
                } else if !text.trim_matches([' ', '\t', '\r', '\n']).is_empty() {
                    return Err(Error::InvalidXml);
                }
            }
            Event::CData(text) => {
                stack
                    .last_mut()
                    .ok_or(Error::InvalidXml)?
                    .text(&text.xml10_content().map_err(|_| Error::InvalidXml)?);
            }
            Event::GeneralRef(name) => {
                let name = name.decode().map_err(|_| Error::InvalidXml)?;
                let value = reference(&name, &entities, expand_entities, 0)?;
                expanded_bytes += value.len();
                if expanded_bytes > 1_000_000 {
                    return Err(Error::LimitExceeded);
                }
                stack.last_mut().ok_or(Error::InvalidXml)?.text(&value);
            }
            Event::DocType(text) => {
                if doctype_seen || root.is_some() || !stack.is_empty() {
                    return Err(Error::InvalidXml);
                }
                doctype_seen = true;
                entities = entities_from_doctype(&text.decode().map_err(|_| Error::InvalidXml)?)?;
            }
            Event::Decl(decl) => {
                if declaration_seen
                    || doctype_seen
                    || root.is_some()
                    || !stack.is_empty()
                    || decl.version().is_err()
                {
                    return Err(Error::InvalidXml);
                }
                declaration_seen = true;
            }
            Event::Eof => break,
            Event::Comment(_) | Event::PI(_) => {}
        }
    }
    if !stack.is_empty() {
        return Err(Error::InvalidXml);
    }
    root.ok_or(Error::InvalidXml)
}

fn attach(
    element: Element,
    stack: &mut [Element],
    root: &mut Option<Element>,
) -> Result<(), Error> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(Child::Element(element));
    } else if root.is_none() {
        *root = Some(element);
    } else {
        return Err(Error::InvalidXml);
    }
    Ok(())
}

/// res2dict/xmlの辞書。DTDの一般エンティティは本文では展開しない。
pub fn to_dict(input: &str) -> Result<Value, Error> {
    let root = parse(input, false)?;
    let mut value = Map::new();
    value.insert(root.name.clone(), root.dict());
    Ok(Value::Object(value))
}

const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

// ElementTreeの既定register_namespaceと同じprefix。
fn preferred_prefix(uri: &str) -> Option<&'static str> {
    match uri {
        XML_NS => Some("xml"),
        "http://www.w3.org/1999/xhtml" => Some("html"),
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#" => Some("rdf"),
        "http://schemas.xmlsoap.org/wsdl/" => Some("wsdl"),
        "http://www.w3.org/2001/XMLSchema" => Some("xs"),
        "http://www.w3.org/2001/XMLSchema-instance" => Some("xsi"),
        "http://purl.org/dc/elements/1.1/" => Some("dc"),
        _ => None,
    }
}

fn rename(
    name: &str,
    attribute: bool,
    scope: &HashMap<String, String>,
    namespaces: &mut Vec<(String, String)>,
) -> Result<String, Error> {
    let (prefix, local) = name.split_once(':').unwrap_or(("", name));
    if local.contains(':') || local.is_empty() {
        return Err(Error::InvalidXml);
    }
    let uri = if prefix.is_empty() && attribute {
        ""
    } else {
        scope.get(prefix).map(String::as_str).unwrap_or("")
    };
    if uri.is_empty() {
        return if prefix.is_empty() {
            Ok(local.into())
        } else {
            Err(Error::InvalidXml)
        };
    }
    let new_prefix = if let Some((_, prefix)) = namespaces.iter().find(|(known, _)| known == uri) {
        prefix.clone()
    } else {
        let prefix = preferred_prefix(uri)
            .map(str::to_string)
            .unwrap_or_else(|| format!("ns{}", namespaces.len()));
        namespaces.push((uri.into(), prefix.clone()));
        prefix
    };
    Ok(format!("{new_prefix}:{local}"))
}

fn normalize_names(
    element: &mut Element,
    parent_scope: &HashMap<String, String>,
    namespaces: &mut Vec<(String, String)>,
) -> Result<(), Error> {
    let mut scope = parent_scope.clone();
    for (name, value) in &element.attrs {
        if name == "xmlns" {
            scope.insert(String::new(), value.clone());
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            if prefix == "xmlns" || (prefix == "xml" && value != XML_NS) || value.is_empty() {
                return Err(Error::InvalidXml);
            }
            scope.insert(prefix.into(), value.clone());
        }
    }
    element.name = rename(&element.name, false, &scope, namespaces)?;
    element
        .attrs
        .retain(|(name, _)| name != "xmlns" && !name.starts_with("xmlns:"));
    let mut seen = std::collections::HashSet::new();
    for (name, _) in &mut element.attrs {
        *name = rename(name, true, &scope, namespaces)?;
        if !seen.insert(name.clone()) {
            return Err(Error::InvalidXml);
        }
    }
    for child in &mut element.children {
        if let Child::Element(child) = child {
            normalize_names(child, &scope, namespaces)?;
        }
    }
    Ok(())
}

fn render(element: &Element, depth: usize, output: &mut String) {
    let indent = "    ".repeat(depth);
    output.push_str(&indent);
    output.push('<');
    output.push_str(&element.name);
    for (name, value) in &element.attrs {
        output.push(' ');
        output.push_str(name);
        output.push_str("=\"");
        output.push_str(
            &escape_text(value)
                .replace('"', "&quot;")
                .replace('\r', "&#13;")
                .replace('\n', "&#10;")
                .replace('\t', "&#9;"),
        );
        output.push('"');
    }
    if element.children.is_empty() {
        output.push_str("/>\n");
        return;
    }
    output.push('>');
    if let [Child::Text(text)] = element.children.as_slice() {
        // ElementTreeの出力をminidomで再パースすると、本文のCRがLFになる。
        output.push_str(&escape_text(
            &text.replace("\r\n", "\n").replace('\r', "\n"),
        ));
    } else {
        output.push('\n');
        for child in &element.children {
            match child {
                Child::Element(child) => render(child, depth + 1, output),
                Child::Text(text) => {
                    output.push_str(&"    ".repeat(depth + 1));
                    output.push_str(&escape_text(
                        &text.replace("\r\n", "\n").replace('\r', "\n"),
                    ));
                    output.push('\n');
                }
            }
        }
        output.push_str(&indent);
    }
    output.push_str("</");
    output.push_str(&element.name);
    output.push_str(">\n");
}

/// dump/xmlの整形文字列。不正XMLはPython版と同じ診断文字列として保存する。
pub fn pretty(input: &str) -> Result<String, Error> {
    let result = (|| {
        let mut root = parse(input, true)?;
        let mut namespaces = Vec::new();
        let scope = HashMap::from([("xml".into(), XML_NS.into())]);
        normalize_names(&mut root, &scope, &mut namespaces)?;
        let declarations: BTreeMap<_, _> = namespaces
            .into_iter()
            .filter(|(_, prefix)| prefix != "xml")
            .map(|(uri, prefix)| (format!("xmlns:{prefix}"), uri))
            .collect();
        root.attrs.splice(0..0, declarations);
        let mut output = String::from("<?xml version=\"1.0\" ?>\n");
        render(&root, 0, &mut output);
        Ok(output)
    })();
    match result {
        Err(Error::InvalidXml) => Ok(format!(
            "Include invalid token\n\n        {input}\n        "
        )),
        result => result,
    }
}
