pub mod view;
pub mod workspace;

use crate::app_state::view::View;
use crate::app_state::workspace::Workspace;
use crate::handler::tree_extensions::TreeExtensions;
use crate::rust_analyzer_client::RustAnalyzerClient;
use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Uri};
use std::collections::HashMap;
use tree_sitter::{Language, Parser};

pub struct AppState {
    pub workspace: Workspace,
    pub parser: Parser,
    pub views: HashMap<String, View>,
    pub completion_items: Vec<CompletionItem>,
    pub language: Language,
    pub ra_client: Option<RustAnalyzerClient>,
}

impl AppState {
    pub fn new(parser: Parser, completion_items: Vec<CompletionItem>, language: Language) -> Self {
        Self {
            workspace: Workspace::default(),
            parser,
            views: HashMap::new(),
            completion_items,
            language,
            ra_client: None,
        }
    }

    pub fn setup() -> Self {
        let mut parser = Parser::new();

        let lang = Language::new(tree_sitter_rshtml::LANGUAGE);
        parser.set_language(&lang).unwrap();

        Self::new(parser, Self::completion_items(), lang)
    }

    pub fn find_use_params(&mut self, use_full_uri: &Uri) -> Option<Vec<String>> {
        let use_uri_str = use_full_uri.to_string();
        if let Some(view) = self.views.get(&use_uri_str) {
            return Some(view.template_params.clone());
        }

        let source_bytes = std::fs::read(use_full_uri.to_file_path().ok()?).ok()?;
        let source = std::str::from_utf8(&source_bytes).ok()?;

        if let Some(tree) = self.parser.parse(&source_bytes, None) {
            return Some(tree.find_template_params(&self.language, source));
        }

        None
    }

    fn completion_items() -> Vec<CompletionItem> {
        let if_ = CompletionItem {
            label: "if".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            detail: Some("if statement".to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            insert_text: Some("if ${1:condition} {\n\t$0\n}".to_string()),
            sort_text: Some("01".to_string()),
            ..Default::default()
        };

        let for_ = CompletionItem {
            label: "for".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            insert_text: Some("for ${1:item} in ${2:items} {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            detail: Some("for loop".to_string()),
            sort_text: Some("02".to_string()),
            ..Default::default()
        };

        let rust_expr_paren_ = CompletionItem {
            label: "@()".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            insert_text: Some("($0)".to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            detail: Some("rust expression.".to_string()),
            sort_text: Some("04".to_string()),
            ..Default::default()
        };

        let use_as_ = CompletionItem {
            label: "use .. as".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            insert_text: Some(r#"use "${1:path/to/component.rs.html}" as ${2:Component}"#.to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            detail: Some("use .. as directive".to_string()),
            sort_text: Some("06".to_string()),
            ..Default::default()
        };

        let use_ = CompletionItem {
            label: "use".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            insert_text: Some(r#"use "${1:path/to/component.rs.html}""#.to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            detail: Some("use directive".to_string()),
            sort_text: Some("07".to_string()),
            ..Default::default()
        };

        let child_content_ = CompletionItem {
            label: "child_content".to_string(),
            kind: Some(CompletionItemKind::Keyword),
            insert_text: Some("child_content()".to_string()),
            insert_text_format: Some(InsertTextFormat::PlainText),
            detail: Some("child content directive".to_string()),
            sort_text: Some("10".to_string()),
            ..Default::default()
        };

        let rust_block_ = CompletionItem {
            label: "@{}".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            insert_text: Some("{\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            detail: Some("rust code block.".to_string()),
            sort_text: Some("11".to_string()),
            ..Default::default()
        };

        let while_ = CompletionItem {
            label: "while".to_string(),
            kind: Some(CompletionItemKind::Snippet),
            insert_text: Some("while ${1:condition} {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::Snippet),
            detail: Some("while loop".to_string()),
            sort_text: Some("12".to_string()),
            ..Default::default()
        };

        vec![if_, for_, while_, use_as_, use_, child_content_, rust_block_, rust_expr_paren_]
    }
}
