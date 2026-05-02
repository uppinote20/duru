//! @handbook 2.3-adaptive-refresh
//! @handbook 3.1-discriminated-enums
//! @tested src/app.rs#tests

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::scan::{FileKind, MemoryFile, Project};
use crate::sessions::{SessionCache, SessionEntry, SessionsSort};

/// Sessions-mode refresh cadence when any session had activity in the last
/// `RECENT_ACTIVITY_SECS` — snappy enough for TTL countdowns to feel live.
pub(crate) const FAST_POLL_MS: u64 = 1000;

/// Sessions-mode refresh cadence when everything is quiet; backs off to
/// reduce filesystem churn while still catching new sessions within a second
/// or two.
pub(crate) const SLOW_POLL_MS: u64 = 2000;

/// Threshold below which a session counts as "recent activity" for the
/// refresh-interval decision.
pub(crate) const RECENT_ACTIVITY_SECS: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Projects,
    Files,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Memory,
    Sessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionsPane {
    Table,
    Detail,
}

pub struct App {
    pub projects: Vec<Project>,
    pub focus: Pane,
    pub project_index: usize,
    pub file_index: usize,
    pub scroll_offset: u16,
    pub content: String,
    pub should_quit: bool,
    pub wants_edit: bool,
    /// Path armed by `d` and awaiting `y/N`. Stays `Option<PathBuf>` for
    /// now because there is only one modal; if a second one lands (e.g.
    /// rename), promote to a `Modal` enum.
    pub(crate) delete_confirm: Option<PathBuf>,

    // Sessions mode
    pub mode: AppMode,
    pub session_cache: SessionCache,
    pub sessions: Vec<SessionEntry>,
    pub session_index: usize,
    pub session_scroll: u16,
    pub sessions_focus: SessionsPane,
    pub sessions_sort: SessionsSort,
    pub sort_reverse: bool,
    pub last_refresh: Instant,
    pub wants_refresh: bool,
    pub skip_real_refresh: bool,
}

impl App {
    pub fn new(projects: Vec<Project>) -> Self {
        let mut app = Self {
            projects,
            focus: Pane::Projects,
            project_index: 0,
            file_index: 0,
            scroll_offset: 0,
            content: String::new(),
            should_quit: false,
            wants_edit: false,
            delete_confirm: None,

            mode: AppMode::Memory,
            session_cache: SessionCache::new(),
            sessions: Vec::new(),
            session_index: 0,
            session_scroll: 0,
            sessions_focus: SessionsPane::Table,
            sessions_sort: SessionsSort::default(),
            sort_reverse: false,
            last_refresh: Instant::now(),
            wants_refresh: false,
            skip_real_refresh: false,
        };
        app.load_content();
        app
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects.get(self.project_index)
    }

    pub fn selected_file(&self) -> Option<&MemoryFile> {
        self.selected_project()
            .and_then(|p| p.files.get(self.file_index))
    }

    pub fn selected_file_path(&self) -> Option<&Path> {
        self.selected_file().map(|f| f.path.as_path())
    }

