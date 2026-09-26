use tower_lsp::lsp_types::Position;
use tree_sitter::Tree;

use crate::backend::Backend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxContext {
    /// Inside template parameter declaration `@(param: Type)`
    TemplateParams,
    /// Inside Rust code or type expressions (`Vec<String>`, `@{ ... }`, `@(...)`, `@if cond`)
    RustCode,
    /// Inside a use directive `@use "..." as Alias`
    UseDirective,
    /// Inside a component tag `<Button attr="val" />`
    ComponentTag,
    /// Inside standard HTML markup or body text
    Html,
    /// Unknown or root level
    Unknown,
}

impl SyntaxContext {
    /// Returns true if HTML component tag completions (`<Component`) should be allowed
    pub fn allows_component_completion(&self) -> bool {
        matches!(self, SyntaxContext::Html | SyntaxContext::Unknown)
    }

    /// Returns true if Rust keywords or expressions are expected
    #[allow(dead_code)]
    pub fn is_rust_context(&self) -> bool {
        matches!(
            self,
            SyntaxContext::RustCode | SyntaxContext::TemplateParams
        )
    }
}

pub fn detect_context_at_position(tree: &Tree, source: &str, position: Position) -> SyntaxContext {
    let byte_offset = Backend::position_to_byte_offset(source, position);
    let root = tree.root_node();

    // If offset is at start/end or descendant is None, default to Html
    let node = match root.descendant_for_byte_range(byte_offset, byte_offset) {
        Some(n) => n,
        None => return SyntaxContext::Html,
    };

    let mut current = Some(node);
    while let Some(n) = current {
        match n.kind() {
            "template_params" | "param" => return SyntaxContext::TemplateParams,
            "rust_text" | "rust_block" | "rust_expr_paren" | "rust_expr_simple" => {
                return SyntaxContext::RustCode;
            }
            "use_directive" => return SyntaxContext::UseDirective,
            "component_tag" => return SyntaxContext::ComponentTag,
            "html_text" => return SyntaxContext::Html,
            _ => {}
        }
        current = n.parent();
    }

    SyntaxContext::Unknown
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
        let ctx = detect_context_at_position(&tree, source, Position::new(0, 29));
        assert!(
            ctx == SyntaxContext::TemplateParams || ctx == SyntaxContext::RustCode,
            "Expected RustCode or TemplateParams inside generics, got: {:?}",
            ctx
        );
        assert!(!ctx.allows_component_completion());
    }

    #[test]
    fn test_context_in_rust_block() {
        let source = "<div>\n@{\n    let list: Vec<String> = Vec::new();\n}\n</div>";
        let tree = parse_rshtml(source);

        // Position inside `Vec<String>` (line 2, col 18)
        let ctx = detect_context_at_position(&tree, source, Position::new(2, 18));
        assert_eq!(ctx, SyntaxContext::RustCode);
        assert!(!ctx.allows_component_completion());
    }

    #[test]
    fn test_context_in_inline_rust_expr() {
        let source = "<div>\n    @(items.iter().collect::<Vec<_>>())\n</div>";
        let tree = parse_rshtml(source);

        // Position inside `::<Vec<_>>` (line 1, col 28)
        let ctx = detect_context_at_position(&tree, source, Position::new(1, 28));
        assert_eq!(ctx, SyntaxContext::RustCode);
        assert!(!ctx.allows_component_completion());
    }

    #[test]
    fn test_context_in_html_body() {
        let source = "<div class=\"container\">\n    <p>Hello</p>\n</div>";
        let tree = parse_rshtml(source);

        // Position inside html text (line 1, col 4)
        let ctx = detect_context_at_position(&tree, source, Position::new(1, 4));
        assert_eq!(ctx, SyntaxContext::Html);
        assert!(ctx.allows_component_completion());
    }

    #[test]
    fn test_context_in_use_directive() {
        let source = "@use \"components/button.rs.html\" as Button";
        let tree = parse_rshtml(source);

        // Position inside the path string (col 10)
        let ctx = detect_context_at_position(&tree, source, Position::new(0, 10));
        assert_eq!(ctx, SyntaxContext::UseDirective);
        assert!(!ctx.allows_component_completion());
    }
}

/// Checks if the cursor at `position` is immediately preceded by `self.` or `@self.`
pub fn is_preceded_by_self(source: &str, position: Position) -> bool {
    let byte_offset = Backend::position_to_byte_offset(source, position);
    if byte_offset == 0 {
        return false;
    }

    let prefix = &source[..byte_offset].trim_end();
    prefix.ends_with("@self.") || prefix.ends_with("self.") || prefix.ends_with("@self") || prefix.ends_with("self")
}

#[cfg(test)]
mod self_context_tests {
    use super::*;

    #[test]
    fn test_is_preceded_by_self() {
        let source = "<div>\n    <p>@self.</p>\n    <p>@self</p>\n    <p>self.</p>\n    <p>other.</p>\n</div>";
        
        // Line 1: "    <p>@self.</p>" -> cursor right after dot (col 13)
        assert!(is_preceded_by_self(source, Position::new(1, 13)));
        
        // Line 2: "    <p>@self</p>" -> cursor right after 'self' (col 12)
        assert!(is_preceded_by_self(source, Position::new(2, 12)));

        // Line 3: "    <p>self.</p>" -> cursor right after dot (col 12)
        assert!(is_preceded_by_self(source, Position::new(3, 12)));

        // Line 4: "    <p>other.</p>" -> cursor right after dot (col 13)
        assert!(!is_preceded_by_self(source, Position::new(4, 13)));
    }
}
