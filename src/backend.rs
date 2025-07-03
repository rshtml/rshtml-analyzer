mod language_server;
pub mod process_highlights;
pub mod semantic_tokens_builder;
mod server_capabilities;
mod tree_extensions;

use crate::app_state::AppState;
use tower_lsp::Client;
use tower_lsp::lsp_types::Position;
use tree_sitter::Point;

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
    
    // pub fn find_manifest_dir(&self, workspace_root: &Path, file_path: &Path) -> Option<PathBuf> {
    //     // path.ancestors()
    //     //     .find(|p| p.join("Cargo.toml").exists())
    //     //     .map(|p| p.to_path_buf())
    //
    //     if !file_path.starts_with(workspace_root) {
    //         return None;
    //     }
    //
    //     let starting_dir = if file_path.is_dir() {
    //         file_path
    //     } else {
    //         file_path.parent()?
    //     };
    //
    //     for current_dir in starting_dir.ancestors() {
    //         if current_dir.join("Cargo.toml").exists() {
    //             return Some(current_dir.to_path_buf());
    //         }
    //
    //         if current_dir == workspace_root {
    //             break;
    //         }
    //     }
    //
    //     None
    // }
}
