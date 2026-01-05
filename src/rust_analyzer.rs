use crate::backend::tree_extensions::TreeExtensions;
use std::{
    fs::{self},
    path::{Path, PathBuf},
};
use syn::{Expr, Item, Lit, Meta};
use tracing::debug;
use tree_sitter::{Language, Tree};
use walkdir::WalkDir;

pub struct RustAnalyzer;

impl RustAnalyzer {
    pub fn analyze(
        language: &Language,
        tree: &Tree,
        source: &str,
        struct_path: &Path,
        struct_name: &str,
    ) {
        let rust_code = tree.rust_code_to_str(language, source);

        debug!("{rust_code}");
    }

    pub fn find_struct_for_rshtml(
        project_root: &Path,
        rshtml_file: &Path,
    ) -> Option<(PathBuf, String)> {
        let src_path = project_root.join("src");

        for entry in WalkDir::new(&src_path).into_iter().filter_map(|e| {
            e.ok()
                .filter(|entry| entry.path().extension().map_or(false, |s| s == "rs"))
        }) {
            let path = entry.path();

            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if !src.contains("RsHtml") {
                continue;
            }

            let syntax = match syn::parse_file(&src) {
                Ok(f) => f,
                Err(_) => continue,
            };

            for item in syntax.items {
                if let Item::Struct(s) = item {
                    if Self::has_derive_rshtml(&s.attrs) {
                        let struct_name = s.ident.to_string();
                        let custom_path = Self::get_rshtml_path_attr(&s.attrs);

                        let mut target_path = PathBuf::from("views");

                        if let Some(p) = custom_path {
                            target_path.push(p);
                        } else {
                            let name = struct_name.replace("Page", "");
                            target_path.push(format!("{}.rs.html", Self::to_snake_case(&name)));
                        }

                        if rshtml_file.ends_with(&target_path) {
                            return Some((path.to_path_buf(), struct_name));
                        }
                    }
                }
            }
        }

        None
    }

    fn has_derive_rshtml(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident("derive")
                && attr
                    .parse_args_with(|input:syn::parse::ParseStream| {
                        let paths = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated(input)?;
                        Ok(paths.iter().any(|p| p.is_ident("RsHtml")))
                    })
                    .unwrap_or(false)
        })
    }

    fn get_rshtml_path_attr(attrs: &[syn::Attribute]) -> Option<String> {
        for attr in attrs {
            if attr.path().is_ident("rshtml") {
                if let Ok(nested) = attr.parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                ) {
                    for meta in nested {
                        if let Meta::NameValue(nv) = meta {
                            if nv.path.is_ident("path") {
                                if let Expr::Lit(expr_lit) = nv.value {
                                    if let Lit::Str(lit_str) = expr_lit.lit {
                                        return Some(lit_str.value());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn to_snake_case(s: &str) -> String {
        let mut result = String::new();
        for (i, c) in s.char_indices() {
            if c.is_uppercase() {
                if i > 0 {
                    result.push('_');
                }
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c);
            }
        }
        result
    }
}
