use lsp_types::Uri;
use std::{fs, path::PathBuf};
use tracing::debug;

// pub fn position_to_byte_offset(text: &str, position: Position) -> usize {
//     let mut line = 0;
//     let mut character = 0;

//     for (byte_offset, ch) in text.char_indices() {
//         if line == position.line && character == position.character {
//             return byte_offset;
//         }

//         if ch == '\n' {
//             line += 1;
//             character = 0;
//         } else if ch != '\r' {
//             character += ch.encode_utf16(&mut [0u16; 2]).len() as u32;
//         }
//     }

//     if line == position.line && character == position.character {
//         return text.len();
//     }

//     text.len()
// }

// pub fn calculate_new_end_point(start_pos: Position, text: &str) -> Point {
//     let mut new_pos = Point { row: start_pos.line as usize, column: start_pos.character as usize };

//     for (i, line) in text.lines().enumerate() {
//         if i == 0 {
//             new_pos.column += line.len();
//         } else {
//             new_pos.row += 1;
//             new_pos.column = line.len();
//         }
//     }

//     new_pos
// }

// pub fn process_changes(content_changes: Vec<TextDocumentContentChangeEvent>, source: &mut String, tree: &mut Tree) {
//     for change in content_changes {
//         match change {
//             TextDocumentContentChangeEvent::TextDocumentContentChangePartial(partial) => {
//                 let range = partial.range;
//                 let start_byte = position_to_byte_offset(source, range.start);
//                 let end_byte = position_to_byte_offset(source, range.end);

//                 let edit = tree_sitter::InputEdit {
//                     start_byte,
//                     old_end_byte: end_byte,
//                     new_end_byte: start_byte + partial.text.len(),
//                     start_position: Point { row: range.start.line as usize, column: range.start.character as usize },
//                     old_end_position: Point { row: range.end.line as usize, column: range.end.character as usize },
//                     new_end_position: calculate_new_end_point(range.start, &partial.text),
//                 };

//                 tree.edit(&edit);

//                 source.replace_range(start_byte..end_byte, &partial.text);
//             }
//             TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(whole) => {
//                 *source = whole.text;
//                 break;
//             }
//         }
//     }
// }

#[allow(dead_code)]
pub fn debug_check_source_mismatch(extract_file: &Option<(Uri, PathBuf)>, extract_source: &str) {
    if let Some((_, extract_file_path)) = extract_file {
        let Ok(extract_text) = fs::read_to_string(&extract_file_path) else {
            return;
        };

        if extract_source != extract_text {
            debug!("(DEBUG CHECK) SOURCES MISMATCH! Analyzing differences...");
            let extracted_lines: Vec<&str> = extract_source.lines().collect();
            let extract_lines: Vec<&str> = extract_text.lines().collect();
            let max_lines = std::cmp::max(extracted_lines.len(), extract_lines.len());

            for i in 0..max_lines {
                let e_line = extracted_lines.get(i).unwrap_or(&"<EOF>");
                let m_line = extract_lines.get(i).unwrap_or(&"<EOF>");

                if e_line != m_line {
                    debug!("Difference found at Line {}:", i + 1);
                    debug!("  Extract (RAM): {:?}", e_line);
                    debug!("  Extract (Disk)  : {:?}", m_line);
                    continue;
                }
            }
        } else {
            debug!("(DEBUG CHECK) sources matched perfectly");
        }
    }
}
