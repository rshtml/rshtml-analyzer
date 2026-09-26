use std::{fs, path::Path};

use serde::Deserialize;
use tracing::info;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct StructField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct TemplateDiagnostic {
    pub message: String,
    pub line: usize,
    pub character: usize,
    pub severity: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct ViewMetaData {
    pub struct_name: String,
    pub template_path: String,
    pub fields: Vec<StructField>,
    #[serde(default)]
    pub diagnostics: Vec<TemplateDiagnostic>,
}

impl ViewMetaData {
    /// Loads view metadata by checking member-level and workspace-level target directories,
    /// handling relative path differences (e.g. with or without crate prefix).
    pub fn load_for_template(
        member_root: &Path,
        workspace_root: &Path,
        relative_template_path: &Path,
    ) -> Option<Self> {
        let candidate_rel_paths = vec![
            relative_template_path.to_path_buf(),
            relative_template_path
                .strip_prefix("views")
                .map(|p| Path::new("views").join(p))
                .unwrap_or_else(|_| relative_template_path.to_path_buf()),
        ];

        let roots = vec![member_root, workspace_root];

        for root in roots {
            for rel in &candidate_rel_paths {
                let meta_file = root
                    .join("target")
                    .join("rshtml")
                    .join("metadata")
                    .join(format!("{}.json", rel.display()));

                if meta_file.exists() {
                    info!("BINGO! Found metadata file at: {:?}", meta_file);
                    if let Ok(content) = fs::read_to_string(&meta_file) {
                        if let Ok(meta) = serde_json::from_str::<ViewMetaData>(&content) {
                            return Some(meta);
                        }
                    }
                }
            }
        }

        info!(
            "Metadata file not found for {:?} under member {:?} or workspace {:?}",
            relative_template_path, member_root, workspace_root
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_view_metadata() {
        let json_data = r#"{
          "struct_name": "IndexPage",
          "template_path": "views/index.rs.html",
          "fields": [
            {
              "name": "title",
              "type": "String"
            },
            {
              "name": "email",
              "type": "String"
            },
            {
              "name": "footer",
              "type": "bool"
            },
            {
              "name": "navbar_sections",
              "type": "Vec < (& 'static str, & 'static str) >"
            },
            {
              "name": "home_time",
              "type": "DateTime < Utc >"
            }
          ],
          "diagnostics": []
        }"#;

        let parsed: Result<ViewMetaData, _> = serde_json::from_str(json_data);
        assert!(parsed.is_ok(), "Failed to parse ViewMetaData: {:?}", parsed.err());
        let meta = parsed.unwrap();
        assert_eq!(meta.struct_name, "IndexPage");
        assert_eq!(meta.template_path, "views/index.rs.html");
        assert_eq!(meta.fields.len(), 5);
        assert_eq!(meta.fields[0].name, "title");
        assert_eq!(meta.fields[0].field_type, "String");
        assert_eq!(meta.fields[2].name, "footer");
        assert_eq!(meta.fields[2].field_type, "bool");
    }
}
