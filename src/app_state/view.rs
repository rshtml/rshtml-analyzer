use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use tree_sitter::Tree;

pub struct View {
    pub source: String,
    pub tree: Tree,
    pub layout_path: Option<PathBuf>,
    pub include_paths: Vec<String>,
    pub use_directives: Vec<(String, Option<String>)>,
    pub section_names: Vec<String>,
    pub completion_items: HashMap<String, Vec<(char, CompletionItem)>>,

    pub version: usize,
}

impl View {
    pub fn new(source: String, tree: Tree, version: usize) -> Self {
        Self {
            source,
            tree,
            layout_path: None,
            include_paths: Vec::new(),
            use_directives: Vec::new(),
            section_names: Vec::new(),
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

    fn use_directive_completion_item(use_name: &str) -> Vec<(char, CompletionItem)> {
        let tag_item = CompletionItem {
            label: use_name.to_owned(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some(format!("{} component", use_name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some(use_name.to_owned() + " ${1:parameters} />"),
            sort_text: Some("01".to_string()),
            ..Default::default()
        };

        let at_item = CompletionItem {
            label: use_name.to_owned(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some(format!("{} component", use_name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some(use_name.to_owned() + "(" + " ${1:parameters} ) { ${2:body} }"),
            sort_text: Some("01".to_string()),
            ..Default::default()
        };

        vec![('<', tag_item), ('@', at_item)]
    }

    pub fn create_use_directive_completion_items(&mut self) {
        let use_names = self.use_directives_names();

        for use_name in use_names {
            let items = Self::use_directive_completion_item(&use_name);

            self.completion_items.entry(use_name).or_default().extend(items);
        }
    }

    pub fn update_use_directive_completion_items(&mut self) {
        let current_names: HashSet<String> = self.use_directives_names().into_iter().collect();

        self.completion_items.retain(|name, _| current_names.contains(name));

        for name in current_names {
            self.completion_items
                .entry(name)
                .or_insert_with_key(|use_name| Self::use_directive_completion_item(use_name));
        }
    }

    pub fn section_completion_item(section_name: &str) -> (char, CompletionItem) {
        let at_item = CompletionItem {
            label: format!("render({})", section_name),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(format!("{} section", section_name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("render(".to_string() + section_name + ")"),
            sort_text: Some("01".to_string()),
            ..Default::default()
        };

        ('@', at_item)
    }

    pub fn create_section_completion_items(&mut self) {
        let section_names = &self.section_names;
        for section_name in section_names {
            let item = Self::section_completion_item(section_name);
            self.completion_items
                .entry(format!("section_{}", section_name.to_owned()))
                .or_default()
                .extend(vec![item]);
        }
    }
}
