//! Recoverable replacement of dependency files on the same filesystem.
use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

const TRANSACTION_DIR: &str = ".dependency-transaction";

/// OS locks are released even if the process exits during an update.
pub(super) fn lock_install(lib_dir: &Path) -> Result<File, String> {
    fs::create_dir_all(lib_dir).map_err(|e| e.to_string())?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lib_dir.join(".dependencies.lock"))
        .map_err(|e| format!("Cannot open dependency lock: {e}"))?;
    file.try_lock()
        .map_err(|e| format!("Cannot lock dependencies (another instance may be updating): {e}"))?;
    recover_install(lib_dir)?;
    // A process can exit before an install journal exists, leaving only staging
    // downloads. Under the exclusive lock, these cannot belong to a live update.
    let canonical_lib = fs::canonicalize(lib_dir).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(lib_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name
            .strip_prefix(".dependency-stage-")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            if let Ok(path) = fs::canonicalize(entry.path()) {
                if path.parent() == Some(canonical_lib.as_path()) {
                    if let Err(error) = fs::remove_dir_all(&path) {
                        tracing::warn!(
                            "[Deps]: Cannot remove abandoned staging directory {}: {}",
                            path.display(),
                            error
                        );
                    }
                }
            }
        }
    }
    Ok(file)
}

pub(super) struct StagingDir(pub PathBuf);

