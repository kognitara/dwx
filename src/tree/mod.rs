use ignore::WalkBuilder;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::SystemTime;
#[derive(Debug, Clone)]
pub struct TreeItem {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_expanded: bool, // Permet de savoir si on a appuyé sur 'l' dessus[cite: 1]
    pub is_visible: bool,  // Géré par le filtre de recherche dynamique[cite: 1]
    pub modified: SystemTime,
}

#[derive(Debug, Clone, Default)]
pub struct TreeState {
    pub items: Vec<TreeItem>,
    pub selected_index: usize, // Là où se trouve le curseur (j/k)[cite: 1]
    pub scroll_offset: usize,  // Pour le défilement de l'arbre[cite: 1]
}
impl TreeState {
    // Récupère l'élément actuellement sous le curseur
    pub fn get_selected_item(&self) -> Option<&TreeItem> {
        // On filtre pour ne compter que les éléments visibles !
        self.items
            .iter()
            .filter(|i| i.is_visible)
            .nth(self.selected_index)
    }
}
pub fn format_time_ago(modified: SystemTime) -> String {
    let duration = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    let secs = duration.as_secs();

    match secs {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{} min", secs / 60),
        3600..=86399 => format!("{} h", secs / 3600),
        86400..=604799 => format!("{} d", secs / 86400),
        _ => format!("{} sem", secs / 604800),
    }
}

impl TreeState {
    pub fn move_down(&mut self) {
        let visible_count = self.items.iter().filter(|i| i.is_visible).count();
        if visible_count > 0 && self.selected_index < visible_count - 1 {
            self.selected_index += 1;
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }
    // Fonction appelée à chaque fois que tu tapes ou effaces une lettre dans la recherche
    pub fn apply_filter(&mut self, query: &str) {
        let query_lower = query.to_lowercase();

        for item in &mut self.items {
            if query.is_empty() {
                item.is_visible = true; // On affiche tout si le filtre est vide
            } else {
                // Règle de visibilité : le nom contient la recherche, OU c'est le dossier parent actuel
                item.is_visible = item.name.to_lowercase().contains(&query_lower);
            }
        }

        // Attention : si on réduit la liste, le curseur (selected_index) pourrait
        // se retrouver hors limites. On le remet à 0 par sécurité.
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn render_preview(&self, height: usize) -> Vec<String> {
        let mut lines = Vec::new();

        // On utilise simplement self.get_selected_item()
        if let Some(item) = self.get_selected_item() {
            if !item.is_dir
                && let Ok(file) = File::open(&item.path)
            {
                let reader = BufReader::new(file);
                for line in reader.lines().take(height).flatten() {
                    lines.push(line);
                }
            } else {
                lines.push("C'est un dossier. Appuyez sur 'l' pour l'ouvrir.".to_string());
            }
        }
        // Ensuite, tu passes `lines` à ton module `syntect` existant pour la coloration
        lines
    }
    pub fn load_directory(&mut self, dir_path: &PathBuf, depth: usize) {
        // Initialisation de WalkBuilder sur le répertoire cible
        let mut builder = WalkBuilder::new(dir_path);

        // La magie pour ton VCS : prise en charge native de ton ignore custom !
        builder.add_custom_ignore_filename(".awqignore");

        // On limite la profondeur à 1 pour ne charger que le niveau actuel
        // (l'expansion des sous-dossiers se fera au clic/bouton 'l')
        builder.max_depth(Some(1));

        // On garde par défaut les fichiers cachés (comme .awq) s'ils ne sont pas explicitement ignorés
        builder.hidden(false);

        let mut new_items = Vec::new();

        // WalkBuilder gère le multi-threading en interne si besoin, mais .build() est un itérateur simple
        for result in builder.build().flatten() {
            let path = result.path();

            // On évite d'ajouter le dossier racine lui-même dans l'arbre
            if path == dir_path {
                continue;
            }

            if let Ok(metadata) = path.metadata() {
                let is_dir = metadata.is_dir();
                let name = path.file_name().expect("msg").to_string_lossy().to_string();

                // Fini les checks manuels `.git` ou `target`, `WalkBuilder` l'a déjà filtré !

                new_items.push(TreeItem {
                    path: path.to_path_buf(),
                    name,
                    is_dir,
                    depth,
                    is_expanded: false,
                    is_visible: true,
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                });
            }
        }

        // Un petit tri alphabétique en mettant les dossiers en premier, c'est toujours plus propre[cite: 1]
        new_items.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        // On insère `new_items` dans l'état `TreeState` global
        self.items.extend(new_items);
    }
}
