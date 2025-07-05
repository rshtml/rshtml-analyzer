use tree_sitter::Tree;

pub struct View {
    pub source: String,
    pub tree: Tree,
    pub include_paths: Vec<String>,
    pub use_directives: Vec<(String, Option<String>)>,

    pub version: usize,
}

impl View {
    pub fn new(source: String, tree: Tree, version: usize) -> Self {
        Self {
            source,
            tree,
            include_paths: Vec::new(),
            use_directives: Vec::new(),
            version,
        }
    }

    pub fn use_directives_names(&self) -> Vec<String> {
        //let mut names: Vec<String> = Vec::new();
        self.use_directives
            .iter()
            .filter_map(|(path, name)| {
                let name_str = name.as_deref().or_else(|| path.trim_end_matches(".rs.html").split('/').last()).unwrap_or("");

                if name_str.is_empty() { None } else { Some(name_str.to_string()) }
            })
            .collect()

        // for use_directive in &self.use_directives {
        //
        //     let name = use_directive.to_owned().1.unwrap_or_else(|| {
        //         use_directive
        //             .0
        //             .to_owned()
        //             .trim_end_matches("rs.html")
        //             .split('/')
        //             .last()
        //             .unwrap_or("")
        //             .to_owned()
        //     });
        //
        //     if !name.is_empty() {
        //         names.push(name);
        //     }
        // }

        //names
    }
}
