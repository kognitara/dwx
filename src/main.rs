mod actions;
mod app;
mod cmds;
mod crypto;
mod storage;
mod tabs;
mod ui;
mod ux;
mod views;
mod workspaces;
use crate::app::App;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "dwx",
    version,
    about = "Pager terminal minimaliste zéro bordure"
)]
pub struct Cli {
    /// Le ou les fichiers à ouvrir dans les workspaces
    #[arg(required = false, num_args = 1..)]
    pub files: Vec<PathBuf>,
}
fn main() {
    // 1. On parse les arguments (qui ne vont plus crasher si vide)
    let cli = Cli::parse();

    let mut app = App::default();
    app.add_workspaces();

    // 2. On traite le pipe S'IL Y EN A UN
    app.add_stdin();

    // 3. On traite les fichiers s'ils ont été fournis en arguments
    for path in cli.files {
        if path.is_file() {
            app.add_file(&path);
        }
    }

    // 4. On charge les onglets
    let hashes: Vec<String> = app.buffers.keys().cloned().collect();
    if let Some(workspace) = app.workspaces.get_mut(0)
        && let Some(view) = App::find_active_view_mut(&mut workspace.root)
    {
        for hash in hashes {
            view.tabs.push(hash);
        }
    }
    // 5. Go !
    app.make().run();
}
