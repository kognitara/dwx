use std::fs::File;
use std::io;
use std::path::PathBuf;

/// Génère un nom de fichier basé sur le hash BLAKE3 de son contenu,
/// tout en conservant son extension d'origine.
pub fn hash(path: &PathBuf) -> String {
    // 1. Ouverture du fichier en lecture seule
    let mut file = File::open(path).expect("no files");

    // 2. Initialisation du hasher BLAKE3
    let mut hasher = blake3::Hasher::new();

    // 3. Streaming direct du fichier vers le hasher (zéro allocation massive)
    io::copy(&mut file, &mut hasher).expect("failed to get the hash");

    // 4. Finalisation du hachage et conversion en chaîne hexadécimale
    let hash_hex = hasher.finalize().to_hex();

    // 6. Construction et retour du nom final
    format!("{}.dwx", &hash_hex[0..7])
}
