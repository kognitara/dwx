mod actions;
mod app;
mod cmds;
mod crypto;
mod storage;
mod tabs;
mod tree;
mod ui;
mod ux;
mod views;
mod workspaces;

use crate::{app::App, tree::MillerState};
use clap::Parser;
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::{env::current_dir, io::stdout, path::PathBuf, process::ExitCode};

#[derive(Parser, Debug)]
#[command(name = "dwx", version, about = "Data Walk eXtended")]
pub struct Cli {
    /// Le ou les fichiers à ouvrir dans les workspaces
    #[arg(required = false, num_args = 1..)]
    pub files: Vec<PathBuf>,
}

fn main() -> ExitCode {
    // 1. On parse les arguments
    let cli = Cli::parse();

    if cli.files.is_empty() {
        execute!(stdout(), EnterAlternateScreen, Hide).expect("faiedl to enteralternate screen");
        // On désactive l'écho des touches et on lit le clavier en temps réel (Raw Mode)
        enable_raw_mode().expect("failed to enabled raw mode");
        let mut state = MillerState::new(current_dir().expect("faield to get current dir"));
        let app_result = MillerState::run(&mut state);
        disable_raw_mode().expect("failed to disabled raw mode");
        execute!(stdout(), Show, LeaveAlternateScreen).expect("failed to exit alternate screen");
        if let Err(err) = app_result {
            eprintln!("Erreur critique dans dwx : {}", err);
            return ExitCode::FAILURE;
        } else {
            return ExitCode::SUCCESS;
        }
    }
    let mut app = App::default();
    app.add_workspaces();

    // 2. On traite le pipe S'IL Y EN A UN
    app.add_stdin();
    for path in &cli.files {
        if path.is_file() {
            app.add_file(path);
        }
    }
    let hashes: Vec<String> = app.buffers.keys().cloned().collect();
    if let Some(workspace) = app.workspaces.get_mut(0)
        && let Some(view) = App::find_active_view_mut(&mut workspace.root)
    {
        for hash in &hashes {
            if !view.tabs.contains(hash) {
                view.tabs.push(hash.to_string());
            }
        }
    }
    app.run()
}