    pub fn load_content(&mut self) {
        self.content = match self.selected_project() {
            Some(project) => match project.files.get(self.file_index) {
                Some(file) => {
                    fs::read_to_string(&file.path).unwrap_or_else(|e| format!("(read error: {e})"))
                }
                None => String::new(),
            },
            None => String::new(),
        };
        self.scroll_offset = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }
        // Sits above the Tab dispatch so a mode toggle can't bypass the
        // modal while it's armed.
        if self.delete_confirm.is_some() {
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                self.execute_delete();
            } else {
                self.delete_confirm = None;
            }
            return;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.toggle_mode();
            return;
        }
        match self.mode {
            AppMode::Memory => self.handle_key_memory(key),
            AppMode::Sessions => self.handle_key_sessions(key),
        }
    }

    fn arm_delete(&mut self) {
        if !matches!(self.focus, Pane::Files | Pane::Preview) {
            return;
        }
        let Some(file) = self.selected_file() else {
            return;
        };
        // GlobalClaudeMd: user's main personal instructions — accidental
        // loss is high-cost. MemoryIndex (MEMORY.md): deleting orphans
        // every memory file the index references; the auto-memory tree
        // breaks invisibly. Both stay deletable from a shell.
        // TODO(duru-flash-hint): surface this refusal via a flash hint
        // once the hint subsystem lands; today the user gets no signal.
        if matches!(file.kind, FileKind::GlobalClaudeMd | FileKind::MemoryIndex) {
            return;
        }
        self.delete_confirm = Some(file.path.clone());
    }

    fn execute_delete(&mut self) {
        let Some(path) = self.delete_confirm.take() else {
            return;
        };
        match fs::remove_file(&path) {
            Ok(()) => self.apply_delete_to_state(&path),
            // File already gone (race with external rm) — the goal is
            // achieved, sync state to match disk.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.apply_delete_to_state(&path);
            }
            // TODO(duru-flash-hint): surface permission/IO errors when the
            // hint subsystem lands. Today the file stays in the list so
            // the user can retry, but they get no signal that the attempt
            // failed.
            Err(_) => {}
        }
    }

    fn apply_delete_to_state(&mut self, path: &Path) {
        let (project_now_empty, new_files_len) = {
            let Some(project) = self.projects.get_mut(self.project_index) else {
                return;
            };
            let Some(file_pos) = project.files.iter().position(|f| f.path == *path) else {
                return;
            };
            project.files.remove(file_pos);
            (project.files.is_empty(), project.files.len())
        };

        if project_now_empty {
            self.projects.remove(self.project_index);
            clamp_index(&mut self.project_index, self.projects.len());
            self.file_index = 0;
            self.focus = Pane::Projects;
        } else {
            clamp_index(&mut self.file_index, new_files_len);
        }
        self.load_content();
    }

    fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            AppMode::Memory => AppMode::Sessions,
            AppMode::Sessions => AppMode::Memory,
        };
        if self.mode == AppMode::Sessions && self.sessions.is_empty() && !self.skip_real_refresh {
            self.wants_refresh = true;
        }
    }

    fn handle_key_sessions(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.sessions_move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.sessions_move_down(),
            KeyCode::Left | KeyCode::Char('h') => {
                if self.sessions_focus == SessionsPane::Detail {
                    self.sessions_focus = SessionsPane::Table;
                }
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                if self.sessions_focus == SessionsPane::Table {
                    self.sessions_focus = SessionsPane::Detail;
                }
            }
            KeyCode::Char('g') => {
                if self.sessions_focus == SessionsPane::Table {
                    self.session_index = 0;
                    self.session_scroll = 0;
                }
            }
            KeyCode::Char('G') => {
                if self.sessions_focus == SessionsPane::Table && !self.sessions.is_empty() {
                    self.session_index = self.sessions.len() - 1;
                    self.session_scroll = 0;
                }
            }
            KeyCode::Char('s') => {
                self.cycle_sort();
                self.wants_refresh = true;
            }
            KeyCode::Char('S') => {
                self.sort_reverse = !self.sort_reverse;
                self.wants_refresh = true;
            }
            KeyCode::Char('r') => self.wants_refresh = true,
            KeyCode::Char('J') => self.jump_to_project_memory(),
            _ => {}
        }
    }

    /// Silent no-op if the project's registry entry outlives its scanned
    /// directory (race after external deletion) or the matched project
    /// has no files.
    fn jump_to_project_memory(&mut self) {
        let Some(entry) = self.sessions.get(self.session_index) else {
            return;
        };
        let Some(project_idx) = self
            .projects
            .iter()
            .position(|p| p.name == entry.project_name)
        else {
            return;
        };
        let Some(file_idx) = pick_jump_target(&self.projects[project_idx].files) else {
            return;
        };

        self.mode = AppMode::Memory;
        self.focus = Pane::Preview;
        self.project_index = project_idx;
        self.file_index = file_idx;
        self.load_content();
    }

    fn cycle_sort(&mut self) {
        self.sessions_sort = match self.sessions_sort {
            SessionsSort::LastActivity => SessionsSort::CacheTtl,
            SessionsSort::CacheTtl => SessionsSort::Project,
            SessionsSort::Project => SessionsSort::Size,
            SessionsSort::Size => SessionsSort::LastActivity,
        };
    }

    pub fn clamp_session_index(&mut self) {
        clamp_index(&mut self.session_index, self.sessions.len());
    }

    pub fn refresh_sessions(&mut self, claude_dir: &Path) {
        use crate::sessions::sort_entries;
        self.session_cache.refresh(claude_dir);
        let mut entries = self.session_cache.entries();
        sort_entries(
            &mut entries,
            self.sessions_sort,
            self.sort_reverse,
            chrono::Utc::now(),
        );
        self.sessions = entries;
        self.clamp_session_index();
        self.last_refresh = Instant::now();
    }

    pub fn refresh_interval(&self) -> Duration {
        if self.mode != AppMode::Sessions {
            return Duration::from_secs(3600);
        }
        let now = chrono::Utc::now();
        let has_recent = self
            .sessions
            .iter()
            .any(|e| (now - e.last_activity).num_seconds() < RECENT_ACTIVITY_SECS);
        if has_recent {
            Duration::from_millis(FAST_POLL_MS)
        } else {
            Duration::from_millis(SLOW_POLL_MS)
        }
    }

    pub fn with_demo_sessions(mut self, demos: Vec<SessionEntry>) -> Self {
        self.sessions = demos;
        self.skip_real_refresh = true;
        self
    }

    fn sessions_move_up(&mut self) {
        match self.sessions_focus {
            SessionsPane::Table => {
                if self.session_index > 0 {
                    self.session_index -= 1;
                    self.session_scroll = 0;
                }
            }
            SessionsPane::Detail => {
                self.session_scroll = self.session_scroll.saturating_sub(1);
            }
        }
    }

    fn sessions_move_down(&mut self) {
        match self.sessions_focus {
            SessionsPane::Table => {
                if self.session_index + 1 < self.sessions.len() {
                    self.session_index += 1;
                    self.session_scroll = 0;
                }
            }
            SessionsPane::Detail => {
                // Detail fits its 6-line content exactly — no scroll bound > 0.
            }
        }
    }

    fn handle_key_memory(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.move_up(),
            KeyCode::Down | KeyCode::Char('j') => self.move_down(),
            KeyCode::Left | KeyCode::Char('h') => self.move_left(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => self.move_right(),
            KeyCode::Char('e') => {
                if matches!(self.focus, Pane::Files | Pane::Preview)
                    && self.selected_file_path().is_some()
                {
                    self.wants_edit = true;
                }
            }
            KeyCode::Char('d') => self.arm_delete(),
            _ => {}
        }
    }

    fn move_up(&mut self) {
        match self.focus {
            Pane::Projects => {
                if self.project_index > 0 {
                    self.project_index -= 1;
                    self.file_index = 0;
                    self.load_content();
                }
            }
            Pane::Files => {
                if self.file_index > 0 {
                    self.file_index -= 1;
                    self.load_content();
                }
            }
            Pane::Preview => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }
    }

    fn move_down(&mut self) {
        match self.focus {
            Pane::Projects => {
                if self.project_index + 1 < self.projects.len() {
                    self.project_index += 1;
                    self.file_index = 0;
                    self.load_content();
                }
            }
            Pane::Files => {
                if let Some(project) = self.selected_project()
                    && self.file_index + 1 < project.files.len()
                {
                    self.file_index += 1;
                    self.load_content();
                }
            }
            Pane::Preview => {
                // Use logical line count as the bound. Paragraph wraps long
                // lines into more visual rows than lines().count() reports,
                // so subtracting a viewport height would prevent reaching
                // the end of wrapped content. Allowing scroll up to
                // total - 1 ensures the last line is always reachable;
                // over-scrolling just shows empty space (like `less`).
                let total = self.content.lines().count() as u16;
                if self.scroll_offset < total {
                    self.scroll_offset = self.scroll_offset.saturating_add(1);
                }
            }
        }
    }

    fn move_left(&mut self) {
        self.focus = match self.focus {
            Pane::Projects => Pane::Projects,
            Pane::Files => Pane::Projects,
            Pane::Preview => Pane::Files,
        };
    }

    fn move_right(&mut self) {
        self.focus = match self.focus {
            Pane::Projects => Pane::Files,
            Pane::Files => Pane::Preview,
            Pane::Preview => Pane::Preview,
        };
    }
}

