use std::fs;
use std::path::{Path, PathBuf};

use super::transaction::{
    begin_install, commit_install, lock_install, recover_install, StagingDir,
};

const SHADERS: [(&str, &[u8]); 2] = [
    (
        "shaders/anime4k/Anime4K_Upscale_CNN_x2_M.glsl",
        include_bytes!("../../shaders/anime4k/Anime4K_Upscale_CNN_x2_M.glsl"),
    ),
    (
        "shaders/artcnn/ArtCNN_C4F16.glsl",
        include_bytes!("../../shaders/artcnn/ArtCNN_C4F16.glsl"),
    ),
];

/// Refresh missing, damaged, or outdated app-managed shaders on every launch.
pub fn ensure_universal_shaders(lib_dir: &Path) -> Result<(), String> {
    let _lock = lock_install(lib_dir)?;
    let changed: Vec<_> = SHADERS
        .iter()
        .filter(|(path, contents)| fs::read(lib_dir.join(path)).ok().as_deref() != Some(*contents))
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    let staging = StagingDir::new(lib_dir)?;
    let mut files: Vec<PathBuf> = Vec::new();
    for (path, contents) in changed {
        let dest = staging.0.join(path);
        fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(dest, contents).map_err(|e| format!("Cannot stage shader {path}: {e}"))?;
        files.push(path.into());
    }
    begin_install(lib_dir, &staging.0, &files)?;
    if let Err(error) = commit_install(lib_dir) {
        recover_install(lib_dir)?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_stale_shaders_and_skips_unchanged_files() {
        let root = StagingDir::new(&std::env::temp_dir()).unwrap();
        ensure_universal_shaders(&root.0).unwrap();
        let path = root.0.join(SHADERS[0].0);
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        ensure_universal_shaders(&root.0).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        fs::write(&path, b"stale").unwrap();
        ensure_universal_shaders(&root.0).unwrap();
        assert_eq!(fs::read(&path).unwrap(), SHADERS[0].1);
    }
}
