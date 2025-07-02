use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;

pub struct Workspace {
    pub root: PathBuf,
    pub members: Vec<Member>,
    pub views_path: PathBuf,
    pub views_layout: String,
}

pub struct Member {
    pub path: PathBuf,
    pub layout: String,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace {
            root: PathBuf::new(),
            members: Vec::new(),
            views_path: PathBuf::new(),
            views_layout: String::new(),
        }
    }
}

impl Workspace {
    pub fn load(&mut self, root: &Path) -> Result<(), String> {
        self.root = root.to_path_buf();
        let cargo_toml = fs::read_to_string(&root).map_err(|e| e.to_string())?;
        let cargo_toml: Value = toml::from_str(&cargo_toml).map_err(|e| e.to_string())?;

        let member_paths = cargo_toml
            .get("workspace")
            .and_then(|workspace| workspace.get("members").and_then(|members| members.as_array()))
            .and_then(|members| {
                Some(
                    members
                        .iter()
                        .map(|member| root.join(member.to_string()))
                        .collect::<Vec<_>>(),
                )
            });

        if let Some(member_paths) = member_paths {
            for member_path in member_paths {
                let cargo_toml = fs::read_to_string(&member_path).map_err(|e| e.to_string())?;
                let cargo_toml: Value = toml::from_str(&cargo_toml).map_err(|e| e.to_string())?;
                let views = self.load_manifest(&cargo_toml)?;
                let member = Member {
                    path: root.join(views.0),
                    layout: views.1.to_string(),
                };

                self.members.push(member);
            }
        } else {
            let views = self.load_manifest(&cargo_toml)?;
            let member = Member {
                path: root.join(views.0),
                layout: views.1.to_string(),
            };

            self.members.push(member);
        }

        Ok(())
    }

    fn load_manifest<'a>(&self, cargo_toml: &'a Value) -> Result<(&'a str, &'a str), String> {
        let default_path = "views";
        let default_layout = "layout.rs.html";
        match cargo_toml.get("package.metadata.rshtml").and_then(|x| x.get("views")) {
            Some(x) => {
                let path = x.get("path").and_then(|x| x.as_str()).unwrap_or(default_path);
                let layout = x.get("layout").and_then(|x| x.as_str()).unwrap_or(default_layout);
                Ok((path, layout))
            }
            None => Ok((default_path, default_layout)),
        }
    }
}
