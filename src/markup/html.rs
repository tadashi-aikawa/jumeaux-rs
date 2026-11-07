//! BeautifulSoup(lxml).html.prettify()とHTMLToDictParserの互換処理。

use std::cell::{Cell, RefCell};
use std::{borrow::Cow, collections::HashSet, rc::Rc};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, states::RawKind,
};
use html5ever::tree_builder::{
    ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink,
};
use html5ever::{Attribute, QualName, TokenizerResult};
use scraper::{ElementRef, Html, HtmlTreeSink, Node};
use serde_json::{Map, Value};

use super::{Error, MAX_DEPTH, escape_text, insert_repeated};

// 入力から作れない名前空間を使い、ユーザー属性と衝突させない。
const MARKER_NS: &str = "urn:jumeaux:explicit-element";

struct ExplicitElements<S> {
    sink: S,
    preserve_initial_newline: Cell<bool>,
    next_id: Cell<usize>,
    after_body: Rc<Cell<bool>>,
    after_html: Cell<bool>,
    in_head: Cell<bool>,
}

impl<S: TokenSink> TokenSink for ExplicitElements<S> {
    type Handle = S::Handle;

    fn process_token(&self, mut token: Token, line: u64) -> TokenSinkResult<Self::Handle> {
        if self.after_html.get() && !matches!(token, Token::EOFToken) {
            return TokenSinkResult::Continue;
        }
        if let Token::TagToken(tag) = &token
            && tag.kind == TagKind::EndTag
        {
            if tag.name.as_ref() == "body" {
                self.after_body.set(true);
            }
            if tag.name.as_ref() == "html" {
                self.after_html.set(true);
            }
        }
        if let Token::TagToken(tag) = &mut token {
            if tag.name.as_ref() == "head" {
                self.in_head.set(tag.kind == TagKind::StartTag);
            }
            if tag.name.as_ref() == "body" {
                self.in_head.set(false);
            }
            if tag.name.as_ref() == "noscript" {
                if tag.kind == TagKind::StartTag {
                    if !self.in_head.get() {
                        let body = html5ever::tokenizer::Tag {
                            kind: TagKind::StartTag,
                            name: "body".into(),
                            self_closing: false,
                            attrs: Vec::new(),
                        };
                        let _ = self.sink.process_token(Token::TagToken(body), line);
                    }
                    // templateのFragmentでhead内でも子タグを保つ。出力時にnoscriptへ戻す。
                    tag.attrs.push(Attribute {
                        name: QualName::new(None, MARKER_NS.into(), "original".into()),
                        value: "noscript".into(),
                    });
                }
                tag.name = "template".into();
            }
        }
        // HTML5はpre/textarea直後のLFを1つ捨てるが、lxmlは保持する。
        if self.preserve_initial_newline.replace(false)
            && let Token::CharacterTokens(text) = &mut token
            && text.starts_with('\n')
        {
            *text = format!("\n{text}").into();
        }
        if let Token::TagToken(tag) = &mut token
            && tag.kind == TagKind::StartTag
        {
            self.preserve_initial_newline
                .set(matches!(tag.name.as_ref(), "pre" | "textarea"));
            tag.attrs.push(Attribute {
                name: QualName::new(None, MARKER_NS.into(), "explicit".into()),
                value: self.next_id.get().to_string().into(),
            });
            self.next_id.set(self.next_id.get() + 1);
            if tag.self_closing && !is_void(&tag.name) {
                let end = html5ever::tokenizer::Tag {
                    kind: TagKind::EndTag,
                    name: tag.name.clone(),
                    self_closing: false,
                    attrs: Vec::new(),
                };
                let _ = self.sink.process_token(token, line);
                if end.name.as_ref() == "body" {
                    self.after_body.set(true);
                }
                if end.name.as_ref() == "html" {
                    self.after_html.set(true);
                }
                if end.name.as_ref() == "head" {
                    self.in_head.set(false);
                }
                let _ = self.sink.process_token(Token::TagToken(end), line);
                return TokenSinkResult::Continue;
            }
        }
        self.sink.process_token(token, line)
    }

