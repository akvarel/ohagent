use crate::error::{io_error, EloopError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_INSTRUCTION_BYTES: usize = 64 * 1024;
const MAX_TREE_ENTRIES: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitContext {
    pub available: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub status_porcelain: Vec<String>,
    pub recent_commits: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryContext {
    pub repository: PathBuf,
    pub project_name: String,
    pub detected_languages: Vec<String>,
    pub build_systems: Vec<String>,
    pub verification_commands: Vec<String>,
    pub instructions: Vec<InstructionFile>,
    pub tree: Vec<String>,
    pub git: GitContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstructionFile {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

pub trait RepositoryContextCollector: Send + Sync {
    fn collect(&self, repository: &Path) -> Result<RepositoryContext>;
}

#[derive(Debug, Default)]
pub struct FilesystemRepositoryContextCollector;

impl RepositoryContextCollector for FilesystemRepositoryContextCollector {
    fn collect(&self, repository: &Path) -> Result<RepositoryContext> {
        let repository = repository
            .canonicalize()
            .map_err(|source| io_error(repository, source))?;
        if !repository.is_dir() {
            return Err(EloopError::InvalidRepository(repository));
        }

        let project_name = repository
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repository")
            .to_string();

        let mut instructions = Vec::new();
        for candidate in ["AGENTS.md", "CLAUDE.md", "README.md"] {
            let path = repository.join(candidate);
            if path.is_file() {
                instructions.push(read_instruction(&repository, &path)?);
            }
        }

        let tree = collect_tree(&repository)?;
        let detected_languages = detect_languages(&tree);
        let (build_systems, verification_commands) = detect_builds(&repository);
        let git = collect_git_context(&repository);

        Ok(RepositoryContext {
            repository,
            project_name,
            detected_languages,
            build_systems,
            verification_commands,
            instructions,
            tree,
            git,
        })
    }
}

fn read_instruction(root: &Path, path: &Path) -> Result<InstructionFile> {
    let bytes = fs::read(path).map_err(|source| io_error(path, source))?;
    let truncated = bytes.len() > MAX_INSTRUCTION_BYTES;
    let slice = &bytes[..bytes.len().min(MAX_INSTRUCTION_BYTES)];
    let content = String::from_utf8_lossy(slice).to_string();
    let relative = path.strip_prefix(root).unwrap_or(path);
    Ok(InstructionFile {
        path: relative.to_string_lossy().to_string(),
        content,
        truncated,
    })
}

fn collect_tree(root: &Path) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    entries.sort();
    entries.truncate(MAX_TREE_ENTRIES);
    Ok(entries)
}

fn visit(root: &Path, directory: &Path, entries: &mut Vec<String>) -> Result<()> {
    if entries.len() >= MAX_TREE_ENTRIES {
        return Ok(());
    }
    let mut children = fs::read_dir(directory)
        .map_err(|source| io_error(directory, source))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| io_error(directory, source))?;
    children.sort_by_key(|entry| entry.file_name());

    for child in children {
        if entries.len() >= MAX_TREE_ENTRIES {
            break;
        }
        let path = child.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let relative_text = relative.to_string_lossy();
        if should_skip(&relative_text) {
            continue;
        }
        if path.is_dir() {
            entries.push(format!("{relative_text}/"));
            visit(root, &path, entries)?;
        } else {
            entries.push(relative_text.to_string());
        }
    }
    Ok(())
}

fn should_skip(relative: &str) -> bool {
    relative.split('/').any(|part| {
        matches!(
            part,
            ".git" | "target" | "node_modules" | ".idea" | ".vscode" | "dist" | "build"
        )
    })
}

fn detect_languages(tree: &[String]) -> Vec<String> {
    let mappings = [
        (".rs", "Rust"),
        (".go", "Go"),
        (".java", "Java"),
        (".kt", "Kotlin"),
        (".py", "Python"),
        (".ts", "TypeScript"),
        (".tsx", "TypeScript/React"),
        (".js", "JavaScript"),
    ];
    let mut languages = Vec::new();
    for (extension, language) in mappings {
        if tree.iter().any(|entry| entry.ends_with(extension)) {
            languages.push(language.to_string());
        }
    }
    languages
}

fn detect_builds(root: &Path) -> (Vec<String>, Vec<String>) {
    let mut systems = Vec::new();
    let mut commands = Vec::new();

    if root.join("Cargo.toml").is_file() {
        systems.push("cargo".to_string());
        commands.extend([
            "cargo fmt --all -- --check".to_string(),
            "cargo clippy --workspace --all-targets -- -D warnings".to_string(),
            "cargo test --workspace".to_string(),
            "cargo build --workspace".to_string(),
        ]);
    }
    if root.join("go.mod").is_file() {
        systems.push("go".to_string());
        commands.extend([
            "gofmt -w . && git diff --exit-code".to_string(),
            "go vet ./...".to_string(),
            "go test ./...".to_string(),
            "go build ./...".to_string(),
        ]);
    }
    if root.join("gradlew").is_file() {
        systems.push("gradle".to_string());
        commands.extend([
            "./gradlew test".to_string(),
            "./gradlew build".to_string(),
        ]);
    } else if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        systems.push("gradle".to_string());
        commands.extend(["gradle test".to_string(), "gradle build".to_string()]);
    }
    if root.join("package.json").is_file() {
        systems.push("node".to_string());
        commands.extend([
            "npm test -- --runInBand".to_string(),
            "npm run build".to_string(),
        ]);
    }
    if root.join("pyproject.toml").is_file() {
        systems.push("python".to_string());
        commands.push("python -m pytest".to_string());
    }

    (systems, commands)
}

fn collect_git_context(root: &Path) -> GitContext {
    let available = run_git(root, &["rev-parse", "--is-inside-work-tree"])
        .map(|output| output.trim() == "true")
        .unwrap_or(false);
    if !available {
        return GitContext {
            available: false,
            branch: None,
            head: None,
            status_porcelain: Vec::new(),
            recent_commits: Vec::new(),
        };
    }

    GitContext {
        available: true,
        branch: run_git(root, &["branch", "--show-current"]).map(trimmed),
        head: run_git(root, &["rev-parse", "HEAD"]).map(trimmed),
        status_porcelain: run_git(root, &["status", "--porcelain=v1"])
            .map(lines)
            .unwrap_or_default(),
        recent_commits: run_git(root, &["log", "-10", "--pretty=format:%h %s"])
            .map(lines)
            .unwrap_or_default(),
    }
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn trimmed(value: String) -> String {
    value.trim().to_string()
}

fn lines(value: String) -> Vec<String> {
    value
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_rust_repository_and_instructions() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='demo'\n")
            .expect("write cargo");
        fs::write(dir.path().join("AGENTS.md"), "# Rules\nUse tests.")
            .expect("write agents");
        fs::create_dir(dir.path().join("src")).expect("create src");
        fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}")
            .expect("write lib");

        let context = FilesystemRepositoryContextCollector
            .collect(dir.path())
            .expect("collect context");

        assert_eq!(context.detected_languages, vec!["Rust"]);
        assert_eq!(context.build_systems, vec!["cargo"]);
        assert!(context
            .verification_commands
            .contains(&"cargo test --workspace".to_string()));
        assert_eq!(context.instructions[0].path, "AGENTS.md");
    }
}
