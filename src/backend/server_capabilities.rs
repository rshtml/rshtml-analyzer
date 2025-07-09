use crate::consts::{SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES};
use tower_lsp::lsp_types::{
    OneOf, SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensServerCapabilities,
    WorkDoneProgressOptions, WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};

pub fn semantic_tokens_capabilities() -> Option<SemanticTokensServerCapabilities> {
    let legend = SemanticTokensLegend {
        token_types: SEMANTIC_TOKEN_TYPES.to_vec(),
        token_modifiers: SEMANTIC_TOKEN_MODIFIERS.to_vec(),
    };

    let semantic_tokens_provider = Some(SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
        work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
        legend,
        range: Some(true),
        full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
    }));

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
