pub mod context;
mod language_server;
pub mod navigation;
pub mod semantic_tokens_builder;
mod server_capabilities;
pub mod tree_extensions;

use crate::app_state::AppState;
use std::path::PathBuf;
use tower_lsp::Client;
use tower_lsp::lsp_types::{Position, TextDocumentContentChangeEvent, Url};
use tree_sitter::{Point, Tree};

pub struct Backend {
    pub client: Client,
    pub state: AppState,
}

impl Backend {
    pub fn new(client: Client, app_state: AppState) -> Self {
        Self {
            client,
            state: app_state,
        }
    }

    pub async fn get_views_path_for_uri(&self, uri: &Url) -> Option<PathBuf> {
        let file_path = uri.to_file_path().ok()?;
        let workspace = self.state.workspace.read().await;
        let member = workspace.get_member_by_view(&file_path)?;
        Some(member.views_path.clone())
    }

    pub fn position_to_byte_offset(text: &str, position: Position) -> usize {
        let mut line = 0;
        let mut character = 0;

        for (byte_offset, ch) in text.char_indices() {
            if line == position.line && character == position.character {
                return byte_offset;
            }

            if ch == '\n' {
                line += 1;
                character = 0;
            } else if ch != '\r' {
                character += ch.encode_utf16(&mut [0u16; 2]).len() as u32;
            }
        }

        if line == position.line && character == position.character {
            return text.len();
        }

        text.len()
    }

    pub fn calculate_new_end_point(start_pos: Position, text: &str) -> Point {
        let mut new_pos = Point {
            row: start_pos.line as usize,
            column: start_pos.character as usize,
        };

        for (i, line) in text.lines().enumerate() {
            if i == 0 {
                new_pos.column += line.len();
            } else {
                new_pos.row += 1;
                new_pos.column = line.len();
            }
        }

        new_pos
    }

    pub fn process_changes(
        &self,
        content_changes: Vec<TextDocumentContentChangeEvent>,
        source: &mut String,
        tree: &mut Tree,
    ) {
        for change in content_changes {
            if let Some(range) = change.range {
                let start_byte = Self::position_to_byte_offset(source, range.start);
                let end_byte = Self::position_to_byte_offset(source, range.end);

                let edit = tree_sitter::InputEdit {
                    start_byte,
                    old_end_byte: end_byte,
                    new_end_byte: start_byte + change.text.len(),
                    start_position: Point {
                        row: range.start.line as usize,
                        column: range.start.character as usize,
                    },
                    old_end_position: Point {
                        row: range.end.line as usize,
                        column: range.end.character as usize,
                    },
                    new_end_position: Self::calculate_new_end_point(range.start, &change.text),
                };

                tree.edit(&edit);

                source.replace_range(start_byte..end_byte, &change.text);
            } else {
                *source = change.text;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Range;
    use tree_sitter::Parser;

    #[test]
    fn test_position_to_byte_offset_ascii() {
        let text = "hello\nworld\nrshtml";
        // Line 0, char 0 -> 0
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(0, 0)),
            0
        );
        // Line 0, char 5 -> 5 ('\n')
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(0, 5)),
            5
        );
        // Line 1, char 0 -> 6 ('w')
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(1, 0)),
            6
        );
        // Line 2, char 6 -> 18 (end of file)
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(2, 6)),
            18
        );
        // Position past the end falls back to text.len()
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(10, 0)),
            18
        );
    }

    #[test]
    fn test_position_to_byte_offset_multibyte_utf16() {
        // '🦀' is 4 bytes in UTF-8, but 2 UTF-16 code units (surrogate pair)
        // 'á' is 2 bytes in UTF-8, 1 UTF-16 code unit
        let text = "🦀 á";
        // At start
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(0, 0)),
            0
        );
        // After '🦀' (2 UTF-16 code units) -> byte offset 4
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(0, 2)),
            4
        );
        // After space (2 + 1 = 3 UTF-16 units) -> byte offset 5
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(0, 3)),
            5
        );
        // After 'á' (3 + 1 = 4 UTF-16 units) -> byte offset 7 (end of text)
        assert_eq!(
            Backend::position_to_byte_offset(text, Position::new(0, 4)),
            7
        );
    }

    #[test]
    fn test_calculate_new_end_point() {
        let start = Position::new(2, 4);

        // Single line inserted
        let pt_single = Backend::calculate_new_end_point(start, "hello");
        assert_eq!(pt_single.row, 2);
        assert_eq!(pt_single.column, 9);

        // Multiline inserted
        let pt_multi = Backend::calculate_new_end_point(start, "hello\nworld\n!");
        assert_eq!(pt_multi.row, 4);
        assert_eq!(pt_multi.column, 1);
    }

    #[test]
    fn test_process_changes_incremental() {
        let mut parser = Parser::new();
        let lang = tree_sitter_rshtml::LANGUAGE.into();
        parser.set_language(&lang).unwrap();

        let initial_text = "<div>hello</div>".to_string();
        let mut tree = parser.parse(&initial_text, None).unwrap();
        let mut source = initial_text;

        // Replace "hello" with "world!" (range: line 0, col 5 to line 0, col 10)
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position::new(0, 5),
                end: Position::new(0, 10),
            }),
            range_length: None,
            text: "world!".to_string(),
        };

        let (service, _) =
            tower_lsp::LspService::new(|client| Backend::new(client, AppState::setup()));
        let backend = service.inner();

        backend.process_changes(vec![change], &mut source, &mut tree);

        assert_eq!(source, "<div>world!</div>");

        // Reparsing with the edited tree should succeed
        let new_tree = parser.parse(&source, Some(&tree)).unwrap();
        assert_eq!(
            new_tree.root_node().to_sexp(),
            parser.parse(&source, None).unwrap().root_node().to_sexp()
        );
    }

    #[test]
    fn test_process_changes_full() {
        let mut parser = Parser::new();
        let lang = tree_sitter_rshtml::LANGUAGE.into();
        parser.set_language(&lang).unwrap();

        let initial_text = "<div>initial</div>".to_string();
        let mut tree = parser.parse(&initial_text, None).unwrap();
        let mut source = initial_text;

        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "<div>replacement</div>".to_string(),
        };

        let (service, _) =
            tower_lsp::LspService::new(|client| Backend::new(client, AppState::setup()));
        let backend = service.inner();

        backend.process_changes(vec![change], &mut source, &mut tree);
        assert_eq!(source, "<div>replacement</div>");
    }
}
