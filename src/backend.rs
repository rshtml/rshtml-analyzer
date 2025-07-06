mod language_server;
pub mod process_highlights;
pub mod semantic_tokens_builder;
mod server_capabilities;
mod tree_extensions;

use crate::app_state::AppState;
use std::path::PathBuf;
use tower_lsp::lsp_types::{Position, TextDocumentContentChangeEvent, Url};
use tower_lsp::Client;
use tracing::debug;
use tree_sitter::{Point, Tree};

pub struct Backend {
    pub client: Client,
    pub state: AppState,
}

impl Backend {
    pub fn new(client: Client, app_state: AppState) -> Self {
        Self { client, state: app_state }
    }

    fn position_to_byte_offset(text: &str, position: Position) -> usize {
        let mut line_counter = 0;
        let mut character_counter = 0;

        // Metnin karakterleri ve onların bayt ofsetleri üzerinde doğrudan gezinelim.
        for (current_byte_offset, ch) in text.char_indices() {
            // Hedef satıra geldik mi?
            if line_counter == position.line {
                // Hedef karaktere geldik mi?
                if character_counter == position.character {
                    // Evet, bu karakterin başlangıç bayt ofsetini döndür.
                    return current_byte_offset;
                }
                // Henüz hedef karaktere gelmedik, saymaya devam.
                character_counter += 1;
            }

            // Yeni bir satıra geçiyorsak sayaçları sıfırla.
            if ch == '\n' {
                line_counter += 1;
                // Hedef satıra yeni geçtiysek, karakter sayacı 0 olmalı.
                if line_counter == position.line {
                    character_counter = 0;
                }
            }
        }

        // Eğer döngü bittiğinde hala hedef pozisyona ulaşamadıysak,
        // bu, pozisyonun metnin son karakterinden sonra olduğu anlamına gelir.
        // (Örneğin, bir satırın sonuna yeni bir karakter ekleme durumu).
        // Bu durumda, metnin toplam uzunluğu doğru bayt ofsetidir.
        if line_counter == position.line && character_counter == position.character {
            return text.len();
        }

        // Eğer istenen satır metinde hiç yoksa (örn. boş dosyada 5. satır istenirse),
        // yine de metnin sonunu döndürmek en güvenli seçenektir.
        text.len()
    }

    fn calculate_new_end_point(start_pos: Position, text: &str) -> Point {
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

    fn process_changes(&self, content_changes: Vec<TextDocumentContentChangeEvent>, source: &mut String, tree: &mut Tree) {
        for change in content_changes {
            if let Some(range) = change.range {
                let start_byte = Self::position_to_byte_offset(&source, range.start);
                let end_byte = Self::position_to_byte_offset(&source, range.end);

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

    fn find_layout(&self, uri: &Url, layout_name: Option<&str>) -> Option<PathBuf> {
        let file_path = uri.to_file_path().ok()?;

        let workspace = self.state.workspace.read().unwrap();
        layout_name
            .and_then(|layout_name| {
                let member = workspace.get_member_by_view(&file_path)?;
                let layout_path = member.views_path.join(layout_name);
                Some(layout_path)
            })
            .or_else(|| workspace.get_layout_path_by_view(&file_path))
    }
}
