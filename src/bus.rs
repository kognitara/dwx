use crate::tree::FileItem;
use std::path::PathBuf;
/// Les événements qui remontent vers l'UI
pub enum WorkerEvent {
    SearchResultBatch {
        items: Vec<FileItem>,
        search_id: usize,
    },
}
/// Les ordres qui descendent vers l'Inspecteur
pub enum InspectorCommand {
    Stop,
    DeepSearch {
        query: String,
        dir: PathBuf,
        search_id: usize,
    },
    MountSshfs {
        connection_string: String, // ex: "user@host:/chemin/distant"
        mount_point: PathBuf,      // ex: "/tmp/dwx_ssh_mnt"
    },
}
