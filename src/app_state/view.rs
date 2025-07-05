use std::collections::HashMap;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use tree_sitter::Tree;

pub struct View {
    pub source: String,
    pub tree: Tree,
    pub include_paths: Vec<String>,
    pub use_directives: Vec<(String, Option<String>)>,
    pub completion_items: HashMap<String, Vec<(char, CompletionItem)>>,

    pub version: usize,
}

impl View {
    pub fn new(source: String, tree: Tree, version: usize) -> Self {
        Self {
            source,
            tree,
            include_paths: Vec::new(),
            use_directives: Vec::new(),
            completion_items: HashMap::new(),
            version,
        }
    }

    pub fn use_directives_names(&self) -> Vec<String> {
        self.use_directives
            .iter()
            .filter_map(|(path, name)| {
                let name_str = name
                    .as_deref()
                    .or_else(|| path.trim_end_matches(".rs.html").split('/').last())
                    .unwrap_or("");

                if name_str.is_empty() { None } else { Some(name_str.to_string()) }
            })
            .collect()
    }

    pub fn create_use_directive_completion_items(&mut self) {
        let use_names = self.use_directives_names();

        for use_name in use_names {
            let tag_item = CompletionItem {
                label: use_name.to_owned(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("{} component", use_name)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                insert_text: Some(use_name.to_owned() + " ${1:parameters} />"),
                sort_text: Some("01".to_string()),
                ..Default::default()
            };

            let at_item = CompletionItem {
                label: use_name.to_owned(),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("{} component", use_name)),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                insert_text: Some(use_name.to_owned() + "(" + " ${1:parameters} ) { ${2:body} }"),
                sort_text: Some("01".to_string()),
                ..Default::default()
            };

            self.completion_items
                .entry(use_name)
                .or_default()
                .extend(vec![('<', tag_item), ('@', at_item)]);
        }
    }
}
