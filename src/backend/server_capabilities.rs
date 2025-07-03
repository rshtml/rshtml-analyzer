use tower_lsp::lsp_types::{
    OneOf,
    SemanticTokenModifier, SemanticTokenType, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensServerCapabilities, WorkDoneProgressOptions
    , WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities,
};

pub fn semantic_tokens_capabilities() -> Option<SemanticTokensServerCapabilities> {
    pub const HTML: SemanticTokenType = SemanticTokenType::new("html");
    pub const RUST: SemanticTokenType = SemanticTokenType::new("rust");

    let legend = SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::TYPE,
            SemanticTokenType::CLASS,
            SemanticTokenType::ENUM,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::STRUCT,
            SemanticTokenType::TYPE_PARAMETER,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::ENUM_MEMBER,
            SemanticTokenType::EVENT,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::METHOD,
            SemanticTokenType::MACRO,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::MODIFIER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::REGEXP,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::DECORATOR,
            HTML,
            RUST,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::STATIC,
            SemanticTokenModifier::DEPRECATED,
            SemanticTokenModifier::ABSTRACT,
            SemanticTokenModifier::ASYNC,
            SemanticTokenModifier::MODIFICATION,
            SemanticTokenModifier::DOCUMENTATION,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
    };

    let semantic_tokens_provider = Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
        SemanticTokensOptions {
            work_done_progress_options: WorkDoneProgressOptions {
                work_done_progress: None,
            },
            legend,
            range: Some(false),
            full: Some(SemanticTokensFullOptions::Delta { delta: Some(false) }),
        },
    ));

    semantic_tokens_provider
}

pub fn workspace_capabilities() -> Option<WorkspaceServerCapabilities> {
    Some(WorkspaceServerCapabilities {
        workspace_folders: Some(WorkspaceFoldersServerCapabilities {
            supported: Some(false),
            change_notifications: Some(OneOf::Left(true)),
        }),

        file_operations: None,
    })
}
