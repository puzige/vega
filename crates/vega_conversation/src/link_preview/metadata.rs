use crate::types::LinkPreviewFailure;
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, CharacterTokens, EndTag, StartTag, TagToken, Token, TokenSink,
        TokenSinkResult, Tokenizer, TokenizerOpts, states::RawKind,
    },
};
use reqwest::Url;

pub(super) struct Metadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<Url>,
}

struct Collector {
    base: Url,
    title: String,
    in_title: bool,
    in_raw: bool,
    closed: bool,
    template_depth: usize,
    og_title: Option<String>,
    description: Option<String>,
    og_description: Option<String>,
    image: Option<Url>,
    unsupported_encoding: bool,
}

pub(super) fn supported_encoding(value: &str) -> bool {
    matches!(
        value
            .trim()
            .trim_matches(['\'', '"'])
            .to_ascii_lowercase()
            .as_str(),
        "utf-8" | "utf8" | "us-ascii"
    )
}

fn text(value: &str, limit: usize) -> Option<String> {
    let normalized = value
        .chars()
        .map(|ch| {
            if ch.is_control() || ch.is_whitespace() {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!normalized.is_empty()).then(|| normalized.chars().take(limit).collect())
}

impl TokenSink for Collector {
    type Handle = ();

    fn process_token(&mut self, token: Token, _: u64) -> TokenSinkResult<()> {
        match token {
            CharacterTokens(value) if self.in_title && self.template_depth == 0 && !self.closed => {
                self.title.push_str(&value);
            }
            CharacterTokens(value)
                if !self.in_raw
                    && self.template_depth == 0
                    && value.chars().any(|ch| !ch.is_whitespace()) =>
            {
                self.closed = true;
            }
            TagToken(tag) => {
                let name = tag.name.as_ref();
                if tag.kind == EndTag {
                    if matches!(
                        name,
                        "title"
                            | "script"
                            | "style"
                            | "noscript"
                            | "xmp"
                            | "iframe"
                            | "noembed"
                            | "noframes"
                    ) {
                        self.in_raw = false;
                    }
                    if name == "title" {
                        self.in_title = false;
                        self.title = text(&self.title, 512).unwrap_or_default();
                    }
                    if name == "template" {
                        self.template_depth = self.template_depth.saturating_sub(1);
                    }
                    if name == "head" && self.template_depth == 0 {
                        self.closed = true;
                    }
                    return TokenSinkResult::Continue;
                }
                if tag.kind != StartTag {
                    return TokenSinkResult::Continue;
                }
                if name == "template" {
                    self.template_depth += 1;
                }
                if self.template_depth == 0
                    && !matches!(
                        name,
                        "html"
                            | "head"
                            | "title"
                            | "base"
                            | "link"
                            | "meta"
                            | "script"
                            | "style"
                            | "noscript"
                            | "noframes"
                            | "template"
                    )
                {
                    self.closed = true;
                }
                if name == "script" {
                    self.in_raw = true;
                    return TokenSinkResult::RawData(RawKind::ScriptData);
                }
                if matches!(
                    name,
                    "style" | "noscript" | "xmp" | "iframe" | "noembed" | "noframes"
                ) {
                    self.in_raw = true;
                    return TokenSinkResult::RawData(RawKind::Rawtext);
                }
                if name == "title" {
                    self.in_raw = true;
                    self.in_title =
                        self.title.is_empty() && self.template_depth == 0 && !self.closed;
                    return TokenSinkResult::RawData(RawKind::Rcdata);
                }
                if name != "meta" || self.closed || self.template_depth > 0 {
                    return TokenSinkResult::Continue;
                }
                let attribute = |name: &str| {
                    tag.attrs
                        .iter()
                        .find(|attribute| attribute.name.local.as_ref().eq_ignore_ascii_case(name))
                        .map(|attribute| attribute.value.as_ref())
                };
                if let Some(encoding) = attribute("charset") {
                    self.unsupported_encoding |= !supported_encoding(encoding);
                }
                if attribute("http-equiv")
                    .is_some_and(|value| value.eq_ignore_ascii_case("content-type"))
                    && let Some(content) = attribute("content")
                {
                    for part in content.split(';').skip(1) {
                        if let Some((name, value)) = part.split_once('=')
                            && name.trim().eq_ignore_ascii_case("charset")
                        {
                            self.unsupported_encoding |= !supported_encoding(value);
                        }
                    }
                }
                let Some(value) = attribute("content") else {
                    return TokenSinkResult::Continue;
                };
                let property = attribute("property")
                    .or_else(|| attribute("name"))
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase();
                match property.as_str() {
                    "og:title" if self.og_title.is_none() => self.og_title = text(value, 512),
                    "og:description" if self.og_description.is_none() => {
                        self.og_description = text(value, 2048)
                    }
                    "description" if self.description.is_none() => {
                        self.description = text(value, 2048)
                    }
                    "og:image"
                        if self.image.is_none()
                            && !value.trim().is_empty()
                            && value.len() <= super::MAX_PREVIEW_URL_BYTES =>
                    {
                        self.image = self
                            .base
                            .join(value.trim())
                            .ok()
                            .filter(|url| matches!(url.scheme(), "http" | "https"));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

pub(super) fn parse(source: &str, base: &Url) -> Result<Metadata, LinkPreviewFailure> {
    let collector = Collector {
        base: base.clone(),
        title: String::new(),
        in_title: false,
        in_raw: false,
        closed: false,
        template_depth: 0,
        og_title: None,
        description: None,
        og_description: None,
        image: None,
        unsupported_encoding: false,
    };
    let mut tokenizer = Tokenizer::new(collector, TokenizerOpts::default());
    let mut input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(source));
    let _ = tokenizer.feed(&mut input);
    tokenizer.end();
    let collector = tokenizer.sink;
    if collector.unsupported_encoding {
        return Err(LinkPreviewFailure::Malformed);
    }
    Ok(Metadata {
        title: collector.og_title.or_else(|| text(&collector.title, 512)),
        description: collector.og_description.or(collector.description),
        image: collector.image,
    })
}
