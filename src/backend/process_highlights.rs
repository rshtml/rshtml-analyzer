use crate::backend::semantic_tokens_builder::SemanticTokensBuilder;
use std::collections::HashMap;
use std::ops::Range;
use tower_lsp::jsonrpc::Error;
use tower_lsp::lsp_types::SemanticToken;
use tree_sitter_highlight::{Highlight, HighlightEvent};

pub fn process_highlights(
    source: &str,
    highlights: impl Iterator<Item = Result<HighlightEvent, tree_sitter_highlight::Error>>,
    highlight_names: &[&str],
    token_type_map: &HashMap<&'static str, u32>,
    range: Option<Range<usize>>,
) -> Result<Vec<SemanticToken>, Error> {
    let mut builder = SemanticTokensBuilder::new();
    let mut highlight_stack: Vec<Highlight> = Vec::new();

    // Byte offset'ten (satır, sütun)'a çevirme işlemini verimli kılmak için
    // satır başlangıçlarının byte offset'lerini önceden hesaplayalım.
    let line_starts: Vec<usize> = std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect();

    for event_result in highlights {
        if let Ok(event) = event_result {
            match event {
                HighlightEvent::Source { start, end } => {
                    if let Some(ref r) = range {
                        // Check for overlap: [start, end) and [r.start, r.end)
                        if start >= r.end || end <= r.start {
                            continue; // No overlap, skip this event                                          
                        }
                    }
                    if let Some(highlight_id) = highlight_stack.last() {
                        //let hi = highlight_id.0;
                        //println!("highlight_id: {}, start: {}, end: {}", hi, start, end);
                        // 1. Gerekli Çeviri: Tree-sitter ID'sini LSP türüne çevir.
                        let token_type = map_highlight_to_lsp_type(*highlight_id, highlight_names, token_type_map);

                        // 2. İsteğe Bağlı Özellik: Modifier'ları şimdilik atlıyoruz (0).
                        let token_modifiers = 0;

                        // 3. Gerekli Çeviri: Byte offset'i satır ve sütuna çevir.
                        // Çok satırlı token'ları doğru işlemek için metni satır satır gezelim.
                        let text_span = &source[start..end];
                        let (mut current_line, mut current_col) = byte_to_line_col(start, &line_starts, source);

                        for (i, line_content) in text_span.lines().enumerate() {
                            if i > 0 {
                                current_line += 1;
                                current_col = 0;
                            }

                            if line_content.is_empty() {
                                continue;
                            }

                            // LSP genellikle UTF-16 kod birimlerini sütun olarak sayar.
                            let length = line_content.encode_utf16().count() as u32;
                            builder.push_token(current_line, current_col, length, token_type, token_modifiers);
                            // Bir sonraki token için sütunu güncelle (göreceli olduğu için builder bunu yapıyor zaten)
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
            println!("Highlight işleme hatası: {:?}", event_result);
            tracing::error!("Highlight işleme hatası: {:?}", event_result);
        }
    }
    Ok(builder.build())
}

/// GEREKLİ: Tree-sitter highlight adını LSP token türüne çevirir.
fn map_highlight_to_lsp_type(highlight_id: Highlight, highlight_names: &[&str], token_type_map: &HashMap<&'static str, u32>) -> u32 {
    let highlight_name = &highlight_names[highlight_id.0];
    //tracing::info!("Highlight yakalandı: '{}'", highlight_name);

    let base_name = highlight_name.split('.').next().unwrap_or(highlight_name);

    let lsp_type_name = match base_name {
        "keyword" => "keyword",
        "comment" => "comment",
        "string" => "string",
        "number" => "number",
        "operator" => "operator",
        "property" => "property",
        "type" => "type",
        "class" | "struct" | "enum" | "interface" => "type",
        "constructor" => "method",
        "function" => {
            if *highlight_name == "function.method" {
                "method"
            } else if *highlight_name == "function.macro" {
                "macro"
            } else {
                "function"
            }
        }
        "variable" => {
            if *highlight_name == "variable.parameter" {
                "parameter"
            } else {
                "variable"
            }
        }
        "constant" | "boolean" => "variable",
        "attribute" | "tag" => "decorator",
        "label" => "namespace",
        "punctuation" => "operator",
        _ => "variable",
    };

    token_type_map.get(lsp_type_name).copied().unwrap_or(1)
}

/// GEREKLİ YARDIMCI: Verilen byte offset'ini (satır, sütun) çiftine dönüştürür.
/// Daha verimli olması için önceden hesaplanmış `line_starts` dizisini kullanır.
fn byte_to_line_col(byte_offset: usize, line_starts: &[usize], source: &str) -> (u32, u32) {
    // `line_starts` dizisinde, `byte_offset`'ten küçük veya eşit olan son elemanı bul.
    // Bu bize satır numarasını verir. `partition_point` bunun için çok verimlidir.
    let line = line_starts.partition_point(|&start| start <= byte_offset) - 1;

    let line_start_byte = line_starts[line];

    // Sütun, satırın başından itibaren olan karakter sayısıdır (UTF-16 birimleriyle).
    let col = source[line_start_byte..byte_offset].encode_utf16().count();

    (line as u32, col as u32)
}
