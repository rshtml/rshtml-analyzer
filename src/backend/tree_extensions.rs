use tracing::error;
use tree_sitter::{Language, Query, QueryCursor, QueryMatch, StreamingIterator};

pub trait TreeExtensions {
    const STRING_TRIMS: &'_ [char] = &[' ', '\'', '"'];

    fn find<T, F>(&self, language: &Language, query_str: &str, source: &str, processor: F) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch) -> Option<T>;

    fn find_includes(&self, language: &Language, source: &str) -> Vec<String>;

    fn find_uses(&self, language: &Language, source: &str) -> Vec<(String, Option<String>)>;

    fn find_extends(&self, language: &Language, source: &str) -> Option<Option<String>>;
}

impl TreeExtensions for tree_sitter::Tree {
    fn find<T, F>(&self, language: &Language, query_str: &str, source: &str, mut processor: F) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch) -> Option<T>,
    {
        let query = Query::new(language, query_str).map_err(|e| format!("Failed to create query: {}", e))?;
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
        self.find(language, query_str, &source, |x| {
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
        self.find(language, query_str, &source, |x| {
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
        let extends = self
            .find(language, query_str, &source, |x| {
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
            .pop()?;

        extends
    }
}
