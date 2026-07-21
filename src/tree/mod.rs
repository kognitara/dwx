use crate::ui::draw;
use crossterm::event::{Event, KeyCode, KeyEventKind, poll, read};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::WalkBuilder;
use is_executable::IsExecutable;
use std::collections::HashMap;
use std::fs::File;
use std::fs::{self};
use std::io::BufReader;
use std::io::{BufRead, stdout};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use std::{env, thread};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

// La "photo" de l'état d'un dossier
pub struct DirState {
    pub selected_index: usize,
    pub scroll_offset: usize,
}
pub enum AppMode {
    Normal,
    Omnibar {
        prefix: char,
        input_buffer: String,
        receiver: Option<Receiver<FileItem>>,
        kill_switch: Option<Arc<AtomicBool>>,
    },
}
pub enum Preview {
    Dir(Vec<FileItem>),
    File(Vec<String>), // Lignes de texte pré-formatées avec les couleurs ANSI
    Empty,
}
pub struct MillerState {
    pub current_dir: PathBuf,
    pub filtered_indices: Vec<usize>, // Le calque : juste une liste de chiffres (les index)
    pub mode: AppMode,
    pub parent_entries: Vec<FileItem>, // Colonne de gauche (Contexte)
    pub current_entries: Vec<FileItem>, // Colonne centrale (Focus)
    // Le registre de navigation
    pub history: HashMap<PathBuf, DirState>,
    pub preview: Preview,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub pending_g: bool,
    pub pending_create: bool,
    pub pending_create_dir: bool,
    pub pending_create_symlink: bool,
    pub pending_create_file: bool,
    pub pending_create_connection: bool,
    pub pending_create_hard_link: bool,
    pub pending_create_archive: bool,
    pub pending_create_branch: bool,
    pub pending_create_new_project: bool,
}

impl MillerState {
    pub fn new(start_dir: PathBuf) -> Self {
        let mut state = Self {
            current_dir: start_dir,
            parent_entries: Vec::new(),
            current_entries: Vec::new(),
            selected_index: 0,
            preview: Preview::Empty,
            filtered_indices: Vec::new(),
            mode: AppMode::Normal,
            scroll_offset: 0,
            pending_g: false,
            pending_create: false,
            pending_create_dir: false,
            pending_create_file: false,
            pending_create_symlink: false,
            pending_create_connection: false,
            pending_create_archive: false,
            pending_create_hard_link: false,
            pending_create_branch: false,
            pending_create_new_project: false,
            history: HashMap::new(),
        };
        state.refresh(); // On charge les données initiales
        state
    }
    pub fn update_preview(&mut self) {
        self.preview = Preview::Empty;

        // On passe par le calque pour trouver le VRAI index du fichier
        if let Some(&actual_index) = self.filtered_indices.get(self.selected_index)
            && let Some(selected) = self.current_entries.get(actual_index)
        {
            if selected.is_dir {
                self.preview = Preview::Dir(Self::read_dir_entries(&selected.path));
            } else {
                self.preview = Preview::File(Self::read_and_highlight(&selected.path));
            }
        }
    }
    fn update_search(state: &mut MillerState) {
        // 1. On extrait les données de l'Omnibar avant le match pour éviter de bloquer l'état (Borrow Checker)
        let (prefix, query) = match &state.mode {
            AppMode::Omnibar {
                prefix,
                input_buffer,
                ..
            } => (*prefix, input_buffer.to_lowercase()),
            _ => return,
        };

        // 2. Le routeur de recherche
        match prefix {
            '/' => {
                // --- STRATÉGIE 1 : FILTRE LOCAL (Synchrone & Instantané) ---
                // On ne touche SURTOUT PAS à current_entries ni aux threads.
                if query.is_empty() {
                    // Si on a tout effacé, le calque redevient complet
                    state.filtered_indices = (0..state.current_entries.len()).collect();
                } else {
                    // Sinon, on filtre
                    state.filtered_indices = state
                        .current_entries
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| item.name.to_lowercase().contains(&query))
                        .map(|(i, _)| i)
                        .collect();
                }
                // On remet la caméra en haut
                state.selected_index = 0;
                state.scroll_offset = 0;
            }
            '?' => {
                // --- STRATÉGIE 2 : RECHERCHE RÉCURSIVE (Asynchrone via Threads) ---
                if let AppMode::Omnibar {
                    receiver,
                    kill_switch,
                    ..
                } = &mut state.mode
                {
                    // 1. Tuer l'ancienne recherche
                    if let Some(ks) = kill_switch {
                        ks.store(true, std::sync::atomic::Ordering::Relaxed);
                    }

                    // 2. Vider les listes pour accueillir les nouveaux résultats
                    state.current_entries.clear();
                    state.filtered_indices.clear();
                    state.selected_index = 0;
                    state.scroll_offset = 0;

                    // 3. Relancer un thread si on a tapé quelque chose
                    if !query.is_empty() {
                        let (new_rx, new_ks) = Self::spawn_search(query, state.current_dir.clone());
                        *receiver = Some(new_rx);
                        *kill_switch = Some(new_ks);
                    } else {
                        *receiver = None;
                        *kill_switch = None;
                    }
                }
            }
            _ => {} // Prêt pour accueillir d'autres préfixes plus tard (ex: '!')
        }