    fn end(&self) {
        self.sink.end();
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.sink
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

// DOM格納はscraperに委譲し、lxmlと異なる挿入先だけを調整する。
struct CompatTreeSink<S: TreeSink> {
    inner: S,
    html: RefCell<Option<S::Handle>>,
    body: RefCell<Option<S::Handle>>,
    after_body: Rc<Cell<bool>>,
}

impl<S: TreeSink> TreeSink for CompatTreeSink<S> {
    type Handle = S::Handle;
    type Output = S::Output;
    type ElemName<'a>
        = S::ElemName<'a>
    where
        Self: 'a;
    fn finish(self) -> Self::Output {
        self.inner.finish()
    }
    fn parse_error(&self, msg: Cow<'static, str>) {
        self.inner.parse_error(msg);
    }
    fn get_document(&self) -> Self::Handle {
        self.inner.get_document()
    }
    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        self.inner.elem_name(target)
    }
    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let html = name.local.as_ref() == "html";
        let body = name.local.as_ref() == "body";
        let handle = self.inner.create_element(name, attrs, flags);
        if html {
            *self.html.borrow_mut() = Some(handle.clone());
        }
        if body {
            *self.body.borrow_mut() = Some(handle.clone());
        }
        handle
    }
    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        self.inner.create_comment(text)
    }
    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Self::Handle {
        self.inner.create_pi(target, data)
    }
    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        if self.after_body.get()
            && self
                .body
                .borrow()
                .as_ref()
                .is_some_and(|body| self.inner.same_node(parent, body))
            && let Some(html) = self.html.borrow().as_ref()
        {
            self.inner.append(html, child);
        } else {
            self.inner.append(parent, child);
        }
    }
    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        _: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        self.inner.append(element, child);
    }
    fn append_doctype_to_document(&self, name: StrTendril, public: StrTendril, system: StrTendril) {
        self.inner.append_doctype_to_document(name, public, system);
    }
    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        self.inner.get_template_contents(target)
    }
    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        self.inner.same_node(x, y)
    }
    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.inner.set_quirks_mode(mode);
    }
    fn append_before_sibling(&self, sibling: &Self::Handle, node: NodeOrText<Self::Handle>) {
        self.inner.append_before_sibling(sibling, node);
    }
    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        self.inner.add_attrs_if_missing(target, attrs);
    }
    fn remove_from_parent(&self, target: &Self::Handle) {
        self.inner.remove_from_parent(target);
    }
    fn reparent_children(&self, node: &Self::Handle, parent: &Self::Handle) {
        self.inner.reparent_children(node, parent);
    }
    fn is_mathml_annotation_xml_integration_point(&self, handle: &Self::Handle) -> bool {
        self.inner
            .is_mathml_annotation_xml_integration_point(handle)
    }
}

fn feed<S: TokenSink>(sink: S, input: &str) -> S {
    let tokenizer = Tokenizer::new(sink, Default::default());
    let queue = BufferQueue::default();
    queue.push_back(input.into());
    while let TokenizerResult::Script(_) = tokenizer.feed(&queue) {}
    tokenizer.end();
    tokenizer.sink
}

#[derive(Debug)]
enum Child {
    Element(Element),
    Text(String),
    Comment(String),
}

#[derive(Debug)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
    explicit: bool,
}

fn convert(
    element: ElementRef<'_>,
    depth: usize,
    seen: &mut HashSet<String>,
) -> Result<Element, Error> {
    let mut pending = vec![(element, depth)];
    let mut preorder = Vec::new();
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err(Error::LimitExceeded);
        }
        let explicit = node
            .value()
            .attrs
            .iter()
            .find(|(name, _)| name.ns.as_ref() == MARKER_NS && name.local.as_ref() == "explicit")
            .is_some_and(|(_, id)| seen.insert(id.to_string()));
        preorder.push((node, explicit));
        let children: Vec<_> = node
            .children()
            .flat_map(|child| {
                if matches!(child.value(), Node::Fragment) {
                    child.children().collect::<Vec<_>>()
                } else {
                    vec![child]
                }
            })
            .filter_map(ElementRef::wrap)
            .collect();
        pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
    }
    let mut built = std::collections::HashMap::new();
    for (node, explicit) in preorder.into_iter().rev() {
        let result = convert_element(node, explicit, &mut built);
        built.insert(node.id(), result);
    }
    Ok(built.remove(&element.id()).expect("converted root"))
}

