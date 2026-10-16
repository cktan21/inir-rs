use fs2::FileExt;
use serde_json::{json, Value};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

pub fn read_many(paths: &[String]) -> Result<Value, String> {
    if paths.len() > 4096 {
        return Err("Too many files in one request".into());
    }
    Ok(Value::Array(
        paths
            .iter()
            .map(|path| match fs::read_to_string(path) {
                Ok(text) => json!({"path": path, "text": text, "error": ""}),
                Err(error) => json!({"path": path, "text": null, "error": error.to_string()}),
            })
            .collect(),
    ))
}

/// Keep a legitimate existing symlink intact and replace its resolved target.
/// A dangling symlink is an error, not permission to overwrite the link itself.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), String> {
    let target = if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::canonicalize(path).map_err(|e| e.to_string())?
    } else {
        path.to_path_buf()
    };
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = target
        .file_name()
        .ok_or("Invalid file name")?
        .to_string_lossy();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent.join(format!(".{name}.inir-lock")))
        .map_err(|e| e.to_string())?;
    lock.lock_exclusive().map_err(|e| e.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    if let Ok(metadata) = fs::metadata(&target) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|e| e.to_string())?;
    }
    temporary.write_all(content).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(&target).map_err(|e| e.to_string())?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_write_preserves_symlinks_and_modes() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        atomic_write(&link, b"new").unwrap();
        assert!(link.is_symlink());
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let dangling = dir.path().join("dangling");
        symlink("missing", &dangling).unwrap();
        assert!(atomic_write(&dangling, b"no").is_err());
        assert!(dangling.is_symlink());
    }
}
