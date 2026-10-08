use fs2::FileExt;
use inir_types::config::{Config, SCHEMA_VERSION};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use tokio::sync::mpsc;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let path = if path.exists() {
            fs::canonicalize(path)?
        } else if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(path)
        };
        if path.file_name().is_none() {
            return Err(invalid("configuration must name a file"));
        }
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The lock lives beside the file, never on the inode replaced by rename.
    fn lock(&self) -> io::Result<File> {
        fs::create_dir_all(
            self.path
                .parent()
                .ok_or_else(|| invalid("missing config directory"))?,
        )?;
        let file = OpenOptions::new()
            .create(true)
            .mode(0o600)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.path.with_extension("json.lock"))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn read_unlocked(&self) -> io::Result<Config> {
        let config = match fs::read(&self.path) {
            Ok(bytes) => {
                serde_json::from_slice::<Config>(&bytes).map_err(|err| invalid(err.to_string()))?
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Config {
                schema_version: SCHEMA_VERSION,
                ..Config::default()
            },
            Err(err) => return Err(err),
        };
        config.validate().map_err(invalid)?;
        Ok(config)
    }

    pub fn read(&self) -> io::Result<Config> {
        self.read_unlocked()
    }

    /// Migrate under the same lock as edits and save an exact, durable backup.
    /// Invalid and future-version files are never rewritten.
    pub fn load_and_migrate(&self) -> io::Result<Config> {
        let _lock = self.lock()?;
        self.migrate_unlocked()
    }

    fn migrate_unlocked(&self) -> io::Result<Config> {
        let mut config = self.read_unlocked()?;
        if config.schema_version < SCHEMA_VERSION {
            let bytes = fs::read(&self.path)?;
            let backup_path = self.path.with_extension("json.bak");
            match OpenOptions::new()
                .create_new(true)
                .mode(0o600)
                .write(true)
                .open(backup_path)
            {
                Ok(mut backup) => {
                    backup.write_all(&bytes)?;
                    backup.sync_all()?;
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
                Err(err) => return Err(err),
            }
            while config.schema_version < SCHEMA_VERSION {
                match config.schema_version {
                    // v0 had no schema marker. All legacy keys retain their names.
                    0 => config.schema_version = 1,
                    version => return Err(invalid(format!("no migration from schema {version}"))),
                }
            }
            self.write_unlocked(&config)?;
        }
        Ok(config)
    }

    fn write_unlocked(&self, config: &Config) -> io::Result<()> {
        config.validate().map_err(invalid)?;
        let directory = self
            .path
            .parent()
            .ok_or_else(|| invalid("missing config directory"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        // New files use tempfile's private permissions; existing modes survive.
        if let Ok(metadata) = fs::metadata(&self.path) {
            temporary
                .as_file()
                .set_permissions(metadata.permissions())?;
        }
        serde_json::to_writer_pretty(&mut temporary, config)
            .map_err(|err| invalid(err.to_string()))?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|err| err.error)?;
        File::open(directory)?.sync_all()
    }

    /// Reload inside the advisory lock before applying a patch. Two settings
    /// processes changing different fields cannot overwrite each other's edits.
    pub fn set(&self, key: &str, value: Value) -> io::Result<Config> {
        let _lock = self.lock()?;
        let mut document = serde_json::to_value(self.migrate_unlocked()?)
            .map_err(|err| invalid(err.to_string()))?;
        let keys: Vec<_> = key.split('.').collect();
        if keys.iter().any(|key| key.is_empty()) || keys[0] == "schema_version" {
            return Err(invalid("invalid setting key"));
        }
        let mut current = &mut document;
        for key in &keys[..keys.len() - 1] {
            let object = current
                .as_object_mut()
                .ok_or_else(|| invalid("setting path crosses a scalar value"))?;
            current = object
                .entry((*key).to_owned())
                .or_insert_with(|| serde_json::json!({}));
        }
        current
            .as_object_mut()
            .ok_or_else(|| invalid("setting parent is not an object"))?
            .insert(keys[keys.len() - 1].to_owned(), value);
        let config: Config =
            serde_json::from_value(document).map_err(|err| invalid(err.to_string()))?;
        config.validate().map_err(invalid)?;
        self.write_unlocked(&config)?;
        Ok(config)
    }

    /// Watch the parent directory: watching only the file loses atomic renames.
    /// The bounded channel coalesces editor bursts and our own write events.
    pub fn watch(&self) -> notify::Result<(RecommendedWatcher, mpsc::Receiver<()>)> {
        let (sender, receiver) = mpsc::channel(1);
        let path = self.path.clone();
        let mut watcher = notify::recommended_watcher(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event)
                    if matches!(
                        event.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) && event.paths.contains(&path) =>
                {
                    let _ = sender.try_send(());
                }
                Err(err) => tracing::warn!(%err, "configuration watcher failed"),
                _ => {}
            },
        )?;
        watcher.watch(self.path.parent().unwrap(), RecursiveMode::NonRecursive)?;
        Ok((watcher, receiver))
    }
}