fn convert_element(
    element: ElementRef<'_>,
    explicit: bool,
    built: &mut std::collections::HashMap<<HtmlTreeSink as TreeSink>::Handle, Element>,
) -> Element {
    let source = element.value();
    let mut attrs: Vec<_> = source
        .attrs
        .iter()
        .filter(|(name, _)| name.ns.as_ref() != MARKER_NS)
        .map(|(name, value)| {
            let name = name
                .prefix
                .as_ref()
                .map_or_else(|| name.local.to_string(), |p| format!("{p}:{}", name.local));
            (name, value.to_string())
        })
        .collect();
    attrs.sort_by(|a, b| a.0.cmp(&b.0));
    let name = source
        .attrs
        .iter()
        .find(|(name, _)| name.ns.as_ref() == MARKER_NS && name.local.as_ref() == "original")
        .map_or_else(|| source.name().to_string(), |(_, value)| value.to_string());
    for (key, value) in &mut attrs {
        if is_list_attribute(&name, key) {
            *value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        }
    }
    // BeautifulSoupのUnicode出力ではmetaのcharsetをutf-8へ置換する。
    if name == "meta" {
        let content_type = attrs
            .iter()
            .any(|(k, v)| k == "http-equiv" && v.eq_ignore_ascii_case("content-type"));
        for (key, value) in &mut attrs {
            if key == "charset" {
                *value = "utf-8".into();
            } else if key == "content"
                && content_type
                && let Some(start) = value.to_ascii_lowercase().find("charset=")
            {
                let end = value[start + 8..]
                    .find([';', ' ', '\t'])
                    .map_or(value.len(), |i| start + 8 + i);
                value.replace_range(start + 8..end, "utf-8");
            }
        }
    }
    let mut children = Vec::new();
    for child in element.children().flat_map(|child| {
        if matches!(child.value(), Node::Fragment) {
            child.children().collect::<Vec<_>>()
        } else {
            vec![child]
        }
    }) {
        match child.value() {
            Node::Element(_) => {
                let child = built.remove(&child.id()).expect("converted child");
                if !child.explicit && !matches!(child.name.as_str(), "html" | "head" | "body") {
                    children.extend(child.children);
                } else if child.explicit
                    || !matches!(child.name.as_str(), "head" | "body")
                    || !child.children.is_empty()
                {
                    children.push(Child::Element(child));
                }
            }
            Node::Text(text) => {
                children.push(Child::Text(text.to_string()));
            }
            Node::Comment(text) => children.push(Child::Comment(text.to_string())),
            _ => {}
        }
    }
    let mut merged = Vec::new();
    for child in children {
        if let Child::Text(text) = &child
            && let Some(Child::Text(last)) = merged.last_mut()
        {
            last.push_str(text);
        } else {
            merged.push(child);
        }
    }
    if !matches!(name.as_str(), "pre" | "textarea") {
        merged.retain(|child| !matches!(child, Child::Text(text) if text.trim().is_empty()));
    }
    Element {
        name,
        attrs,
        children: merged,
        explicit,
    }
}

fn is_list_attribute(tag: &str, attr: &str) -> bool {
    matches!(attr, "class" | "accesskey" | "dropzone")
        || matches!(
            (tag, attr),
            ("a" | "link" | "area", "rel" | "rev")
                | ("a" | "area", "ping")
                | ("td" | "th", "headers")
                | ("form", "accept-charset")
                | ("object", "archive")
                | ("iframe", "sandbox")
        )
}

fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "command"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn render(element: &Element, depth: usize, inline: bool, output: &mut String) {
    if !inline {
        output.push_str(&" ".repeat(depth));
    }
    output.push('<');
    output.push_str(&element.name);
    for (name, value) in &element.attrs {
        output.push(' ');
        output.push_str(name);
        output.push('=');
        let quote = if value.contains('"') && !value.contains('\'') {
            '\''
        } else {
            '"'
        };
        output.push(quote);
        let value = escape_text(value);
        output.push_str(&if quote == '"' {
            value.replace('"', "&quot;")
        } else {
            value
        });
        output.push(quote);
    }
    if is_void(&element.name) {
        output.push_str("/>");
    } else {
        output.push('>');
        let preserve = inline || matches!(element.name.as_str(), "pre" | "textarea");
        if !preserve {
            output.push('\n');
        }
        for child in &element.children {
            match child {
                Child::Element(child) => render(child, depth + 1, preserve, output),
                Child::Text(text) => {
                    let text = if preserve { text.as_str() } else { text.trim() };
                    if !preserve {
                        output.push_str(&" ".repeat(depth + 1));
                    }
                    if matches!(element.name.as_str(), "script" | "style") {
                        output.push_str(text);
                    } else {
                        output.push_str(&escape_text(text));
                    }
                    if !preserve {
                        output.push('\n');
                    }
                }
                Child::Comment(text) => {
                    if !preserve {
                        output.push_str(&" ".repeat(depth + 1));
                    }
                    output.push_str("<!--");
                    output.push_str(if preserve { text } else { text.trim() });
                    output.push_str("-->");
                    if !preserve {
                        output.push('\n');
                    }
                }
            }
        }
        if !preserve {
            output.push_str(&" ".repeat(depth));
        }
        output.push_str("</");
        output.push_str(&element.name);
        output.push('>');
    }
    if !inline {
        output.push('\n');
    }
}

