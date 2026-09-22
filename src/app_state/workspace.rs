use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;
use tracing::debug;

pub struct Workspace {
    pub root: PathBuf,
    pub members: Vec<Member>,
}

pub struct Member {
    pub path: PathBuf,
    pub views_path: PathBuf,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace {
            root: PathBuf::new(),
            members: Vec::new(),
        }
    }
}

impl Workspace {
    pub fn load(&mut self, root: &Path) -> Result<(), String> {
        let mut new_workspace = Self {
            root: root.to_path_buf(),
            ..Default::default()
        };

        let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
        let cargo_toml: Value = toml::from_str(&cargo_toml).map_err(|e| e.to_string())?;

        let member_paths = cargo_toml
            .get("workspace")
            .and_then(|workspace| {
                workspace
                    .get("members")
                    .and_then(|members| members.as_array())
            })
            .map(|members| {
                members
                    .iter()
                    .map(|member| root.join(member.to_string().trim_matches('"')))
                    .collect::<Vec<_>>()
            });

        if let Some(member_paths) = member_paths {
            for member_path in member_paths {
                debug!("MEMBER PATH: {}", member_path.to_str().unwrap());
                let cargo_toml = fs::read_to_string(member_path.join("Cargo.toml"))
                    .map_err(|e| e.to_string())?;
                let cargo_toml: Value = toml::from_str(&cargo_toml).map_err(|e| e.to_string())?;
                let views_path = self.load_manifest(&cargo_toml)?;
                let member = Member {
                    path: member_path.clone(),
                    views_path: member_path.join(views_path),
                };

                new_workspace.members.push(member);
            }
        } else {
            let views_path = self.load_manifest(&cargo_toml)?;
            let member = Member {
                path: root.to_path_buf(),
                views_path: root.join(views_path),
            };

            debug!("MEMBER PATH: {}", member.path.to_string_lossy().to_string());

            new_workspace.members.push(member);
        }

        self.root = new_workspace.root;
        self.members = new_workspace.members;

        Ok(())
    }

    fn load_manifest<'a>(&self, cargo_toml: &'a Value) -> Result<&'a str, String> {
        let default_path = "views";
        let path = cargo_toml
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("rshtml"))
            .and_then(|r| r.get("views"))
            .and_then(|v| v.get("path"))
            .and_then(|p| p.as_str())
            .unwrap_or(default_path);

        Ok(path)
    }

    pub fn get_member_by_view(&self, view_path: &Path) -> Option<&Member> {
        self.members
            .iter()
            .find(|&member| view_path.starts_with(&member.path))
    }

    // pub fn get_layout_path_by_view(&self, view_path: &Path) -> Option<PathBuf> {
    //     for member in &self.members {
    //         if view_path.starts_with(&member.path) {
    //             let mut path = member.views_path.clone();
    //             path.push(&member.views_layout);
    //             return Some(path);
    //         }
    //     }

    //     None
    // }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_manifest_default() {
        let ws = Workspace::default();
        let cargo_toml: Value = toml::from_str("[package]\nname = \"my-project\"\n").unwrap();
        let path = ws.load_manifest(&cargo_toml).unwrap();
        assert_eq!(path, "views");
    }

    #[test]
    fn test_load_manifest_custom_path() {
        let ws = Workspace::default();
        let cargo_toml: Value = toml::from_str(
            r#"
            [package]
            name = "my-project"

            [package.metadata.rshtml.views]
            path = "src/templates"
            "#,
        )
        .unwrap();
        let path = ws.load_manifest(&cargo_toml).unwrap();
        assert_eq!(path, "src/templates");
    }

    #[test]
    fn test_get_member_by_view() {
        let mut ws = Workspace::default();
        ws.members = vec![
            Member {
                path: PathBuf::from("/home/user/project/crate_a"),
                views_path: PathBuf::from("/home/user/project/crate_a/views"),
            },
            Member {
                path: PathBuf::from("/home/user/project/crate_b"),
                views_path: PathBuf::from("/home/user/project/crate_b/views"),
            },
        ];

        let member =
            ws.get_member_by_view(Path::new("/home/user/project/crate_a/views/index.rs.html"));
        assert!(member.is_some());
        assert_eq!(
            member.unwrap().path,
            PathBuf::from("/home/user/project/crate_a")
        );

        let non_member =
            ws.get_member_by_view(Path::new("/home/user/project/crate_c/views/index.rs.html"));
        assert!(non_member.is_none());
    }
}
