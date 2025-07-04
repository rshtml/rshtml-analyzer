use tree_sitter::Tree;

pub struct View {
    pub source: String,
    pub tree: Tree,
    pub include_paths: Vec<String>,
    pub use_directives: Vec<(String, Option<String>)>,

    pub version: usize,
}

impl View {
    pub(crate) fn new(source: String, tree: Tree, version: usize) -> Self {
        Self {
            source,
            tree,
            include_paths: Vec::new(),
            use_directives: Vec::new(),
            version,
        }
    }
}