impl StagingDir {
    pub(super) fn new(lib_dir: &Path) -> Result<Self, String> {
        let path = lib_dir.join(format!(".dependency-stage-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).map_err(|e| format!("Cannot create dependency staging area: {e}"))?;
        Ok(Self(path))
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    existed: bool,
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Caller holds the install lock. The journal is durable before any live file moves.
pub(super) fn begin_install(
    lib_dir: &Path,
    staged: &Path,
    files: &[PathBuf],
) -> Result<(), String> {
    if files.is_empty() || files.iter().any(|path| !safe_relative(path)) {
        return Err("Invalid dependency installation file list".into());
    }
    let entries: Vec<_> = files
        .iter()
        .map(|path| Entry {
            path: path.clone(),
            existed: lib_dir.join(path).exists(),
        })
        .collect();
    let transaction = lib_dir.join(TRANSACTION_DIR);
    fs::create_dir(&transaction)
        .map_err(|e| format!("Cannot start dependency transaction: {e}"))?;
    let result = (|| -> Result<(), String> {
        let mut journal =
            File::create(transaction.join("journal.tmp")).map_err(|e| e.to_string())?;
        journal
            .write_all(&serde_json::to_vec(&entries).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        journal.sync_all().map_err(|e| e.to_string())?;
        drop(journal);
        fs::rename(
            transaction.join("journal.tmp"),
            transaction.join("journal.json"),
        )
        .map_err(|e| e.to_string())?;
        for entry in &entries {
            let dest = lib_dir.join(&entry.path);
            let backup = transaction.join("backup").join(&entry.path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if entry.existed {
                fs::create_dir_all(backup.parent().unwrap()).map_err(|e| e.to_string())?;
                fs::rename(&dest, &backup)
                    .map_err(|e| format!("Cannot back up {}: {e}", dest.display()))?;
            }
            fs::rename(staged.join(&entry.path), &dest)
                .map_err(|e| format!("Cannot install {}: {e}", dest.display()))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        // A failed journal write cannot have moved files yet.
        if serde_json::from_slice::<Vec<Entry>>(
            &fs::read(transaction.join("journal.json")).unwrap_or_default(),
        )
        .is_err()
        {
            let _ = fs::remove_dir_all(&transaction);
            return Err(error);
        }
        return match recover_install(lib_dir) {
            Ok(()) => Err(error),
            Err(recovery) => Err(format!("{error}; rollback failed: {recovery}")),
        };
    }
    Ok(())
}

/// Mark success before discarding backups; a crash during cleanup must not roll back.
pub(super) fn commit_install(lib_dir: &Path) -> Result<(), String> {
    let transaction = lib_dir.join(TRANSACTION_DIR);
    File::create(transaction.join("committed"))
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("Cannot commit dependency installation: {e}"))?;
    let _ = fs::remove_dir_all(transaction);
    Ok(())
}

/// Restores interrupted installations. Each restore is itself safe to repeat.
pub(super) fn recover_install(lib_dir: &Path) -> Result<(), String> {
    let transaction = lib_dir.join(TRANSACTION_DIR);
    if !transaction.exists() {
        return Ok(());
    }
    if !transaction.join("committed").exists() {
        let journal_path = transaction.join("journal.json");
        if journal_path.exists() {
            let entries: Vec<Entry> =
                serde_json::from_slice(&fs::read(&journal_path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Invalid dependency recovery journal: {e}"))?;
            if entries.iter().any(|entry| !safe_relative(&entry.path)) {
                return Err("Unsafe path in dependency recovery journal".into());
            }
            for entry in entries.iter().rev() {
                let dest = lib_dir.join(&entry.path);
                let backup = transaction.join("backup").join(&entry.path);
                if backup.exists() {
                    if dest.exists() {
                        fs::remove_file(&dest).map_err(|e| e.to_string())?;
                    }
                    fs::rename(&backup, &dest)
                        .map_err(|e| format!("Cannot restore {}: {e}", dest.display()))?;
                } else if !entry.existed && dest.exists() {
                    fs::remove_file(&dest).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    fs::remove_dir_all(transaction).map_err(|e| format!("Cannot clean dependency transaction: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> StagingDir {
        let root = std::env::temp_dir().join(format!("referee-deps-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        StagingDir(root)
    }

    #[test]
    fn interrupted_install_restores_old_files_and_removes_new_files() {
        let root = workspace();
        let staged = StagingDir::new(&root.0).unwrap();
        fs::write(root.0.join("encoder"), b"old").unwrap();
        fs::write(staged.0.join("encoder"), b"new").unwrap();
        fs::write(staged.0.join("extra"), b"extra").unwrap();
        begin_install(&root.0, &staged.0, &["encoder".into(), "extra".into()]).unwrap();
        recover_install(&root.0).unwrap();
        recover_install(&root.0).unwrap();
        assert_eq!(fs::read(root.0.join("encoder")).unwrap(), b"old");
        assert!(!root.0.join("extra").exists());
    }

    #[test]
    fn failed_install_rolls_back_files_already_moved() {
        let root = workspace();
        let staged = StagingDir::new(&root.0).unwrap();
        fs::write(root.0.join("encoder"), b"old").unwrap();
        fs::write(staged.0.join("encoder"), b"new").unwrap();
        assert!(begin_install(&root.0, &staged.0, &["encoder".into(), "missing".into()]).is_err());
        assert_eq!(fs::read(root.0.join("encoder")).unwrap(), b"old");
    }

    #[test]
    fn committed_install_survives_recovery() {
        let root = workspace();
        let staged = StagingDir::new(&root.0).unwrap();
        fs::write(root.0.join("encoder"), b"old").unwrap();
        fs::write(staged.0.join("encoder"), b"new").unwrap();
        begin_install(&root.0, &staged.0, &["encoder".into()]).unwrap();
        commit_install(&root.0).unwrap();
        recover_install(&root.0).unwrap();
        assert_eq!(fs::read(root.0.join("encoder")).unwrap(), b"new");
    }

    #[test]
    fn rejects_paths_outside_library() {
        let root = workspace();
        assert!(begin_install(&root.0, &root.0, &["../encoder".into()]).is_err());
    }

    #[test]
    fn prevents_concurrent_updaters_and_releases_lock_on_drop() {
        let root = workspace();
        let lock = lock_install(&root.0).unwrap();
        assert!(lock_install(&root.0).is_err());
        drop(lock);
        assert!(lock_install(&root.0).is_ok());
    }

    #[test]
    fn removes_abandoned_staging_downloads_under_lock() {
        let root = workspace();
        let staging = StagingDir::new(&root.0).unwrap();
        fs::write(staging.0.join("partial.archive"), b"partial").unwrap();
        let _lock = lock_install(&root.0).unwrap();
        assert!(!staging.0.exists());
    }
}
