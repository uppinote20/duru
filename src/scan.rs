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

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub files: Vec<MemoryFile>,
}

/// Decode Claude Code's encoded project directory name to a human-readable name.
///
/// Claude Code encodes paths: `/` → `-`, `_` → `-`.
/// So `/Users/kim/Project/_active/clavis` → `-Users-kim-Project--active-clavis`
/// (`--active` = path separator `-` + underscore-replaced `-` from `_active`)
///
/// Strategy:
/// 1. Replace `--` with `/_` to restore underscore-prefixed directories
/// 2. Split by `-` and greedy-match against filesystem to resolve literal dashes
///
/// Returns `None` if the original project directory no longer exists on disk.
fn decode_project_name(encoded: &str) -> Option<String> {
    // Step 1: `--` → `/_` (restores underscore-prefixed dirs like `_active`)
    let normalized = encoded.replace("--", "/_");

    // Step 2: Split by `-`, keeping segments that may contain `/` from step 1
    let segments: Vec<&str> = normalized.split('-').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }

    // Step 3: Greedy filesystem matching — every segment must resolve
    let mut path = PathBuf::from("/");
    let mut i = 0;

    while i < segments.len() {
        let remaining = segments.len() - i;
        let max_width = remaining.min(8);
        let mut matched = false;

        for width in (1..=max_width).rev() {
            let candidate_name = segments[i..i + width].join("-");
            let candidate_path = path.join(&candidate_name);
            if candidate_path.is_dir() {
                path = candidate_path;
                i += width;
                matched = true;
                break;
            }
        }

        if !matched {
            // Path doesn't fully resolve — project directory likely deleted/moved
            return None;
        }
    }

    path.file_name().map(|n| n.to_string_lossy().to_string())
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
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    .map(|e| {
                        let path = e.path();
                        let name = e.file_name().to_string_lossy().to_string();
                        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let kind = if name == "MEMORY.md" {
                            FileKind::MemoryIndex
                        } else {
                            FileKind::Memory
                        };
                        MemoryFile {
                            kind,
                            path,
                            name,
                            size,
                        }
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

            let name = decode_project_name(&dir_name)?;
            Some(Project {
                name,
                path: project_path,
                files,
            })
        })
        .collect();

    // Sort projects alphabetically
    project_entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Deduplicate: Claude Code has two encoding schemes (old: `_active`, new: `--active`)
    // Keep the entry with more files; if equal, keep whichever comes first
    project_entries.dedup_by(|b, a| {
        if a.name == b.name {
            if b.files.len() > a.files.len() {
                *a = std::mem::take(b);
            }
            true
        } else {
            false
        }
    });

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

        // Create a "project" dir inside the temp dir so greedy decode resolves it
        let real_project = claude_dir.join("testproject");
        fs::create_dir_all(&real_project).unwrap();

        // Encode the project path as Claude Code would
        let encoded = claude_dir
            .join("testproject")
            .to_string_lossy()
            .replace('/', "-");
        let project_dir = claude_dir.join("projects").join(&encoded);
        let memory_dir = project_dir.join("memory");
        fs::create_dir_all(&memory_dir).unwrap();
        fs::write(project_dir.join("CLAUDE.md"), "# Project").unwrap();
        fs::write(memory_dir.join("MEMORY.md"), "# Index").unwrap();
        fs::write(memory_dir.join("notes.md"), "# Notes").unwrap();

        let projects = scan_claude_dir(&claude_dir);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "testproject");
        assert_eq!(projects[0].files.len(), 3);
        assert_eq!(projects[0].files[0].name, "CLAUDE.md");
        assert_eq!(projects[0].files[1].name, "MEMORY.md");
        assert_eq!(projects[0].files[2].name, "notes.md");
    }

    #[test]
    fn scan_excludes_deleted_projects() {
        let (_tmp, claude_dir) = create_test_dir();

        // Project entry exists in ~/.claude/projects/ but original dir is gone
        let project_dir = claude_dir.join("projects").join("-Users-fake-deleted");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join("CLAUDE.md"), "# Stale").unwrap();

        let projects = scan_claude_dir(&claude_dir);
        assert!(projects.is_empty()); // excluded because /Users/fake/deleted doesn't exist
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
    fn decode_nonexistent_path_returns_none() {
        let result = decode_project_name("-Users-test-myproject");
        assert_eq!(result, None);
    }

    #[test]
    fn decode_real_path_returns_some_basename() {
        if let Some(home) = dirs::home_dir() {
            let home_str = home.to_string_lossy().replace('/', "-");
            let result = decode_project_name(&home_str);
            let expected = home.file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(result, Some(expected));
        }
    }
}
