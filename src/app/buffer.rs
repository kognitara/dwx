use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug)]
pub struct Buffer {
    /// Le nom d'origine (ex: "README.md")
    pub original_name: String,

    /// Le chemin du fichier d'origine
    pub original_path: PathBuf,

    /// L'emplacement du fichier temporaire sur ton système (ex: "/tmp/79ecefd.dwx")
    pub temp_path: PathBuf,

    /// Le contenu découpé en lignes pour faciliter la pagination par crossterm.
    /// (Beaucoup plus facile à manipuler qu'une seule énorme String)
    pub lines: Vec<String>,

    /// Le dernier timestamp de modification du fichier
    pub last_modified: SystemTime,
}

impl Default for Buffer {
    fn default() -> Self {
        Self {
            original_name: String::new(),
            original_path: PathBuf::new(),
            temp_path: PathBuf::new(),
            lines: Vec::new(),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }
}
