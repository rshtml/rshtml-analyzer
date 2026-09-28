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
                let member = Member {
                    path: member_path.clone(),
                };

                new_workspace.members.push(member);
            }
        } else {
            let member = Member {
                path: root.to_path_buf(),
            };

            debug!("MEMBER PATH: {}", member.path.to_string_lossy().to_string());

            new_workspace.members.push(member);
        }

        self.root = new_workspace.root;
        self.members = new_workspace.members;

        Ok(())
    }
}
