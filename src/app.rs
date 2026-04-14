use std::fs;
use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::scan::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Projects,
    Files,
    Preview,
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
        };
        app.load_content();
        app
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects.get(self.project_index)
    }

    pub fn selected_file_path(&self) -> Option<&Path> {
        self.selected_project()
            .and_then(|p| p.files.get(self.file_index))
            .map(|f| f.path.as_path())
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
                self.scroll_offset = self.scroll_offset.saturating_add(1);
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
}
