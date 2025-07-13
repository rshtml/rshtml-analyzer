use tower_lsp::lsp_types;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};
use tracing::error;
use tree_sitter::{Language, Query, QueryCursor, QueryMatch, Range, StreamingIterator};

pub trait TreeExtensions {
    const STRING_TRIMS: &'_ [char] = &[' ', '\'', '"'];

    fn find<T, F>(&self, language: &Language, query_str: &str, source: &str, processor: F) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch) -> Option<T>;

    fn find_includes(&self, language: &Language, source: &str) -> Vec<String>;

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)>;

    fn find_extends(&self, language: &Language, source: &str) -> Option<Option<String>>;

    fn find_sections(&self, language: &Language, source: &str) -> Vec<String>;

    fn find_error(&self, language: &Language, source: &str) -> Vec<Diagnostic>;

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
    fn find<T, F>(&self, language: &Language, query_str: &str, source: &str, mut processor: F) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch) -> Option<T>,
    {
        let query = Query::new(language, query_str).map_err(|e| format!("Failed to create query: {e}"))?;
        let mut query_cursor = QueryCursor::new();
        let source_bytes = source.as_bytes();

        let mut matches = query_cursor.matches(&query, self.root_node(), source_bytes);

        let mut results: Vec<T> = Vec::new();

        while let Some(match_) = matches.next() {
            if let Some(t) = processor(match_) {
                results.push(t);
            }
        }

        Ok(results)
    }

    fn find_includes(&self, language: &Language, source: &str) -> Vec<String> {
        let query_str = "(include_directive path: (string_line) @include_path)";
        self.find(language, query_str, source, |x| {
            let include_path = x
                .captures
                .first()?
                .node
                .utf8_text(source.as_bytes())
                .ok()?
                .trim_matches(Self::STRING_TRIMS)
                .to_string();

            Some(include_path)
        })
        .unwrap_or_else(|x| {
            error!("Error during include path query: {}", x);
            vec![]
        })
    }

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)> {
        let query_str = "(use_directive path: (string_line) @use_path (as_clause alias: (rust_identifier) @use_alias)?)";
        self.find(language, query_str, source, |x| {
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

    fn find_extends(&self, language: &Language, source: &str) -> Option<Option<String>> {
        let query_str = "(extends_directive) @directive";
        self.find(language, query_str, source, |x| {
            let capture = Some(x.captures.first().and_then(|x| {
                Some(
                    x.node
                        .child_by_field_name("path")?
                        .utf8_text(source.as_bytes())
                        .ok()?
                        .trim_matches(Self::STRING_TRIMS)
                        .to_string(),
                )
            }));

            Some(capture)
        })
        .ok()?
        .pop()?
    }

    fn find_sections(&self, language: &Language, source: &str) -> Vec<String> {
        let query_str = "[(section_directive name: (string_line) @name) (section_block name: (rust_identifier) @name)]";
        self.find(language, query_str, source, |x| {
            let section_name = x
                .captures
                .first()?
                .node
                .utf8_text(source.as_bytes())
                .ok()?
                .trim_matches(Self::STRING_TRIMS)
                .to_string();

            Some(section_name)
        })
        .unwrap_or_else(|x| {
            error!("Error during include path query: {}", x);
            vec![]
        })
    }

    fn find_error(&self, language: &Language, source: &str) -> Vec<Diagnostic> {
        let query_str = "[(ERROR) @error (MISSING) @missing]";
        self.find(language, query_str, source, |x| {
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
                format!("Syntax error in `{}`", node.utf8_text(source.as_bytes()).ok()?)
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
}
