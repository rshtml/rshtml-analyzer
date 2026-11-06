use lsp_types::Position;

pub fn position_to_byte_offset(text: &str, position: Position) -> usize {
    let target_character = position.character as usize;
    let mut line = 0;
    let mut character = 0;

    for (byte_offset, ch) in text.char_indices() {
        if line == position.line && character == target_character {
            return byte_offset;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else if ch != '\r' {
            character += ch.len_utf16();
        }
    }

    if line == position.line && character == target_character {
        return text.len();
    }

    text.len()
}
