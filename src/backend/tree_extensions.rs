use tracing::error;
use tree_sitter::{Language, Query, QueryCursor, QueryMatch, StreamingIterator};

pub trait TreeExtensions {
    fn find<T, F>(
        &self,
        language: &Language,
        query_str: &str,
        source: &str,
        processor: F,
    ) -> Result<Vec<T>, String>
    where
        F: FnMut(&QueryMatch) -> Option<T>;

    fn find_includes(&self, language: &Language, source: &str) -> Vec<String>;
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
        let query = Query::new(language, query_str)
            .map_err(|e| format!("Failed to create query: {}", e))?;
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
                .trim_matches('"')
                .to_string();

            Some(include_path)
        })
        .unwrap_or_else(|x| {
            error!("Error during include path query: {}", x);
            vec![]
        })
    }
}
