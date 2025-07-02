use tower_lsp::lsp_types::SemanticToken;

pub struct SemanticTokensBuilder {
    tokens: Vec<SemanticToken>,
    prev_line: u32,
    prev_char: u32,
}

impl SemanticTokensBuilder {
    pub fn new() -> Self {
        Self {
            tokens: Vec::new(),
            prev_line: 0,
            prev_char: 0,
        }
    }

    pub fn push_token(&mut self, line: u32, char: u32, length: u32, token_type: u32, token_modifiers: u32) {
        let delta_line = line - self.prev_line;
        let delta_char = if delta_line == 0 { char - self.prev_char } else { char };

        self.tokens.push(SemanticToken {
            delta_line,
            delta_start: delta_char,
            length,
            token_type,
            token_modifiers_bitset: token_modifiers,
        });

        self.prev_line = line;
        self.prev_char = char;
    }

    pub fn build(self) -> Vec<SemanticToken> {
        self.tokens
    }
}
