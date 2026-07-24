use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::fs;
use std::fs::File;
use std::os::unix::fs::symlink;
use std::path::Path;
// Spécifique aux environnements Unix
use std::path::PathBuf;
use tar::Archive;
use tar::Builder;

use crate::workspaces::Workspace;
pub mod split;
// --- CRÉATION DE LIENS ---

pub fn create_hardlink(original: &PathBuf, link_target: &PathBuf) -> std::io::Result<()> {
    // Crée un lien physique (pointe vers le même inode)
    fs::hard_link(original, link_target)
}

pub fn create_symlink(original: &PathBuf, link_target: &PathBuf) -> std::io::Result<()> {
    // Crée un lien symbolique (raccourci)
    symlink(original, link_target)
}

// --- LECTURE DE LA CIBLE D'UN LIEN ---
// Très utile pour afficher "lien -> destination" dans ton interface
pub fn read_symlink_target(link_path: &PathBuf) -> Option<PathBuf> {
    fs::read_link(link_path).ok()
}

pub fn extract_tar_gz(archive_path: &PathBuf, dest_dir: &PathBuf) -> std::io::Result<()> {
    // 1. On ouvre le fichier compressé
    let tar_gz = File::open(archive_path)?;

    // 2. On le décompresse (flate2)
    let tar = GzDecoder::new(tar_gz);

    // 3. On lit l'archive (tar) et on l'extrait dans le dossier cible
    let mut archive = Archive::new(tar);
    archive.unpack(dest_dir)?;

    Ok(())
}

pub fn create_tar_gz_archive(workspace: &mut Workspace, name: String) -> std::io::Result<()> {
    workspace.pending_create_archive = false;
    let tar_gz = File::create("/tmp/dwx.tar.gz")?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut builder = Builder::new(enc);

    // 1. On stocke une référence au dossier de base pour le Borrow Checker et pour calculer les chemins relatifs
    let base_dir = &workspace.miller.current_dir;

    let walk = ignore::WalkBuilder::new(base_dir)
        .threads(4)
        .add_custom_ignore_filename(".awqignore")
        .add_custom_ignore_filename(".hgignore")
        .add_custom_ignore_filename(".gitignore")
        .add_custom_ignore_filename(".dockerignore")
        .standard_filters(true)
        .build();

    for d in walk.flatten() {
        let path = d.path();

        // 2. On calcule le chemin relatif pour l'intérieur de l'archive
        let name_in_archive = match path.strip_prefix(base_dir) {
            Ok(p) => p,
            Err(_) => path, // Fallback de sécurité
        };

        // WalkBuilder renvoie souvent le dossier racine lui-même en premier.
        // Si le chemin relatif est vide, on l'ignore.
        if name_in_archive.as_os_str().is_empty() {
            continue;
        }

        // 3. On ajoute chaque élément individuellement (AUCUNE récursion via append_dir_all)
        if path.is_dir() {
            // Ajoute uniquement l'entrée du dossier (utile pour garder les dossiers vides)
            builder.append_dir(name_in_archive, path)?;
        } else if path.is_file() {
            // Ouvre le fichier et l'ajoute avec son chemin relatif
            let mut f = File::open(path)?;
            builder.append_file(name_in_archive, &mut f)?;
        }
    }

    builder.into_inner()?;
    if Path::new("/tmp/dwx.tar.gz").is_file() {
        let _ = std::fs::copy(
            "/tmp/dwx.tar.gz",
            base_dir.join(name).with_added_extension("tar.gz"),
        );
        let _ = fs::remove_file("/tmp/dwx.tar.gz");
    }

    workspace.miller.refresh();
    Ok(())
}

/// Lit une archive .tar.gz et retourne la liste des fichiers qu'elle contient
pub fn list_tar_gz_contents<P: AsRef<Path>>(archive_path: P) -> std::io::Result<Vec<String>> {
    // 1. On ouvre le fichier en lecture seule
    let tar_gz = File::open(archive_path)?;

    // 2. On le passe dans un décodeur GZIP (l'inverse de GzEncoder)
    let tar = GzDecoder::new(tar_gz);

    // 3. On charge le flux décompressé dans l'Archive tar
    let mut archive = Archive::new(tar);

    let mut file_list = Vec::new();

    // 4. On parcourt les entrées une par une (sans rien écrire sur le disque)
    for entry in archive.entries()? {
        let entry = entry?;

        // On extrait le chemin virtuel du fichier dans l'archive
        let path = entry.path()?;

        // On le convertit en String et on l'ajoute à notre liste
        file_list.push(path.to_string_lossy().into_owned());

        // Bonus : tu peux aussi récupérer la taille si besoin pour ton UI
        // let size = entry.header().size()?;
    }
    Ok(file_list)
}
