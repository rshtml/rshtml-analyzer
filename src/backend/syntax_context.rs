use crate::backend::Backend;
use tower_lsp::lsp_types::Position;
use tree_sitter::Tree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxContext {
    TemplateParams,
    RustCode,
    UseDirective,
    ComponentTag,
    ComponentParameter(String),
    ComponentParameterValue,
    Html,
}

impl SyntaxContext {
    pub fn detect(tree: &Tree, source: &str, position: Position) -> Self {
        let byte_offset = Backend::position_to_byte_offset(source, position);
        let root = tree.root_node();

        let node = match root.descendant_for_byte_range(byte_offset, byte_offset) {
            Some(n) => n,
            None => return Self::Html,
        };

        let mut current = Some(node);
        while let Some(n) = current {
            match n.kind() {
                "template_params" | "param" => return Self::TemplateParams,
                "rust_text" | "rust_block" | "rust_expr_paren" | "rust_expr_simple" => {
                    return Self::RustCode;
                }
                "use_directive" => return Self::UseDirective,
                "component_tag_identifier" => return Self::ComponentTag,
                "component_tag" => {
                    let mut in_param_zone = false;
                    let mut past_body_or_close = false;
                    let mut component_name = String::new();

                    let mut cursor = n.walk();
                    for child in n.children(&mut cursor) {
                        if child.kind() == "component_tag_identifier"
                            && byte_offset > child.end_byte()
                        {
                            if let Ok(text) = child.utf8_text(source.as_bytes()) {
                                component_name = text.to_string();
                            }

                            in_param_zone = true;
                        }

                        if child.kind() == "component_tag_body" || child.kind() == "tag_end_open" {
                            if byte_offset >= child.start_byte() {
                                in_param_zone = false;
                                past_body_or_close = true;
                            }
                            break;
                        }
                    }

                    if in_param_zone {
                        return Self::ComponentParameter(component_name);
                    } else if past_body_or_close {
                        return Self::Html;
                    } else {
                        return Self::ComponentTag;
                    }
                }
                "component_tag_parameter" => {
                    let mut after_equals = false;
                    let mut cursor = n.walk();

                    for child in n.children(&mut cursor) {
                        if child.kind() == "equals" {
                            if byte_offset >= child.end_byte() {
                                after_equals = true;
                            }
                            break;
                        }
                    }

                    if after_equals {
                        return Self::ComponentParameterValue;
                    } else {
                        let mut component_name = String::new();

                        let mut current_parent = n.parent();
                        while let Some(parent) = current_parent {
                            if parent.kind() == "component_tag" {
                                if let Some(name_node) = parent.child_by_field_name("name")
                                    && let Ok(text) = name_node.utf8_text(source.as_bytes())
                                {
                                    component_name = text.to_string();
                                }
                                break;
                            }
                            current_parent = parent.parent();
                        }

                        return Self::ComponentParameter(component_name);
                    }
                }
                "html_text" => return Self::Html,
                _ => {}
            }
            current = n.parent();
        }

        Self::Html
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_rshtml(source: &str) -> Tree {
        let mut parser = Parser::new();
        let lang = tree_sitter_rshtml::LANGUAGE.into();
        parser.set_language(&lang).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn test_context_in_template_params_with_generics() {
        // Line 0: @(title: String, items: Vec<String>)
        let source = "@(title: String, items: Vec<String>)";
        let tree = parse_rshtml(source);

        // Position inside `Vec<` (line 0, col 29)
        let ctx = SyntaxContext::detect(&tree, source, Position::new(0, 29));
        assert!(
            ctx == SyntaxContext::TemplateParams || ctx == SyntaxContext::RustCode,
            "Expected RustCode or TemplateParams inside generics, got: {:?}",
            ctx
        );
    }

    #[test]
    fn test_context_in_rust_block() {
        let source = "<div>\n@{\n    let list: Vec<String> = Vec::new();\n}\n</div>";
        let tree = parse_rshtml(source);

        // Position inside `Vec<String>` (line 2, col 18)
        let ctx = SyntaxContext::detect(&tree, source, Position::new(2, 18));
        assert_eq!(ctx, SyntaxContext::RustCode);
    }

    #[test]
    fn test_context_in_inline_rust_expr() {
        let source = "<div>\n    @(items.iter().collect::<Vec<_>>())\n</div>";
        let tree = parse_rshtml(source);

        // Position inside `::<Vec<_>>` (line 1, col 28)
        let ctx = SyntaxContext::detect(&tree, source, Position::new(1, 28));
        assert_eq!(ctx, SyntaxContext::RustCode);
    }

    #[test]
    fn test_context_in_html_body() {
        let source = "<div class=\"container\">\n    <p>Hello</p>\n</div>";
        let tree = parse_rshtml(source);

        // Position inside html text (line 1, col 4)
        let ctx = SyntaxContext::detect(&tree, source, Position::new(1, 4));
        assert_eq!(ctx, SyntaxContext::Html);
    }

    #[test]
    fn test_context_in_use_directive() {
        let source = "@use \"components/button.rs.html\" as Button";
        let tree = parse_rshtml(source);

        // Position inside the path string (col 10)
        let ctx = SyntaxContext::detect(&tree, source, Position::new(0, 10));
        assert_eq!(ctx, SyntaxContext::UseDirective);
    }
}
