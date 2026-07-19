use std::path::PathBuf;

#[derive(Default, Debug)]
pub struct Buffer {
    /// Le nom d'origine (ex: "README.md")
    pub original_name: String,

    /// L'emplacement du fichier temporaire sur ton système (ex: "/tmp/79ecefd.dwx")
    pub temp_path: PathBuf,

    /// Le contenu découpé en lignes pour faciliter la pagination par crossterm.
    /// (Beaucoup plus facile à manipuler qu'une seule énorme String)
    pub lines: Vec<String>,
}
