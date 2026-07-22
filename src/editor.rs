use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode},
    queue,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType, size},
};
use std::{
    fs::read_to_string,
    io::{Write, stdout},
    path::Path,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;

pub fn view_file_with_scroll<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    // 1. Initialisation de Syntect
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();

    // Un thème sombre élégant et lisible
    let theme = &ts.themes["base16-ocean.dark"];

    // Détecte le langage selon l'extension, sinon bascule en texte brut
    let syntax = ps
        .find_syntax_for_file(path.as_ref())
        .unwrap_or_default()
        .unwrap_or_else(|| ps.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, theme);

    // 2. Chargement et pré-coloration de tout le fichier
    let content = read_to_string(path.as_ref())?;
    let mut highlighted_lines: Vec<Vec<(Style, String)>> = Vec::new();

    // On utilise split_inclusive pour garder les \n (syntect en a besoin pour son contexte)
    for line in content.split_inclusive('\n') {
        let regions = h.highlight_line(line, &ps).unwrap();

        // On nettoie les \n et \r de la fin pour ne pas casser la grille crossterm au moment d'afficher
        let cleaned_regions: Vec<(Style, String)> = regions
            .into_iter()
            .map(|(style, text)| {
                (
                    style,
                    text.trim_end_matches(&['\n', '\r', '\t'][..]).to_string(),
                )
            })
            .collect();
        highlighted_lines.push(cleaned_regions);
    }

    let total_lines = highlighted_lines.len();
    let mut stdout = stdout();
    let mut scroll_offset = 0;

    // 3. Boucle d'affichage "Mushin"
    loop {
        let (cols, rows) = size()?;
        let max_visible_rows = rows.saturating_sub(1) as usize;

        queue!(
            stdout,
            SetBackgroundColor(Color::Black),
            SetForegroundColor(Color::White)
        )?;

        // Dessin des lignes colorées
        for i in 0..max_visible_rows {
            queue!(stdout, MoveTo(0, i as u16), Clear(ClearType::CurrentLine))?;

            if let Some(regions) = highlighted_lines.get(scroll_offset + i) {
                let mut current_width = 0;

                for (style, text) in regions {
                    // Conversion de la couleur Syntect (RGB) vers Crossterm
                    let fg = Color::Rgb {
                        r: style.foreground.r,
                        g: style.foreground.g,
                        b: style.foreground.b,
                    };
                    queue!(stdout, SetForegroundColor(fg))?;

                    let text_chars = text.chars().count();

                    // Logique de troncature si le texte dépasse le bord droit
                    if current_width + text_chars > cols as usize {
                        let allowed_space = (cols as usize).saturating_sub(current_width + 1);
                        if allowed_space > 0 {
                            let trunc: String = text.chars().take(allowed_space).collect();
                            queue!(stdout, Print(trunc))?;
                        }
                        // On indique que la ligne est coupée
                        queue!(stdout, SetForegroundColor(Color::DarkGrey), Print("…"))?;
                        break;
                    } else {
                        queue!(stdout, Print(text))?;
                        current_width += text_chars;
                    }
                }
            }
        }

        stdout.flush()?;

        // 5. Écoute du clavier
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    if scroll_offset + max_visible_rows < total_lines {
                        scroll_offset += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll_offset = scroll_offset.saturating_sub(1);
                }
                KeyCode::Char('d') => {
                    scroll_offset = (scroll_offset + max_visible_rows / 2)
                        .min(total_lines.saturating_sub(max_visible_rows));
                }
                KeyCode::Char('u') => {
                    scroll_offset = scroll_offset.saturating_sub(max_visible_rows / 2);
                }
                _ => {}
            }
        }
    }
    Ok(())
}
