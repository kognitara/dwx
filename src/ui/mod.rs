use crossterm::{
    QueueableCommand,
    cursor::{Hide, MoveTo},
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType, size},
};
use std::io::{Write, stdout};

use crate::tree::{FileItem, MillerState, Preview};

// Supposons que tu as accès à state de type &MillerState
pub fn draw(state: &MillerState) -> std::io::Result<()> {
    let mut stdout = stdout();

    // 1. On récupère la taille dynamique du terminal
    let (cols, rows) = size()?;

    // 2. Calcul des largeurs des 3 colonnes
    let col1_w = (cols as f32 * 0.20) as u16;
    let col2_w = (cols as f32 * 0.30) as u16;
    let col3_w = cols.saturating_sub(col1_w).saturating_sub(col2_w); // Le reste de l'écran

    // On efface l'écran à chaque frame
    stdout.queue(Clear(ClearType::All))?;
    stdout.queue(Hide)?; // Cacher le curseur du terminal

    // --- COLONNE 1 : LE PARENT (Grisé pour le contexte) ---
    for (i, item) in state.parent_entries.iter().take(rows as usize).enumerate() {
        stdout.queue(MoveTo(0, i as u16))?;
        let display_name = format_item(item, col1_w);
        stdout.queue(SetForegroundColor(Color::DarkGrey))?;
        stdout.queue(Print(display_name))?;
    }

    // --- COLONNE 2 : LE DOSSIER COURANT (Focus actif) ---
    for (i, item) in state.current_entries.iter().take(rows as usize).enumerate() {
        stdout.queue(MoveTo(col1_w, i as u16))?;
        let display_name = format_item(item, col2_w);

        if i == state.selected_index {
            // Surbrillance subtile pour le curseur (fond gris foncé, texte blanc)
            stdout.queue(SetBackgroundColor(Color::DarkGrey))?;
            stdout.queue(SetForegroundColor(Color::White))?;
        } else if item.is_dir {
            stdout.queue(SetForegroundColor(Color::Blue))?;
        } else {
            stdout.queue(SetForegroundColor(Color::Reset))?;
        }

        stdout.queue(Print(display_name))?;
        stdout.queue(ResetColor)?; // On nettoie tout de suite après
    }

    // --- COLONNE 3 : L'APERÇU (Si c'est un dossier) ---
    for (i, item) in state.preview_entries.iter().take(rows as usize).enumerate() {
        stdout.queue(MoveTo(col1_w + col2_w, i as u16))?;
        let display_name = format_item(item, col3_w);

        if item.is_dir {
            stdout.queue(SetForegroundColor(Color::Blue))?;
        } else {
            stdout.queue(SetForegroundColor(Color::Reset))?;
        }
        stdout.queue(Print(display_name))?;
    }
    match &state.preview {
        Preview::Dir(entries) => {
            // Affichage classique du contenu d'un dossier
            for (i, item) in entries.iter().take(rows as usize).enumerate() {
                stdout.queue(MoveTo(col1_w + col2_w, i as u16))?;
                let display_name = format_item(item, col3_w);

                if item.is_dir {
                    stdout.queue(SetForegroundColor(Color::Blue))?;
                } else {
                    stdout.queue(SetForegroundColor(Color::Reset))?;
                }
                stdout.queue(Print(display_name))?;
            }
        }
        Preview::File(lines) => {
            // Affichage du code source coloré
            for (i, line) in lines.iter().take(rows as usize).enumerate() {
                stdout.queue(MoveTo(col1_w + col2_w, i as u16))?;

                // Comme "line" contient déjà les séquences d'échappement ANSI de syntect,
                // crossterm va naturellement afficher les bonnes couleurs sans qu'on ait
                // besoin d'utiliser SetForegroundColor.

                // Note : Tronquer une chaîne ANSI est complexe car on risque de couper
                // le code couleur. Pour un explorateur terminal, on affiche simplement la ligne.
                stdout.queue(Print(line))?;
            }
        }
        Preview::Empty => {}
    }

    stdout.queue(ResetColor)?;
    stdout.flush()?;

    // On s'assure de tout réinitialiser et on pousse l'affichage à l'écran
    stdout.queue(ResetColor)?;
    stdout.flush()?;

    Ok(())
}

/// Formate le nom d'un fichier pour qu'il rentre exactement dans la largeur de la colonne
fn format_item(item: &FileItem, max_width: u16) -> String {
    let max_usize = max_width as usize;
    let mut name = item.name.clone();

    // On ajoute un petit slash visuel pour repérer les dossiers immédiatement
    if item.is_dir {
        name.push('/');
    }

    if name.chars().count() > max_usize {
        // On coupe si c'est trop long
        let mut truncated: String = name.chars().take(max_usize.saturating_sub(1)).collect();
        truncated.push('…');
        truncated
    } else {
        // On remplit avec des espaces si c'est plus court (Padding)
        format!("{:<width$}", name, width = max_usize)
    }
}
