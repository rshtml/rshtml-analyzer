use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::{
    Hover, HoverContents, Location, MarkupContent, MarkupKind, Position, Range, Url,
};
use tree_sitter::Tree;

use crate::backend::Backend;
use crate::backend::tree_extensions::TreeExtensions;

#[derive(Debug, PartialEq, Eq)]
pub enum ResolvedTarget {
    Component {
        name: String,
        target_path: PathBuf,
        range: Range,
        params: Vec<String>,
    },
    UseDirective {
        path: String,
        target_path: PathBuf,
        range: Range,
    },
}

impl ResolvedTarget {
    pub fn to_location(&self) -> Option<Location> {
        let path = match self {
            ResolvedTarget::Component { target_path, .. } => target_path,
            ResolvedTarget::UseDirective { target_path, .. } => target_path,
        };

        let uri = Url::from_file_path(path).ok()?;
        Some(Location {
            uri,
            range: Range::default(),
        })
    }

    pub fn to_hover(&self) -> Hover {
        let (range, markdown) = match self {
            ResolvedTarget::Component {
                name,
                target_path,
                range,
                params,
            } => {
                let params_doc = if params.is_empty() {
                    "_No parameters_".to_string()
                } else {
                    params
                        .iter()
                        .map(|p| format!("- `{}`", p))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                let file_name = target_path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or_default();

                let doc = format!(
                    "### Component `<{name}>`\n\n**File:** `{file_name}`\n\n**Parameters:**\n{params_doc}"
                );

                (*range, doc)
            }
            ResolvedTarget::UseDirective {
                path,
                target_path: _,
                range,
            } => {
                let doc = format!("### Component Import\n\n`{path}`");
                (*range, doc)
            }
        };

        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: Some(range),
        }
    }
}

pub fn resolve_target_at_position(
    tree: &Tree,
    source: &str,
    position: Position,
    use_directives: &[(String, Option<String>, Vec<String>)],
    views_path: &Path,
) -> Option<ResolvedTarget> {
    let byte_offset = Backend::position_to_byte_offset(source, position);
    let root = tree.root_node();
    let node = root.descendant_for_byte_range(byte_offset, byte_offset)?;

    // Case 1: Cursor is over component_tag_identifier (e.g. <Button /> or </Button>)
    let component_node = if node.kind() == "component_tag_identifier" {
        Some(node)
    } else if let Some(parent) = node.parent() {
        if parent.kind() == "component_tag_identifier" {
            Some(parent)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(comp_node) = component_node {
        let name = comp_node
            .utf8_text(source.as_bytes())
            .ok()?
            .trim()
            .to_string();
        let range = <Tree as TreeExtensions>::from_range(comp_node.range());

        // Find matching use directive for this component
        for (use_path, use_alias, params) in use_directives {
            let matches = match use_alias {
                Some(alias) => alias == &name,
                None => {
                    let file_stem = use_path
                        .trim_end_matches(".rs.html")
                        .split('/')
                        .next_back()
                        .unwrap_or("");
                    file_stem == name
                }
            };

            if matches {
                return Some(ResolvedTarget::Component {
                    name,
                    target_path: views_path.join(use_path),
                    range,
                    params: params.clone(),
                });
            }
        }
    }

    // Case 2: Cursor is over string_line inside use_directive (e.g. @use "components/button.rs.html")
    let string_node = if node.kind() == "string_line" {
        Some(node)
    } else if let Some(parent) = node.parent() {
        if parent.kind() == "string_line" {
            Some(parent)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(str_node) = string_node {
        if let Some(parent) = str_node.parent() {
            if parent.kind() == "use_directive" {
                let raw_text = str_node.utf8_text(source.as_bytes()).ok()?;
                let clean_path = raw_text
                    .trim()
                    .trim_matches(<Tree as TreeExtensions>::STRING_TRIMS);
                let range = <Tree as TreeExtensions>::from_range(str_node.range());

                return Some(ResolvedTarget::UseDirective {
                    path: clean_path.to_string(),
                    target_path: views_path.join(clean_path),
                    range,
                });
            }
        }
    }

    None
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
    fn test_resolve_component_target() {
        let source = r#"
@use "components/button.rs.html" as Button

<Button label="Click" />
"#;
        let tree = parse_rshtml(source);
        let use_directives = vec![(
            "components/button.rs.html".to_string(),
            Some("Button".to_string()),
            vec!["label".to_string()],
        )];
        let views_path = PathBuf::from("/project/views");

        // Position on <Button (line 3, col 2)
        let pos = Position::new(3, 2);
        let target = resolve_target_at_position(&tree, source, pos, &use_directives, &views_path);

        assert!(target.is_some());
        match target.unwrap() {
            ResolvedTarget::Component {
                name,
                target_path,
                params,
                ..
            } => {
                assert_eq!(name, "Button");
                assert_eq!(target_path, views_path.join("components/button.rs.html"));
                assert_eq!(params, vec!["label"]);
            }
            _ => panic!("Expected Component target"),
        }
    }

    #[test]
    fn test_resolve_use_directive_target() {
        let source = r#"@use "components/button.rs.html" as Button"#;
        let tree = parse_rshtml(source);
        let use_directives = vec![];
        let views_path = PathBuf::from("/project/views");

        // Position inside "components/button.rs.html" (line 0, col 10)
        let pos = Position::new(0, 10);
        let target = resolve_target_at_position(&tree, source, pos, &use_directives, &views_path);

        assert!(target.is_some());
        match target.unwrap() {
            ResolvedTarget::UseDirective {
                path, target_path, ..
            } => {
                assert_eq!(path, "components/button.rs.html");
                assert_eq!(target_path, views_path.join("components/button.rs.html"));
            }
            _ => panic!("Expected UseDirective target"),
        }
    }

    #[test]
    fn test_target_to_hover_and_location() {
        let target = ResolvedTarget::Component {
            name: "Button".to_string(),
            target_path: PathBuf::from("/project/views/components/button.rs.html"),
            range: Range::new(Position::new(3, 1), Position::new(3, 7)),
            params: vec!["label".to_string(), "variant".to_string()],
        };

        let hover = target.to_hover();
        if let HoverContents::Markup(markup) = hover.contents {
            assert!(markup.value.contains("Component `<Button>`"));
            assert!(markup.value.contains("- `label`"));
            assert!(markup.value.contains("- `variant`"));
        } else {
            panic!("Expected Markup hover content");
        }

        let loc = target.to_location().unwrap();
        assert_eq!(
            loc.uri.to_file_path().unwrap(),
            PathBuf::from("/project/views/components/button.rs.html")
        );
    }
}
