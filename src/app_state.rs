mod highlight;
pub mod view;
pub mod workspace;

use crate::app_state::highlight::Highlight;
use crate::app_state::view::View;
use crate::app_state::workspace::Workspace;
use crate::backend::tree_extensions::TreeExtensions;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Url};
use tree_sitter::{Language, Parser};
use tree_sitter_highlight::HighlightConfiguration;

pub struct AppState {
    pub workspace: RwLock<Workspace>,
    pub parser: Mutex<Parser>,
    pub highlight: Highlight,
    pub views: Arc<RwLock<HashMap<String, View>>>,
    pub completion_items: Vec<CompletionItem>,
    pub language: Language,
}

impl AppState {
    pub fn new(
        parser: Parser,
        highlight: Highlight,
        completion_items: Vec<CompletionItem>,
        language: Language,
    ) -> Self {
        Self {
            workspace: RwLock::new(Workspace::default()),
            parser: Mutex::new(parser),
            highlight,
            views: Arc::new(RwLock::new(HashMap::new())),
            completion_items,
            language,
        }
    }

    pub(crate) fn setup() -> Self {
        let mut parser = Parser::new();

        let lang = Language::new(tree_sitter_rshtml::LANGUAGE);
        parser.set_language(&lang).unwrap();

        let mut highlight_config = HighlightConfiguration::new(
            lang.clone(),
            "rshtml",
            include_str!("../queries/rshtml/highlights.scm"),
            include_str!("../queries/rshtml/injections.scm"),
            "",
        )
        .unwrap();

        // Rust highlights
        let mut highlight_config_rust = HighlightConfiguration::new(
            Language::new(tree_sitter_rust::LANGUAGE),
            "rust",
            include_str!("../queries/rust/highlights.scm"),
            include_str!("../queries/rust/injections.scm"),
            "",
        )
        .unwrap();

        // Html highlights
        // let mut highlight_config_html = HighlightConfiguration::new(
        //     Language::new(tree_sitter_html::LANGUAGE),
        //     "html",
        //     include_str!("../queries/html/highlights.scm"),
        //     include_str!("../queries/html/injections.scm"),
        //     "",
        // )
        // .unwrap();

        let mut cn: HashSet<String> = highlight_config
            .names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cnr: HashSet<String> = highlight_config_rust
            .names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        // let cnh: HashSet<String> = highlight_config_html
        //     .names()
        //     .iter()
        //     .map(|s| s.to_string())
        //     .collect();

        cn.extend(cnr);
        // cn.extend(cnh);
        let final_capture_names: Vec<String> = cn.into_iter().collect();

        highlight_config.configure(final_capture_names.as_ref());

        highlight_config_rust.configure(final_capture_names.as_ref());
        // highlight_config_html.configure(final_capture_names.as_ref());

        let mut highlights = Highlight::new(highlight_config, final_capture_names.clone());
        highlights
            .highlight_injects
            .insert("rust", highlight_config_rust);
        // highlights
        //     .highlight_injects
        //     .insert("html", highlight_config_html);

        Self::new(parser, highlights, Self::completion_items(), lang)
    }

    pub async fn find_use_params(&self, use_full_path: &Path) -> Option<Vec<String>> {
        {
            let use_uri_str = Url::from_file_path(use_full_path).ok()?.to_string();
            let views = self.views.read().await;
            if let Some(view) = views.get(&use_uri_str) {
                return Some(view.template_params.clone());
            }
        }

        let source_bytes = tokio::fs::read(use_full_path).await.ok()?;
        let source = str::from_utf8(&source_bytes).ok()?;

        let mut parser = self.parser.lock().await;
        if let Some(tree) = parser.parse(&source_bytes, None) {
            return Some(tree.find_template_params(&self.language, source));
        }

        None
    }

    fn completion_items() -> Vec<CompletionItem> {
        let if_ = CompletionItem {
            label: "if".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("if statement".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("if ${1:condition} {\n\t$0\n}".to_string()),
            sort_text: Some("01".to_string()),
            ..Default::default()
        };

        let for_ = CompletionItem {
            label: "for".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("for ${1:item} in ${2:items} {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("for loop".to_string()),
            sort_text: Some("02".to_string()),
            ..Default::default()
        };

        let match_ = CompletionItem {
            label: "match".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some(
                "match ${1:expression} {\n\t${2:pattern} => {\n\t\t$0\n\t},\n}".to_string(),
            ),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("match statement".to_string()),
            sort_text: Some("03".to_string()),
            ..Default::default()
        };

        let rust_expr_paren_ = CompletionItem {
            label: "@()".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("($0)".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("rust expression.".to_string()),
            sort_text: Some("04".to_string()),
            ..Default::default()
        };

        let use_as_ = CompletionItem {
            label: "use .. as".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some(
                r#"use "${1:path/to/component.rs.html}" as ${2:Component}"#.to_string(),
            ),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("use .. as directive".to_string()),
            sort_text: Some("06".to_string()),
            ..Default::default()
        };

        let use_ = CompletionItem {
            label: "use".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some(r#"use "${1:path/to/component.rs.html}""#.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("use directive".to_string()),
            sort_text: Some("07".to_string()),
            ..Default::default()
        };

        let child_content_ = CompletionItem {
            label: "child_content".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            insert_text: Some("child_content()".to_string()),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            detail: Some("child content directive".to_string()),
            sort_text: Some("10".to_string()),
            ..Default::default()
        };

        let rust_block_ = CompletionItem {
            label: "@{}".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("{\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("rust code block.".to_string()),
            sort_text: Some("11".to_string()),
            ..Default::default()
        };

        let while_ = CompletionItem {
            label: "while".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text: Some("while ${1:condition} {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            detail: Some("while loop".to_string()),
            sort_text: Some("12".to_string()),
            ..Default::default()
        };

        vec![
            if_,
            for_,
            while_,
            match_,
            use_as_,
            use_,
            child_content_,
            rust_block_,
            rust_expr_paren_,
        ]
    }
}
