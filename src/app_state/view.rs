mod extract;
mod navigation_target;
mod position_to_byte_offset;
mod syntax_context;

use crate::handler::tree_extensions::TreeExtensions;
use extract::Extract;
pub use extract::{FIRST_LINE_SHIFT, PARAMS_SHIFT, Snippet};
use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position, TextDocumentContentChangeEvent, Uri};
pub use navigation_target::NavigationTarget;
use position_to_byte_offset::position_to_byte_offset;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};
pub use syntax_context::SyntaxContext;
use tracing::debug;
use tree_sitter::{Language, Point, Tree};

pub struct View {
    pub source: String,
    pub tree: Tree,
    pub use_directives: Vec<(String, Option<String>, Vec<String>)>,
    pub template_params: Vec<String>,
    pub completion_items: HashMap<String, (char, CompletionItem)>,
    pub extract_file: Option<Uri>,

    pub version: usize,
}

impl View {
    pub fn new(source: String, tree: Tree, version: usize, extract_file: Option<Uri>) -> Self {
        Self {
            source,
            tree,
            use_directives: Vec::new(),
            template_params: Vec::new(),
            completion_items: HashMap::new(),
            extract_file,
            version,
        }
    }

    pub fn use_directives_names_and_params(&self) -> Vec<(String, Vec<String>)> {
        self.use_directives
            .iter()
            .filter_map(|(path, name, params)| {
                let name_str =
                    name.as_deref().or_else(|| path.trim_end_matches(".rs.html").split('/').next_back()).unwrap_or("");

                if name_str.is_empty() { None } else { Some((name_str.to_string(), params.to_owned())) }
            })
            .collect()
    }

    pub fn sync_use_directives(&mut self, use_directives: Vec<(String, Option<String>)>) -> Vec<(usize, String)> {
        if self.use_directives.len() == use_directives.len()
            && self.use_directives.iter().zip(&use_directives).all(|((op, oa, _), (np, na))| op == np && oa == na)
        {
            return Vec::new();
        }

        self.use_directives.retain(|(old_path, _, _)| use_directives.iter().any(|(new_p, _)| new_p == old_path));

        let mut new_use_ids = Vec::new();

        for (path, alias) in use_directives {
            if let Some(existing) = self.use_directives.iter_mut().find(|(p, _, _)| *p == path) {
                existing.1 = alias;
            } else {
                self.use_directives.push((path.clone(), alias, Vec::new()));
                let id = self.use_directives.len() - 1;
                new_use_ids.push((id, path));
            }
        }

        new_use_ids
    }

