use std::collections::{HashMap, HashSet};
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
        let mut view = View::new("".to_string(), dummy_tree(), 1);
        view.use_directives = vec![
            (
                "components/button.rs.html".to_string(),
                Some("Button".to_string()),
                vec!["label".to_string()],
            ),
            ("views/header.rs.html".to_string(), None, vec![]),
        ];

        let res = view.use_directives_names_and_params();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], ("Button".to_string(), vec!["label".to_string()]));
        assert_eq!(res[1], ("header".to_string(), vec![]));
    }

    #[test]
    fn test_sync_use_directives() {
        let mut view = View::new("".to_string(), dummy_tree(), 1);

        // Initial sync
        let new_uses = view.sync_use_directives(vec![(
            "components/button.rs.html".to_string(),
            Some("Button".to_string()),
        )]);
        assert_eq!(new_uses.len(), 1);
        assert_eq!(new_uses[0], (0, "components/button.rs.html".to_string()));

        // Add dummy param to button
        view.use_directives[0].2 = vec!["variant".to_string()];

        // Sync identical list -> returns empty and keeps existing params
        let new_uses2 = view.sync_use_directives(vec![(
            "components/button.rs.html".to_string(),
            Some("Button".to_string()),
        )]);
        assert!(new_uses2.is_empty());
        assert_eq!(view.use_directives[0].2, vec!["variant".to_string()]);

        // Sync with a new directive and removing button
        let new_uses3 = view.sync_use_directives(vec![(
            "views/card.rs.html".to_string(),
            Some("Card".to_string()),
        )]);
        assert_eq!(new_uses3.len(), 1);
        assert_eq!(view.use_directives.len(), 1);
        assert_eq!(view.use_directives[0].0, "views/card.rs.html");
    }

    #[test]
    fn test_completion_items_generation() {
        let mut view = View::new("".to_string(), dummy_tree(), 1);
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
        assert_eq!(
            completion.insert_text,
            Some("Button label=\"${1:label}\" active=\"${2:active}\"/>".to_string())
        );
    }
}
