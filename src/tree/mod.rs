use is_executable::IsExecutable;
use std::collections::HashMap;
use std::fs::{self};
use std::path::{Path, PathBuf};

// La "photo" de l'état d'un dossier
#[derive(Clone, Default)]
pub struct DirState {
    pub selected_index: usize,
    pub scroll_offset: usize,
}

#[derive(Clone, Debug)]
pub struct FileItem {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_executable: bool,
    pub is_symlink: bool,
}

impl FileItem {
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_dir = path.is_dir();
        let is_file = path.is_file();
        let is_executable = path.is_executable();
        let is_symlink = path.is_symlink();
        Self {
            path,
            name,
            is_dir,
            is_executable,
            is_file,
            is_symlink,
        }
    }
}

#[derive(Clone)]
pub struct MillerState {
    pub current_dir: PathBuf,
    pub filtered_indices: Vec<usize>,
    pub parent_entries: Vec<FileItem>,
    pub current_entries: Vec<FileItem>,
    pub history: HashMap<PathBuf, DirState>,
    pub selected_index: usize,
    pub scroll_offset: usize,
}

impl Default for MillerState {
    fn default() -> Self {
        Self {
            current_dir: Default::default(),
            filtered_indices: Default::default(),
            parent_entries: Default::default(),
            current_entries: Default::default(),
            history: Default::default(),
            selected_index: Default::default(),
            scroll_offset: Default::default(),
        }
    }
}

impl MillerState {
    pub fn new(start_dir: PathBuf) -> Self {
        let mut state = Self {
            current_dir: start_dir,
            parent_entries: Vec::new(),
            current_entries: Vec::new(),
            selected_index: 0,
            filtered_indices: Vec::new(),
            scroll_offset: 0,
            history: HashMap::new(),
        };
        state.refresh();
        state
    }
    pub fn filter(&mut self, query: &str) {
        if query.is_empty() {
            // Si la recherche est vide, on réaffiche tout
            self.filtered_indices = (0..self.current_entries.len()).collect();
        } else {
            let query_lower = query.to_lowercase();
            self.filtered_indices = self
                .current_entries
                .iter()
                .enumerate()
                // On garde l'index si le nom contient la recherche
                .filter(|(_, item)| item.name.to_lowercase().contains(&query_lower))
                .map(|(i, _)| i)
                .collect();
        }

        // Sécurité : on remet le curseur en haut pour ne pas pointer dans le vide
        self.selected_index = 0;
        self.scroll_offset = 0;
    }
    pub fn set_dir(&mut self, new_dir: PathBuf) {
        self.current_dir = new_dir;
        // On n'oublie pas de remonter la caméra tout en haut
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.refresh();
    }
    fn read_dir_entries(dir: &Path) -> Vec<FileItem> {
        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                entries.push(FileItem::from_path(entry.path()));
            }
        }
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        entries
    }

    pub fn enter_dir(&mut self) {
        if let Some(&actual_index) = self.filtered_indices.get(self.selected_index) {
            if let Some(selected) = self.current_entries.get(actual_index) {
                if selected.path.is_dir() {
                    self.history.insert(
                        self.current_dir.clone(),
                        DirState {
                            selected_index: self.selected_index,
                            scroll_offset: self.scroll_offset,
                        },
                    );
                    self.current_dir = selected.path.clone();

                    if let Some(saved_state) = self.history.get(&self.current_dir) {
                        self.selected_index = saved_state.selected_index;
                        self.scroll_offset = saved_state.scroll_offset;
                    } else {
                        self.selected_index = 0;
                        self.scroll_offset = 0;
                    }
                    self.refresh();
                }
            }
        }
    }

    pub fn go_parent(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            let previous_dir_name = self
                .current_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            self.current_dir = parent.to_path_buf();
            self.refresh();

            if let Some(pos) = self
                .current_entries
                .iter()
                .position(|e| e.name == previous_dir_name)
            {
                self.selected_index = pos;
            } else {
                self.selected_index = 0;
            }
            // On refait le filtre pour s'assurer que le curseur est valide
            self.filtered_indices = (0..self.current_entries.len()).collect();
        }
    }

    pub fn refresh(&mut self) {
        self.current_entries = Self::read_dir_entries(&self.current_dir);

        if self.current_entries.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.current_entries.len() {
            self.selected_index = self.current_entries.len() - 1;
        }

        if let Some(parent) = self.current_dir.parent() {
            self.parent_entries = Self::read_dir_entries(parent);
        } else {
            self.parent_entries.clear();
        }

        // On réinitialise le filtre
        self.filtered_indices = (0..self.current_entries.len()).collect();
    }

    pub fn move_down(&mut self, visible_rows: usize) -> Option<PathBuf> {
        if self.selected_index + 1 < self.filtered_indices.len() {
            self.selected_index += 1;
            if self.selected_index >= self.scroll_offset + visible_rows {
                self.scroll_offset += 1;
            }
            return self.get_selected_path();
        }
        None
    }

    pub fn move_up(&mut self) -> Option<PathBuf> {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset -= 1;
            }
            return self.get_selected_path();
        }
        None
    }

    pub fn get_selected_path(&self) -> Option<PathBuf> {
        if let Some(&actual_index) = self.filtered_indices.get(self.selected_index)
            && let Some(selected) = self.current_entries.get(actual_index)
        {
            return Some(selected.path.clone());
        }
        None
    }
}
