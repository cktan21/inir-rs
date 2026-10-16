//! Opt-in development watcher. Compiled utility only; never active in production.
use notify::{EventKind, RecursiveMode, Watcher};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    os::fd::{FromRawFd, OwnedFd},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

pub fn scanner_reach(root: &Path) -> Vec<PathBuf> {
    fn scan_dir(
        root: &Path,
        directory: PathBuf,
        dirs: &mut HashSet<PathBuf>,
        files: &mut HashSet<PathBuf>,
    ) {
        let Ok(directory) = fs::canonicalize(directory) else {
            return;
        };
        if !dirs.insert(directory.clone()) {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        paths.sort();
        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy();
            if name.ends_with(".qml")
                && name.chars().next().is_some_and(char::is_uppercase)
                && path.is_file()
            {
                scan_file(root, path, dirs, files);
            }
        }
    }
    fn scan_file(
        root: &Path,
        path: PathBuf,
        dirs: &mut HashSet<PathBuf>,
        files: &mut HashSet<PathBuf>,
    ) {
        static IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w.]+").unwrap());
        static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"\n]+)""#).unwrap());
        if !files.insert(path.clone()) {
            return;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            return;
        };
        let directory = path.parent().unwrap();
        let mut imports = Vec::new();
        for line in text.lines().map(str::trim) {
            if line.starts_with("import") {
                if let Some(index) = line.find(" qs.") {
                    if let Some(name) = IMPORT.find(&line[index + 4..]) {
                        imports.push(root.join(name.as_str().replace('.', "/")));
                    }
                } else if let Some(capture) = QUOTED.captures(line) {
                    let reference = &capture[1];
                    imports.push(if let Some(path) = reference.strip_prefix("root:") {
                        root.join(path.trim_start_matches('/'))
                    } else {
                        directory.join(reference)
                    });
                }
            } else if line.contains('{') {
                break;
            }
        }
        scan_dir(root, directory.to_owned(), dirs, files);
        for target in imports {
            if target.is_dir() {
                scan_dir(root, target, dirs, files);
            }
        }
    }
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
    let mut dirs = HashSet::new();
    let mut files = HashSet::new();
    scan_file(&root, root.join("shell.qml"), &mut dirs, &mut files);
    for directory in dirs {
        if let Ok(entries) = fs::read_dir(directory) {
            files.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "js")),
            );
        }
    }
    let mut result: Vec<_> = files.into_iter().collect();
    result.sort();
    result
}

fn digest(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let mut hash = DefaultHasher::new();
    bytes.hash(&mut hash);
    Some(hash.finish())
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn reloaded_since(pid: u32, since: f64) -> bool {
    static LINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d)(\.\d+)\s+INFO:\s+Reloading configuration")
            .unwrap()
    });
    let Ok(log) = fs::read_to_string(format!(
        "/run/user/{}/quickshell/by-pid/{pid}/log.log",
        unsafe { libc::getuid() }
    )) else {
        return false;
    };
    for line in log.lines().rev().take(400) {
        let Some(capture) = LINE.captures(line) else {
            continue;
        };
        let Ok(date) = std::ffi::CString::new(capture[1].to_owned()) else {
            continue;
        };
        let mut tm = unsafe { std::mem::zeroed::<libc::tm>() };
        tm.tm_isdst = -1;
        let parsed =
            unsafe { libc::strptime(date.as_ptr(), c"%Y-%m-%d %H:%M:%S".as_ptr(), &mut tm) };
        if parsed.is_null() {
            continue;
        }
        let timestamp =
            unsafe { libc::mktime(&mut tm) } as f64 + capture[2].parse::<f64>().unwrap_or_default();
        return timestamp >= since - 0.05;
    }
    false
}

