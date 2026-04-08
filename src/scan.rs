use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    GlobalClaudeMd,
    ProjectClaudeMd,
    MemoryIndex,
    Memory,
}

#[derive(Debug, Clone)]
pub struct MemoryFile {
    pub kind: FileKind,
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub files: Vec<MemoryFile>,
}

/// Decode Claude Code's encoded project directory name to a human-readable name.
/// Encoding: `/Users/kim/my-project` → `-Users-kim-my-project`
/// We try to find the actual directory on disk, falling back to the last segment.
fn decode_project_name(encoded: &str) -> String {
    // Reconstruct path: leading `-` → `/`, remaining `-` → `/`
    let decoded_path = encoded.replacen('-', "/", 1);
    let path = Path::new(&decoded_path);

    if path.exists()
        && let Some(name) = path.file_name()
    {
        return name.to_string_lossy().to_string();
    }

    // Fallback: take the last non-empty segment after splitting by `-`
    encoded
        .rsplit('-')
        .find(|s| !s.is_empty())
        .unwrap_or(encoded)
        .to_string()
}

/// Scan `~/.claude/` and return all projects with memory files.
pub fn scan_claude_dir(claude_dir: &Path) -> Vec<Project> {
    let mut projects = Vec::new();

    // 1. Global CLAUDE.md
    let global_claude_md = claude_dir.join("CLAUDE.md");
    if global_claude_md.is_file() {
        let size = fs::metadata(&global_claude_md)
            .map(|m| m.len())
            .unwrap_or(0);
        projects.push(Project {
            name: "GLOBAL".to_string(),
            path: claude_dir.to_path_buf(),
            files: vec![MemoryFile {
                kind: FileKind::GlobalClaudeMd,
                path: global_claude_md,
                name: "CLAUDE.md".to_string(),
                size,
            }],
        });
    }

    // 2. Scan projects
    let projects_dir = claude_dir.join("projects");
    if !projects_dir.is_dir() {
        return projects;
    }

    let mut project_entries: Vec<Project> = fs::read_dir(&projects_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let dir_name = entry.file_name().to_string_lossy().to_string();
            let project_path = entry.path();
            let mut files = Vec::new();

            // Check for CLAUDE.md in project dir
            let claude_md = project_path.join("CLAUDE.md");
            if claude_md.is_file() {
                let size = fs::metadata(&claude_md).map(|m| m.len()).unwrap_or(0);
                files.push(MemoryFile {
                    kind: FileKind::ProjectClaudeMd,
                    path: claude_md,
                    name: "CLAUDE.md".to_string(),
                    size,
                });
            }

            // Check for memory/*.md files
            let memory_dir = project_path.join("memory");
            if memory_dir.is_dir()
                && let Ok(entries) = fs::read_dir(&memory_dir)
            {
                    let mut memory_files: Vec<MemoryFile> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            e.path().extension().is_some_and(|ext| ext == "md")
                        })
                        .map(|e| {
                            let path = e.path();
                            let name = e.file_name().to_string_lossy().to_string();
                            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            let kind = if name == "MEMORY.md" {
                                FileKind::MemoryIndex
                            } else {
                                FileKind::Memory
                            };
                            MemoryFile { kind, path, name, size }
                        })
                        .collect();

                    // MEMORY.md first, then alphabetical
                    memory_files.sort_by(|a, b| {
                        let a_is_index = a.kind == FileKind::MemoryIndex;
                        let b_is_index = b.kind == FileKind::MemoryIndex;
                        b_is_index.cmp(&a_is_index).then(a.name.cmp(&b.name))
                    });

                    files.extend(memory_files);
            }

            // Skip projects with no files
            if files.is_empty() {
                return None;
            }

            Some(Project {
                name: decode_project_name(&dir_name),
                path: project_path,
                files,
            })
        })
        .collect();

    // Sort projects alphabetically
    project_entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    projects.extend(project_entries);

    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().to_path_buf();
        (tmp, claude_dir)
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let (_tmp, claude_dir) = create_test_dir();
        let projects = scan_claude_dir(&claude_dir);
        assert!(projects.is_empty());
    }

    #[test]
    fn scan_finds_global_claude_md() {
        let (_tmp, claude_dir) = create_test_dir();
        fs::write(claude_dir.join("CLAUDE.md"), "# Global").unwrap();

        let projects = scan_claude_dir(&claude_dir);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "GLOBAL");
        assert_eq!(projects[0].files.len(), 1);
        assert_eq!(projects[0].files[0].kind, FileKind::GlobalClaudeMd);
    }

    #[test]
    fn scan_finds_project_files() {
        let (_tmp, claude_dir) = create_test_dir();

        let project_dir = claude_dir.join("projects").join("-Users-test-myproject");
        let memory_dir = project_dir.join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::write(project_dir.join("CLAUDE.md"), "# Project").unwrap();
        fs::write(memory_dir.join("MEMORY.md"), "# Index").unwrap();
        fs::write(memory_dir.join("notes.md"), "# Notes").unwrap();

        let projects = scan_claude_dir(&claude_dir);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].files.len(), 3);
        assert_eq!(projects[0].files[0].name, "CLAUDE.md");
        assert_eq!(projects[0].files[1].name, "MEMORY.md");
        assert_eq!(projects[0].files[2].name, "notes.md");
    }

    #[test]
    fn scan_skips_projects_without_files() {
        let (_tmp, claude_dir) = create_test_dir();

        let project_dir = claude_dir.join("projects").join("-Users-test-empty");
        fs::create_dir_all(project_dir).unwrap();

        let projects = scan_claude_dir(&claude_dir);
        assert!(projects.is_empty());
    }

    #[test]
    fn decode_project_name_takes_last_segment() {
        assert_eq!(decode_project_name("-Users-test-myproject"), "myproject");
    }
}
