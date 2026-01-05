use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, SemanticTokens};
use tree_sitter::Tree;

pub struct View {
    pub source: String,
    pub tree: Tree,
    pub use_directives: Vec<(String, Option<String>, Vec<String>)>,
    pub template_params: Vec<String>,
    pub completion_items: HashMap<String, (char, CompletionItem)>,
    pub semantic_tokens: SemanticTokens,
    pub semantic_tokens_version: u64,
    pub struct_info: Option<(PathBuf, String)>,

    pub version: usize,
}

impl View {
    pub fn new(source: String, tree: Tree, version: usize) -> Self {
        Self {
            source,
            tree,
            use_directives: Vec::new(),
            template_params: Vec::new(),
            completion_items: HashMap::new(),
            semantic_tokens: SemanticTokens::default(),
            semantic_tokens_version: 0,
            struct_info: None,
            version,
        }
    }

    pub fn use_directives_names_and_params(&self) -> Vec<(String, Vec<String>)> {
        self.use_directives
            .iter()
            .filter_map(|(path, name, params)| {
                let name_str = name
                    .as_deref()
                    .or_else(|| path.trim_end_matches(".rs.html").split('/').next_back())
                    .unwrap_or("");

                if name_str.is_empty() {
                    None
                } else {
                    Some((name_str.to_string(), params.to_owned()))
                }
            })
            .collect()
    }

    pub fn sync_use_directives(
        &mut self,
        use_directives: Vec<(String, Option<String>)>,
    ) -> Vec<(usize, String)> {
        if self.use_directives.len() == use_directives.len()
            && self
                .use_directives
                .iter()
                .zip(&use_directives)
                .all(|((op, oa, _), (np, na))| op == np && oa == na)
        {
            return Vec::new();
        }

        self.use_directives
            .retain(|(old_path, _, _)| use_directives.iter().any(|(new_p, _)| new_p == old_path));

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

    fn use_directive_completion_item(
        use_name: &str,
        use_params: &[String],
    ) -> (char, CompletionItem) {
        let snippet_params = use_params
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{}=\"${{{}:{}}}\"", name, i + 1, name))
            .collect::<Vec<String>>()
            .join(" ");

        let tag_item = CompletionItem {
            label: use_name.to_owned(),
            kind: Some(CompletionItemKind::STRUCT),
            detail: Some(format!("{use_name} component")),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some(format!("{use_name} {snippet_params}/>")),
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

        self.completion_items
            .retain(|name, _| current_names_and_params.iter().any(|(n, _)| n == name));

        for (name, params) in current_names_and_params {
            self.completion_items
                .entry(name)
                .or_insert_with_key(|use_name| {
                    Self::use_directive_completion_item(use_name, &params)
                });
        }
    }
}
