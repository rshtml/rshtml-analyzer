use tower_lsp::lsp_types;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};
use tracing::error;
use tree_sitter::{Language, Query, QueryCursor, QueryMatch, Range, StreamingIterator};

pub trait TreeExtensions {
    const STRING_TRIMS: &'_ [char] = &[' ', '\'', '"'];

    fn find<T, F>(
        &self,
        language: &Language,
        query_str: &str,
        source: &str,
        processor: F,
    ) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch, &[&str]) -> Option<T>;

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)>;

    fn find_template_params(&self, language: &Language, source: &str) -> Vec<String>;

    fn find_error(&self, language: &Language, source: &str) -> Vec<Diagnostic>;

    fn rust_code_to_str(&self, language: &Language, source: &str) -> String;

    fn from_range(range: Range) -> lsp_types::Range {
        let start_point = range.start_point;
        let end_point = range.end_point;
        lsp_types::Range {
            start: lsp_types::Position {
                line: start_point.row as u32,
                character: start_point.column as u32,
            },
            end: lsp_types::Position {
                line: end_point.row as u32,
                character: end_point.column as u32,
            },
        }
    }
}

impl TreeExtensions for tree_sitter::Tree {
    fn find<T, F>(
        &self,
        language: &Language,
        query_str: &str,
        source: &str,
        mut processor: F,
    ) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch, &[&str]) -> Option<T>,
    {
        let query =
            Query::new(language, query_str).map_err(|e| format!("Failed to create query: {e}"))?;
        let mut query_cursor = QueryCursor::new();
        let source_bytes = source.as_bytes();

        let mut matches = query_cursor.matches(&query, self.root_node(), source_bytes);

        let mut results: Vec<T> = Vec::new();

        while let Some(match_) = matches.next() {
            if let Some(t) = processor(match_, query.capture_names()) {
                results.push(t);
            }
        }

        Ok(results)
    }

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)> {
        let query_str = "(use_directive path: (string_line) @use_path (as_clause alias: (component_tag_identifier) @use_alias)?)";
        self.find(language, query_str, source, |x, _| {
            let mut captures = x.captures.iter();
            let use_path = captures
                .next()?
                .node
                .utf8_text(source.as_bytes())
                .ok()?
                .trim()
                .trim_matches(Self::STRING_TRIMS)
                .to_string();

            let use_alias = captures
                .next()
                .and_then(|x| x.node.utf8_text(source.as_bytes()).ok())
                .map(|x| x.trim().to_string());

            Some((use_path, use_alias))
        })
        .unwrap_or_else(|x| {
            error!("Error during use_path query: {}", x);
            vec![]
        })
    }

    fn find_template_params(&self, language: &Language, source: &str) -> Vec<String> {
        let query_str = "[(template_params (param (param_name) @param_name))]";
        self.find(language, query_str, source, |x, _| {
            let mut param_names = Vec::with_capacity(x.captures.len());
            for c in x.captures {
                param_names.push(
                    c.node
                        .utf8_text(source.as_bytes())
                        .map(|s| s.trim_matches(Self::STRING_TRIMS).to_string())
                        .ok()?,
                );
            }
            Some(param_names)
        })
        .ok()
        .map(|results| results.into_iter().flatten().collect::<Vec<String>>())
        .unwrap_or_else(|| {
            error!("Error during template params query");
            vec![]
        })
    }

    fn find_error(&self, language: &Language, source: &str) -> Vec<Diagnostic> {
        let query_str = "[(ERROR) @error (MISSING) @missing]";
        self.find(language, query_str, source, |x, _| {
            let node = x.captures.first()?.node;

            let range = if node.is_missing() {
                node.parent().map_or(node.range(), |parent| parent.range())
            } else {
                node.range()
            };

            let range = Self::from_range(range);
            let severity = Some(DiagnosticSeverity::ERROR);

            let message = if node.is_missing() {
                format!("Missing `{}`", node.kind().replace('_', " "))
            } else {
                format!(
                    "Syntax error in `{}`",
                    node.utf8_text(source.as_bytes()).ok()?
                )
            };

            let diagnostic = Diagnostic {
                range,
                message,
                severity,
                ..Default::default()
            };

            Some(diagnostic)
        })
        .unwrap_or_else(|err| {
            error!("Error during error query: {}", err);
            vec![]
        })
    }

    fn rust_code_to_str(&self, language: &Language, source: &str) -> String {
        let source_bytes = source.as_bytes();
        let query_str = r#"[
            (rust_expr_simple (rust_text) @expr)
            (rust_expr_paren (rust_text) @expr)
            (if_stmt head: (rust_text) @keep (open_brace) @keep (close_brace) @keep)
            (else_clause head: (rust_text) @keep)
            (for_stmt head: (rust_text) @keep (open_brace) @keep (close_brace) @keep)
            (rust_block content: (rust_text) @keep)
        ]"#;
        let mut virtual_bytes: Vec<u8> = source
            .as_bytes()
            .iter()
            .map(|&b| if b == b'\n' { b'\n' } else { b' ' })
            .collect();

        let mut offset: usize = 0;

        self.find(language, query_str, source, |x, capture_names| {
            for c in x.captures {
                let range = c.node.byte_range();
                let start = range.start + offset;
                let end = range.end + offset;

                let rust_slice = &source_bytes[range.clone()];

                let capture_name = capture_names[c.index as usize];

                virtual_bytes[start..end].copy_from_slice(rust_slice);

                if capture_name == "expr" {
                    if end < virtual_bytes.len() {
                        let next_char = virtual_bytes[end];

                        if next_char == b'\n' || next_char == b'\r' {
                            virtual_bytes.insert(end, b';');

                            offset += 1;
                        } else {
                            virtual_bytes[end] = b';';
                        }
                    } else if end == virtual_bytes.len() {
                        // file end
                        virtual_bytes.push(b';');
                        offset += 1;
                    }
                }
            }

            Some(())
        })
        .unwrap_or_else(|err| {
            error!("Error during rust text query: {err:?}");
            vec![]
        });

        String::from_utf8(virtual_bytes).unwrap_or_default()
    }
}
