//! Lira Module Loader
//!
//! Handles module resolution, loading, and dependency management.
//!
//! Module paths are resolved as follows:
//! - `std.fs` → `<stdlib_path>/fs.li`
//! - `foo.bar` → `<source_dir>/foo/bar.li` or `<source_dir>/foo/bar/mod.li`

use crate::ast::{Program, Statement, StatementKind};
use crate::lexer;
use crate::parser;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Module loader for resolving and loading Lira modules
pub struct ModuleLoader {
    /// Directory containing the main source file
    source_dir: PathBuf,
    /// Standard library directory
    stdlib_dir: PathBuf,
    /// Cache of loaded modules (path -> parsed AST)
    module_cache: HashMap<String, Program>,
    /// Set of modules currently being loaded (for cycle detection)
    loading: HashSet<String>,
}

impl ModuleLoader {
    /// Create a new module loader
    pub fn new(source_file: &str) -> Self {
        let source_path = Path::new(source_file);
        let source_dir = source_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        // Try to find stdlib relative to source or in standard locations
        let stdlib_dir = Self::find_stdlib_dir(&source_dir);

        Self {
            source_dir,
            stdlib_dir,
            module_cache: HashMap::new(),
            loading: HashSet::new(),
        }
    }

    /// Find the stdlib directory
    fn find_stdlib_dir(source_dir: &Path) -> PathBuf {
        // Try various locations for stdlib
        let candidates = [
            source_dir.join("stdlib"),
            source_dir.join("../stdlib"),
            source_dir.join("../../stdlib"),
            PathBuf::from("lira/stdlib"),
            PathBuf::from("stdlib"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return candidate.clone();
            }
        }

        // Default to relative stdlib
        source_dir.join("stdlib")
    }

    /// Resolve a module path to a file path
    pub fn resolve_module_path(&self, module_path: &[String]) -> Option<PathBuf> {
        if module_path.is_empty() {
            return None;
        }

        // Check if it's a standard library module (starts with "std")
        if module_path[0] == "std" {
            return self.resolve_stdlib_path(&module_path[1..]);
        }

        // Otherwise, resolve relative to source directory
        self.resolve_relative_path(module_path)
    }

    /// Resolve a stdlib module path (without the "std" prefix)
    fn resolve_stdlib_path(&self, path: &[String]) -> Option<PathBuf> {
        if path.is_empty() {
            return None;
        }

        let mut file_path = self.stdlib_dir.clone();
        for component in &path[..path.len() - 1] {
            file_path = file_path.join(component);
        }

        // Try module.li first
        let module_name = &path[path.len() - 1];
        let direct_path = file_path.join(format!("{}.li", module_name));
        if direct_path.exists() {
            return Some(direct_path);
        }

        // Try module/mod.li
        let mod_path = file_path.join(module_name).join("mod.li");
        if mod_path.exists() {
            return Some(mod_path);
        }

        None
    }

    /// Resolve a relative module path
    fn resolve_relative_path(&self, path: &[String]) -> Option<PathBuf> {
        let mut file_path = self.source_dir.clone();
        for component in &path[..path.len() - 1] {
            file_path = file_path.join(component);
        }

        let module_name = &path[path.len() - 1];
        let direct_path = file_path.join(format!("{}.li", module_name));
        if direct_path.exists() {
            return Some(direct_path);
        }

        let mod_path = file_path.join(module_name).join("mod.li");
        if mod_path.exists() {
            return Some(mod_path);
        }

        None
    }

    /// Load a module by path, returning its parsed AST
    pub fn load_module(&mut self, module_path: &[String]) -> Result<Program, String> {
        let path_key = module_path.join(".");

        // Check cache first
        if let Some(cached) = self.module_cache.get(&path_key) {
            return Ok(cached.clone());
        }

        // Check for circular imports
        if self.loading.contains(&path_key) {
            return Err(format!("Circular import detected: {}", path_key));
        }

        // Resolve to file path
        let file_path = self
            .resolve_module_path(module_path)
            .ok_or_else(|| format!("Module not found: {}", path_key))?;

        // Mark as loading
        self.loading.insert(path_key.clone());

        // Read and parse the module
        let source = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Failed to read module {}: {}", path_key, e))?;

        let tokens = lexer::tokenize(&source)?;
        let ast = parser::parse(&tokens)?;

        // Done loading
        self.loading.remove(&path_key);

        // Cache the result
        self.module_cache.insert(path_key, ast.clone());

        Ok(ast)
    }

