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
    // Keep the more recently modified entry
    project_entries.dedup_by(|b, a| {
        if a.name == b.name {
            let a_mod = fs::metadata(&a.path).and_then(|m| m.modified()).ok();
            let b_mod = fs::metadata(&b.path).and_then(|m| m.modified()).ok();
            if b_mod > a_mod {
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

/// Demo data for screenshots and testing
pub fn demo_projects() -> Vec<Project> {
    let demo_file = |kind: FileKind, name: &str, size: u64| MemoryFile {
        kind,
        path: PathBuf::from(format!("/tmp/duru-demo/{name}")),
        name: name.to_string(),
        size,
    };

    // Write demo content files to /tmp so preview works
    let demo_dir = Path::new("/tmp/duru-demo");
    let _ = fs::create_dir_all(demo_dir);

    let files_data = [
        (
            "CLAUDE-global.md",
            "# Claude Code Workflow Rules\n\nCore principles: see `~/.claude/SOUL.md`.\n\n## 0. Language Rules\n\n- **GitHub-facing text in English**: release notes,\n  PR descriptions, commit message titles\n- **Commit message body**: local language allowed\n- **Conversation**: match user's language\n\n## 1. Git Workflow\n\n### Commit\nAlways use `/group-commit` skill for commits.\n\n### Branch Targeting\nCheck current branch before commit:\n- `main`/`master`: only version bumps / CI files\n- `feature/*`: proceed as normal\n\n### PR Title (Conventional Commits)\nFormat: `type: subject`\nAllowed: feat | fix | docs | style | refactor | perf | test | chore",
        ),
        (
            "CLAUDE-project.md",
            "# my-webapp\n\nA Next.js web application with TypeScript.\n\n## Tech Stack\n- Next.js 15 (App Router)\n- TypeScript 5.7\n- Tailwind CSS 4\n- Prisma ORM\n\n## Build & Test\n```bash\npnpm dev      # development server\npnpm build    # production build\npnpm test     # run tests\npnpm lint     # eslint check\n```\n\n## Key Conventions\n1. Server components by default\n2. Client components only when needed\n3. API routes in `app/api/`",
        ),
        (
            "MEMORY.md",
            "# my-webapp — Memory Index\n\n- [User Profile](user_profile.md) — Senior full-stack developer, prefers concise responses\n- [API Patterns](api_patterns.md) — REST conventions, error handling, auth middleware\n- [Deployment Notes](deployment.md) — Vercel config, env vars, preview branches",
        ),
        (
            "user_profile.md",
            "---\nname: User Profile\ndescription: Developer role and preferences\ntype: user\n---\n\nSenior full-stack developer with 8 years experience.\nPrefers TypeScript, concise code reviews, no trailing summaries.\n\n**Stack expertise**: React, Node.js, PostgreSQL, Redis\n**Current focus**: Performance optimization and API design",
        ),
        (
            "api_patterns.md",
            "---\nname: API Patterns\ndescription: REST API conventions for this project\ntype: feedback\n---\n\nAll API routes follow this pattern:\n- Validate input with Zod schemas\n- Return consistent error format: `{ error: string, code: number }`\n- Use middleware for auth checks\n\n**Why:** Inconsistent error responses caused frontend bugs in Q1.\n**How to apply:** Every new API route must use `validateRequest()` wrapper.",
        ),
        (
            "deployment.md",
            "---\nname: Deployment Notes\ndescription: Vercel deployment configuration and gotchas\ntype: reference\n---\n\nDeployed on Vercel with automatic preview branches.\n\n- Production: `main` branch\n- Preview: all PR branches\n- Environment variables in Vercel dashboard\n- Edge functions for middleware (auth, i18n)\n\nKnown issue: Cold starts on first request after deploy (~2s).",
        ),
        (
            "CLAUDE-rust.md",
            "# rust-analyzer\n\nA Rust-based code analysis tool.\n\n## Build & Test\n```bash\ncargo build\ncargo test\ncargo clippy -- -D warnings\n```\n\n## Conventions\n- No `unsafe` code\n- All public APIs documented\n- Error types use `thiserror`",
        ),
        (
            "MEMORY-rust.md",
            "# rust-analyzer — Memory Index\n\n- [Build Quirks](build_quirks.md) — Cross-compilation flags, CI caching strategy",
        ),
        (
            "build_quirks.md",
            "---\nname: Build Quirks\ndescription: CI/CD build configuration notes\ntype: project\n---\n\nCross-compile requires `cross` tool for Linux musl targets.\nCI caching key should include Cargo.lock hash.\n\n**Why:** Build times went from 12min to 3min after proper caching.\n**How to apply:** Always update cache key when adding dependencies.",
        ),
        (
            "CLAUDE-design.md",
            "# design-system\n\nShared component library for all frontend projects.\n\n## Stack\n- React 19\n- Storybook 8\n- CSS Modules\n\n## Usage\n```bash\npnpm storybook    # component playground\npnpm build:lib    # build for distribution\n```",
        ),
    ];

    for (name, content) in &files_data {
        let _ = fs::write(demo_dir.join(name), content);
    }

    vec![
        Project {
            name: "GLOBAL".to_string(),
            path: PathBuf::from("/tmp/duru-demo"),
            files: vec![demo_file(
                FileKind::GlobalClaudeMd,
                "CLAUDE-global.md",
                2480,
            )],
        },
        Project {
            name: "design-system".to_string(),
            path: PathBuf::from("/tmp/duru-demo"),
            files: vec![demo_file(
                FileKind::ProjectClaudeMd,
                "CLAUDE-design.md",
                890,
            )],
        },
        Project {
            name: "my-webapp".to_string(),
            path: PathBuf::from("/tmp/duru-demo"),
            files: vec![
                demo_file(FileKind::ProjectClaudeMd, "CLAUDE-project.md", 1240),
                demo_file(FileKind::MemoryIndex, "MEMORY.md", 320),
                demo_file(FileKind::Memory, "api_patterns.md", 580),
                demo_file(FileKind::Memory, "deployment.md", 640),
                demo_file(FileKind::Memory, "user_profile.md", 420),
            ],
        },
        Project {
            name: "rust-analyzer".to_string(),
            path: PathBuf::from("/tmp/duru-demo"),
            files: vec![
                demo_file(FileKind::ProjectClaudeMd, "CLAUDE-rust.md", 760),
                demo_file(FileKind::MemoryIndex, "MEMORY-rust.md", 180),
                demo_file(FileKind::Memory, "build_quirks.md", 350),
            ],
        },
    ]
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
