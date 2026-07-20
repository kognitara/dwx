use crate::ui::draw;
use crossterm::event::{Event, KeyCode, KeyEventKind, read};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use is_executable::IsExecutable;
use std::env;
use std::fs::{self, Permissions};
use std::fs::{File, Metadata};
use std::io::BufRead;
use std::io::BufReader;
use std::io::stdout;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

pub enum Preview {
    Dir(Vec<FileItem>),
    File(Vec<String>), // Lignes de texte pré-formatées avec les couleurs ANSI
    Empty,
}
pub struct MillerState {
    pub current_dir: PathBuf,

    // Les 3 colonnes d'affichage
    pub parent_entries: Vec<FileItem>, // Colonne de gauche (Contexte)
    pub current_entries: Vec<FileItem>, // Colonne centrale (Focus)
    pub preview_entries: Vec<FileItem>, // Colonne de droite (Aperçu d'un sous-dossier)
    pub preview: Preview,
    pub selected_index: usize,
}

impl MillerState {
    pub fn new(start_dir: PathBuf) -> Self {
        let mut state = Self {
            current_dir: start_dir,
            parent_entries: Vec::new(),
            current_entries: Vec::new(),
            preview_entries: Vec::new(),
            selected_index: 0,
            preview: Preview::Empty,
        };

        state.refresh(); // On charge les données initiales
        state
    }
    /// Lit un fichier et retourne ses lignes avec la coloration syntaxique ANSI
    fn read_and_highlight(path: &Path) -> Vec<String> {
        // Initialisation de syntect (ces objets pourraient être mis en cache dans MillerState pour encore plus de perfs)
        let ps = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();

        // Choix d'un thème adapté pour un fond sombre
        let theme = &ts.themes["base16-ocean.dark"];

        // 1. Détection du langage d'après l'extension du fichier
        let syntax = ps
            .find_syntax_for_file(path)
            .unwrap_or(None)
            .unwrap_or_else(|| ps.find_syntax_plain_text());

        let mut h = HighlightLines::new(syntax, theme);
        let mut highlighted_lines = Vec::new();

        // 2. Lecture sécurisée (On limite l'aperçu pour éviter de freeze sur un log de 2Go)
        if let Ok(file) = File::open(path) {
            let reader = BufReader::new(file);
            for line in reader.lines().map_while(Result::ok).take(100) {
                // On rajoute un saut de ligne car syntect en a besoin pour son parsing
                let line_with_nl = format!("{}\n", line);

                // 3. Application des couleurs
                if let Ok(ranges) = h.highlight_line(&line_with_nl, &ps) {
                    // Conversion des structs de couleur en chaîne de caractères avec codes ANSI
                    let escaped = as_24_bit_terminal_escaped(&ranges[..], true);
                    // On enlève le retour à la ligne pour le rendu crossterm
                    highlighted_lines.push(escaped.trim_end().to_string());
                }
            }
        } else {
            highlighted_lines.push("Impossible de lire le fichier...".to_string());
        }

        // Toujours forcer la réinitialisation des couleurs à la fin
        highlighted_lines.push("\x1b[0m".to_string());
        highlighted_lines
    }
    /// Fonction utilitaire pour lire un dossier et retourner un vecteur trié
    fn read_dir_entries(dir: &Path) -> Vec<FileItem> {
        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                entries.push(FileItem::from_path(entry.path()));
            }
        }
        // Optionnel : Trier pour avoir les dossiers en premier, puis par ordre alphabétique
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        entries
    }
    pub fn enter_dir(&mut self) {
        // Clippy est content : on fait tout sur une seule ligne
        if let Some(selected) = self.current_entries.get(self.selected_index)
            && selected.is_dir
        {
            self.current_dir = selected.path.clone();
            self.selected_index = 0;
            self.refresh();
        }
    }

    // On passe ton state initialisé à cette fonction
    pub fn run(mut state: MillerState) -> std::io::Result<()> {
        // On dessine l'état initial une première fois avant de bloquer sur read()
        draw(&state)?;

        loop {
            // L'application attend ici jusqu'à ce qu'une touche soit pressée
            if let Event::Key(key) = read()? {
                // Sécurité : on ignore les événements de relâchement de touche
                // pour éviter que l'action ne s'exécute deux fois sur certains systèmes
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    // --- QUITTER ---
                    KeyCode::Char('q') | KeyCode::Esc => break,

                    // --- NAVIGATION VERTICALE (Haut/Bas) ---
                    KeyCode::Char('j') | KeyCode::Down => {
                        state.move_down();
                        draw(&state)?; // On redessine après chaque changement
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        state.move_up();
                        draw(&state)?;
                    }

                    // --- GLISSEMENT DROITE (Entrer / Espace) ---
                    KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
                        state.enter_dir();
                        draw(&state)?;
                    }

                    // --- GLISSEMENT GAUCHE (Retour) ---
                    KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                        state.go_parent();
                        draw(&state)?;
                    }

                    // --- HANDOFF (Ouvrir l'éditeur) ---
                    KeyCode::Char('o') | KeyCode::Enter => {
                        if let Some(selected) = state.current_entries.get(state.selected_index) {
                            // 1. Préparation de l'éditeur (hx par défaut)
                            let editor = env::var("EDITOR").unwrap_or_else(|_| "hx".to_string());

                            // 2. On rend le terminal à l'OS (Crucial !)
                            disable_raw_mode()?;
                            execute!(stdout(), LeaveAlternateScreen)?;

                            // 3. On lance l'éditeur et on bloque dwx en attendant qu'il se ferme
                            let _ = Command::new(&editor).arg(&selected.path).status()?;

                            // 4. L'éditeur est fermé, on reprend le contrôle du terminal
                            execute!(stdout(), EnterAlternateScreen)?;
                            enable_raw_mode()?;

                            // 5. On rafraîchit l'état au cas où tu aurais créé/supprimé/renommé
                            // des choses depuis ton éditeur, et on redessine l'interface
                            state.refresh();
                            draw(&state)?;
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
    /// Glissement vers la gauche (Revenir au parent)
    pub fn go_parent(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            // Petite astuce d'UX : on mémorise le nom du dossier d'où on vient
            // pour repositionner le curseur dessus une fois dans le parent
            let previous_dir_name = self
                .current_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            self.current_dir = parent.to_path_buf();
            self.refresh(); // Un premier rafraîchissement pour charger la nouvelle liste

            // On cherche où se trouve notre ancien dossier pour y placer le curseur
            if let Some(pos) = self
                .current_entries
                .iter()
                .position(|e| e.name == previous_dir_name)
            {
                self.selected_index = pos;
            } else {
                self.selected_index = 0;
            }

            self.refresh(); // Un second rafraîchissement pour mettre à jour la colonne d'aperçu
        }
    }
    /// Met à jour les 3 colonnes en fonction du `current_dir` et du `selected_index`
    pub fn refresh(&mut self) {
        // 1. Colonne centrale : On lit le dossier actuel
        self.current_entries = Self::read_dir_entries(&self.current_dir);

        // On sécurise l'index au cas où le dossier contient moins de fichiers qu'avant
        if self.current_entries.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.current_entries.len() {
            self.selected_index = self.current_entries.len() - 1;
        }

        // 2. Colonne de gauche : On lit le parent (s'il existe)
        if let Some(parent) = self.current_dir.parent() {
            self.parent_entries = Self::read_dir_entries(parent);
        } else {
            self.parent_entries.clear(); // On est à la racine du système
        }
        // 3. Colonne de droite : C'est ici qu'on utilise le nouvel enum et la fonction !
        self.preview = Preview::Empty;

        if let Some(selected) = self.current_entries.get(self.selected_index) {
            if selected.is_dir {
                self.preview = Preview::Dir(Self::read_dir_entries(&selected.path));
            } else {
                // Fini le code mort ! On appelle enfin la coloration syntaxique
                self.preview = Preview::File(Self::read_and_highlight(&selected.path));
            }
        }
    }

    // Déplacement vers le bas (j)
    pub fn move_down(&mut self) {
        if !self.current_entries.is_empty() && self.selected_index < self.current_entries.len() - 1
        {
            self.selected_index += 1;
            // On rafraîchit seulement si la nouvelle sélection est un dossier
            // pour mettre à jour la colonne de droite
            self.refresh();
        }
    }

    // Déplacement vers le haut (k)
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.refresh();
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileItem {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub is_executable: bool,
    pub is_symlink: bool,
    pub meta: Metadata,
}

impl FileItem {
    // Un petit constructeur pratique
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
        let meta = path.as_path().metadata().expect("failed to get metadata");
        Self {
            path,
            name,
            is_dir,
            is_executable,
            is_file,
            is_symlink,
            meta,
        }
    }
}
