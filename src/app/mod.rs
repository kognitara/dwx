use crate::{app::buffer::Buffer, crypto::hash};
use arboard::Clipboard;
use crossterm::event::poll;
use crossterm::style::SetBackgroundColor;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::{
    event::{self, KeyCode, KeyModifiers},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType, EnterAlternateScreen, enable_raw_mode, size},
};
use regex::Regex;
use std::io::{IsTerminal, Read, stdin};
use std::time::{Duration, SystemTime};
use std::{
    collections::HashMap,
    fs::{File, create_dir_all, read_to_string},
    path::PathBuf,
    process::ExitCode,
};
use std::{
    fs::remove_dir_all,
    io::{Write, stdout},
};
pub mod buffer;
use similar::{ChangeTag, TextDiff};
#[derive(Debug, Default, Clone, Copy)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
#[derive(Debug, Clone, Default, Copy)]
pub enum SplitDirection {
    #[default]
    Vertical, // Coupe de gauche à droite (côte à côte)
    Horizontal, // Coupe de haut en bas (superposé)
}

impl Rect {
    pub fn split(&self, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
        // Sécurité : on s'assure que le ratio reste entre 10% et 90% pour ne pas écraser une vue
        let clamped_ratio = ratio.clamp(0.1, 0.9);

        match direction {
            SplitDirection::Vertical => {
                let left_width = ((self.width as f32) * clamped_ratio).round() as u16;
                let left = Rect {
                    width: left_width,
                    ..*self
                };
                let right = Rect {
                    x: self.x + left_width,
                    width: self.width.saturating_sub(left_width),
                    ..*self
                };
                (left, right)
            }
            SplitDirection::Horizontal => {
                let top_height = ((self.height as f32) * clamped_ratio).round() as u16;
                let top = Rect {
                    height: top_height,
                    ..*self
                };
                let bottom = Rect {
                    y: self.y + top_height,
                    height: self.height.saturating_sub(top_height),
                    ..*self
                };
                (top, bottom)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum SplitNode {
    Leaf(View), // Une vue réelle
    Split {
        ratio: f32,
        direction: SplitDirection,
        left: Box<SplitNode>,
        right: Box<SplitNode>,
    },
}

impl Default for SplitNode {
    fn default() -> Self {
        SplitNode::Leaf(View::default())
    }
}
#[derive(Debug, Default)]
pub struct App {
    /// Tes données pures (ce que tu as déjà)
    pub buffers: HashMap<String, Buffer>,
    is_searching: bool,
    search_query: String,
    /// La liste de tes espaces de travail (1 à 9 max)
    pub workspaces: Vec<Workspace>,
    pub show_filenames: bool,
    /// L'index du workspace actuellement affiché
    pub active_workspace: usize,
    pub window_mode: bool,
    pub is_diff_mode: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Workspace {
    pub root: SplitNode,
}
#[derive(Debug, Clone)]
pub struct View {
    /// La liste des hash des fichiers ouverts dans cette vue (les onglets/tabs)
    pub tabs: Vec<String>,
    /// L'onglet actuellement visible
    pub active_tab: usize,
    /// Pour le scrolling vertical (à quelle ligne on se trouve)
    pub scroll_offset: usize,
    pub is_active: bool,
    pub scroll_x: usize,
}
impl Default for View {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            scroll_offset: 0,
            is_active: true,
            scroll_x: 0, // La première vue a toujours le focus par défaut
        }
    }
}
impl View {
    /// Passe à l'onglet suivant de manière circulaire
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Revient à l'onglet précédent de manière circulaire
    pub fn previous_tab(&mut self) {
        if !self.tabs.is_empty() {
            if self.active_tab == 0 {
                self.active_tab = self.tabs.len() - 1;
            } else {
                self.active_tab -= 1;
            }
        }
    }

    /// Récupère le hash du fichier actuellement visible
    pub fn get_active_tab_hash(&self) -> Option<&String> {
        self.tabs.get(self.active_tab)
    }
}

impl App {
    pub fn scroll_left(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
        {
            view.scroll_x = view.scroll_x.saturating_sub(1);
        }
    }

    pub fn scroll_right(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
        {
            view.scroll_x += 1;
        }
    }
    // Nouvelle fonction pour trouver la vue active sans la modifier
    pub fn find_active_view(node: &SplitNode) -> Option<&View> {
        match node {
            SplitNode::Leaf(view) => {
                if view.is_active {
                    Some(view)
                } else {
                    None
                }
            }
            SplitNode::Split { left, right, .. } => {
                if let Some(v) = Self::find_active_view(left) {
                    return Some(v);
                }
                Self::find_active_view(right)
            }
        }
    }
    pub fn copy_active_view_to_clipboard(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
            && let Some(hash) = view.get_active_tab_hash()
            && let Some(buffer) = self.buffers.get(hash)
        {
            // On rassemble toutes les lignes avec un retour à la ligne
            let text_to_copy = buffer.lines.join("\n");

            // ASTUCE : On utilise un thread_local pour s'assurer que la connexion
            // Wayland n'est créée qu'une seule fois et survit à la fin de la fonction.
            thread_local! {
                static CLIPBOARD: std::cell::RefCell<Option<Clipboard>> = std::cell::RefCell::new(Clipboard::new().ok());
            }

            CLIPBOARD.with(|clipboard_cell| {
                if let Some(clipboard) = clipboard_cell.borrow_mut().as_mut() {
                    let _ = clipboard.set_text(text_to_copy);
                }
            });
        }
    }
    pub fn search_next(&mut self) {
        if self.search_query.is_empty() {
            return;
        }
        let re = match Regex::new(&self.search_query) {
            Ok(r) => r,
            Err(_) => return,
        };

        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
            && let Some(hash) = view.get_active_tab_hash()
            && let Some(buffer) = self.buffers.get(hash)
        {
            let current = view.scroll_offset;

            // Cherche vers le bas
            for (i, line) in buffer.lines.iter().enumerate().skip(current + 1) {
                if re.is_match(line) {
                    view.scroll_offset = i;
                    return; // Trouvé, on arrête
                }
            }
            // Wrap-around (reprend du début)
            for (i, line) in buffer.lines.iter().enumerate().take(current + 1) {
                if re.is_match(line) {
                    view.scroll_offset = i;
                    return;
                }
            }
        }
    }

    // Nouvelle fonction pour chercher vers le haut (Précédent)
    pub fn search_previous(&mut self) {
        if self.search_query.is_empty() {
            return;
        }
        let re = match Regex::new(&self.search_query) {
            Ok(r) => r,
            Err(_) => return,
        };

        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
            && let Some(hash) = view.get_active_tab_hash()
            && let Some(buffer) = self.buffers.get(hash)
        {
            let current = view.scroll_offset;
            let total_lines = buffer.lines.len();

            // Cherche vers le haut (on inverse l'itérateur)
            for i in (0..current).rev() {
                if re.is_match(&buffer.lines[i]) {
                    view.scroll_offset = i;
                    return;
                }
            }
            // Wrap-around (reprend de la fin)
            for i in (current..total_lines).rev() {
                if re.is_match(&buffer.lines[i]) {
                    view.scroll_offset = i;
                    return;
                }
            }
        }
    }
    pub fn cycle_focus(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            let mut view_count = 0;
            let mut active_idx = None;

            // 1. On compte les fenêtres et on repère l'index de la fenêtre active
            Self::find_active_idx(&workspace.root, &mut view_count, &mut active_idx);

            // S'il y a plus d'une fenêtre, on bascule
            if view_count > 1 {
                let next_idx = active_idx.map(|i| (i + 1) % view_count).unwrap_or(0);
                let mut current = 0;
                // 2. On applique le nouveau focus
                Self::apply_focus_idx(&mut workspace.root, &mut current, next_idx);
            }
        }
    }

    fn find_active_idx(node: &SplitNode, count: &mut usize, active: &mut Option<usize>) {
        match node {
            SplitNode::Leaf(v) => {
                if v.is_active {
                    *active = Some(*count);
                }
                *count += 1;
            }
            SplitNode::Split { left, right, .. } => {
                Self::find_active_idx(left, count, active);
                Self::find_active_idx(right, count, active);
            }
        }
    }

    fn apply_focus_idx(node: &mut SplitNode, current: &mut usize, target: usize) {
        match node {
            SplitNode::Leaf(v) => {
                v.is_active = *current == target; // Devient true UNIQUEMENT si c'est la cible
                *current += 1;
            }
            SplitNode::Split { left, right, .. } => {
                Self::apply_focus_idx(left, current, target);
                Self::apply_focus_idx(right, current, target);
            }
        }
    }
    pub fn next_tab_action(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            // On utilise notre nouvelle fonction pour cibler la bonne fenêtre
            if let Some(view) = Self::find_active_view_mut(&mut workspace.root) {
                view.next_tab();
            }
        }
    }
    pub fn previous_tab_action(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
        {
            view.previous_tab();
        }
    }
    pub fn split_active_view(&mut self, direction: SplitDirection) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            // Plus besoin de réassigner workspace.root, la fonction modifie l'arbre en place
            Self::recursive_split(&mut workspace.root, direction);
        }
    }

    fn recursive_split(node: &mut SplitNode, direction: SplitDirection) {
        match node {
            SplitNode::Leaf(view) => {
                // SÉCURITÉ : On ne split QUE la vue active
                if !view.is_active {
                    return;
                }

                let old_view = std::mem::take(view);
                let mut new_view = old_view.clone();

                // La nouvelle vue créée ne vole pas le focus à l'ancienne
                new_view.is_active = false;

                *node = SplitNode::Split {
                    direction,
                    left: Box::new(SplitNode::Leaf(old_view)),
                    right: Box::new(SplitNode::Leaf(new_view)),
                    ratio: 0.5,
                };
            }
            SplitNode::Split { left, right, .. } => {
                // On fouille dans les deux branches pour trouver la vue active
                Self::recursive_split(left, direction);
                Self::recursive_split(right, direction);
            }
        }
    }
    /// Passe au workspace suivant de manière circulaire
    pub fn next_workspace(&mut self) {
        if !self.workspaces.is_empty() {
            self.active_workspace = (self.active_workspace + 1) % self.workspaces.len();
        }
    }

    /// Revient au workspace précédent de manière circulaire
    pub fn previous_workspace(&mut self) {
        if !self.workspaces.is_empty() {
            if self.active_workspace == 0 {
                self.active_workspace = self.workspaces.len() - 1;
            } else {
                self.active_workspace -= 1;
            }
        }
    }

    pub fn cleanup_temp_files(&self) {
        let _ = remove_dir_all("/tmp/dwx");
    }

    /// Saute directement à un workspace spécifique (pour les raccourcis 1 à 9)
    pub fn go_to_workspace(&mut self, index: usize) {
        if index < self.workspaces.len() {
            self.active_workspace = index;
        }
    }
    pub fn draw(&mut self) -> std::io::Result<()> {
        let (cols, rows) = size()?;
        let mut stdout = stdout();

        // Nettoyage de l'écran avant le rendu
        queue!(stdout, Clear(ClearType::All))?;

        // Rendu de l'indicateur de workspace en haut à droite
        let ws_text = format!("[{}/{}]", self.active_workspace + 1, self.workspaces.len());
        let x_pos = cols.saturating_sub(ws_text.len() as u16);
        queue!(
            stdout,
            crossterm::cursor::MoveTo(x_pos, 0),
            SetForegroundColor(Color::DarkGrey),
            Print(ws_text),
            ResetColor
        )?;

        // Définir la zone de dessin (on laisse 2 lignes pour la barre d'onglets)
        let area = Rect {
            x: 0,
            y: 2,
            width: cols,
            height: rows.saturating_sub(2),
        };

        // Lancement du rendu récursif depuis la racine du workspace actif
        let root = &self.workspaces[self.active_workspace].root;
        self.draw_node(root, area, &mut stdout)?;
        if self.is_searching || !self.search_query.is_empty() {
            let mut match_count = 0;
            let mut valid_regex = true;

            // 1. Calcul des occurrences
            if !self.search_query.is_empty() {
                match Regex::new(&self.search_query) {
                    Ok(re) => {
                        // On récupère le texte du panneau actif
                        if let Some(workspace) = self.workspaces.get(self.active_workspace)
                            && let Some(view) = Self::find_active_view(&workspace.root)
                            && let Some(hash) = view.get_active_tab_hash()
                            && let Some(buffer) = self.buffers.get(hash)
                        {
                            // On compte les correspondances sur chaque ligne
                            for line in &buffer.lines {
                                match_count += re.find_iter(line).count();
                            }
                        }
                    }
                    Err(_) => valid_regex = false, // Si on est en train de taper un truc invalide (ex: "[a-")
                }
            }

            // 2. Affichage de la requête à gauche
            queue!(
                stdout,
                crossterm::cursor::MoveTo(0, rows.saturating_sub(1)), // Tout en bas
                SetBackgroundColor(Color::Black),
                Clear(ClearType::CurrentLine), // Nettoie la ligne pour éviter les résidus
                SetForegroundColor(Color::White),
                Print(format!("/{}", self.search_query)),
            )?;

            // 3. Affichage du compteur à droite
            if !self.search_query.is_empty() {
                // On prépare le texte
                let count_str = if !valid_regex {
                    "[Invalid regex]".to_string()
                } else if match_count == 0 {
                    "[No results]".to_string()
                } else {
                    format!(
                        "[{} occurrence{}]",
                        match_count,
                        if match_count > 1 { "s" } else { "" }
                    )
                };
                // On prépare la couleur
                let count_color = if !valid_regex || match_count == 0 {
                    Color::Red
                } else {
                    Color::Green
                };

                // On calcule la position X pour l'aligner à droite
                let x_pos = cols.saturating_sub(count_str.len() as u16);

                queue!(
                    stdout,
                    crossterm::cursor::MoveTo(x_pos, rows.saturating_sub(1)),
                    SetForegroundColor(count_color),
                    Print(count_str),
                )?;
            }

            queue!(stdout, ResetColor)?;
        }
        stdout.flush()
    }

    fn draw_node(
        &self,
        node: &SplitNode,
        area: Rect,
        stdout: &mut std::io::Stdout,
    ) -> std::io::Result<()> {
        match node {
            SplitNode::Leaf(view) => {
                // 1. Rendu des onglets pour cette vue
                let mut x_offset = area.x;
                if view.is_active {
                    queue!(
                        stdout,
                        crossterm::cursor::MoveTo(area.x, area.y),
                        SetBackgroundColor(Color::Black),
                        SetForegroundColor(Color::Green),
                        Print("▶ "),
                        ResetColor
                    )?;
                    x_offset += 2;
                }
                for (i, tab_hash) in view.tabs.iter().enumerate() {
                    let text = if self.show_filenames {
                        &self.buffers[tab_hash].original_name
                    } else {
                        tab_hash
                    };

                    queue!(stdout, crossterm::cursor::MoveTo(x_offset, area.y))?;
                    if i == view.active_tab {
                        queue!(
                            stdout,
                            SetForegroundColor(Color::White),
                            SetAttribute(Attribute::Underlined),
                            Print(text),
                            ResetColor,
                            SetAttribute(Attribute::NoUnderline)
                        )?;
                    } else {
                        queue!(
                            stdout,
                            SetForegroundColor(Color::DarkGrey),
                            Print(text),
                            ResetColor
                        )?;
                    }
                    x_offset += text.len() as u16 + 1;
                }

                // 2. PRÉPARATION DU CONTENU (Side-by-Side Diff ou Normal)
                let content_height = area.height.saturating_sub(1) as usize;
                let mut rendered_lines: Vec<(String, Color)> = Vec::new();
                let mut is_diff_rendered = false;

                if self.is_diff_mode
                    && let Some((b1, b2)) = self.get_diff_buffers()
                {
                    let current_hash = view.get_active_tab_hash();

                    // On identifie les deux volets principaux
                    let mut views = Vec::new();
                    Self::collect_views(&self.workspaces[self.active_workspace].root, &mut views);

                    let hash_left = views.first().and_then(|v| v.get_active_tab_hash());
                    let hash_right = views.get(1).and_then(|v| v.get_active_tab_hash());

                    let is_left = current_hash == hash_left;
                    let is_right = current_hash == hash_right;

                    // Si la vue courante fait partie du diff, on calcule et on aligne
                    if is_left || is_right {
                        is_diff_rendered = true;
                        let t1 = b1.lines.join("\n");
                        let t2 = b2.lines.join("\n");
                        let diff = TextDiff::from_lines(&t1, &t2);

                        for change in diff.iter_all_changes() {
                            // On nettoie les retours à la ligne pour le rendu
                            let val = change.value().trim_end_matches(['\r', '\n']).to_string();

                            match change.tag() {
                                ChangeTag::Equal => rendered_lines.push((val, Color::Reset)),
                                ChangeTag::Delete => {
                                    if is_left {
                                        rendered_lines.push((val, Color::Red));
                                    } else {
                                        rendered_lines.push(("".to_string(), Color::Reset)); // Ligne vide d'alignement
                                    }
                                }
                                ChangeTag::Insert => {
                                    if is_left {
                                        rendered_lines.push(("".to_string(), Color::Reset)); // Ligne vide d'alignement
                                    } else {
                                        rendered_lines.push((val, Color::Green));
                                    }
                                }
                            }
                        }
                    }
                }

                // Si le mode diff est désactivé ou qu'on n'a pas 2 volets valides, comportement normal
                if !is_diff_rendered
                    && let Some(buffer) = self
                        .buffers
                        .get(view.get_active_tab_hash().unwrap_or(&"".to_string()))
                {
                    for line in &buffer.lines {
                        rendered_lines.push((line.clone(), Color::Reset));
                    }
                }

                // 3. RENDU DES LIGNES (Avec coloration et recherche)
                let search_re = if !self.search_query.is_empty() {
                    Regex::new(&self.search_query).ok()
                } else {
                    None
                };

                let visible_lines = rendered_lines
                    .iter()
                    .skip(view.scroll_offset)
                    .take(content_height);

                for (i, (line, base_color)) in visible_lines.enumerate() {
                    queue!(
                        stdout,
                        crossterm::cursor::MoveTo(area.x, area.y + 1 + i as u16)
                    )?;
                    let truncated: String = line
                        .chars()
                        .skip(view.scroll_x)
                        .take(area.width as usize)
                        .collect();
                    if *base_color != Color::Reset {
                        queue!(stdout, SetForegroundColor(*base_color))?;
                    }

                    if let Some(re) = &search_re {
                        let mut last_end = 0;
                        for mat in re.find_iter(&truncated) {
                            let start = mat.start();
                            let end = mat.end();

                            if start > last_end {
                                queue!(stdout, Print(&truncated[last_end..start]))?;
                            }

                            // Surlignage de la recherche
                            queue!(
                                stdout,
                                SetForegroundColor(Color::Black),
                                SetBackgroundColor(Color::White),
                                Print(&truncated[start..end]),
                                ResetColor
                            )?;

                            // Restauration de la couleur du diff
                            if *base_color != Color::Reset {
                                queue!(stdout, SetForegroundColor(*base_color))?;
                            }

                            last_end = end;
                        }

                        if last_end < truncated.len() {
                            queue!(stdout, Print(&truncated[last_end..]))?;
                        }
                    } else {
                        queue!(stdout, Print(&truncated))?;
                    }

                    // Sécurité
                    queue!(stdout, ResetColor)?;
                }
            }
            SplitNode::Split {
                direction,
                left,
                right,
                ratio,
            } => {
                // Diviser la zone en deux et appeler récursivement
                let (rect1, rect2) = area.split(*direction, *ratio);
                self.draw_node(left, rect1, stdout)?;
                self.draw_node(right, rect2, stdout)?;
            }
        }
        Ok(())
    }

    /// Récupère les deux buffers si le workspace est divisé en deux panes
    pub fn get_diff_buffers(&self) -> Option<(&Buffer, &Buffer)> {
        let workspace = self.workspaces.get(self.active_workspace)?;

        // On a besoin d'une fonction pour extraire les deux feuilles d'un split
        // Si ton arbre est complexe, on se contente de chercher les deux premières feuilles
        let mut views = Vec::new();
        Self::collect_views(&workspace.root, &mut views);

        if views.len() >= 2 {
            let hash1 = views[0].get_active_tab_hash()?;
            let hash2 = views[1].get_active_tab_hash()?;

            return Some((self.buffers.get(hash1)?, self.buffers.get(hash2)?));
        }
        None
    }

    fn collect_views<'a>(node: &'a SplitNode, views: &mut Vec<&'a View>) {
        match node {
            SplitNode::Leaf(view) => views.push(view),
            SplitNode::Split { left, right, .. } => {
                Self::collect_views(left, views);
                Self::collect_views(right, views);
            }
        }
    }

    pub fn close_active_view(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            // On extrait l'arbre actuel pour le manipuler
            let current_root = std::mem::take(&mut workspace.root);

            // On reconstruit l'arbre en supprimant la vue active
            if let Some(new_root) = Self::recursive_close(current_root) {
                workspace.root = new_root;

                // IMPORTANT : On vient de détruire la fenêtre qui avait le focus.
                // Il faut obligatoirement redonner le focus à une autre fenêtre, sinon l'application gèlera.
                Self::force_focus_first(&mut workspace.root);
            } else {
                // S'il n'y avait qu'une seule fenêtre et qu'on la ferme, on recrée une vue vide par défaut.
                workspace.root = SplitNode::default();
            }
        }
    }

    fn recursive_close(node: SplitNode) -> Option<SplitNode> {
        match node {
            SplitNode::Leaf(view) => {
                // Si c'est la vue active, on renvoie None pour exiger sa destruction
                if view.is_active {
                    None
                } else {
                    Some(SplitNode::Leaf(view))
                }
            }
            SplitNode::Split {
                direction,
                left,
                right,
                ratio,
            } => {
                let new_left = Self::recursive_close(*left);
                let new_right = Self::recursive_close(*right);

                match (new_left, new_right) {
                    (Some(l), Some(r)) => {
                        // Aucun des enfants n'a été fermé, on reconstruit le split intact
                        Some(SplitNode::Split {
                            direction,
                            left: Box::new(l),
                            right: Box::new(r),
                            ratio,
                        })
                    }
                    (Some(l), None) => Some(l), // Le côté droit a été fermé, le côté gauche remonte pour remplacer le split
                    (None, Some(r)) => Some(r), // Le côté gauche a été fermé, le côté droit remonte
                    (None, None) => None,       // Cas extrême (ne devrait pas arriver)
                }
            }
        }
    }

    // Fonction de secours qui donne le focus à la première vue trouvée
    fn force_focus_first(node: &mut SplitNode) -> bool {
        match node {
            SplitNode::Leaf(view) => {
                view.is_active = true;
                true // Focus réattribué avec succès
            }
            SplitNode::Split { left, right, .. } => {
                // On tente de donner le focus à gauche d'abord, puis à droite
                if Self::force_focus_first(left) {
                    return true;
                }
                Self::force_focus_first(right)
            }
        }
    }

    pub fn add_workspaces(&mut self) -> &mut Self {
        self.workspaces.push(Workspace {
            root: SplitNode::default(),
        });
        self
    }
    pub fn add_file(&mut self, file: &PathBuf) -> &mut Self {
        let name = file
            .file_name()
            .expect("no filename")
            .to_str()
            .expect("msg")
            .to_string();
        let h = hash(file);
        let _ = create_dir_all("/tmp/dwx");

        // Récupère le timestamp de modification du fichier
        let last_modified = std::fs::metadata(file)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        self.buffers.insert(
            h.clone(),
            Buffer {
                original_name: name.to_string(),
                original_path: file.clone(),
                temp_path: PathBuf::from(format!("/tmp/dwx/{h}")),
                lines: read_to_string(file)
                    .expect("failed to get content")
                    .lines()
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
                last_modified,
            },
        );
        if !self.workspaces.is_empty()
            && let Some(workspace) = self.workspaces.get_mut(self.active_workspace)
            && let Some(view) = Self::find_active_view_mut(&mut workspace.root)
            && !view.tabs.contains(&h)
        {
            view.tabs.push(h.clone());
            // On donne automatiquement le focus au nouvel onglet
            view.active_tab = view.tabs.len() - 1;
        }
        self
    }

    fn collect_views_mut<'a>(node: &'a mut SplitNode, views: &mut Vec<&'a mut View>) {
        match node {
            SplitNode::Leaf(view) => views.push(view),
            SplitNode::Split { left, right, .. } => {
                Self::collect_views_mut(left, views);
                Self::collect_views_mut(right, views);
            }
        }
    }
    pub fn scroll_up(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            if self.is_diff_mode {
                // Mode Diff : On récupère les vues mutables et on fait défiler les deux premières
                let mut views = Vec::new();
                Self::collect_views_mut(&mut workspace.root, &mut views);

                for view in views.into_iter().take(2) {
                    view.scroll_offset = view.scroll_offset.saturating_sub(1);
                }
            } else if let Some(view) = Self::find_active_view_mut(&mut workspace.root) {
                // Mode normal : On ne fait défiler que la vue active
                view.scroll_offset = view.scroll_offset.saturating_sub(1);
            }
        }
    }

    pub fn scroll_down(&mut self) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            if self.is_diff_mode {
                // Mode Diff : Défilement synchronisé des deux panneaux vers le bas
                let mut views = Vec::new();
                Self::collect_views_mut(&mut workspace.root, &mut views);

                // Pour faire propre, on devrait idéalement vérifier la limite max de chaque buffer,
                // mais pour commencer on peut forcer l'incrémentation des deux
                for view in views.into_iter().take(2) {
                    view.scroll_offset += 1;
                }
            } else if let Some(view) = Self::find_active_view_mut(&mut workspace.root) {
                // Mode normal : Ta logique d'origine avec la vérification de la taille max
                let max_lines = self
                    .buffers
                    .get(view.get_active_tab_hash().unwrap_or(&String::new()))
                    .map(|b| b.lines.len())
                    .unwrap_or(0);

                if view.scroll_offset + 1 < max_lines {
                    view.scroll_offset += 1;
                }
            }
        }
    }

    pub fn make(&mut self) -> &mut Self {
        for x in self.buffers.values() {
            let mut f = File::create(&x.temp_path).expect("failed to create file");
            f.write_all(x.lines.join("\n").as_bytes()).expect("msg");
            f.sync_all().expect("failed to gen");
        }
        self
    }

    pub fn find_active_view_mut(node: &mut SplitNode) -> Option<&mut View> {
        match node {
            SplitNode::Leaf(view) => {
                if view.is_active {
                    Some(view)
                } else {
                    None
                }
            }
            SplitNode::Split { left, right, .. } => {
                if let Some(v) = Self::find_active_view_mut(left) {
                    return Some(v);
                }
                Self::find_active_view_mut(right)
            }
        }
    }
    pub fn add_stdin(&mut self) -> &mut Self {
        let mut stdin = stdin();

        // On vérifie si on reçoit des données d'un pipe (ex: cat *.rs | dwx)
        if !stdin.is_terminal() {
            let mut content = Vec::new();

            // On lit tout le pipe
            if stdin.read_to_end(&mut content).is_ok() {
                // On s'assure que le dossier de travail existe
                let _ = create_dir_all("/tmp/dwx");

                // On crée un fichier temporaire pour stocker le flux
                let temp_file_path = PathBuf::from("/tmp/dwx/stdin_pipe.txt");

                if let Ok(mut f) = File::create(&temp_file_path)
                    && f.write_all(&content).is_ok()
                {
                    // On force l'écriture sur le disque
                    let _ = f.sync_all();

                    // Et on réutilise ta fonction magique comme pour n'importe quel fichier !
                    self.add_file(&temp_file_path);
                }
            }
        }
        self
    }
    fn recursive_adjust_ratio(node: &mut SplitNode, delta: f32) -> bool {
        match node {
            SplitNode::Leaf(view) => {
                // Si c'est la vue active, on signale au nœud parent qu'il doit s'ajuster
                view.is_active
            }
            SplitNode::Split {
                left, right, ratio, ..
            } => {
                // On fouille dans la branche de gauche puis celle de droite
                let found_in_left = Self::recursive_adjust_ratio(left, delta);
                let found_in_right = Self::recursive_adjust_ratio(right, delta);

                // Si l'un des enfants DIRECTS est la vue active, c'est ce split qu'on modifie
                if found_in_left || found_in_right {
                    // On applique le delta et on utilise clamp pour la sécurité
                    *ratio = (*ratio + delta).clamp(0.1, 0.9);

                    // On retourne false pour arrêter la propagation du signal.
                    // Ainsi, les "grands-parents" ne se redimensionneront pas.
                    return false;
                }

                // Si on n'a rien trouvé à ce niveau, on continue de renvoyer false
                false
            }
        }
    }
    pub fn adjust_active_ratio(&mut self, delta: f32) {
        if let Some(workspace) = self.workspaces.get_mut(self.active_workspace) {
            // On lance la recherche et l'ajustement depuis la racine du workspace
            Self::recursive_adjust_ratio(&mut workspace.root, delta);
        }
    }
    pub fn run(&mut self) -> ExitCode {
        let mut stdout = stdout();

        enable_raw_mode().expect("Raw mode");
        execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide).ok();
        // --- 1. PRISE DE CONTRÔLE DU TERMINAL ---
        // On active le mode brut (les touches sont lues instantanément)
        enable_raw_mode().expect("Échec de l'activation du mode brut");
        // On passe sur l'écran alternatif et on cache le curseur
        execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide)
            .expect("Échec de la transition vers l'écran alternatif");
        let mut needs_redraw = true;
        // --- 2. BOUCLE PRINCIPALE ---
        loop {
            // A. Rendu de l'interface
            if needs_redraw {
                self.draw().expect("Erreur de rendu");
                needs_redraw = false; // On baisse le drapeau une fois l'écran mis à jour
            }
            for (_, buffer) in self.buffers.iter_mut() {
                if let Ok(metadata) = std::fs::metadata(&buffer.original_path)
                    && let Ok(modified) = metadata.modified()
                    && modified > buffer.last_modified
                    && let Ok(new_content) = std::fs::read_to_string(&buffer.original_path)
                {
                    buffer.lines = new_content.lines().map(|s| s.to_string()).collect();
                    buffer.last_modified = modified;

                    // NOUVEAU : Le fichier a été modifié, on demande un rafraîchissement
                    needs_redraw = true;
                }
            }

            // B. Gestion des événements clavier
            if poll(Duration::from_millis(10)).expect("msg")
                && let Ok(event) = event::read()
                && let Some(e) = event.as_key_event()
            {
                needs_redraw = true;
                if self.is_searching {
                    match e.code {
                        KeyCode::Esc => self.is_searching = false,
                        KeyCode::Enter => {
                            self.search_next();
                            self.is_searching = false;
                        }
                        KeyCode::Backspace => {
                            self.search_query.pop();
                        }
                        KeyCode::Char(c) => {
                            self.search_query.push(c);
                        }
                        _ => {}
                    }
                }
                // Logique Mode Fenêtre (window_mode)
                else if self.window_mode {
                    match e.code {
                        KeyCode::Char('v') => self.split_active_view(SplitDirection::Vertical),
                        KeyCode::Char('h') => self.split_active_view(SplitDirection::Horizontal),
                        KeyCode::Tab => self.cycle_focus(),
                        KeyCode::Char('q') => self.close_active_view(),
                        KeyCode::Esc => self.window_mode = false,
                        _ => {}
                    }
                }
                // Logique standard
                else {
                    match e.code {
                        KeyCode::Char('y') => {
                            self.copy_active_view_to_clipboard();
                        }
                        KeyCode::Char('/') => {
                            self.is_searching = true;
                            self.search_query.clear();
                        }
                        KeyCode::Char('d') => {
                            self.is_diff_mode = !self.is_diff_mode;

                            // L'Auto-Split magique :
                            // Si on active le diff mais qu'on a qu'une seule vue, on prépare le terrain
                            if self.is_diff_mode {
                                let mut view_count = 0;
                                let mut active_idx = None;

                                // On compte combien de panneaux sont actuellement ouverts
                                if let Some(workspace) = self.workspaces.get(self.active_workspace)
                                {
                                    Self::find_active_idx(
                                        &workspace.root,
                                        &mut view_count,
                                        &mut active_idx,
                                    );
                                }

                                // S'il n'y a qu'un seul panneau, on automatise la mise en page
                                if view_count < 2 {
                                    self.split_active_view(SplitDirection::Vertical); // 1. Coupe en deux
                                    self.cycle_focus(); // 2. Saute sur la vue de droite
                                    self.next_tab_action(); // 3. Affiche le fichier suivant
                                }
                            }
                        }
                        KeyCode::Char('h') => self.scroll_left(),
                        KeyCode::Char('l') => self.scroll_right(),
                        KeyCode::Char('w')
                            if event
                                .as_key_event()
                                .expect("msg")
                                .modifiers
                                .contains(KeyModifiers::CONTROL) =>
                        {
                            self.window_mode = true
                        }
                        KeyCode::PageDown => self.next_workspace(),
                        KeyCode::PageUp => self.previous_workspace(),
                        KeyCode::Char('>') => self.adjust_active_ratio(0.05),
                        KeyCode::Char('<') => self.adjust_active_ratio(-0.05),
                        KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                            if let Some(digit) = c.to_digit(10) {
                                self.go_to_workspace((digit - 1) as usize);
                            }
                        }
                        KeyCode::Esc => {
                            self.cleanup_temp_files();
                            break;
                        }
                        KeyCode::Char('n') => self.search_next(),
                        KeyCode::Char('N') => self.search_previous(),
                        KeyCode::F(2) => self.show_filenames = !self.show_filenames,
                        KeyCode::Tab | KeyCode::Right => self.next_tab_action(),
                        KeyCode::BackTab | KeyCode::Left => self.previous_tab_action(),
                        KeyCode::Up | KeyCode::Char('k') => self.scroll_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.scroll_down(),
                        _ => {}
                    }
                }
            }
        }

        // Cette étape est vitale pour rendre le terminal à l'utilisateur dans un état propre
        execute!(stdout, LeaveAlternateScreen, crossterm::cursor::Show,)
            .expect("Échec de la restauration de l'écran");
        enable_raw_mode().expect("Échec de la désactivation du mode brut");

        ExitCode::SUCCESS
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.cleanup_temp_files();
    }
}
