use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    terminal::{
        Clear, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    },
};
use std::io;
use std::io::stdout;
use std::time::Duration;

pub mod bus;
pub mod editor;
pub mod tree; // Ton nouveau MillerState tout propre
pub mod ui;
pub mod workspaces; // Ton Workspace // Là où tu as ta fonction draw_ui
use workspaces::{AppMode, Workspace};

use crate::tree::FileItem;

fn load(workspace: &mut Workspace) -> FileItem {
    workspace
        .miller
        .current_entries
        .get(workspace.miller.selected_index)
        .expect("no file")
        .clone()
}

fn main() -> io::Result<()> {
    // 1. Initialisation du Terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, Hide, EnterAlternateScreen)?;

    // 2. Initialisation de l'État (Dossier courant par défaut)
    let start_dir = std::env::current_dir().unwrap_or_else(|_| dirs::home_dir().expect("home"));
    let mut workspace = Workspace::new(start_dir);

    // 3. La Boucle Mushin
    loop {
        // A. On dépile les messages des threads en arrière-plan
        workspace.poll_bus();

        let current_file_item = load(&mut workspace);

        // B. On dessine l'interface
        ui::draw_ui(&mut workspace);

        // C. On écoute le clavier (16ms)
        if event::poll(Duration::from_millis(16))?
            && let Event::Key(key) = event::read()?
        {
            // On ignore les touches relâchées, on ne gère que les pressions
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if workspace.pending_g {
                workspace.pending_g = false;
                let target_dir = match key.code {
                    KeyCode::Char('h') => dirs::home_dir(),
                    KeyCode::Char('x') => dirs::download_dir(),
                    KeyCode::Char('d') => dirs::document_dir(),
                    KeyCode::Char('m') => dirs::audio_dir(),
                    KeyCode::Char('b') => dirs::executable_dir(),
                    KeyCode::Char('c') => dirs::config_dir(),
                    KeyCode::Char('p') => dirs::picture_dir(),
                    KeyCode::Char('v') => dirs::video_dir(),
                    KeyCode::Char('f') => dirs::font_dir(),
                    KeyCode::Char('t') => dirs::template_dir(),
                    KeyCode::Char('j') => dirs::desktop_dir(),
                    KeyCode::Char('r') => Some(std::path::PathBuf::from("/")),
                    _ => None, // Si on tape une autre touche, on annule l'action
                };

                if let Some(new_dir) = target_dir {
                    // Si le dossier existe, on s'y téléporte !
                    if new_dir.exists() && new_dir.is_dir() {
                        workspace.miller.set_dir(new_dir);
                        queue!(stdout, Clear(crossterm::terminal::ClearType::All)).unwrap();
                    }
                }
                continue;
            }
            match workspace.mode {
                AppMode::Normal => {
                    match key.code {
                        KeyCode::F(2) => {
                            if current_file_item.path.is_file() {
                                workspace.mode = AppMode::Omnibar {
                                    prefix: '*',
                                    input_buffer: current_file_item.name,
                                };
                            } else {
                                continue;
                            }
                        }
                        KeyCode::Char('/') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '/',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('f') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '+',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('a') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '+',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('s') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '+',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Enter => {
                            if !current_file_item.path.is_file() {
                                continue;
                            }
                            queue!(stdout, Hide, LeaveAlternateScreen).expect("failed");
                            editor::view_file_with_scroll(current_file_item.path.as_path())
                                .expect("failed");
                            queue!(
                                stdout,
                                Hide,
                                EnterAlternateScreen,
                                Clear(crossterm::terminal::ClearType::All)
                            )
                            .expect("failed");
                            continue;
                        }
                        KeyCode::Char('h') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '#',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('?') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '?',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('p') => {
                            workspace.mode = AppMode::Omnibar {
                                prefix: '+',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('q') => break,
                        KeyCode::Char('j') | KeyCode::Down => {
                            workspace.move_down(20); // Ça bouge le curseur ET met à jour le preview d'un coup !
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            workspace.move_up();
                        }
                        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => {
                            workspace.enter_dir();
                        }
                        KeyCode::Left | KeyCode::Backspace => {
                            workspace.go_parent();
                        }
                        // --- MODES ET DRAPEAUX ---
                        KeyCode::Char('n') => {
                            workspace.pending_create = true;
                        }
                        KeyCode::Char('d') if workspace.pending_create => {
                            workspace.pending_create = false;
                            workspace.pending_create_dir = true;
                            workspace.mode = AppMode::Omnibar {
                                prefix: '+',
                                input_buffer: String::new(),
                            };
                        }
                        KeyCode::Char('g') => {
                            workspace.pending_g = true;
                            continue;
                        }
                        // Rafraîchissement manuel
                        KeyCode::F(5) => workspace.miller.refresh(),
                        _ => {
                            // Si on tape une touche non reconnue, on annule les actions en cours
                            workspace.pending_create = false;
                            workspace.pending_g = false;
                        }
                    }
                }
                AppMode::Omnibar {
                    prefix,
                    ref mut input_buffer,
                } => {
                    match key.code {
                        // Quitter la barre avec Échap
                        KeyCode::Esc => {
                            queue!(stdout, Clear(crossterm::terminal::ClearType::All)).unwrap();
                            workspace.mode = AppMode::Normal;
                            if prefix == '/' {
                                workspace.miller.filter("");
                                workspace.update_preview();
                            } else if prefix == '*' {
                                workspace.miller.refresh();
                            }
                        }

                        // Valider la recherche
                        KeyCode::Enter => {
                            queue!(stdout, Clear(crossterm::terminal::ClearType::All)).unwrap();
                            if prefix == '*' && current_file_item.path.is_file() {
                                workspace
                                    .miller
                                    .rename(current_file_item.name, input_buffer.to_string());
                            }
                            workspace.mode = AppMode::Normal;
                        }
                        // Effacer un caractère
                        KeyCode::Backspace => {
                            input_buffer.pop();
                            if prefix == '/' {
                                workspace.miller.filter(input_buffer.as_str()); // À implémenter plus tard
                            }
                        }
                        // Taper du texte
                        KeyCode::Char(c) => {
                            input_buffer.push(c);
                            // Si on est en mode recherche ('/'), on peut filtrer en temps réel ici !
                            match prefix.to_string().as_str() {
                                "/" => {
                                    queue!(stdout, Clear(crossterm::terminal::ClearType::All))
                                        .unwrap();
                                    workspace.miller.filter(input_buffer.as_str());
                                }
                                "?" => {
                                    queue!(stdout, Clear(crossterm::terminal::ClearType::All))
                                        .unwrap();
                                    workspace.search_id += 1;
                                    // 2. On vide VRAIMENT la liste juste avant de lancer la recherche
                                    workspace.miller.current_entries.clear();
                                    workspace.miller.filtered_indices.clear();
                                    // On envoie l'ordre de recherche au thread !
                                    let dir_to_search = workspace.miller.current_dir.clone();
                                    let _ = workspace.tx_inspector.send(
                                        bus::InspectorCommand::DeepSearch {
                                            query: input_buffer.to_string(),
                                            dir: dir_to_search,
                                            search_id: workspace.search_id,
                                        },
                                    );
                                }
                                _ => {
                                    continue;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    execute!(stdout, Show)?;
    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
