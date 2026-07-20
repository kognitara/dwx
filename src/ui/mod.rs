use crossterm::{
    QueueableCommand,
    cursor::{Hide, MoveTo},
    style::{Attribute::Bold, Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType, size},
};
use std::io::{Write, stdout};

use crate::tree::{FileItem, MillerState, Preview};
fn echo(
    output: &mut std::io::Stdout,
    item: &FileItem,
    display_name: &String,
    hover: bool,
    hover_bg_color: Color,
    hover_fg_color: Color,
) -> std::io::Result<()> {
    // 1. Définir la couleur du texte façon "Ranger"
    let fg_color = if item.is_symlink {
        Color::Cyan
    } else if item.is_dir {
        Color::Blue
    } else if item.is_executable {
        Color::Green
    } else {
        Color::Reset // Fichier standard avec la couleur par défaut du terminal
    };

    // 2. Appliquer la couleur de fond si c'est sélectionné
    if hover {
        output.queue(SetBackgroundColor(hover_bg_color))?;
        output.queue(SetForegroundColor(hover_fg_color))?;
    } else {
        output.queue(SetBackgroundColor(Color::Reset))?;
        output.queue(SetForegroundColor(fg_color))?;
    }
    
    output.queue(Print(Bold))?;
    // 3. Afficher le texte et réinitialiser
    output.queue(Print(display_name))?;
    output.queue(ResetColor)?;
    Ok(())
}
pub fn draw(state: &MillerState) -> std::io::Result<()> {
    let mut stdout = stdout();

    // 1. On récupère la taille dynamique du terminal
    let (cols, rows) = size()?;

    // 2. Calcul des largeurs des 3 colonnes
    let col1_w = (cols as f32 * 0.20) as u16;
    let col2_w = (cols as f32 * 0.30) as u16;
    let col3_w = cols.saturating_sub(col1_w).saturating_sub(col2_w);

    // On efface l'écran à chaque frame et on cache le curseur
    stdout.queue(Clear(ClearType::All))?;
    stdout.queue(Hide)?;

    // --- COLONNE 1 : LE PARENT ---
    for (i, item) in state.parent_entries.iter().take(rows as usize).enumerate() {
        stdout.queue(MoveTo(0, i as u16))?;
        let display_name = format_item(item, col1_w);

        // VRAIE CONDITION : On compare le chemin de l'item avec le dossier courant de l'état
        let is_current_parent = item.path == state.current_dir;
        echo(
            &mut stdout,
            item,
            &display_name,
            is_current_parent,
            Color::Blue,
            Color::Black,
        )?;
    }

    // --- COLONNE 2 : LE DOSSIER COURANT (Focus actif) ---
    for (i, item) in state.current_entries.iter().take(rows as usize).enumerate() {
        stdout.queue(MoveTo(col1_w, i as u16))?;
        let display_name = format_item(item, col2_w);

        // Couleur de focus standard (Gris foncé ou Blanc, selon tes préférences)
        if item.is_dir {
            echo(
                &mut stdout,
                item,
                &display_name,
                i == state.selected_index,
                Color::Blue,
                Color::Black,
            )?;
        } else if item.is_executable {
            echo(
                &mut stdout,
                item,
                &display_name,
                i == state.selected_index,
                Color::Green,
                Color::Black,
            )?;
        } else if item.is_file {
            echo(
                &mut stdout,
                item,
                &display_name,
                i == state.selected_index,
                Color::White,
                Color::Black,
            )?;
        }
    }

    // --- COLONNE 3 : L'APERÇU ---
    // (Ancienne boucle preview_entries supprimée ici pour éviter les conflits d'affichage)

    match &state.preview {
        Preview::Dir(entries) => {
            // Affichage du contenu d'un dossier en aperçu
            for (i, item) in entries.iter().take(rows as usize).enumerate() {
                stdout.queue(MoveTo(col1_w + col2_w, i as u16))?;
                let display_name = format_item(item, col3_w);

                if item.is_dir {
                    echo(
                        &mut stdout,
                        item,
                        &display_name,
                        i == 0,
                        Color::Blue,
                        Color::Black,
                    )?;
                } else if item.is_file {
                    echo(
                        &mut stdout,
                        item,
                        &display_name,
                        i == 0,
                        Color::White,
                        Color::Black,
                    )?;
                } else if item.is_executable {
                    echo(
                        &mut stdout,
                        item,
                        &display_name,
                        i == 0,
                        Color::Green,
                        Color::Black,
                    )?;
                } else if item.is_symlink {
                    echo(
                        &mut stdout,
                        item,
                        &display_name,
                        i == 0,
                        Color::Cyan,
                        Color::Black,
                    )?;
                }
                // CONDITION POUR SURBRILLER LE PREMIER : i == 0
            }
        }
        Preview::File(lines) => {
            // Affichage du code source coloré par syntect
            for (i, line) in lines.iter().take(rows as usize).enumerate() {
                stdout.queue(MoveTo(col1_w + col2_w, i as u16))?;
                stdout.queue(Print(line))?;
            }
        }
        Preview::Empty => {}
    }

    // On s'assure de tout réinitialiser et on pousse l'affichage à l'écran
    stdout.queue(ResetColor)?;
    stdout.flush()?;

    Ok(())
}

/// Formate le nom d'un fichier pour qu'il rentre exactement dans la largeur de la colonne
fn format_item(item: &FileItem, max_width: u16) -> String {
    // On retire 1 pour créer une gouttière d'espacement entre les colonnes
    let display_width = max_width.saturating_sub(1) as usize;
    let name = item.name.clone();

    let char_count = name.chars().count();

    if char_count > display_width {
        // On coupe si c'est trop long
        let mut truncated: String = name.chars().take(display_width.saturating_sub(1)).collect();
        truncated.push('…');
        truncated
    } else {
        // Le secret est ici : on calcule et on répète MANUELLEMENT les espaces.
        // Crossterm sera obligé de peindre le fond sur chaque espace.
        let spaces_needed = display_width - char_count;
        let padding = " ".repeat(spaces_needed);
        format!("{}{}", name, padding)
    }
}
