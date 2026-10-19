use std::{mem, ops::Range};

#[derive(Debug, Clone)]
pub enum Snippet {
    ExprSimple(Range<usize>, String),
    ExprParen(Range<usize>, String),
    Directive(Range<usize>, String),
    Statement(Range<usize>, String),
    Params(Range<usize>, String),
    Html(Range<usize>, usize),
}

impl Snippet {
    pub fn start(&self) -> usize {
        match self {
            Snippet::ExprSimple(r, _)
            | Snippet::Directive(r, _)
            | Snippet::Statement(r, _)
            | Snippet::ExprParen(r, _)
            | Snippet::Params(r, _)
            | Snippet::Html(r, _) => r.start,
        }
    }
}

const PREFIX: &[u8] = b"fn render(child_content: impl ::rshtml::View,";
const PREFIX_NO_PARAMS: &[u8] =
    b"fn render(child_content: impl ::rshtml::View)->::std::fmt::Result{";
const POSTFIX: &[u8] = b"->::std::fmt::Result{";

const _: () = assert!(PREFIX_NO_PARAMS.len() == FIRST_LINE_SHIFT);

pub const PARAMS_SHIFT: usize = PREFIX.len();
pub const FIRST_LINE_SHIFT: usize = PREFIX.len() + POSTFIX.len();

#[derive(Debug, Default, Clone)]
pub struct Extract {
    pub snippets: Vec<Snippet>,
    pub source: Vec<u8>,
}

impl Extract {
    pub fn new(source: Vec<u8>) -> Self {
        Self { source, snippets: Vec::new() }
    }

    pub fn extract(&mut self) {
        self.snippets.sort_unstable_by_key(|b| std::cmp::Reverse(b.start()));
        self.process();
    }

    pub fn push(&mut self, snippet: Snippet) -> &mut Self {
        self.snippets.push(snippet);
        self
    }

    fn process(&mut self) {
        let snippets = mem::take(&mut self.snippets);
        let mut params_range = None;

        for snippet in snippets {
            match snippet {
                Snippet::Directive(range, expr) => {
                    let expr = format!("{};", expr);
                    self.source_replace(&expr, range.start - 1);
                }
                Snippet::ExprParen(range, expr) => {
                    let expr = format!("&{};", expr);
                    self.source_replace(&expr, range.start - 1);
                }
                Snippet::ExprSimple(range, expr) => {
                    let expr = format!("&{};", expr);
                    self.source_replace_insert(expr.as_str(), range.start - 1);
                }
                Snippet::Statement(range, block) => {
                    self.source_replace(&block, range.start);
                }
                Snippet::Params(range, params) => {
                    self.source_replace(&params, range.start);
                    params_range = Some(range);
                }
                Snippet::Html(range, mut bytes_to_delete) => {
                    if bytes_to_delete > 0 {
                        let mut r = range.end;
                        let mut w = range.end;

                        while r > range.start {
                            r -= 1;
                            let b = self.source[r];
                            if bytes_to_delete > 0 && b == b' ' {
                                bytes_to_delete -= 1;
                                continue;
                            }
                            w -= 1;
                            self.source[w] = b;
                        }

                        self.source.drain(range.start..w);
                    }
                }
            }
        }

        if let Some(range) = params_range {
            let mut out = Vec::with_capacity(self.source.len() + FIRST_LINE_SHIFT);

            out.extend_from_slice(PREFIX);
            out.extend_from_slice(&self.source[..range.start]);
            out.push(b' ');
            out.extend_from_slice(&self.source[range.start + 1..range.end]);
            out.extend_from_slice(POSTFIX);
            out.extend_from_slice(&self.source[range.end..]);

            self.source = out;
        } else {
            let mut out = Vec::with_capacity(self.source.len() + 64);
            out.extend_from_slice(PREFIX_NO_PARAMS);
            out.extend_from_slice(&self.source);
            self.source = out;
        }
    }

    pub fn source_replace(&mut self, content: &str, start: usize) {
        let bytes = content.as_bytes();
        let end = (start + bytes.len()).min(self.source.len());
        if start < end {
            self.source[start..end].copy_from_slice(&bytes[..end - start]);
        }
    }

    pub fn source_replace_insert(&mut self, content: &str, start: usize) {
        for (buf_idx, &byte) in (start..).zip(content.as_bytes()) {
            match self.source.get(buf_idx) {
                Some(b'\n') => self.source.insert(buf_idx, byte),
                Some(_) => self.source[buf_idx] = byte,
                None => self.source.push(byte),
            }
        }
    }
}