async fn parent_exit(pid: u32) {
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    if fd >= 0 {
        let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        if let Ok(fd) = tokio::io::unix::AsyncFd::new(fd) {
            let _ = fd.readable().await;
            return;
        }
    }
    loop {
        if unsafe { libc::kill(pid as i32, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub async fn watch(root: PathBuf, pid: u32, stop: &CancellationToken) -> Result<(), String> {
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let changed = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));
    let wake = Arc::new(tokio::sync::Notify::new());
    let queue = changed.clone();
    let signal = wake.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else {
            return;
        };
        if !matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Modify(notify::event::ModifyKind::Name(_))
                | EventKind::Access(notify::event::AccessKind::Close(
                    notify::event::AccessMode::Write
                ))
        ) {
            return;
        }
        let paths = event.paths.into_iter().filter(|path| {
            !path.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some(".git" | "node_modules" | "prebuilt" | "native")
                )
            }) && (path.file_name().is_some_and(|name| name == "qmldir")
                || path
                    .extension()
                    .is_some_and(|ext| ext == "qml" || ext == "js"))
        });
        queue.lock().unwrap().extend(paths);
        signal.notify_one();
    })
    .map_err(|error| error.to_string())?;
    for tree in ["modules", "services"] {
        watcher
            .watch(&root.join(tree), RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;
    }
    let mut reach: HashSet<_> = scanner_reach(&root).into_iter().collect();
    let mut hashes: HashMap<_, _> = reach
        .iter()
        .filter_map(|path| digest(path).map(|hash| (path.clone(), hash)))
        .collect();
    let mut pending: Option<Instant> = None;
    let mut watched: Option<(Instant, f64)> = None;
    let parent = parent_exit(pid);
    tokio::pin!(parent);
    loop {
        let due = pending
            .or(watched.map(|(at, _)| at))
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(86400));
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            _ = &mut parent => return Ok(()),
            _ = wake.notified() => {
                let paths = std::mem::take(&mut *changed.lock().unwrap());
                for path in paths {
                    let Some(hash) = digest(&path) else { continue; };
                    if hashes.insert(path.clone(), hash) == Some(hash) { continue; }
                    if reach.contains(&path) { watched.get_or_insert((Instant::now() + Duration::from_millis(1200), now())); }
                    else { pending = Some(Instant::now() + Duration::from_millis(350)); }
                }
            }
            _ = tokio::time::sleep_until(due.into()) => {
                if pending.is_none() && watched.is_none() { continue; }
                let reload = pending.is_some() || watched.is_some_and(|(_, since)| !reloaded_since(pid, since));
                pending = None; watched = None;
                if reload {
                    let command = tokio::process::Command::new("qs").arg("-p").arg(&root).args(["ipc", "call", "dev", "reload"])
                        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).kill_on_drop(true).status();
                    // The parent/stop branches also cover the cooldown and IPC command.
                    tokio::select! {
                        _ = stop.cancelled() => return Ok(()),
                        _ = &mut parent => return Ok(()),
                        _ = tokio::time::timeout(Duration::from_secs(10), command) => {},
                    }
                }
                tokio::select! {
                    _ = stop.cancelled() => return Ok(()),
                    _ = &mut parent => return Ok(()),
                    _ = tokio::time::sleep(Duration::from_millis(2500)) => {},
                }
                reach = scanner_reach(&root).into_iter().collect();
            }
        }
    }
}

pub fn reach_json(root: &Path) -> Value {
    json!(scanner_reach(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reach_respects_imports_and_does_not_follow_loader_sources() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("services")).unwrap();
        fs::create_dir_all(root.join("modules/lazy")).unwrap();
        fs::write(root.join("shell.qml"), "import qs.services\nQtObject {}\n").unwrap();
        fs::write(
            root.join("services/Used.qml"),
            "Item { source: \"../modules/lazy/Unused.qml\" }",
        )
        .unwrap();
        fs::write(root.join("services/helper.js"), "const value = 1").unwrap();
        fs::write(root.join("modules/lazy/Unused.qml"), "Item {}").unwrap();
        let paths = scanner_reach(root);
        assert!(paths.contains(&root.join("services/Used.qml")));
        assert!(paths.contains(&root.join("services/helper.js")));
        assert!(!paths.contains(&root.join("modules/lazy/Unused.qml")));
    }

    #[test]
    fn quoted_parent_imports_cannot_recurse_forever() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("services")).unwrap();
        fs::write(root.join("shell.qml"), "import qs.services\nQtObject {}\n").unwrap();
        fs::write(
            root.join("services/Parent.qml"),
            "import \"..\"\nQtObject {}\n",
        )
        .unwrap();
        assert_eq!(scanner_reach(root).len(), 2);
    }
}