/// Clamp an index into a Vec of length `len`. Empty Vec → index 0; in
/// bounds → unchanged; over the end → last element.
fn clamp_index(index: &mut usize, len: usize) {
    *index = if len == 0 { 0 } else { (*index).min(len - 1) };
}

/// CLAUDE.md beats MEMORY.md because the user is asking "what does this
/// project want me to do?" — primary instructions, not the auto-memory
/// index.
fn pick_jump_target(files: &[MemoryFile]) -> Option<usize> {
    files
        .iter()
        .position(|f| matches!(f.kind, FileKind::ProjectClaudeMd | FileKind::GlobalClaudeMd))
        .or_else(|| files.iter().position(|f| f.kind == FileKind::MemoryIndex))
        .or_else(|| (!files.is_empty()).then_some(0))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{FileKind, MemoryFile};
    use std::path::PathBuf;

    fn make_test_projects() -> Vec<Project> {
        vec![
            Project {
                name: "GLOBAL".to_string(),
                path: PathBuf::from("/tmp/test"),
                files: vec![MemoryFile {
                    kind: FileKind::GlobalClaudeMd,
                    path: PathBuf::from("/tmp/test/CLAUDE.md"),
                    name: "CLAUDE.md".to_string(),
                    size: 100,
                }],
            },
            Project {
                name: "myproject".to_string(),
                path: PathBuf::from("/tmp/test/projects/myproject"),
                files: vec![
                    MemoryFile {
                        kind: FileKind::ProjectClaudeMd,
                        path: PathBuf::from("/tmp/test/projects/myproject/CLAUDE.md"),
                        name: "CLAUDE.md".to_string(),
                        size: 200,
                    },
                    MemoryFile {
                        kind: FileKind::MemoryIndex,
                        path: PathBuf::from("/tmp/test/projects/myproject/memory/MEMORY.md"),
                        name: "MEMORY.md".to_string(),
                        size: 50,
                    },
                ],
            },
        ]
    }

    #[test]
    fn new_app_starts_on_projects_pane() {
        let app = App::new(make_test_projects());
        assert_eq!(app.focus, Pane::Projects);
        assert_eq!(app.project_index, 0);
        assert_eq!(app.file_index, 0);
    }

    #[test]
    fn new_app_starts_in_memory_mode() {
        let app = App::new(make_test_projects());
        assert_eq!(app.mode, AppMode::Memory);
    }

    fn app_in_sessions_mode() -> App {
        use crate::sessions::demo_sessions;
        let mut app = App::new(make_test_projects());
        app.sessions = demo_sessions();
        app.mode = AppMode::Sessions;
        app
    }

    #[test]
    fn sessions_mode_j_moves_row_down() {
        let mut app = app_in_sessions_mode();
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.session_index, 1);
    }

    #[test]
    fn sessions_mode_k_moves_row_up() {
        let mut app = app_in_sessions_mode();
        app.session_index = 2;
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(app.session_index, 1);
    }

    #[test]
    fn sessions_mode_j_does_not_overflow() {
        let mut app = app_in_sessions_mode();
        app.session_index = app.sessions.len() - 1;
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.session_index, app.sessions.len() - 1);
    }

    #[test]
    fn sessions_mode_l_enters_detail() {
        let mut app = app_in_sessions_mode();
        app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(app.sessions_focus, SessionsPane::Detail);
    }

    #[test]
    fn sessions_mode_h_exits_detail() {
        let mut app = app_in_sessions_mode();
        app.sessions_focus = SessionsPane::Detail;
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert_eq!(app.sessions_focus, SessionsPane::Table);
    }

    #[test]
    fn sessions_mode_g_jumps_top() {
        let mut app = app_in_sessions_mode();
        app.session_index = 3;
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.session_index, 0);
    }

    #[test]
    fn sessions_mode_shift_g_jumps_bottom() {
        let mut app = app_in_sessions_mode();
        app.session_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.session_index, app.sessions.len() - 1);
    }

    #[test]
    fn sessions_mode_q_quits() {
        let mut app = app_in_sessions_mode();
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn sessions_mode_e_ignored() {
        let mut app = app_in_sessions_mode();
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(!app.wants_edit);
    }

    #[test]
    fn sessions_mode_detail_k_scrolls_up() {
        let mut app = app_in_sessions_mode();
        app.sessions_focus = SessionsPane::Detail;
        app.session_scroll = 2;
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(app.session_scroll, 1);
    }

    #[test]
    fn sessions_mode_detail_j_does_not_scroll_past_zero() {
        let mut app = app_in_sessions_mode();
        app.sessions_focus = SessionsPane::Detail;
        for _ in 0..20 {
            app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        }
        assert_eq!(app.session_scroll, 0);
    }

    #[test]
    fn sessions_mode_j_resets_detail_scroll() {
        let mut app = app_in_sessions_mode();
        app.session_scroll = 3;
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(app.session_index, 1);
        assert_eq!(app.session_scroll, 0);
    }

    #[test]
    fn sessions_mode_k_resets_detail_scroll() {
        let mut app = app_in_sessions_mode();
        app.session_index = 2;
        app.session_scroll = 3;
        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(app.session_index, 1);
        assert_eq!(app.session_scroll, 0);
    }

    #[test]
    fn sessions_mode_g_resets_detail_scroll() {
        let mut app = app_in_sessions_mode();
        app.session_index = 3;
        app.session_scroll = 3;
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.session_index, 0);
        assert_eq!(app.session_scroll, 0);
    }

    #[test]
    fn sessions_mode_shift_g_resets_detail_scroll() {
        let mut app = app_in_sessions_mode();
        app.session_index = 0;
        app.session_scroll = 3;
        app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.session_index, app.sessions.len() - 1);
        assert_eq!(app.session_scroll, 0);
    }

    #[test]
    fn sessions_mode_s_cycles_sort() {
        let mut app = app_in_sessions_mode();
        assert_eq!(app.sessions_sort, SessionsSort::LastActivity);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.sessions_sort, SessionsSort::CacheTtl);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.sessions_sort, SessionsSort::Project);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.sessions_sort, SessionsSort::Size);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.sessions_sort, SessionsSort::LastActivity);
    }

    #[test]
    fn sessions_mode_shift_s_toggles_sort_reverse() {
        let mut app = app_in_sessions_mode();
        assert!(!app.sort_reverse);
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert!(app.sort_reverse);
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert!(!app.sort_reverse);
    }

    #[test]
    fn sessions_mode_shift_s_requests_immediate_refresh() {
        // The header arrow flips on the next render, so the table rows must
        // be re-sorted the same tick — not up to `FAST_POLL_MS` later.
        let mut app = app_in_sessions_mode();
        assert!(!app.wants_refresh);
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert!(app.wants_refresh);
    }

    #[test]
    fn sessions_mode_s_requests_immediate_refresh() {
        // Same rationale as `S`: the active-column indicator moves on render,
        // rows must re-sort the same tick.
        let mut app = app_in_sessions_mode();
        assert!(!app.wants_refresh);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(app.wants_refresh);
    }

    #[test]
    fn sessions_mode_s_preserves_sort_reverse() {
        let mut app = app_in_sessions_mode();
        app.sort_reverse = true;
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.sessions_sort, SessionsSort::CacheTtl);
        assert!(
            app.sort_reverse,
            "cycling sort must not clear the reverse flag"
        );
    }

    #[test]
    fn sessions_mode_r_sets_wants_refresh() {
        let mut app = app_in_sessions_mode();
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(app.wants_refresh);
    }

    #[test]
    fn session_index_clamps_when_list_shrinks() {
        let mut app = app_in_sessions_mode();
        app.session_index = 4;
        app.sessions.truncate(2);
        app.clamp_session_index();
        assert_eq!(app.session_index, 1);
    }

    #[test]
    fn session_index_clamps_to_zero_when_empty() {
        let mut app = app_in_sessions_mode();
        app.session_index = 4;
        app.sessions.clear();
        app.clamp_session_index();
        assert_eq!(app.session_index, 0);
    }

    #[test]
    fn refresh_interval_fast_when_activity_recent() {
        let app = app_in_sessions_mode();
        assert_eq!(app.refresh_interval(), Duration::from_millis(FAST_POLL_MS));
    }

    #[test]
    fn refresh_interval_slow_when_all_idle() {
        let mut app = app_in_sessions_mode();
        let old = chrono::Utc::now() - chrono::Duration::hours(1);
        for s in &mut app.sessions {
            s.last_activity = old;
        }
        assert_eq!(app.refresh_interval(), Duration::from_millis(SLOW_POLL_MS));
    }

    #[test]
    fn refresh_interval_disabled_in_memory_mode() {
        let app = App::new(make_test_projects());
        assert!(app.refresh_interval() >= Duration::from_secs(60));
    }

    #[test]
    fn tab_key_toggles_mode() {
        let mut app = App::new(make_test_projects());
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mode, AppMode::Sessions);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mode, AppMode::Memory);
    }

    #[test]
    fn back_tab_also_toggles_mode() {
        let mut app = App::new(make_test_projects());
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.mode, AppMode::Sessions);
    }

    #[test]
    fn toggle_preserves_memory_state() {
        let mut app = App::new(make_test_projects());
        app.focus = Pane::Files;
        app.project_index = 1;
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.focus, Pane::Files);
        assert_eq!(app.project_index, 1);
    }

    #[test]
    fn move_down_increments_project_index() {
        let mut app = App::new(make_test_projects());
        app.move_down();
        assert_eq!(app.project_index, 1);
    }

    #[test]
    fn move_down_does_not_overflow() {
        let mut app = App::new(make_test_projects());
        app.move_down();
        app.move_down();
        assert_eq!(app.project_index, 1);
    }

    #[test]
    fn move_right_changes_focus() {
        let mut app = App::new(make_test_projects());
        assert_eq!(app.focus, Pane::Projects);
        app.move_right();
        assert_eq!(app.focus, Pane::Files);
        app.move_right();
        assert_eq!(app.focus, Pane::Preview);
        app.move_right();
        assert_eq!(app.focus, Pane::Preview);
    }

    #[test]
    fn move_left_changes_focus() {
        let mut app = App::new(make_test_projects());
        app.focus = Pane::Preview;
        app.move_left();
        assert_eq!(app.focus, Pane::Files);
        app.move_left();
        assert_eq!(app.focus, Pane::Projects);
        app.move_left();
        assert_eq!(app.focus, Pane::Projects);
    }

    #[test]
    fn q_key_sets_should_quit() {
        let mut app = App::new(make_test_projects());
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.should_quit);
    }

    #[test]
    fn e_key_sets_wants_edit_in_files_pane() {
        let mut app = App::new(make_test_projects());
        app.focus = Pane::Files;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(app.wants_edit);
    }

    #[test]
    fn e_key_sets_wants_edit_in_preview_pane() {
        let mut app = App::new(make_test_projects());
        app.focus = Pane::Preview;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(app.wants_edit);
    }

    #[test]
    fn e_key_ignored_in_projects_pane() {
        let mut app = App::new(make_test_projects());
        app.focus = Pane::Projects;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(!app.wants_edit);
    }

    #[test]
    fn e_key_ignored_when_no_files() {
        let mut app = App::new(vec![Project {
            name: "empty".to_string(),
            path: PathBuf::from("/tmp/empty"),
            files: vec![],
        }]);
        app.focus = Pane::Files;
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert!(!app.wants_edit);
    }

    #[test]
    fn selected_file_path_returns_path_when_file_selected() {
        let app = App::new(make_test_projects());
        assert_eq!(
            app.selected_file_path(),
            Some(std::path::Path::new("/tmp/test/CLAUDE.md"))
        );
    }

    #[test]
    fn selected_file_path_returns_none_for_empty_project() {
        let app = App::new(vec![Project {
            name: "empty".to_string(),
            path: PathBuf::from("/tmp/empty"),
            files: vec![],
        }]);
        assert_eq!(app.selected_file_path(), None);
    }

    // --- Memory file delete (issue #40) ---

    /// Real on-disk files so `fs::remove_file` actually has something to
    /// remove. Returns the TempDir to keep it alive for the test scope.
    fn app_with_real_files(files: &[(FileKind, &str)]) -> (tempfile::TempDir, App) {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("proj");
        std::fs::create_dir_all(&project_dir).unwrap();

        let memory_files: Vec<MemoryFile> = files
            .iter()
            .map(|(kind, name)| {
                let path = project_dir.join(name);
                std::fs::write(&path, b"test content").unwrap();
                MemoryFile {
                    kind: kind.clone(),
                    path,
                    name: name.to_string(),
                    size: 12,
                }
            })
            .collect();

        let mut app = App::new(vec![Project {
            name: "proj".to_string(),
            path: project_dir,
            files: memory_files,
        }]);
        app.focus = Pane::Files;
        (tmp, app)
    }

    #[test]
    fn delete_d_arms_confirm_when_on_memory_file() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_some());
    }

    #[test]
    fn delete_armed_holds_path_of_selected_file() {
        let (_tmp, mut app) = app_with_real_files(&[
            (FileKind::ProjectClaudeMd, "CLAUDE.md"),
            (FileKind::Memory, "notes.md"),
        ]);
        app.file_index = 1;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert_eq!(
            app.delete_confirm.as_ref().and_then(|p| p.file_name()),
            Some(std::ffi::OsStr::new("notes.md"))
        );
    }

    #[test]
    fn delete_d_ignored_in_projects_pane() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        app.focus = Pane::Projects;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_none());
    }

    #[test]
    fn delete_d_ignored_when_no_file_selected() {
        let mut app = App::new(vec![Project {
            name: "empty".to_string(),
            path: PathBuf::from("/tmp/empty-del"),
            files: vec![],
        }]);
        app.focus = Pane::Files;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_none());
    }

    #[test]
    fn delete_refuses_global_claude_md() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::GlobalClaudeMd, "CLAUDE.md")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(
            app.delete_confirm.is_none(),
            "global CLAUDE.md must not be deletable from duru"
        );
    }

    #[test]
    fn delete_refuses_memory_index() {
        // MEMORY.md indexes the auto-memory tree — deleting it orphans
        // every memory file it references.
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::MemoryIndex, "MEMORY.md")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(
            app.delete_confirm.is_none(),
            "MEMORY.md must not be deletable from duru"
        );
    }

    #[test]
    fn delete_y_after_arm_removes_file_from_disk() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        let path = app.selected_file_path().unwrap().to_path_buf();
        assert!(path.exists());

        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        assert!(!path.exists(), "file should be removed from disk");
        assert!(
            app.delete_confirm.is_none(),
            "modal must clear after confirm"
        );
    }

    #[test]
    fn delete_y_after_arm_removes_file_from_project_state() {
        let (_tmp, mut app) =
            app_with_real_files(&[(FileKind::Memory, "a.md"), (FileKind::Memory, "b.md")]);
        app.file_index = 0; // delete a.md
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        assert_eq!(app.projects[0].files.len(), 1);
        assert_eq!(app.projects[0].files[0].name, "b.md");
    }

    #[test]
    fn delete_n_cancels_confirm() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        let path = app.selected_file_path().unwrap().to_path_buf();
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_none());
        assert!(path.exists(), "file must still exist after cancel");
    }

    #[test]
    fn delete_arbitrary_key_cancels_confirm() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        // a navigation key should also cancel
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_none());
    }

    #[test]
    fn delete_clamps_file_index_when_last_file_removed() {
        let (_tmp, mut app) =
            app_with_real_files(&[(FileKind::Memory, "a.md"), (FileKind::Memory, "b.md")]);
        app.file_index = 1; // delete the last file
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        assert_eq!(app.projects[0].files.len(), 1);
        assert_eq!(
            app.file_index, 0,
            "file_index must clamp to last valid position"
        );
    }

    #[test]
    fn delete_removes_project_when_last_file_removed() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "lone.md")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        assert!(
            app.projects.is_empty(),
            "empty project must disappear from scan"
        );
        assert_eq!(app.focus, Pane::Projects, "focus must retreat from Files");
    }

    #[test]
    fn delete_treats_already_gone_file_as_success() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "racing.md")]);
        let path = app.selected_file_path().unwrap().to_path_buf();
        std::fs::remove_file(&path).unwrap(); // race: gone before confirm

        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));

        assert!(
            app.projects.is_empty() || app.projects[0].files.is_empty(),
            "state must update even when file was already gone"
        );
    }

    #[test]
    fn tab_during_delete_confirm_clears_confirm() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_some());
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(
            app.delete_confirm.is_none(),
            "Tab must not leave the modal hanging"
        );
    }

    #[test]
    fn delete_d_ignored_in_sessions_mode() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        app.mode = AppMode::Sessions;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(app.delete_confirm.is_none());
    }

    #[test]
    fn delete_d_works_from_preview_pane() {
        let (_tmp, mut app) = app_with_real_files(&[(FileKind::Memory, "notes.md")]);
        app.focus = Pane::Preview;
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(
            app.delete_confirm.is_some(),
            "delete should be reachable from Preview, matching `e` (edit)"
        );
    }

    // --- Jump-to-Memory (issue #11) ---

    fn make_jump_test_session(project_name: &str) -> SessionEntry {
        SessionEntry {
            session_id: format!("sess-{project_name}"),
            short_id: format!("s-{project_name}"),
            project_name: project_name.to_string(),
            cwd: None,
            transcript_path: PathBuf::from(format!("/tmp/{project_name}.jsonl")),
            started_at: None,
            last_activity: chrono::Utc::now(),
            file_size: 1000,
            permission_mode: None,
            registry_source: None,
            is_alive: None,
            cache_ttl_secs: 300,
        }
    }

    fn make_jump_test_projects() -> Vec<Project> {
        vec![
            Project {
                name: "GLOBAL".to_string(),
                path: PathBuf::from("/tmp/jump/global"),
                files: vec![MemoryFile {
                    kind: FileKind::GlobalClaudeMd,
                    path: PathBuf::from("/tmp/jump/global/CLAUDE.md"),
                    name: "CLAUDE.md".to_string(),
                    size: 100,
                }],
            },
            // alpha: has CLAUDE.md (should win priority)
            Project {
                name: "alpha".to_string(),
                path: PathBuf::from("/tmp/jump/alpha"),
                files: vec![
                    MemoryFile {
                        kind: FileKind::Memory,
                        path: PathBuf::from("/tmp/jump/alpha/notes.md"),
                        name: "notes.md".to_string(),
                        size: 30,
                    },
                    MemoryFile {
                        kind: FileKind::ProjectClaudeMd,
                        path: PathBuf::from("/tmp/jump/alpha/CLAUDE.md"),
                        name: "CLAUDE.md".to_string(),
                        size: 200,
                    },
                ],
            },
            // beta: only MEMORY.md (no CLAUDE.md)
            Project {
                name: "beta".to_string(),
                path: PathBuf::from("/tmp/jump/beta"),
                files: vec![
                    MemoryFile {
                        kind: FileKind::Memory,
                        path: PathBuf::from("/tmp/jump/beta/aaa.md"),
                        name: "aaa.md".to_string(),
                        size: 10,
                    },
                    MemoryFile {
                        kind: FileKind::MemoryIndex,
                        path: PathBuf::from("/tmp/jump/beta/MEMORY.md"),
                        name: "MEMORY.md".to_string(),
                        size: 50,
                    },
                ],
            },
            // gamma: only generic memory files (no CLAUDE.md, no MEMORY.md)
            Project {
                name: "gamma".to_string(),
                path: PathBuf::from("/tmp/jump/gamma"),
                files: vec![MemoryFile {
                    kind: FileKind::Memory,
                    path: PathBuf::from("/tmp/jump/gamma/first.md"),
                    name: "first.md".to_string(),
                    size: 20,
                }],
            },
        ]
    }

    fn app_with_jump_fixture() -> App {
        let mut app = App::new(make_jump_test_projects());
        app.sessions = vec![
            make_jump_test_session("alpha"),       // [0] → projects[1]
            make_jump_test_session("beta"),        // [1] → projects[2]
            make_jump_test_session("gamma"),       // [2] → projects[3]
            make_jump_test_session("GLOBAL"),      // [3] → projects[0]
            make_jump_test_session("nonexistent"), // [4] → no match
        ];
        app.mode = AppMode::Sessions;
        app
    }

    #[test]
    fn jump_switches_to_memory_mode_and_focuses_preview() {
        let mut app = app_with_jump_fixture();
        app.session_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Memory);
        assert_eq!(app.focus, Pane::Preview);
    }

    #[test]
    fn jump_picks_project_claude_md_when_present() {
        let mut app = app_with_jump_fixture();
        app.session_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.project_index, 1);
        // alpha intentionally orders [notes.md, CLAUDE.md] so that picking
        // by kind (not index 0) is the only way file_index=1 is reached.
        assert_eq!(app.file_index, 1);
        assert_eq!(
            app.projects[app.project_index].files[app.file_index].kind,
            FileKind::ProjectClaudeMd
        );
    }

    #[test]
    fn jump_picks_memory_index_when_no_claude_md() {
        let mut app = app_with_jump_fixture();
        app.session_index = 1;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.project_index, 2);
        assert_eq!(
            app.projects[app.project_index].files[app.file_index].kind,
            FileKind::MemoryIndex
        );
    }

    #[test]
    fn jump_picks_first_file_when_no_claude_or_memory_index() {
        let mut app = app_with_jump_fixture();
        app.session_index = 2;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.project_index, 3);
        assert_eq!(app.file_index, 0);
    }

    #[test]
    fn jump_to_global_session_picks_global_claude_md() {
        let mut app = app_with_jump_fixture();
        app.session_index = 3;
        // Perturb initial Memory-mode state so the test actually verifies
        // the jump (otherwise project_index=0 trivially holds).
        app.project_index = 2;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Memory);
        assert_eq!(app.project_index, 0);
        assert_eq!(
            app.projects[app.project_index].files[app.file_index].kind,
            FileKind::GlobalClaudeMd
        );
    }

    #[test]
    fn jump_with_unknown_project_keeps_mode_unchanged() {
        let mut app = app_with_jump_fixture();
        app.session_index = 4;
        let prior_focus = app.focus;
        let prior_project = app.project_index;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Sessions);
        assert_eq!(app.focus, prior_focus);
        assert_eq!(app.project_index, prior_project);
    }

    #[test]
    fn jump_with_empty_session_list_is_noop() {
        let mut app = App::new(make_jump_test_projects());
        app.sessions.clear();
        app.mode = AppMode::Sessions;
        app.session_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Sessions);
    }

    #[test]
    fn jump_works_from_detail_pane_focus() {
        let mut app = app_with_jump_fixture();
        app.session_index = 0;
        app.sessions_focus = SessionsPane::Detail;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Memory);
        assert_eq!(app.project_index, 1);
    }

    #[test]
    fn tab_after_jump_returns_to_sessions() {
        let mut app = app_with_jump_fixture();
        app.session_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Memory);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mode, AppMode::Sessions);
    }

    #[test]
    fn jump_in_memory_mode_is_ignored() {
        let mut app = App::new(make_jump_test_projects());
        app.sessions = vec![make_jump_test_session("alpha")];
        app.mode = AppMode::Memory;
        app.project_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Memory);
        assert_eq!(app.project_index, 0);
    }

    #[test]
    fn jump_no_op_when_matched_project_has_no_files() {
        // Defends the pick_jump_target Option contract: an empty files vec
        // would otherwise switch to Memory mode with a blank preview.
        let mut app = App::new(vec![Project {
            name: "ghost".to_string(),
            path: PathBuf::from("/tmp/ghost"),
            files: vec![],
        }]);
        app.sessions = vec![make_jump_test_session("ghost")];
        app.mode = AppMode::Sessions;
        app.session_index = 0;
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT));
        assert_eq!(app.mode, AppMode::Sessions);
    }
}
