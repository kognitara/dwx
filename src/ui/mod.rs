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
pub fn draw(state: &mut MillerState) -> std::io::Result<()> {
    let mut stdout = stdout();
    // On efface l'écran à chaque frame et on cache le curseur
    stdout.queue(Clear(ClearType::All))?;
    stdout.queue(Hide)?;
    // 1. On récupère la taille dynamique du terminal
    let (cols, rows) = size()?;

    // 2. Calcul des largeurs des 3 colonnes
    let col1_w = (cols as f32 * 0.20) as u16;
    let col2_w = (cols as f32 * 0.30) as u16;
    let col3_w = cols.saturating_sub(col1_w).saturating_sub(col2_w);

    // --- COLONNE 1 : LE PARENT ---
    // On trouve le chemin du parent direct
    let parent_path = state.current_dir.parent().unwrap_or(&state.current_dir);

    // On va chercher son offset dans l'historique (0 par défaut si jamais visité)
    let parent_scroll_offset = state
        .history
        .get(parent_path)
        .map(|s| s.scroll_offset)
        .unwrap_or(0);

    // On applique le skip avec l'offset historique !
    for (i, item) in state
        .parent_entries
        .iter()
        .skip(parent_scroll_offset)
        .take(rows as usize)
        .enumerate()
    {
        stdout.queue(MoveTo(0, i as u16))?;
        let display_name = format_item(item, col1_w);

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

    // 3. On applique le skip() comme pour la Colonne 2
    for (i, item) in state
        .parent_entries
        .iter()
        .skip(parent_scroll_offset)
        .take(rows as usize)
        .enumerate()
    {
        stdout.queue(MoveTo(0, i as u16))?;
        let display_name = format_item(item, col1_w);

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

    // --- COLONNE 2 : LE DOSSIER COURANT (Focus actif filtré) ---
    // Au lieu de lire current_entries, on lit notre calque filtered_indices !
    // On garde une petite marge en bas (rows - 1) pour ne pas écraser la barre de recherche
    let visible_rows = rows.saturating_sub(1) as usize;

    for (i, &actual_index) in state
        .filtered_indices
        .iter()
        .skip(state.scroll_offset)
        .take(visible_rows)
        .enumerate()
    {
        stdout.queue(MoveTo(col1_w, i as u16))?;

        // On récupère le VRAI fichier en utilisant l'index du calque
        if let Some(item) = state.current_entries.get(actual_index) {
            let display_name = format_item(item, col2_w);

            // Le fichier est sélectionné si sa position DANS LE CALQUE correspond au selected_index
            let is_selected = (i + state.scroll_offset) == state.selected_index;

            // Ton ancienne logique de couleur reste la même
            if item.is_dir {
                echo(
                    &mut stdout,
                    item,
                    &display_name,
                    is_selected,
                    Color::Blue,
                    Color::Black,
                )?;
            } else if item.is_executable {
                echo(
                    &mut stdout,
                    item,
                    &display_name,
                    is_selected,
                    Color::Green,
                    Color::Black,
                )?;
            } else if item.is_file {
                echo(
                    &mut stdout,
                    item,
                    &display_name,
                    is_selected,
                    Color::White,
                    Color::Black,
                )?;
            } else {
                echo(
                    &mut stdout,
                    item,
                    &display_name,
                    is_selected,
                    Color::Black,
                    Color::White,
                )?;
            }
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
            let max_preview_width = col3_w.saturating_sub(1) as usize;

            for (i, line) in lines.iter().take(rows as usize).enumerate() {
                stdout.queue(MoveTo(col1_w + col2_w, i as u16))?;

                // On coupe violemment la ligne si elle dépasse la largeur de la colonne 3
                let truncated_line: String = line.chars().take(max_preview_width).collect();
                stdout.queue(Print(truncated_line))?;
            }
        }
        Preview::Empty => {}
    }
    // --- L'OMNIBAR (Barre de recherche) ---
    // Si on est en mode saisie, on dessine la barre tout en bas de l'écran
    if let crate::tree::AppMode::Omnibar {
        prefix,
        input_buffer,
        receiver: _,
        kill_switch: _,
    } = &state.mode
    {
        // On se place sur la toute dernière ligne du terminal
        stdout.queue(MoveTo(0, rows.saturating_sub(1)))?;

        // Un petit style visuel distinct (tu peux l'ajuster pour ton thème sombre)
        stdout.queue(SetBackgroundColor(Color::White))?;
        stdout.queue(SetForegroundColor(Color::Black))?;
        stdout.queue(Print(Bold))?;

        // On remplit la ligne d'espaces pour que le fond gris aille jusqu'au bout
        let prompt = format!("{} {} ", prefix, input_buffer);
        let padding = " ".repeat(cols.saturating_sub(prompt.chars().count() as u16) as usize);

        stdout.queue(Print(format!("{}{}", prompt, padding)))?;
        stdout.queue(ResetColor)?;
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
