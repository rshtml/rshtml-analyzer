use std::path::{Component, Path, PathBuf};
use std::{fs, io};
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
        Workspace { root: PathBuf::new(), members: Vec::new() }
    }
}

impl Workspace {
    pub fn load(&mut self, root: PathBuf) -> Result<(), String> {
        let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
        let cargo_toml: Value = toml::from_str(&cargo_toml).map_err(|e| e.to_string())?;

        let member_paths = cargo_toml
            .get("workspace")
            .and_then(|workspace| workspace.get("members").and_then(|members| members.as_array()))
            .map(|members| {
                members.iter().map(|member| root.join(member.to_string().trim_matches('"'))).collect::<Vec<_>>()
            });

        let mut members: Vec<Member> = Vec::new();

        if let Some(member_paths) = member_paths {
            for member_path in member_paths {
                debug!("MEMBER PATH: {}", member_path.to_str().unwrap());
                let member = Member { path: member_path.clone() };

                members.push(member);
            }
        } else {
            let member = Member { path: root.to_path_buf() };

            debug!("MEMBER PATH: {}", member.path.to_string_lossy().to_string());

            members.push(member);
        }

        self.root = root;
        self.members = members;

        Ok(())
    }

    pub fn find_root_and_relative_path(&self, path: &Path) -> (PathBuf, PathBuf) {
        for member in &self.members {
            if let Ok(rel_path) = path.strip_prefix(&member.path) {
                return (member.path.to_owned(), rel_path.to_path_buf());
            }
        }

        if let Ok(rel_path) = path.strip_prefix(&self.root) {
            return (self.root.to_owned(), rel_path.to_path_buf());
        }

        (self.root.to_owned(), path.to_path_buf())
    }

    pub fn generate_extract_file_path(&self, path: &Path) -> io::Result<PathBuf> {
        let (root_path, relative_path) = self.find_root_and_relative_path(path);

        let manifest_dir =
            fs::canonicalize(&root_path).map_err(|e| io::Error::new(e.kind(), format!("{root_path:?}: {e}")))?;

        let mut target_path = manifest_dir.join(".rshtml");

        let absolute_path = fs::canonicalize(manifest_dir.join(&relative_path))
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", relative_path.display())))?;

        if let Ok(rel_path) = absolute_path.strip_prefix(&manifest_dir) {
            target_path.push(rel_path);
        } else {
            const HEX: &[u8; 16] = b"0123456789abcdef";

            target_path.push("__ext__");

            for component in absolute_path.components() {
                match component {
                    Component::Normal(c) => {
                        target_path.push(c);
                    }
                    Component::Prefix(prefix) => {
                        let bytes = prefix.as_os_str().as_encoded_bytes();
                        let mut hex = String::with_capacity(bytes.len() * 2 + 4);
                        hex.push_str("__");
                        for b in bytes {
                            hex.push(HEX[(b >> 4) as usize] as char);
                            hex.push(HEX[(b & 0xf) as usize] as char);
                        }
                        hex.push_str("__");

                        target_path.push(hex);
                    }
                    Component::CurDir | Component::ParentDir | Component::RootDir => {}
                }
            }
        }

        // .rs.html -> .rs
        if target_path.extension().is_some_and(|e| e == "html") {
            target_path.set_extension("");
        }

        Ok(target_path)
    }
}
