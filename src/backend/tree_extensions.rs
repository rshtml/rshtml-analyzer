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
        F: FnMut(&QueryMatch) -> Option<T>;

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)>;

    fn find_template_params(&self, language: &Language, source: &str) -> Vec<String>;

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
    fn find<T, F>(
        &self,
        language: &Language,
        query_str: &str,
        source: &str,
        mut processor: F,
    ) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch) -> Option<T>,
    {
        let query =
            Query::new(language, query_str).map_err(|e| format!("Failed to create query: {e}"))?;
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

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)> {
        let query_str = "(use_directive path: (string_line) @use_path (as_clause alias: (component_tag_identifier) @use_alias)?)";
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

    fn find_template_params(&self, language: &Language, source: &str) -> Vec<String> {
        let query_str = "[(template_params (param (param_name) @param_name))]";
        self.find(language, query_str, source, |x| {
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
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tree_sitter::Parser;

    pub fn parse_rshtml(source: &str) -> (tree_sitter::Tree, Language) {
        let mut parser = Parser::new();
        let lang: Language = tree_sitter_rshtml::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        let tree = parser.parse(source, None).unwrap();
        (tree, lang)
    }

    #[test]
    fn test_find_uses_with_and_without_alias() {
        let source = r#"
@use "components/button.rs.html" as Button
@use "views/header.rs.html"
<div>Test</div>
"#;
        let (tree, lang) = parse_rshtml(source);
        let uses = tree.find_uses(&lang, source);

        assert_eq!(uses.len(), 2);
        assert_eq!(
            uses[0],
            (
                "components/button.rs.html".to_string(),
                Some("Button".to_string())
            )
        );
        assert_eq!(uses[1], ("views/header.rs.html".to_string(), None));
    }

    #[test]
    fn test_find_template_params() {
        let source = r#"
@(title: String, count: usize)
<h1>@title</h1>
"#;
        let (tree, lang) = parse_rshtml(source);
        let params = tree.find_template_params(&lang, source);

        assert_eq!(params, vec!["title", "count"]);
    }

    #[test]
    fn test_find_template_params_empty() {
        let source = r#"
<div>No params here</div>
"#;
        let (tree, lang) = parse_rshtml(source);
        let params = tree.find_template_params(&lang, source);

        assert!(params.is_empty());
    }

    #[test]
    fn test_find_error_valid_source() {
        let source = r#"
<div>
    <p>Hello World</p>
</div>
"#;
        let (tree, lang) = parse_rshtml(source);
        let errors = tree.find_error(&lang, source);

        assert!(
            errors.is_empty(),
            "Valid code should produce no diagnostics: {:?}",
            errors
        );
    }

    #[test]
    fn test_find_error_invalid_syntax() {
        // In rshtml, an incomplete directive like `@use "..." as` requires an alias identifier
        let source = r#"@use "components/button.rs.html" as"#;
        let (tree, lang) = parse_rshtml(source);
        let errors = tree.find_error(&lang, source);

        assert!(
            !errors.is_empty(),
            "Incomplete directive should produce diagnostics, tree was: {}",
            tree.root_node().to_sexp()
        );
        assert_eq!(errors[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn test_parse_generics_in_template_params() {
        let source = r#"@(title: String, items: Vec<String>, map: HashMap<String, Option<i32>>)"#;
        let (tree, lang) = parse_rshtml(source);
        let errors = tree.find_error(&lang, source);
        let params = tree.find_template_params(&lang, source);

        assert_eq!(params, vec!["title", "items", "map"]);
        assert!(
            errors.is_empty(),
            "Generics in template_params must be valid: {:?}",
            errors
        );
    }

    #[test]
    fn test_parse_when_editor_auto_closes_html_tag_in_generics() {
        // In rshtml grammar, everything after `:` in template_params is matched as opaque `(rust_text)`.
        let corrupted_source = r#"@(title: String, items: Vec<String></String>)"#;
        let (tree, _lang) = parse_rshtml(corrupted_source);
        let sexp = tree.root_node().to_sexp();

        assert!(sexp.contains("(param (param_name) (colon) (rust_text))"));
    }

    #[test]
    fn test_parse_generics_in_rust_code_block() {
        let source = r#"
@{
    let mut list: Vec<String> = Vec::new();
    let comp = 10 < 20;
}
<div>@list.len()</div>
"#;
        let (tree, lang) = parse_rshtml(source);
        let errors = tree.find_error(&lang, source);
        assert!(
            errors.is_empty(),
            "Generics and < comparison inside @{{ ... }} must not cause syntax errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_parse_generics_in_inline_rust_expr() {
        let source = r#"
<div>
    @(items.iter().collect::<Vec<_>>().join(", "))
</div>
"#;
        let (tree, lang) = parse_rshtml(source);
        let errors = tree.find_error(&lang, source);
        assert!(
            errors.is_empty(),
            "Turbofish ::<Vec<_>> inside @(...) must not cause syntax errors: {:?}",
            errors
        );
    }
}