    fn use_directive_completion_item(use_name: &str, use_params: &[String]) -> (char, CompletionItem) {
        let snippet_params = use_params
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{}=\"${{{}:{}}}\"", name, i + 1, name))
            .collect::<Vec<String>>()
            .join(" ");

        let tag_item = CompletionItem {
            label: use_name.to_owned(),
            kind: Some(CompletionItemKind::Struct),
            detail: Some(format!("{use_name} component")),
            insert_text_format: Some(InsertTextFormat::Snippet),
            insert_text: Some(if snippet_params.is_empty() {
                format!("{use_name}/>")
            } else {
                format!("{use_name} {snippet_params}/>")
            }),
            sort_text: Some("01".to_string()),
            ..Default::default()
        };

        ('<', tag_item)
    }

    pub fn create_use_directive_completion_items(&mut self) {
        let use_names_and_params = self.use_directives_names_and_params();

        for (use_name, use_params) in use_names_and_params {
            let item = Self::use_directive_completion_item(&use_name, &use_params);

            self.completion_items.insert(use_name, item);
        }
    }

    pub fn update_use_directive_completion_items(&mut self) {
        let current_names_and_params: HashSet<(String, Vec<String>)> =
            self.use_directives_names_and_params().into_iter().collect();

        self.completion_items.retain(|name, _| current_names_and_params.iter().any(|(n, _)| n == name));

        for (name, params) in current_names_and_params {
            self.completion_items
                .entry(name)
                .or_insert_with_key(|use_name| Self::use_directive_completion_item(use_name, &params));
        }
    }

    pub fn process_changes(&mut self, content_changes: Vec<TextDocumentContentChangeEvent>) {
        for change in content_changes {
            match change {
                TextDocumentContentChangeEvent::TextDocumentContentChangePartial(partial) => {
                    let range = partial.range;
                    let start_byte = position_to_byte_offset(&self.source, range.start);
                    let end_byte = position_to_byte_offset(&self.source, range.end);

                    let edit = tree_sitter::InputEdit {
                        start_byte,
                        old_end_byte: end_byte,
                        new_end_byte: start_byte + partial.text.len(),
                        start_position: Point {
                            row: range.start.line as usize,
                            column: range.start.character as usize,
                        },
                        old_end_position: Point { row: range.end.line as usize, column: range.end.character as usize },
                        new_end_position: self.calculate_new_end_point(range.start, &partial.text),
                    };

                    self.tree.edit(&edit);

                    self.source.replace_range(start_byte..end_byte, &partial.text);
                }
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(whole) => {
                    self.source = whole.text;
                    break;
                }
            }
        }
    }

    fn calculate_new_end_point(&self, start_pos: Position, text: &str) -> Point {
        let mut new_pos = Point { row: start_pos.line as usize, column: start_pos.character as usize };

        for (i, line) in text.lines().enumerate() {
            if i == 0 {
                new_pos.column += line.len();
            } else {
                new_pos.row += 1;
                new_pos.column = line.len();
            }
        }

        new_pos
    }

    pub fn extract_source(&self, language: &Language) -> String {
        let snippets = self.tree.extract_snippets(language, &self.source);

        let cleaned_source =
            self.source.as_bytes().iter().map(|&b| if b == b'\n' { b'\n' } else { b' ' }).collect::<Vec<u8>>();

        let mut extract = Extract::new(cleaned_source);
        for snippet in snippets {
            extract.push(snippet);
        }
        extract.extract();

        let extract_source = format!(
            "{1} {0}",
            " Ok(()) } use super::*;use ::rshtml::{Render, View};use std::fmt::Display;",
            String::from_utf8_lossy(&extract.source)
        );

        extract_source
    }

    pub fn detect_syntax_context(&self, position: Position) -> SyntaxContext {
        SyntaxContext::detect(&self.tree, &self.source, position)
    }

    pub fn resolve_navigation_target(&self, position: Position, uri: &Uri) -> Option<NavigationTarget> {
        NavigationTarget::resolve_at(&self.tree, &self.source, position, &self.use_directives, uri)
    }

    #[allow(dead_code)]
    #[cfg(debug_assertions)]
    pub fn debug_check_source_mismatch(&self, language: &Language, extract_file: &Option<(Uri, PathBuf)>) {
        let extract_source = self.extract_source(&language);

        if let Some((_, extract_file_path)) = extract_file {
            let Ok(extract_text) = fs::read_to_string(&extract_file_path) else {
                return;
            };

            if extract_source != extract_text {
                debug!("(DEBUG CHECK) SOURCES MISMATCH! Analyzing differences...");
                let extracted_lines: Vec<&str> = extract_source.lines().collect();
                let extract_lines: Vec<&str> = extract_text.lines().collect();
                let max_lines = std::cmp::max(extracted_lines.len(), extract_lines.len());

                for i in 0..max_lines {
                    let e_line = extracted_lines.get(i).unwrap_or(&"<EOF>");
                    let m_line = extract_lines.get(i).unwrap_or(&"<EOF>");

                    if e_line != m_line {
                        debug!("Difference found at Line {}:", i + 1);
                        debug!("  Extract (RAM): {:?}", e_line);
                        debug!("  Extract (Disk)  : {:?}", m_line);
                        continue;
                    }
                }
            } else {
                debug!("(DEBUG CHECK) sources matched perfectly");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn dummy_tree() -> Tree {
        let mut parser = Parser::new();
        let lang = tree_sitter_rshtml::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        parser.parse("", None).unwrap()
    }

    #[test]
    fn test_use_directives_names_and_params() {
        let mut view = View::new("".to_string(), dummy_tree(), 1, None);
        view.use_directives = vec![
            ("components/button.rs.html".to_string(), Some("Button".to_string()), vec!["label".to_string()]),
            ("views/header.rs.html".to_string(), None, vec![]),
        ];

        let res = view.use_directives_names_and_params();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], ("Button".to_string(), vec!["label".to_string()]));
        assert_eq!(res[1], ("header".to_string(), vec![]));
    }

    #[test]
    fn test_sync_use_directives() {
        let mut view = View::new("".to_string(), dummy_tree(), 1, None);

        // Initial sync
        let new_uses =
            view.sync_use_directives(vec![("components/button.rs.html".to_string(), Some("Button".to_string()))]);
        assert_eq!(new_uses.len(), 1);
        assert_eq!(new_uses[0], (0, "components/button.rs.html".to_string()));

        // Add dummy param to button
        view.use_directives[0].2 = vec!["variant".to_string()];

        // Sync identical list -> returns empty and keeps existing params
        let new_uses2 =
            view.sync_use_directives(vec![("components/button.rs.html".to_string(), Some("Button".to_string()))]);
        assert!(new_uses2.is_empty());
        assert_eq!(view.use_directives[0].2, vec!["variant".to_string()]);

        // Sync with a new directive and removing button
        let new_uses3 = view.sync_use_directives(vec![("views/card.rs.html".to_string(), Some("Card".to_string()))]);
        assert_eq!(new_uses3.len(), 1);
        assert_eq!(view.use_directives.len(), 1);
        assert_eq!(view.use_directives[0].0, "views/card.rs.html");
    }

    #[test]
    fn test_completion_items_generation() {
        let mut view = View::new("".to_string(), dummy_tree(), 1, None);
        view.use_directives = vec![(
            "components/button.rs.html".to_string(),
            Some("Button".to_string()),
            vec!["label".to_string(), "active".to_string()],
        )];

        view.create_use_directive_completion_items();

        let item = view.completion_items.get("Button");
        assert!(item.is_some());
        let (trigger, completion) = item.unwrap();
        assert_eq!(*trigger, '<');
        assert_eq!(completion.label, "Button");
        assert_eq!(completion.insert_text, Some("Button label=\"${1:label}\" active=\"${2:active}\"/>".to_string()));
    }
}
