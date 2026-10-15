use std::{mem, ops::Range};

#[derive(Debug, Clone)]
pub enum Snippet {
    ExprSimple(Range<usize>, String),
    ExprParen(Range<usize>, String),
    Expr(Range<usize>, String),
    Statement(Range<usize>, String),
    Param {
        name: (Range<usize>, String),
        ty: Option<(Range<usize>, String)>,
    },
}

impl Snippet {
    pub fn start(&self) -> usize {
        match self {
            Snippet::ExprSimple(r, _)
            | Snippet::Expr(r, _)
            | Snippet::Statement(r, _)
            | Snippet::ExprParen(r, _) => r.start,
            Snippet::Param { .. } => 0,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Extract {
    pub snippets: Vec<Snippet>,
    pub source: Vec<u8>,
}

impl Extract {
    pub fn new(source: Vec<u8>) -> Self {
        Self {
            source,
            snippets: Vec::new(),
        }
    }

    pub fn extract(&mut self, original_source: &str) {
        self.snippets
            .sort_unstable_by_key(|b| std::cmp::Reverse(b.start()));

        let snippets = mem::take(&mut self.snippets);

        for snippet in snippets {
            if let Snippet::ExprSimple(range, expr) = snippet {
                self.source_replace_insert(expr.as_str(), range.start - 1);
            }
        }

        self.fix_utf8_column_shifts(original_source);
    }

    pub fn push(&mut self, snippet: Snippet) {
        match snippet {
            Snippet::Expr(range, expr) => {
                let expr = format!("&{};", expr);
                self.source_replace(&expr, range.start - 1);
            }
            Snippet::ExprParen(range, expr) => {
                let expr = format!("&{};", expr);
                self.source_replace(&expr, range.start - 1);
            }
            Snippet::ExprSimple(range, expr) => {
                let expr = format!("&{};", expr);
                self.snippets.push(Snippet::ExprSimple(range, expr));
            }
            Snippet::Statement(range, block) => {
                self.source_replace(&block, range.start);
            }
            Snippet::Param { .. } => {}
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

    fn fix_utf8_column_shifts(&mut self, original_source: &str) {
        for (byte_idx, ch) in original_source.char_indices().rev() {
            let char_len = ch.len_utf8();
            if char_len > 1 {
                let extra_bytes = char_len - 1;
                if byte_idx + char_len <= self.source.len()
                    && self.source[byte_idx..byte_idx + char_len]
                        .iter()
                        .all(|&b| b == b' ')
                {
                    self.source.drain(byte_idx + 1..byte_idx + 1 + extra_bytes);
                }
            }
        }
    }
}