        // 3. On demande un redessin immédiat
        let _ = crate::ui::draw(state);
    }

    pub fn spawn_search(
        query: String,
        start_dir: std::path::PathBuf,
    ) -> (Receiver<FileItem>, Arc<AtomicBool>) {
        // 1. Création du canal de communication
        let (tx, rx) = mpsc::channel();

        // 2. Création du Kill Switch (partagé entre le main thread et le worker)
        let kill_switch = Arc::new(AtomicBool::new(false));
        let worker_kill = Arc::clone(&kill_switch);

        // 3. Lancement du thread secondaire
        thread::spawn(move || {
            // WalkBuilder gère le multi-threading interne et les .gitignore
            let walker = WalkBuilder::new(start_dir).build();

            for result in walker.flatten() {
                // À chaque fichier, le worker regarde si on a ordonné son exécution
                if worker_kill.load(Ordering::Relaxed) {
                    break; // Le thread se suicide silencieusement
                }
                // On ignore les erreurs de permission
                let path = result.path();
                let name = result.file_name().to_string_lossy().to_string();

                // Filtrage exact strict
                if name.to_lowercase().contains(&query) {
                    let item = FileItem::from_path(path.to_path_buf());

                    // On envoie le fichier au thread principal.
                    // Si le canal est fermé (ex: l'utilisateur a quitté dwx), on arrête.
                    if tx.send(item).is_err() {
                        break;
                    }
                }
            }
        });
        // On retourne le récepteur et la télécommande du kill switch au thread principal
        (rx, kill_switch)
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
        if let Some(&actual_index) = self.filtered_indices.get(self.selected_index)
            && let Some(selected) = self.current_entries.get(actual_index)
            && selected.path.is_dir()
        {
            self.history.insert(
                self.current_dir.clone(),
                DirState {
                    selected_index: self.selected_index,
                    scroll_offset: self.scroll_offset,
                },
            );
            self.current_dir = selected.path.clone();
            self.selected_index = 0;
            if let Some(saved_state) = self.history.get(&self.current_dir) {
                self.selected_index = saved_state.selected_index;
                self.scroll_offset = saved_state.scroll_offset;
            } else {
                // C'est la première fois qu'on y entre, on remet les compteurs à zéro
                self.selected_index = 0;
                self.scroll_offset = 0;
            }
            self.refresh();
        }
    }
    pub fn run(state: &mut MillerState) -> std::io::Result<()> {
        draw(state)?;

        loop {
            let mut needs_redraw = false;
            state.update_preview();
            // --- 1. LECTURE DU STREAMING EN ARRIÈRE-PLAN ---
            // Si on est en mode Omnibar, on vérifie si le Worker nous a envoyé des fichiers
            if let AppMode::Omnibar {
                receiver: Some(rx), ..
            } = &state.mode
            {
                // try_recv() lit le canal sans bloquer. S'il est vide, on passe à la suite.
                while let Ok(item) = &rx.try_recv() {
                    // On ajoute le fichier trouvé à notre source de vérité
                    state.current_entries.push(item.clone());

                    // On met à jour le calque (le nouvel élément est à la fin)
                    state.filtered_indices.push(state.current_entries.len() - 1);
                    // NOUVEAU : Si c'est le tout premier fichier trouvé, on charge son aperçu
                    needs_redraw = true;
                }
            }
            // Si de nouveaux fichiers sont arrivés, on met à jour l'écran en direct
            if needs_redraw {
                draw(state)?;
            }

            // --- 2. ÉCOUTE DU CLAVIER (Non-bloquant, 16ms = ~60 FPS) ---
            if poll(Duration::from_millis(16))?
                && let Event::Key(key) = read()?
                && key.kind == KeyEventKind::Press
            {
                if state.pending_g {
                    state.pending_g = false; // On désarme le piège immédiatement

                    let target_dir = match key.code {
                        KeyCode::Char('h') => dirs::home_dir(),
                        KeyCode::Char('D') => dirs::download_dir(),
                        KeyCode::Char('d') => dirs::document_dir(),
                        KeyCode::Char('a') => dirs::audio_dir(),
                        KeyCode::Char('b') => dirs::executable_dir(),
                        KeyCode::Char('c') => dirs::config_dir(),
                        KeyCode::Char('p') => dirs::picture_dir(),
                        KeyCode::Char('v') => dirs::video_dir(),
                        KeyCode::Char('f') => dirs::font_dir(),
                        KeyCode::Char('t') => dirs::template_dir(),
                        KeyCode::Char('r') => Some(std::path::PathBuf::from("/")),
                        _ => None, // Si on tape une autre touche, on annule l'action
                    };

                    if let Some(new_dir) = target_dir {
                        // Si le dossier existe, on s'y téléporte !
                        if new_dir.exists() {
                            state.current_dir = new_dir;
                            // On n'oublie pas de remonter la caméra tout en haut
                            state.selected_index = 0;
                            state.scroll_offset = 0;
                            state.refresh();
                            draw(state)?;
                        }
                    }
                    continue; // On passe directement au cycle suivant sans lire les autres raccourcis
                } else if state.pending_create {
                    state.pending_create = false;
                    match &mut state.mode {
                        AppMode::Normal => {
                            match key.code {
                                KeyCode::Char('m') => {
                                    state.mode = AppMode::Omnibar {
                                        prefix: 'm', // Le préfixe 'm' indique qu'on veut lire un manuel
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('d') => {
                                    state.pending_create_dir = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_file = false;
                                    state.pending_create_symlink = false;
                                    state.pending_create_connection = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('s') => {
                                    state.pending_create_symlink = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_file = false;
                                    state.pending_create_connection = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('h') => {
                                    state.pending_create_hard_link = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_file = false;
                                    state.pending_create_symlink = false;
                                    state.pending_create_connection = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                // jump ssh
                                KeyCode::Char('j') => {
                                    state.pending_create_connection = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_file = false;
                                    state.pending_create_symlink = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('b') => {
                                    state.pending_create_branch = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_connection = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_file = false;
                                    state.pending_create_symlink = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('f') => {
                                    state.pending_create_file = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_symlink = false;
                                    state.pending_create_connection = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('n') => {
                                    state.pending_create_new_project = true;
                                    state.pending_create_file = false;
                                    state.pending_create_file = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_symlink = false;
                                    state.pending_create_connection = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                KeyCode::Char('a') => {
                                    state.pending_create_archive = true;
                                    state.pending_create_new_project = false;
                                    state.pending_create_branch = false;
                                    state.pending_create_dir = false;
                                    state.pending_create_symlink = false;
                                    state.pending_create_connection = false;
                                    state.pending_create_file = false;
                                    state.mode = AppMode::Omnibar {
                                        prefix: '+', // Le signe + indique qu'on ajoute quelque chose
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                _ => {
                                    continue;
                                }
                            }
                        }
                        AppMode::Omnibar {
                            prefix: _,
                            input_buffer: _,
                            receiver: _,
                            kill_switch: _,
                        } => {}
                    }
                } else {
                    match &mut state.mode {
                        AppMode::Normal => {
                            match key.code {
                                KeyCode::Char('n') => {
                                    state.pending_create = true;
                                    continue;
                                }
                                KeyCode::Char('g') => {
                                    state.pending_g = true;
                                    continue;
                                }
                                // --- RAFRAÎCHISSEMENT MANUEL (Nettoyer le filtre) ---
                                KeyCode::F(5) => {
                                    state.refresh();
                                    draw(state)?;
                                }
                                KeyCode::Char('o') | KeyCode::Enter => {
                                    if let Some(selected) =
                                        state.current_entries.get(state.selected_index)
                                    {
                                        let mut stdout = stdout();
                                        // 1. Préparation de l'éditeur (hx par défaut)
                                        let editor =
                                            env::var("EDITOR").unwrap_or_else(|_| "hx".to_string());

                                        // 2. On rend le terminal à l'OS (Crucial !)
                                        disable_raw_mode()?;
                                        execute!(stdout, LeaveAlternateScreen)?;

                                        // 3. On lance l'éditeur et on bloque dwx en attendant qu'il se ferme
                                        let _ =
                                            Command::new(&editor).arg(&selected.path).status()?;

                                        // 4. L'éditeur est fermé, on reprend le contrôle du terminal
                                        execute!(stdout, EnterAlternateScreen)?;
                                        enable_raw_mode()?;

                                        // 5. On rafraîchit l'état au cas où tu aurais créé/supprimé/renommé
                                        // des choses depuis ton éditeur, et on redessine l'interface
                                        state.refresh();
                                        draw(state)?;
                                    }
                                }
                                KeyCode::Char('q') | KeyCode::Esc => break,
                                KeyCode::Char('j') | KeyCode::Down => {
                                    state.move_down(state.scroll_offset);
                                    draw(state)?; // On redessine après chaque changement
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    state.move_up();
                                    draw(state)?;
                                }
                                // --- GLISSEMENT GAUCHE (Retour) ---
                                KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                                    state.go_parent();
                                    draw(state)?;
                                }

                                // --- GLISSEMENT DROITE (Entrer / Espace) ---
                                KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
                                    state.enter_dir();
                                    draw(state)?;
                                }
                                KeyCode::Char('/') => {
                                    // On bascule en recherche récursive !
                                    // On vide les listes pour préparer les résultats vierges
                                    state.selected_index = 0;
                                    state.scroll_offset = 0;

                                    state.mode = AppMode::Omnibar {
                                        prefix: '/',
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                // ... Tes anciens raccourcis (j, k, h, l, o) restent ici ...
                                KeyCode::Char('?') => {
                                    // On bascule en recherche récursive !
                                    // On vide les listes pour préparer les résultats vierges
                                    state.current_entries.clear();
                                    state.filtered_indices.clear();
                                    state.selected_index = 0;
                                    state.scroll_offset = 0;

                                    state.mode = AppMode::Omnibar {
                                        prefix: '?',
                                        input_buffer: String::new(),
                                        receiver: None,
                                        kill_switch: None,
                                    };
                                    draw(state)?;
                                }
                                _ => {}
                            }
                        }
                        AppMode::Omnibar {
                            prefix,
                            input_buffer,
                            receiver: _,
                            kill_switch,
                        } => {
                            match key.code {
                                KeyCode::Esc => {
                                    // 1. Tuer la recherche en cours s'il y en a une
                                    if let Some(ks) = kill_switch {
                                        ks.store(true, std::sync::atomic::Ordering::Relaxed);
                                    }
                                    // 2. On revient à la normale, on recharge le dossier courant
                                    state.mode = AppMode::Normal;
                                    draw(state)?;
                                }
                                KeyCode::Enter => {
                                    if *prefix == 'm' && !input_buffer.trim().is_empty() {
                                        let mut stdout = std::io::stdout();

                                        // 1. On rend l'affichage au système
                                        crossterm::terminal::disable_raw_mode()?;
                                        crossterm::execute!(
                                            stdout,
                                            crossterm::terminal::LeaveAlternateScreen
                                        )?;

                                        // 2. L'astuce pour les sections : on découpe le texte avec split_whitespace()
                                        // Si tu tapes "3 printf", ça devient ["3", "printf"]
                                        let args: Vec<&str> =
                                            input_buffer.split_whitespace().collect();

                                        // 3. On lance la commande man avec les arguments
                                        let _ = std::process::Command::new("man")
                                            .args(&args)
                                            .status()?;

                                        // 4. Tu as quitté le man, on reprend le contrôle de l'interface
                                        crossterm::execute!(
                                            stdout,
                                            crossterm::terminal::EnterAlternateScreen
                                        )?;
                                        crossterm::terminal::enable_raw_mode()?;

                                        // On nettoie l'état et on redessine
                                        state.mode = AppMode::Normal;
                                        state.refresh();
                                        let _ = draw(state);
                                        continue;
                                    } else if *prefix == '+' && !input_buffer.to_string().is_empty()
                                    {
                                        // On construit le chemin complet à partir du dossier courant
                                        let target_path = state.current_dir.join(&input_buffer);

                                        if state.pending_create_dir {
                                            // S'il y a un slash à la fin, on crée un dossier (et ses parents si besoin)
                                            let _ = std::fs::create_dir_all(target_path);
                                        } else if state.pending_create_file {
                                            // Sinon, on crée un fichier vide
                                            let _ = std::fs::File::create(target_path);
                                        } else if state.pending_create_hard_link {
                                            fs::hard_link(
                                                state
                                                    .current_entries
                                                    .get(state.selected_index)
                                                    .expect("failed to get original")
                                                    .name
                                                    .as_str(),
                                                input_buffer.as_str(),
                                            )
                                            .expect("");
                                        } else if state.pending_create_symlink {
                                            symlink(
                                                state
                                                    .current_entries
                                                    .get(state.selected_index)
                                                    .expect("")
                                                    .name
                                                    .as_str(),
                                                input_buffer.as_str(),
                                            )
                                            .expect("faield to create symlink");
                                        } else if state.pending_create_archive {
                                            let archive_path =
                                                state.current_dir.join(&input_buffer);
                                            // 2. On branche le compresseur Gzip et le constructeur Tar
                                            let enc = GzEncoder::new(
                                                File::create(archive_path)
                                                    .expect("failed to create file"),
                                                Compression::default(),
                                            );
                                            let mut builder = tar::Builder::new(enc);
                                            let selected = state
                                                .current_entries
                                                .get(state.selected_index)
                                                .expect("failed to get selected path");
                                            // On récupère le nom de l'élément cible pour structurer l'archive proprement
                                            let item_name = state
                                                .current_dir
                                                .as_path()
                                                .file_name()
                                                .unwrap_or_default();

                                            // 3. La logique diffère si c'est un dossier ou un fichier unique
                                            if selected.is_dir {
                                                // S'il y a une erreur de permissions en lisant le dossier, on l'ignore silencieusement avec let _
                                                let _ = builder
                                                    .append_dir_all(item_name, &selected.path);
                                            } else if let Ok(mut f) =
                                                std::fs::File::open(&selected.path)
                                            {
                                                let _ = builder.append_file(item_name, &mut f);
                                            }

                                            // 4. On finalise l'écriture de l'archive
                                            let _ = builder.into_inner();
                                        }
                                        state.refresh();
                                        state.pending_create_archive = false;
                                        state.pending_create_dir = false;
                                        state.pending_create_file = false;
                                        state.pending_create_symlink = false;
                                        state.pending_create_hard_link = false;
                                    }
                                    // On valide, on laisse le worker finir s'il n'a pas terminé,
                                    // et on fige les résultats
                                    state.mode = AppMode::Normal;
                                    draw(state)?;
                                }
                                KeyCode::Char(c) => {
                                    input_buffer.push(c);
                                    Self::update_search(state);
                                }
                                KeyCode::Backspace => {
                                    input_buffer.pop();
                                    Self::update_search(state);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            } else {
                continue;
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
        self.filtered_indices = (0..self.current_entries.len()).collect();
    }

    // Déplacement vers le bas (j)
    // Déplacement vers le bas (j)
    pub fn move_down(&mut self, visible_rows: usize) {
        // CORRECTION : On bloque le curseur en fonction de la taille du filtre, pas du dossier entier !
        if self.selected_index + 1 < self.filtered_indices.len() {
            self.selected_index += 1;
            if self.selected_index >= self.scroll_offset + visible_rows {
                self.scroll_offset += 1;
            }
            self.update_preview(); // <--- On actualise l'aperçu !
        }
    }

    // Déplacement vers le haut (k)
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset -= 1;
            }
            self.update_preview(); // <--- On actualise l'aperçu !
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
