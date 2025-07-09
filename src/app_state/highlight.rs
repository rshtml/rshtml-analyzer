use crate::backend::semantic_tokens_builder::SemanticTokensBuilder;
use crate::consts::{SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Mutex;
use tower_lsp::jsonrpc::{Error, ErrorCode};
use tower_lsp::lsp_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType};
use tracing::error;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

pub struct Highlight {
    pub highlighter: Mutex<Highlighter>,
    pub highlight_config: HighlightConfiguration,
    pub highlight_injects: HashMap<&'static str, HighlightConfiguration>,
    pub highlight_names: Vec<String>,
    pub token_type_map: HashMap<SemanticTokenType, u32>,
    //pub token_modifier_map: HashMap<SemanticTokenModifier, u32>,
}

impl Highlight {
    pub fn new(highlight_config: HighlightConfiguration, highlight_names: Vec<String>) -> Self {
        Self {
            highlighter: Mutex::new(Highlighter::default()),
            highlight_config,
            highlight_names,
            highlight_injects: HashMap::new(),
            token_type_map: Self::build_token_type_map(),
            //token_modifier_map: Self::build_token_modifier_map(),
        }
    }

    fn build_token_type_map() -> HashMap<SemanticTokenType, u32> {
        SEMANTIC_TOKEN_TYPES
            .iter()
            .enumerate()
            .map(|(i, token)| (token.clone(), i as u32))
            .collect()
    }

    #[allow(dead_code)]
    fn build_token_modifier_map() -> HashMap<SemanticTokenModifier, u32> {
        SEMANTIC_TOKEN_MODIFIERS
            .iter()
            .enumerate()
            .map(|(i, modifier)| (modifier.clone(), i as u32))
            .collect()
    }

    pub fn highlight(&self, source: &str, range: Option<Range<usize>>) -> Result<Vec<SemanticToken>, Error> {
        if let Ok(mut highlighter) = self.highlighter.lock() {
            let highlight_events = highlighter
                .highlight(&self.highlight_config, source.as_bytes(), None, |lang_name| {
                    self.highlight_injects.get(lang_name)
                })
                .map_err(|e| {
                    error!("Error during highlighting: {}", e);
                    Error::new(ErrorCode::InternalError)
                })?;

            self.highlight_events_to_semantic_tokens(highlight_events, source, range)
        } else {
            error!("Failed to lock highlighter mutex");
            Ok(vec![])
        }
    }

    fn highlight_events_to_semantic_tokens(
        &self,
        highlight_events: impl Iterator<Item = Result<HighlightEvent, tree_sitter_highlight::Error>>,
        source: &str,
        range: Option<Range<usize>>,
    ) -> Result<Vec<SemanticToken>, Error> {
        let mut builder = SemanticTokensBuilder::new();
        let mut highlight_stack: Vec<tree_sitter_highlight::Highlight> = Vec::new();

        let line_starts: Vec<usize> = std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect();

        for highlight_event in highlight_events {
            if let Ok(highlight_event) = highlight_event {
                match highlight_event {
                    HighlightEvent::Source { start, end } => {
                        if let Some(ref r) = range {
                            if start >= r.end || end <= r.start {
                                continue;
                            }
                        }

                        if let Some(highlight_id) = highlight_stack.last() {
                            let token_type = self.ts_highlight_to_lsp_type(*highlight_id);

                            let token_modifiers = 0;

                            let text_span = &source[start..end];
                            let (mut current_line, mut current_col) = self.byte_to_line_col(start, &line_starts, source);

                            for (i, line_content) in text_span.lines().enumerate() {
                                if i > 0 {
                                    current_line += 1;
                                    current_col = 0;
                                }

                                if line_content.is_empty() {
                                    continue;
                                }

                                let length = line_content.encode_utf16().count() as u32;
                                builder.push_token(current_line, current_col, length, token_type, token_modifiers);
                            }
                        }
                    }
                    HighlightEvent::HighlightStart(highlight_id) => {
                        highlight_stack.push(highlight_id);
                    }
                    HighlightEvent::HighlightEnd => {
                        highlight_stack.pop();
                    }
                }
            } else {
                error!("Highlight process error: {:?}", highlight_event);
            }
        }

        Ok(builder.build())
    }

    fn ts_highlight_to_lsp_type(&self, highlight_id: tree_sitter_highlight::Highlight) -> u32 {
        let highlight_name = &self.highlight_names[highlight_id.0].as_str();
        let base_name = highlight_name.split('.').next().unwrap_or(highlight_name);

        let lsp_type_name = match base_name {
            "keyword" => SemanticTokenType::KEYWORD,
            "comment" => SemanticTokenType::COMMENT,
            "string" => SemanticTokenType::STRING,
            "number" => SemanticTokenType::NUMBER,
            "operator" => SemanticTokenType::OPERATOR,
            "property" => SemanticTokenType::PROPERTY,
            "type" | "class" | "struct" | "enum" | "interface" => SemanticTokenType::TYPE,
            "constructor" => SemanticTokenType::METHOD,
            "function" => match *highlight_name {
                "function.method" => SemanticTokenType::METHOD,
                "function.macro" => SemanticTokenType::MACRO,
                _ => SemanticTokenType::FUNCTION,
            },
            "variable" => match *highlight_name {
                "variable.parameter" => SemanticTokenType::PARAMETER,
                _ => SemanticTokenType::VARIABLE,
            },
            "constant" | "boolean" => SemanticTokenType::VARIABLE,
            "attribute" | "tag" => SemanticTokenType::DECORATOR,
            "label" => SemanticTokenType::NAMESPACE,
            "punctuation" => SemanticTokenType::OPERATOR,
            _ => SemanticTokenType::VARIABLE,
        };

        self.token_type_map.get(&lsp_type_name).copied().unwrap_or(1)
    }

    fn byte_to_line_col(&self, byte_offset: usize, line_starts: &[usize], source: &str) -> (u32, u32) {
        let line = line_starts.partition_point(|&start| start <= byte_offset) - 1;

        let line_start_byte = line_starts[line];

        let col = source[line_start_byte..byte_offset].encode_utf16().count();

        (line as u32, col as u32)
    }
}
