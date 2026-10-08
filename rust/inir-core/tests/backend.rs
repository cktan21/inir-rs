use inir_core::{
    config::ConfigStore,
    events::{Domain, Event},
    runtime::Backend,
    services::system::{parse_os_release, Sampler},
    state::AppState,
};
use inir_types::config::Config;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};

#[test]
fn procfs_units_and_cpu_deltas() {
    let mut sampler = Sampler::default();
    let memory = "MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 200 kB\nSwapFree: 100 kB\n";
    let first = sampler
        .parse("10.5 8.0", memory, "cpu 100 0 0 800 100 0 0 0 100 0")
        .unwrap();
    assert_eq!(first.uptime, 10.5);
    assert_eq!(first.memory_total, 1_024_000);
    assert_eq!(first.memory_available, 409_600);
    assert_eq!(first.cpu_usage, None);
    let second = sampler
        .parse("11.5 9.0", memory, "cpu 200 0 0 850 150 0 0 0 200 0")
        .unwrap();
    assert_eq!(second.cpu_usage, Some(0.5));
    let reset = sampler
        .parse("12.0 9.0", memory, "cpu 1 0 0 1 0 0 0 0")
        .unwrap();
    assert_eq!(reset.cpu_usage, None);
    assert!(sampler.parse("bad", memory, "cpu 1 0 0 1").is_err());
}

#[test]
fn parses_quoted_and_unquoted_os_release() {
    let fields =
        parse_os_release("# comment\nNAME='A distro'\nID=arch\nHOME_URL=\"https://example.org\"\n");
    assert_eq!(fields["NAME"], "A distro");
    assert_eq!(fields["ID"], "arch");
    assert_eq!(fields["HOME_URL"], "https://example.org");
}

#[test]
fn reducer_diffs_domains() {
    let mut state = AppState::default();
    let mut resource = state.resources.clone();
    assert_eq!(state.reduce(Event::Resource(resource.clone())), None);
    resource.memory_total = 1024;
    assert_eq!(
        state.reduce(Event::Resource(resource.clone())),
        Some(Domain::Resource)
    );
    assert_eq!(state.reduce(Event::Resource(resource)), None);
    assert_eq!(state.config, Config::default());
}

#[test]
fn migration_preserves_legacy_keys_and_original_backup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let original = br#"{"appearance":{"custom":{"colors":["red"]}},"bar":{"height":40},"performance":{"lowPower":false,"custom":12}}"#;
    fs::write(&path, original).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let store = ConfigStore::new(&path).unwrap();
    let config = store.load_and_migrate().unwrap();
    assert_eq!(config.schema_version, 1);
    assert_eq!(
        fs::metadata(path.with_extension("json.bak"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read(path.with_extension("json.bak")).unwrap(), original);
    store.set("performance.lowPower", json!(true)).unwrap();
    let value = serde_json::to_value(store.load_and_migrate().unwrap()).unwrap();
    assert_eq!(value["bar"]["height"], 40);
    assert_eq!(value["appearance"]["custom"]["colors"], json!(["red"]));
    assert_eq!(value["performance"]["custom"], 12);
    assert_eq!(value["performance"]["lowPower"], true);
    assert_eq!(fs::read(path.with_extension("json.bak")).unwrap(), original);
}

#[test]
fn invalid_or_future_configuration_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let store = ConfigStore::new(&path).unwrap();
    for bytes in [
        "{",
        "",
        r#"{"schema_version":99}"#,
        r#"{"performance":{"lowPower":"yes"}}"#,
    ] {
        fs::write(&path, bytes).unwrap();
        assert!(store.load_and_migrate().is_err());
        assert!(store.set("performance.lowPower", json!(true)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
    }
    assert!(!path.with_extension("json.bak").exists());
}

#[test]
fn validation_does_not_persist_a_bad_patch() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let store = ConfigStore::new(&path).unwrap();
    store.set("performance.blurBackend", json!("auto")).unwrap();
    let original = fs::read(&path).unwrap();
    for (key, value) in [
        ("performance.lowPower", json!("yes")),
        ("performance.blurBackend", json!("bogus")),
        ("schema_version", json!(99)),
        (".x", json!(0)),
        ("performance.blurBackend.x", json!(true)),
    ] {
        assert!(store.set(key, value).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}

#[test]
fn config_child() {
    let Ok(path) = std::env::var("INIR_TEST_CONFIG_PATH") else {
        return;
    };
    let child = std::env::var("INIR_TEST_CONFIG_CHILD").unwrap();
    let store = ConfigStore::new(path).unwrap();
    for index in 0..20 {
        store
            .set(
                &format!("extension.worker{child}.field{index}"),
                json!(index),
            )
            .unwrap();
    }
}

#[test]
fn independent_process_writers_do_not_lose_updates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let mut children = Vec::new();
    for child in 0..4 {
        children.push(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "config_child"])
                .env("INIR_TEST_CONFIG_PATH", &path)
                .env("INIR_TEST_CONFIG_CHILD", child.to_string())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let value = serde_json::to_value(ConfigStore::new(path).unwrap().read().unwrap()).unwrap();
    for child in 0..4 {
        for index in 0..20 {
            assert_eq!(
                value["extension"][format!("worker{child}")][format!("field{index}")],
                index
            );
        }
    }
}

#[tokio::test]
async fn watcher_survives_atomic_replacements() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let store = ConfigStore::new(&path).unwrap();
    store.set("performance.lowPower", json!(false)).unwrap();
    let (_watcher, mut changes) = store.watch().unwrap();
    for value in [true, false] {
        let replacement = directory.path().join("replacement.json");
        fs::write(
            &replacement,
            json!({"schema_version": 1, "performance": {"lowPower": value}}).to_string(),
        )
        .unwrap();
        fs::rename(replacement, &path).unwrap();
        tokio::time::timeout(Duration::from_secs(2), changes.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store.read().unwrap().performance.unwrap().low_power,
            Some(value)
        );
        while changes.try_recv().is_ok() {}
    }
}

#[tokio::test]
async fn resource_sampling_stops_without_consumers() {
    let backend = Backend::new().unwrap();
    let mut resources = backend.resources.clone();
    tokio::time::timeout(Duration::from_secs(3), resources.changed())
        .await
        .unwrap()
        .unwrap();
    resources.borrow_and_update();
    assert!(
        tokio::time::timeout(Duration::from_millis(1100), resources.changed())
            .await
            .is_err()
    );
    let first = backend.resource_consumer();
    let second = backend.resource_consumer();
    assert_eq!(backend.resource_consumers(), 2);
    tokio::time::timeout(Duration::from_secs(3), resources.changed())
        .await
        .unwrap()
        .unwrap();
    drop(first);
    assert_eq!(backend.resource_consumers(), 1);
    drop(second);
    assert_eq!(backend.resource_consumers(), 0);
    tokio::time::sleep(Duration::from_millis(150)).await;
    resources.borrow_and_update();
    assert!(
        tokio::time::timeout(Duration::from_millis(1100), resources.changed())
            .await
            .is_err()
    );
}