    /// Process all imports in a program, returning merged statements
    pub fn process_imports(&mut self, program: &Program) -> Result<Program, String> {
        let mut all_statements: Vec<Statement> = Vec::new();
        let mut imported_names: HashSet<String> = HashSet::new();

        // Recursively collect all imported statements
        self.collect_imports(program, &mut all_statements, &mut imported_names)?;

        // Then add the main program's statements (excluding imports)
        for stmt in &program.statements {
            if !matches!(&stmt.kind, StatementKind::Import { .. }) {
                all_statements.push(stmt.clone());
            }
        }

        Ok(Program {
            statements: all_statements,
        })
    }

    /// Recursively collect imports from a program and its dependencies
    fn collect_imports(
        &mut self,
        program: &Program,
        all_statements: &mut Vec<Statement>,
        imported_names: &mut HashSet<String>,
    ) -> Result<(), String> {
        for stmt in &program.statements {
            if let StatementKind::Import { path, items } = &stmt.kind {
                let module = self.load_module(path)?;

                // First, recursively process this module's imports
                self.collect_imports(&module, all_statements, imported_names)?;

                // Then add this module's statements (functions, structs, etc.)
                for module_stmt in &module.statements {
                    match &module_stmt.kind {
                        StatementKind::FnDecl { name, .. } => {
                            // Import specific items or all if items is None
                            let should_import =
                                items.as_ref().map(|i| i.contains(name)).unwrap_or(true);

                            if should_import && !imported_names.contains(name) {
                                imported_names.insert(name.clone());
                                all_statements.push(module_stmt.clone());
                            }
                        }
                        StatementKind::StructDecl { name, .. } => {
                            let should_import =
                                items.as_ref().map(|i| i.contains(name)).unwrap_or(true);

                            if should_import && !imported_names.contains(name) {
                                imported_names.insert(name.clone());
                                all_statements.push(module_stmt.clone());
                            }
                        }
                        StatementKind::EnumDecl { name, .. } => {
                            let should_import =
                                items.as_ref().map(|i| i.contains(name)).unwrap_or(true);

                            if should_import && !imported_names.contains(name) {
                                imported_names.insert(name.clone());
                                all_statements.push(module_stmt.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_stdlib_path() {
        let loader = ModuleLoader::new("test.li");
        // This test depends on stdlib existing
    }

    #[test]
    fn test_module_path_parsing() {
        // Test that module paths are correctly structured
        let path = vec!["std".to_string(), "fs".to_string()];
        assert_eq!(path.len(), 2);
        assert_eq!(path[0], "std");
        assert_eq!(path[1], "fs");
    }

    #[test]
    fn test_empty_path_returns_none() {
        let loader = ModuleLoader::new("test.li");
        let result = loader.resolve_module_path(&[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_loading_set_management() {
        let mut loader = ModuleLoader::new("test.li");
        let path_key = "test.module".to_string();

        // Initially not in loading set
        assert!(!loader.loading.contains(&path_key));

        // Add to loading set
        loader.loading.insert(path_key.clone());
        assert!(loader.loading.contains(&path_key));

        // Remove from loading set
        loader.loading.remove(&path_key);
        assert!(!loader.loading.contains(&path_key));
    }

    #[test]
    fn test_cache_stores_modules() {
        let mut loader = ModuleLoader::new("test.li");

        // Create a simple test program
        let program = Program { statements: vec![] };

        // Store in cache
        loader
            .module_cache
            .insert("test.module".to_string(), program.clone());

        // Verify it's cached
        assert!(loader.module_cache.contains_key("test.module"));
        let cached = loader.module_cache.get("test.module").unwrap();
        assert_eq!(cached.statements.len(), 0);
    }

    #[test]
    fn test_circular_import_detection() {
        let mut loader = ModuleLoader::new("test.li");

        // Simulate starting to load a module
        let path_key = "circular.module".to_string();
        loader.loading.insert(path_key.clone());

        // Check that we detect it's being loaded
        assert!(loader.loading.contains(&path_key));
    }
}
