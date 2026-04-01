use std::path::Path;

use crate::models::policy::{Classification, DangerLevel, Tag};

/// Classify a file path based on extension and path patterns.
/// Returns both tags (for retention policy) and danger level (for safety checks).
pub fn classify(path: &Path) -> Classification {
    let tags = classify_tags(path);
    let danger_level = check_danger(path);
    Classification { tags, danger_level }
}

fn classify_tags(path: &Path) -> Vec<Tag> {
    let mut tags = Vec::new();
    let path_str = path.to_string_lossy();
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // Build artifacts
    let build_dirs = [
        "node_modules",
        "dist",
        "build",
        "target",
        "__pycache__",
        ".cache",
        ".next",
        "out",
    ];
    let build_extensions = ["o", "obj", "pyc", "pyo", "class", "wasm"];
    if build_dirs.iter().any(|d| {
        path_str.contains(&format!("/{}/", d)) || path_str.ends_with(&format!("/{}", d))
    }) || build_extensions.contains(&extension.as_str())
    {
        tags.push(Tag::Build);
    }

    // Temp files
    let temp_extensions = ["tmp", "temp", "swp", "swo", "bak", "log", "old"];
    if temp_extensions.contains(&extension.as_str())
        || file_name.starts_with('~')
        || file_name.ends_with('~')
        || file_name.starts_with('#')
    {
        tags.push(Tag::Temp);
    }

    // User content (source code, docs)
    let content_extensions = [
        "rs", "py", "js", "ts", "jsx", "tsx", "go", "java", "c", "cpp", "h", "hpp", "rb", "php",
        "swift", "kt", "scala", "sh", "bash", "zsh", "md", "txt", "rst", "org", "tex", "doc",
        "docx", "pdf", "json", "yaml", "yml", "toml", "xml", "csv", "html", "css", "scss",
        "less", "sql", "graphql",
    ];
    if content_extensions.contains(&extension.as_str()) {
        tags.push(Tag::Content);
    }

    // Config/secrets - protected
    let protected_names = [".env", ".env.local", ".env.production", ".env.development"];
    let protected_extensions = ["pem", "key", "p12", "pfx", "jks"];
    let protected_patterns = ["credential", "secret", "token", "password", "private_key"];
    if protected_names
        .iter()
        .any(|n| file_name == *n || file_name.starts_with(&format!("{}.", n)))
        || protected_extensions.contains(&extension.as_str())
        || protected_patterns
            .iter()
            .any(|p| file_name.to_lowercase().contains(p))
    {
        tags.push(Tag::Protected);
    }

    tags
}

fn check_danger(path: &Path) -> DangerLevel {
    let path_str = path.to_string_lossy();
    let canonical = path.to_string_lossy();

    // Absolute blocked paths
    let blocked_paths = ["/"];
    for bp in &blocked_paths {
        if canonical == *bp {
            return DangerLevel::Blocked(format!("Cannot delete {}", bp));
        }
    }

    // Warning paths (require --yes-i-am-sure)
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();

    // Home directory itself
    if !home.is_empty() && canonical == home {
        return DangerLevel::Warning("This will archive your entire home directory".to_string());
    }

    // System paths
    let system_paths = [
        "/home", "/etc", "/usr", "/var", "/boot", "/bin", "/sbin", "/lib", "/opt",
    ];
    for sp in &system_paths {
        if canonical == *sp {
            return DangerLevel::Warning(format!("This will archive system directory {}", sp));
        }
    }

    // .git directory
    if path_str.ends_with("/.git") || path_str == ".git" {
        return DangerLevel::Warning("This will archive your git history".to_string());
    }

    // SSH/GPG keys
    let sensitive_dirs = [".ssh", ".gnupg", ".gpg"];
    for sd in &sensitive_dirs {
        let suffix = format!("/{}", sd);
        if path_str.ends_with(&suffix) || path_str.contains(&format!("{}/", &suffix)) {
            return DangerLevel::Warning(format!(
                "This will archive sensitive directory {}",
                sd
            ));
        }
    }

    DangerLevel::Safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_node_modules_as_build() {
        let c = classify(Path::new("/project/node_modules"));
        assert!(c.tags.contains(&Tag::Build));
    }

    #[test]
    fn classify_dot_env_as_protected() {
        let c = classify(Path::new("/project/.env"));
        assert!(c.tags.contains(&Tag::Protected));
    }

    #[test]
    fn classify_dot_env_local_as_protected() {
        let c = classify(Path::new("/project/.env.local"));
        assert!(c.tags.contains(&Tag::Protected));
    }

    #[test]
    fn classify_root_as_blocked() {
        let c = classify(Path::new("/"));
        assert!(matches!(c.danger_level, DangerLevel::Blocked(_)));
    }

    #[test]
    fn classify_home_as_warning() {
        if let Some(home) = dirs::home_dir() {
            let c = classify(&home);
            assert!(matches!(c.danger_level, DangerLevel::Warning(_)));
        }
    }

    #[test]
    fn classify_python_file_as_content() {
        let c = classify(Path::new("/project/test.py"));
        assert!(c.tags.contains(&Tag::Content));
    }

    #[test]
    fn classify_rust_file_as_content() {
        let c = classify(Path::new("/project/main.rs"));
        assert!(c.tags.contains(&Tag::Content));
    }

    #[test]
    fn classify_tmp_file_as_temp() {
        let c = classify(Path::new("/project/file.tmp"));
        assert!(c.tags.contains(&Tag::Temp));
    }

    #[test]
    fn classify_tilde_backup_as_temp() {
        let c = classify(Path::new("/project/file.txt~"));
        assert!(c.tags.contains(&Tag::Temp));
    }

    #[test]
    fn classify_emacs_autosave_as_temp() {
        let c = classify(Path::new("/project/#file.txt#"));
        assert!(c.tags.contains(&Tag::Temp));
    }

    #[test]
    fn classify_pem_as_protected() {
        let c = classify(Path::new("/project/server.pem"));
        assert!(c.tags.contains(&Tag::Protected));
    }

    #[test]
    fn classify_object_file_as_build() {
        let c = classify(Path::new("/project/main.o"));
        assert!(c.tags.contains(&Tag::Build));
    }

    #[test]
    fn classify_pycache_as_build() {
        let c = classify(Path::new("/project/__pycache__/module.pyc"));
        assert!(c.tags.contains(&Tag::Build));
    }

    #[test]
    fn classify_git_dir_as_warning() {
        let c = classify(Path::new("/project/.git"));
        assert!(matches!(c.danger_level, DangerLevel::Warning(_)));
    }

    #[test]
    fn classify_ssh_dir_as_warning() {
        let c = classify(Path::new("/home/user/.ssh"));
        assert!(matches!(c.danger_level, DangerLevel::Warning(_)));
    }

    #[test]
    fn classify_system_dir_as_warning() {
        let c = classify(Path::new("/etc"));
        assert!(matches!(c.danger_level, DangerLevel::Warning(_)));
    }

    #[test]
    fn classify_normal_file_as_safe() {
        let c = classify(Path::new("/project/readme.md"));
        assert_eq!(c.danger_level, DangerLevel::Safe);
    }

    #[test]
    fn classify_credential_file_as_protected() {
        let c = classify(Path::new("/project/credentials.json"));
        assert!(c.tags.contains(&Tag::Protected));
    }

    #[test]
    fn classify_secret_file_as_protected() {
        let c = classify(Path::new("/project/app_secret.yaml"));
        assert!(c.tags.contains(&Tag::Protected));
    }
}