/// dump/htmlの整形文字列。末尾改行を含む。
pub fn pretty(input: &str) -> Result<String, Error> {
    let after_body = Rc::new(Cell::new(false));
    let tree_sink = CompatTreeSink {
        inner: HtmlTreeSink::new(Html::new_document()),
        html: RefCell::new(None),
        body: RefCell::new(None),
        after_body: after_body.clone(),
    };
    let builder = TreeBuilder::new(
        tree_sink,
        TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
    );
    let sink = feed(
        ExplicitElements {
            sink: builder,
            preserve_initial_newline: Cell::new(false),
            next_id: Cell::new(0),
            after_body,
            after_html: Cell::new(false),
            in_head: Cell::new(false),
        },
        input,
    );
    let document = sink.sink.sink.finish();
    let root = convert(document.root_element(), 0, &mut HashSet::new())?;
    if !root.explicit && root.children.is_empty() {
        return Err(Error::MissingHtml);
    }
    let mut output = String::new();
    render(&root, 0, false, &mut output);
    Ok(output)
}

#[derive(Default)]
struct DictState {
    stack: Vec<(String, Map<String, Value>)>,
    document: Map<String, Value>,
    error: Option<Error>,
}

impl DictState {
    fn end_tag(&mut self, name: &str) {
        let Some((tag, mut value)) = self.stack.pop() else {
            self.error = Some(Error::MalformedHtml);
            return;
        };
        if name != tag {
            self.error = Some(Error::MalformedHtml);
        }
        // Pythonのcleanは空文字を残し、空白だけの文字列を削除する。
        value.retain(|_, value| {
            if let Value::String(text) = value {
                let trimmed = text.trim_matches([' ', '\n', '\r', '\t']);
                if !text.is_empty() && trimmed.is_empty() {
                    return false;
                }
                *text = trimmed.to_string();
            }
            true
        });
        let parent = self
            .stack
            .last_mut()
            .map_or(&mut self.document, |(_, value)| value);
        insert_repeated(parent, tag, Value::Object(value));
    }
}

#[derive(Default)]
struct DictSink(RefCell<DictState>);

impl TokenSink for DictSink {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => {
                let mut value = Map::new();
                for attr in &tag.attrs {
                    value.insert(
                        format!("#{}", attr.name.local),
                        Value::String(attr.value.to_string()),
                    );
                }
                value.insert("##value".into(), Value::String(String::new()));
                state.stack.push((tag.name.to_string(), value));
                if tag.self_closing {
                    state.end_tag(&tag.name);
                } else if tag.name.as_ref() == "script" {
                    return TokenSinkResult::RawData(RawKind::ScriptData);
                } else if tag.name.as_ref() == "style" {
                    return TokenSinkResult::RawData(RawKind::Rawtext);
                }
            }
            Token::TagToken(tag) => state.end_tag(&tag.name),
            Token::CharacterTokens(text) => {
                if let Some((_, value)) = state.stack.last_mut()
                    && let Some(Value::String(current)) = value.get_mut("##value")
                {
                    current.push_str(&text);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

/// res2dict/htmlの辞書。prettify後の空白と属性のcleanも再現する。
pub fn to_dict(input: &str) -> Result<Value, Error> {
    let formatted = pretty(input)?;
    let state = feed(DictSink::default(), &formatted).0.into_inner();
    if let Some(error) = state.error {
        return Err(error);
    }
    if !state.stack.is_empty() {
        return Err(Error::MalformedHtml);
    }
    Ok(Value::Object(state.document))
}
