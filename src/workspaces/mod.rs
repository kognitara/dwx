use crate::actions::list_tar_gz_contents;
use crate::bus::{InspectorCommand, WorkerEvent};
use crate::tree::{FileItem, MillerState};
use crossterm::queue;
use crossterm::terminal::Clear;
use ignore::WalkBuilder;
use is_executable::IsExecutable;
use std::io::stdout;
use std::path::{Path, PathBuf};
use std::process::Command;
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

/// Structure responsable de maintenir le point de montage en vie.
/// Quand elle sort de la portée (out of scope), le montage est nettoyé.
pub struct MountGuard {
    mount_point: PathBuf,
}

impl MountGuard {
    pub fn new(mount_point: &Path) -> Self {
        Self {
            mount_point: mount_point.to_path_buf(),
        }
    }
}

// C'est ici que la magie opère
impl Drop for MountGuard {
    fn drop(&mut self) {
        // 1. Démonter le système de fichiers (silencieusement)
        #[cfg(target_os = "linux")]
        let _ = Command::new("fusermount")
            .arg("-u")
            .arg("-z") // -z pour forcer le démontage paresseux (lazy unmount) si occupé
            .arg(&self.mount_point)
            .output();

        #[cfg(not(target_os = "linux"))]
        let _ = Command::new("umount").arg(&self.mount_point).output();

        // 2. Supprimer le dossier temporaire (optionnel, mais propre)
        // On ignore les erreurs au cas où le démontage aurait échoué
        let _ = fs::remove_dir_all(&self.mount_point);
    }
}

pub struct Workspace {
    pub miller: MillerState,
    pub mode: AppMode,
    pub active_pane: usize,
    pub search_id: usize,
    // Les données reçues par le Bus (Inspecteur)
    pub current_perms: String,
    pub current_preview: Vec<String>, // Stockera le texte colorisé par syntect
    pub active_mounts: Vec<MountGuard>,
    // Les drapeaux d'action clavier (Anciennement dans MillerState)
    pub pending_g: bool,
    pub pending_create: bool,
    pub pending_create_dir: bool,
    pub pending_create_file: bool,
    pub pending_create_archive: bool,
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
                            .standard_filters(true)
                            .threads(4)
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
                    InspectorCommand::MountSshfs {
                        connection_string,
                        mount_point,
                    } => {
                        let mut cmd = Command::new("sshfs");

                        // L'option spécifique à FreeBSD (Option 1 de ta doc)
                        #[cfg(target_os = "freebsd")]
                        cmd.arg("-o").arg("idmap=user");

                        // On assemble le reste de la commande
                        let status = cmd
                            .arg(&connection_string) // ex: "username@example.org:/chemin"
                            .arg(&mount_point) // ex: "/tmp/dwx_ssh"
                            .status();

                        if let Ok(exit_status) = status
                            && exit_status.success()
                        {}
                    }
                }
            }
        });

        let base_miller = MillerState::new(start_dir);

        // 3. Initialisation de l'espace de travail
        let mut ws = Self {
            miller: base_miller.clone(),
            mode: AppMode::Normal,
            active_pane: 1, // Focus sur CURRENT
            preview: Preview::Empty,
            current_perms: String::from("rwxr-xr-x"),
            current_preview: Vec::new(), // Vecteur vide au démarrage
            pending_g: false,
            pending_create: false,
            pending_create_dir: false,
            pending_create_file: false,
            pending_create_archive: false,
            panes: Vec::new(),
            rx_ui,
            tx_inspector,
            search_id: 0,
            active_mounts: Vec::new(),
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
            } else if path.is_file() {
                // On convertit le nom du fichier en String pour vérifier son extension proprement
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();

                if file_name.ends_with(".tar.gz") {
                    // C'est une archive, on liste son contenu
                    let mut entries = Vec::new();

                    // On utilise "if let Ok" plutôt que "expect" pour éviter que dwx ne crash si l'archive est corrompue
                    if let Ok(contents) = list_tar_gz_contents(&path) {
                        for x in contents {
                            entries.push(FileItem {
                                path: PathBuf::from(&x),
                                name: x,
                                is_dir: false, // On simplifie pour l'aperçu
                                is_file: true,
                                is_executable: false,
                                is_symlink: false,
                            });
                        }
                    }
                    self.preview = Preview::Dir(entries);
                } else {
                    // Si ce n'est pas une archive, on essaie de le lire comme du texte
                    let mut lines = Vec::new();
                    if let Ok(content) = fs::read_to_string(&path) {
                        for line in content.lines().take(50) {
                            lines.push(line.to_string());
                        }
                        self.preview = Preview::File(lines);
                    } else if path.is_executable() {
                        // Si la lecture texte échoue et que c'est un binaire
                        lines.push("Bin file...".to_string());
                        self.preview = Preview::File(lines);
                    }
                }
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
    pub fn clear(&mut self) {
        queue!(stdout(), Clear(crossterm::terminal::ClearType::All)).expect("failed to clear");
        self.preview = Preview::Empty; // 3. On vide SEULEMENT maintenant (ou pas du tout si tu préfères écraser directement avec le nouveau)
    }
    pub fn move_down(&mut self, visible_rows: usize) {
        if self.miller.move_down(visible_rows).is_some() {
            self.clear();
            crate::ui::draw_ui(self); // 2. On dessine le ~ (l'ancien aperçu reste visible !)
            self.update_preview();
        }
    }

    pub fn move_up(&mut self) {
        if self.miller.move_up().is_some() {
            self.clear();
            crate::ui::draw_ui(self);
            self.update_preview();
        }
    }

    pub fn enter_dir(&mut self) {
        self.clear();
        self.miller.enter_dir();
        crate::ui::draw_ui(self);
        self.update_preview();
    }

    pub fn go_parent(&mut self) {
        self.clear();
        self.miller.go_parent();
        crate::ui::draw_ui(self);
        self.update_preview();
    }
}
