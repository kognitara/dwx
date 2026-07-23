use crate::workspaces::{AppMode, Preview, Workspace};
use crossterm::{
    cursor::{self, MoveTo},
    queue,
    style::{
        Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor, Stylize,
    },
    terminal::{Clear, ClearType, size},
};
use std::io::{Write, stdout};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

pub fn draw_ui(workspace: &mut Workspace) {
    let mut stdout = stdout();
    let (cols, rows) = size().unwrap_or((100, 24));
    let col1_w = (cols as f32 * 0.20).round() as u16;
    let col2_w = (cols as f32 * 0.30).round() as u16;
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

    draw_header(0, col1_x, "PARENT");
    draw_header(1, col2_x, "CURRENT");
    draw_header(2, col3_x, "PREVIEW");

    let max_rows = rows.saturating_sub(3);

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
        let icon = devicons::FileIcon::from(item.path.to_path_buf());
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

        if let Some(item) = workspace.miller.current_entries.get(actual_item_idx) {
            queue!(stdout, cursor::MoveTo(col2_x + 2, y)).unwrap();
            let icon = devicons::FileIcon::from(item.path.to_path_buf());

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

    while y < rows {
        queue!(
            stdout,
            cursor::MoveTo(col3_x + 2, y),
            Clear(ClearType::UntilNewLine)
        )
        .unwrap();
        y += 1;
    }

    // ---------------------------------------------------------
    // 5. COLONNE 5 : INSPECT / PREVIEW (`workspace.preview`)
    // ---------------------------------------------------------
    y = 2;
    let preview_max_width = (cols.saturating_sub(col3_x + 2)) as usize;
    match &workspace.preview {
        Preview::Dir(entries) => {
            for item in entries.iter().take(max_rows as usize) {
                if y >= rows {
                    break;
                }
                let icon = devicons::FileIcon::from(item.path.to_path_buf());
                let full_text = format!("{} {}", icon.icon, item.name.as_str());

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
                    Print(display_text),
                    Clear(ClearType::UntilNewLine),
                    ResetColor,
                )
                .unwrap();
                y += 1;
            }
            while y < rows {
                queue!(
                    stdout,
                    cursor::MoveTo(col3_x + 2, y),
                    Clear(ClearType::UntilNewLine)
                )
                .unwrap();
                y += 1;
            }
        }
        Preview::File(lines) => {
            // --- NOUVEAU : Coloration Syntaxique Syntect ---
            static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
            static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

            let ps = SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines);
            let ts = THEME_SET.get_or_init(ThemeSet::load_defaults);
            let theme = &ts.themes["base16-ocean.dark"];

            // Récupérer l'extension du fichier actuellement sélectionné pour appliquer la bonne syntaxe
            let mut syntax = ps.find_syntax_plain_text();

            if let Some(&actual_item_idx) = workspace
                .miller
                .filtered_indices
                .get(workspace.miller.selected_index)
            {
                if let Some(item) = workspace.miller.current_entries.get(actual_item_idx) {
                    if let Some(ext) = item.path.extension().and_then(|s| s.to_str()) {
                        if let Some(found_syntax) = ps.find_syntax_by_extension(ext) {
                            syntax = found_syntax;
                        }
                    }
                }
            }

            let mut h = HighlightLines::new(syntax, theme);

            for line in lines.iter().take(max_rows as usize) {
                if y >= rows {
                    break;
                }
                queue!(stdout, cursor::MoveTo(col3_x + 2, y)).unwrap();

                // Application de la coloration syntaxique
                let regions = h.highlight_line(line, ps).unwrap_or_default();
                let mut current_width = 0;

                for (style, text) in regions {
                    let fg = Color::Rgb {
                        r: style.foreground.r,
                        g: style.foreground.g,
                        b: style.foreground.b,
                    };

                    let text_chars = text.chars().count();

                    if current_width + text_chars > preview_max_width {
                        let allowed = preview_max_width.saturating_sub(current_width + 1);
                        if allowed > 0 {
                            let trunc: String = text.chars().take(allowed).collect();
                            queue!(stdout, SetForegroundColor(fg), Print(trunc)).unwrap();
                        }
                        // Troncature visuelle
                        queue!(stdout, SetForegroundColor(Color::DarkGrey), Print("…")).unwrap();
                        break;
                    } else {
                        queue!(stdout, SetForegroundColor(fg), Print(text)).unwrap();
                        current_width += text_chars;
                    }
                }

                // On s'assure que la fin de la ligne est nettoyée et que la couleur est réinitialisée
                queue!(stdout, Clear(ClearType::UntilNewLine), ResetColor).unwrap();
                y += 1;
            }

            while y < rows {
                queue!(
                    stdout,
                    cursor::MoveTo(col3_x + 2, y),
                    Clear(ClearType::UntilNewLine)
                )
                .unwrap();
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
    stdout.flush().unwrap();
}
