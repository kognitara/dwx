use crate::workspaces::{AppMode, Preview, Workspace};
use crossterm::{
    cursor::{self, MoveTo},
    queue,
    style::{
        Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor, Stylize,
    },
    terminal::size,
};
use devicons::FileIcon;
use std::io::{Write, stdout};

pub fn draw_ui(workspace: &mut Workspace) {
    let mut stdout = stdout();
    let (cols, rows) = size().unwrap_or((100, 24));
    // 1. Calcul strict des largeurs de colonnes (10% / % / 30% / 50%)
    let col1_w = (cols as f32 * 0.20).round() as u16; // SYSINFO (élargi pour pousser vers la droite)
    let col2_w = (cols as f32 * 0.30).round() as u16; // PARENT
    // 2. On calcule les positions X AVANT d'appeler la preview
    let col1_x = 0;
    let col2_x = col1_w;
    let col3_x = col1_w + col2_w;

    // ---------------------------------------------------------
    // 2. DESSIN DES EN-TÊTES (Avec contraste de focus)
    // ---------------------------------------------------------
    let mut draw_header = |pane_idx: usize, x: u16, title: &str| {
        queue!(stdout, cursor::MoveTo(x + 2, 0)).unwrap();
        if workspace.active_pane == pane_idx {
            queue!(
                stdout,
                SetForegroundColor(Color::Red),
                SetAttribute(crossterm::style::Attribute::Bold),
                Print(title),
                ResetColor,
            )
            .unwrap();
        } else {
            queue!(
                stdout,
                SetAttribute(crossterm::style::Attribute::Bold),
                SetForegroundColor(Color::White),
                Print(title),
                ResetColor,
            )
            .unwrap();
        }
    };

    // Plus de colonne ROOT, on décale les index
    draw_header(0, col1_x, "PARENT");
    draw_header(1, col2_x, "CURRENT");
    draw_header(2, col3_x, "PREVIEW");

    let max_rows = rows.saturating_sub(3); // On garde de la place pour la ligne d'en-tête
    // ---------------------------------------------------------
    // 1. COLONNE 1  : SYS INFOS
    // ---------------------------------------------------------
    let mut y = 2;
    let current = workspace.miller.current_dir.to_path_buf();
    for item in workspace
        .miller
        .parent_entries
        .iter()
        .take(max_rows as usize)
    {
        if y >= rows {
            break;
        }
        let icon = FileIcon::from(item.path.to_path_buf());
        queue!(stdout, cursor::MoveTo(col1_x + 2, y)).unwrap();
        if item.is_dir && item.path.to_path_buf().eq(&current) {
            queue!(
                stdout,
                SetAttribute(crossterm::style::Attribute::Bold),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::Blue),
                Print(format!(
                    "{} {} {}",
                    icon.icon,
                    item.name.as_str(),
                    "*".red().italic().bold()
                )),
                ResetColor,
            )
            .unwrap();
        } else if item.is_dir {
            queue!(
                stdout,
                SetAttribute(crossterm::style::Attribute::Bold),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::Blue),
                Print(format!("{} {}", icon.icon, item.name.as_str())),
                ResetColor,
            )
            .unwrap();
        } else if item.is_executable {
            queue!(
                stdout,
                SetAttribute(crossterm::style::Attribute::Bold),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::Green),
                Print(format!("{} {}", icon.icon, item.name.as_str())),
                ResetColor,
            )
            .unwrap();
        } else if item.is_symlink {
            queue!(
                stdout,
                SetAttribute(crossterm::style::Attribute::Bold),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::Cyan),
                Print(format!("{} {}", icon.icon, item.name.as_str())),
                ResetColor,
            )
            .unwrap();
        } else {
            queue!(
                stdout,
                SetAttribute(crossterm::style::Attribute::Bold),
                SetBackgroundColor(Color::Black),
                SetForegroundColor(Color::White),
                Print(format!("{} {}", icon.icon, item.name.as_str())),
                ResetColor,
            )
            .unwrap();
        };
        y += 1;
    }

    // ---------------------------------------------------------
    // 4. COLONNE 4 : CURRENT (Dossier courant avec curseur actif)
    // ---------------------------------------------------------
    y = 2;
    let start_idx = workspace.miller.scroll_offset;

    // On itère sur les index FILTRÉS, pas sur tous les fichiers
    let visible_indices = workspace
        .miller
        .filtered_indices
        .iter()
        .enumerate()
        .skip(start_idx)
        .take(max_rows as usize);

    for (loop_idx, &actual_item_idx) in visible_indices {
        if y >= rows {
            break;
        }

        // On récupère le vrai fichier grâce à l'index filtré
        if let Some(item) = workspace.miller.current_entries.get(actual_item_idx) {
            queue!(stdout, cursor::MoveTo(col2_x + 2, y)).unwrap();
            let icon = FileIcon::from(item.path.to_path_buf());

            // Le curseur de sélection correspond à notre position dans la liste filtrée
            let is_selected = loop_idx == workspace.miller.selected_index;

            let display_name = if item.is_dir {
                format!("{}/", item.name)
            } else {
                item.name.clone()
            };

            if is_selected {
                queue!(
                    stdout,
                    SetAttribute(crossterm::style::Attribute::Bold),
                    SetBackgroundColor(Color::Black),
                    SetForegroundColor(Color::Red),
                    Print(format!("> {} {}", icon.icon, display_name)),
                    ResetColor,
                )
                .unwrap();
            } else {
                let color = if item.is_dir {
                    Color::Blue
                } else if item.is_executable {
                    Color::Green
                } else if item.is_symlink {
                    Color::Cyan
                } else {
                    Color::White
                };
                queue!(
                    stdout,
                    SetAttribute(crossterm::style::Attribute::Bold),
                    SetBackgroundColor(Color::Black),
                    SetForegroundColor(color),
                    Print(format!("  {} {}", icon.icon, display_name)),
                    ResetColor,
                )
                .unwrap();
            }
            y += 1;
        }
    }

    // On nettoie les lignes restantes en bas si le filtre a réduit la liste
    while y < rows {
        queue!(stdout, cursor::MoveTo(col3_x + 2, y)).unwrap();
        y += 1;
    }

    // ---------------------------------------------------------
    // 5. COLONNE 5 : INSPECT / PREVIEW (`workspace.preview`)
    // ---------------------------------------------------------
    y = 2;
    let preview_max_width = (cols.saturating_sub(col3_x + 2)) as usize;
    // On calcule la place dispo pour le texte
    match &workspace.preview {
        Preview::Dir(entries) => {
            for item in entries.iter().take(max_rows as usize) {
                if y >= rows {
                    break;
                }
                let icon = FileIcon::from(item.path.to_path_buf());
                // On prépare le texte complet (icône + nom)
                let full_text = format!("{} {}", icon.icon, item.name.as_str());

                // Coloration standard : Bleu pour les dossiers, neutre pour les fichiers
                let color = if item.is_dir {
                    Color::Blue
                } else if item.is_executable {
                    Color::Green
                } else if item.is_symlink {
                    Color::Cyan
                } else {
                    Color::White
                };

                queue!(stdout, cursor::MoveTo(col3_x + 2, y)).unwrap();
                // On le tronque avec "…" s'il dépasse le cadre
                let display_text = if full_text.chars().count() > preview_max_width {
                    let mut trunc: String = full_text
                        .chars()
                        .take(preview_max_width.saturating_sub(1))
                        .collect();
                    trunc.push('…');
                    trunc
                } else {
                    full_text
                };

                queue!(
                    stdout,
                    SetAttribute(crossterm::style::Attribute::Bold),
                    SetBackgroundColor(Color::Black),
                    SetForegroundColor(color),
                    // On utilise display_text au lieu de construire le format directement ici
                    Print(display_text),
                    ResetColor,
                )
                .unwrap();
                y += 1;
            }
            // --- CORRECTION : Nettoyage des lignes fantômes ---
            while y < rows {
                queue!(stdout, cursor::MoveTo(col3_x + 2, y)).unwrap();
                y += 1;
            }
        }
        Preview::File(lines) => {
            for line in lines.iter().take(max_rows as usize) {
                if y >= rows {
                    break;
                }
                queue!(stdout, cursor::MoveTo(col3_x + 2, y)).unwrap();

                // Troncature propre de la ligne de texte
                let display_line = if line.chars().count() > preview_max_width {
                    let mut trunc: String = line
                        .chars()
                        .take(preview_max_width.saturating_sub(1))
                        .collect();
                    trunc.push('…');
                    trunc
                } else {
                    line.clone()
                };
                queue!(stdout, Print(display_line), ResetColor,).unwrap();
                y += 1;
            }

            // --- CORRECTION : Nettoyage des lignes fantômes ---
            while y < rows {
                queue!(stdout, cursor::MoveTo(col3_x + 2, y)).unwrap();
                y += 1;
            }
        }
        Preview::Empty => {}
    }

    // ---------------------------------------------------------
    // 6. OMNIBAR (Barre de recherche / création)
    // ---------------------------------------------------------
    if let AppMode::Omnibar {
        prefix,
        input_buffer,
    } = &workspace.mode
    {
        // On se place sur la toute dernière ligne en bas de l'écran
        queue!(stdout, MoveTo(0, rows.saturating_sub(1))).unwrap();
        queue!(
            stdout,
            SetForegroundColor(Color::Black),
            SetAttribute(crossterm::style::Attribute::Bold),
            crossterm::style::SetBackgroundColor(Color::Red),
            Print(format!(" {} {} ", prefix, input_buffer)),
            Print(" ".repeat((cols as usize).saturating_sub(input_buffer.len() + 4))),
            Print(ResetColor)
        )
        .unwrap();
    }
    // Pousse tout le rendu vers le terminal en un seul coup (Mushin pur)
    stdout.flush().unwrap();
}
