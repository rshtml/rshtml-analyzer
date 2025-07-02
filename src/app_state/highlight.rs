use std::collections::HashMap;
use std::sync::Mutex;
use tree_sitter_highlight::{HighlightConfiguration, Highlighter};
use crate::consts::{TOKEN_MODIFIERS, TOKEN_TYPES};

pub struct Highlight {
    pub highlighter: Mutex<Highlighter>,
    pub highlight_config: HighlightConfiguration,
    pub highlight_injects: HashMap<&'static str, HighlightConfiguration>,
    pub highlight_names: Vec<String>,
    pub token_type_map: HashMap<&'static str, u32>,
    //pub token_modifier_map: HashMap<&'static str, u32>,
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

    fn build_token_type_map() -> HashMap<&'static str, u32> {
        TOKEN_TYPES
            .iter()
            .enumerate()
            .map(|(i, &token)| (token, i as u32))
            .collect()
    }

    #[allow(dead_code)]
    fn build_token_modifier_map() -> HashMap<&'static str, u32> {
        TOKEN_MODIFIERS
            .iter()
            .enumerate()
            .map(|(i, &modifier)| (modifier, i as u32))
            .collect()
    }
}
