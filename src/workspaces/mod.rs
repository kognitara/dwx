use crate::bus::{InspectorCommand, WorkerEvent};
use crate::tree::{FileItem, MillerState};
use ignore::WalkBuilder;
use std::path::PathBuf;
use std::sync::mpsc;
use std::{fs, thread};

#[derive(Clone)]
pub enum AppMode {
    Normal,
    Omnibar { prefix: char, input_buffer: String },
}
#[derive(Clone)]
pub enum Preview {
    Dir(Vec<FileItem>),
    File(Vec<String>), // Lignes de texte (pour le futur syntect ou texte brut)
    Empty,
}

pub struct Workspace {
    pub miller: MillerState,
    pub mode: AppMode,
    pub active_pane: usize,
    pub search_id: usize,
    // Les données reçues par le Bus (Inspecteur)
    pub current_perms: String,
    pub current_preview: Vec<String>, // Stockera le texte colorisé par syntect

    // Les drapeaux d'action clavier (Anciennement dans MillerState)
    pub pending_g: bool,
    pub pending_create: bool,
    pub pending_create_dir: bool,
    pub pending_create_file: bool,
    // 1. Navigation
    pub panes: Vec<MillerState>,
    pub preview: Preview,
    // 2. Communication
    pub rx_ui: mpsc::Receiver<WorkerEvent>,
    pub tx_inspector: mpsc::Sender<InspectorCommand>,
}

impl Workspace {
    #[must_use]
    pub fn new(start_dir: PathBuf) -> Self {
        let (tx_inspector, rx_inspector) = mpsc::channel::<InspectorCommand>();
        let (tx_ui, rx_ui) = mpsc::channel::<WorkerEvent>();

        thread::spawn(move || {
            let tx_reply = tx_ui;

            while let Ok(command) = rx_inspector.recv() {
                match command {
                    InspectorCommand::DeepSearch {
                        query,
                        dir,
                        search_id,
                    } => {
                        let query_lower = query.to_lowercase();
                        let mut batch = Vec::new();

                        // Le moteur de recherche magique (respecte les .gitignore)
                        let walker = WalkBuilder::new(dir)
                            .threads(4)
                            .standard_filters(true)
                            .add_custom_ignore_filename(".awqignore")
                            .build();

                        for result in walker.flatten() {
                            let path = result.path();
                            let file_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_lowercase();

                            // Si le fichier correspond à la recherche
                            if file_name.contains(&query_lower) {
                                let x = FileItem::from_path(path.to_path_buf());
                                batch.push(x.clone());
                            }
                        }
                        let _ = tx_reply.send(WorkerEvent::SearchResultBatch {
                            items: batch,
                            search_id,
                        });
                    }
                    InspectorCommand::Stop => break,
                }
            }
        });

        let base_miller = MillerState::new(start_dir);

        // 3. Initialisation de l'espace de travail
        let mut ws = Self {
            miller: base_miller.clone(),
            mode: AppMode::Normal,
            active_pane: 2, // Focus sur CURRENT
            preview: Preview::Empty,
            current_perms: String::from("rwxr-xr-x"),
            current_preview: Vec::new(), // Vecteur vide au démarrage
            pending_g: false,
            pending_create: false,
            pending_create_dir: false,
            pending_create_file: false,
            panes: vec![base_miller.clone(); 5],
            rx_ui,
            tx_inspector,
            search_id: 0,
        };
        ws.update_preview();
        ws
    }
    /// Calcule l'aperçu de l'élément actuellement sélectionné dans le MillerState
    pub fn update_preview(&mut self) {
        self.preview = Preview::Empty;

        // On récupère le chemin sélectionné grâce à notre méthode propre
        if let Some(path) = self.miller.get_selected_path() {
            if path.is_dir() {
                // Si c'est un dossier, on lit son contenu pour l'afficher dans la colonne de droite
                let mut entries = Vec::new();
                if let Ok(read_dir) = fs::read_dir(&path) {
                    for entry in read_dir.flatten() {
                        entries.push(FileItem::from_path(entry.path()));
                    }
                }
                entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                });
                self.preview = Preview::Dir(entries);
            } else {
                // Si c'est un fichier, on lit les premières lignes en attendant le thread Inspecteur
                let mut lines = Vec::new();
                if let Ok(content) = fs::read_to_string(&path) {
                    for line in content.lines().take(50) {
                        // On prend les 50 premières lignes
                        lines.push(line.to_string());
                    }
                } else {
                    lines.push("Bin file or not lissible...".to_string());
                }
                self.preview = Preview::File(lines);
            }
        }
    }

    pub fn poll_bus(&mut self) {
        while let Ok(event) = self.rx_ui.try_recv() {
            match event {
                WorkerEvent::SearchResultBatch {
                    mut items,
                    search_id,
                } => {
                    if search_id == self.search_id {
                        self.miller.current_entries.append(&mut items);
                        // On met à jour le filtre pour afficher les nouveaux arrivants
                        self.miller.filtered_indices =
                            (0..self.miller.current_entries.len()).collect();
                    }
                }
            }
        }
    }

    pub fn chmod(&mut self, mode: AppMode) {
        self.mode = mode;
    }

    // Méthodes de déplacement qui mettent à jour l'aperçu automatiquement
    pub fn move_down(&mut self, visible_rows: usize) {
        if self.miller.move_down(visible_rows).is_some() {
            self.update_preview();
        }
    }

    pub fn move_up(&mut self) {
        if self.miller.move_up().is_some() {
            self.update_preview();
        }
    }

    pub fn enter_dir(&mut self) {
        self.miller.enter_dir();
        self.update_preview();
    }

    pub fn go_parent(&mut self) {
        self.miller.go_parent();
        self.update_preview();
    }
}
