//! `035`: the legacy cache check and its consent move run only while every live hiqlite node of
//! either version is excluded, refuse without panicking, and say what they left.
//!
//! Only the public start API is used, so the same file runs against the published build and
//! shows each defect there: F-126 (a live 0.14 node's cache moved), F-127 (the move before the
//! unclean-stop marker, then a panic), F-128 ("Nothing was changed" beside a created owner lock)
//! and F-130 (a 0.14 snapshot left by an interrupted move, restored without refusal).
//!
//! A live 0.14 node is stood in for by a second process holding the same `flock` 0.14 holds,
//! on `logs/lock.hql` or `logs_cache/lock.hql`. The real 0.14 binary is `035` section 5's
//! harness, not this file.

#![cfg(all(feature = "sqlite", feature = "cache"))]

use hiqlite::{Error, Node, NodeConfig};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

const CONSENT: &str = "HQL_CACHE_LEGACY_MOVE_ASIDE";
const HOLD: &str = "HQL_TEST_HOLD_LOCK";

/// The child half: hold an exclusive `flock` on the named file until killed. A no-op in a
/// normal run.
#[test]
fn child_holds_lock() {
    let Ok(path) = std::env::var(HOLD) else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    fs4::FileExt::try_lock(&file).unwrap();
    println!("LOCKED");
    std::thread::sleep(std::time::Duration::from_secs(60));
}

struct Holder(Child);

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hold(path: &str) -> Holder {
    let mut holder = Holder(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "child_holds_lock",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HOLD, path)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut out = BufReader::new(holder.0.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        // On a failed assertion the holder is dropped, which kills and waits for it.
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the holder exited early"
        );
        if line.contains("LOCKED") {
            return holder;
        }
    }
}

/// Every entry under `dir`: kind, inode, size and bytes.
fn tree(dir: &str) -> BTreeMap<String, (bool, u64, u64, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, (bool, u64, u64, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            #[cfg(unix)]
            let ino = std::os::unix::fs::MetadataExt::ino(&meta);
            #[cfg(not(unix))]
            let ino = 0;
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if meta.is_dir() {
                out.insert(rel, (true, ino, 0, Vec::new()));
                walk(root, &path, out);
            } else {
                out.insert(rel, (false, ino, meta.len(), std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(Path::new(dir), Path::new(dir), &mut out);
    out
}

fn fresh(case: &str) -> String {
    let dir = format!("../target/test_data/upgrade_exclusion/{case}");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What a cleanly stopped hiqlite 0.14 disk cache leaves: WAL files, no marker, snapshots.
fn legacy_cache(dir: &str) {
    std::fs::create_dir_all(format!("{dir}/logs_cache")).unwrap();
    std::fs::write(
        format!("{dir}/logs_cache/00000000000000000001.wal"),
        b"legacy wal",
    )
    .unwrap();
    std::fs::create_dir_all(format!("{dir}/state_machine_cache/snapshots")).unwrap();
    std::fs::write(
        format!("{dir}/state_machine_cache/snapshots/s1"),
        b"legacy snapshot",
    )
    .unwrap();
}

fn config(dir: &str) -> NodeConfig {
    let mut c = NodeConfig {
        node_id: 1,
        nodes: vec![Node {
            id: 1,
            addr_raft: "127.0.0.1:38612".to_string(),
            addr_api: "127.0.0.1:38611".to_string(),
        }],
        data_dir: dir.to_string().into(),
        secret_raft: "SuperSecureSecret1337".to_string(),
        secret_api: "SuperSecureSecret1337".to_string(),
        cache_storage_disk: true,
        ..Default::default()
    };
    c.enc_keys.enc_key_active = "k1".into();
    c.enc_keys.enc_keys = vec![("k1".into(), vec![7u8; 32])];
    c
}

fn set_consent(consent: Option<&str>) {
    // One test, run sequentially, owns this variable.
    unsafe {
        match consent {
            Some(v) => std::env::set_var(CONSENT, v),
            None => std::env::remove_var(CONSENT),
        }
    }
}

/// Start in a task, so a panic in the start is observed as one rather than ending the test.
async fn start(dir: &str) -> Result<Result<hiqlite::Client, Error>, String> {
    let cfg = config(dir);
    tokio::spawn(async move { hiqlite::start_node(cfg).await })
        .await
        .map_err(|join| format!("the start panicked: {join}"))
}

/// The entries a refused start may add: the owner lock, if it was absent (`035` B-3).
fn assert_only_owner_lock_added(
    before: &BTreeMap<String, (bool, u64, u64, Vec<u8>)>,
    after: &BTreeMap<String, (bool, u64, u64, Vec<u8>)>,
) {
    let mut after = after.clone();
    if !before.contains_key("hiqlite-owner.lock") {
        assert!(
            after.remove("hiqlite-owner.lock").is_some(),
            "the owner lock is created"
        );
    }
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "a refused start adds or removes no other entry"
    );
    for (path, entry) in before {
        let other = &after[path];
        // The owner note is diagnostics, but a refused start leaves the previous one alone.
        assert_eq!(entry, other, "{path} changed (kind, inode, size or bytes)");
    }
}

/// The scenarios share the consent variable and the ports, so they run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// F-126: a live 0.14 node holds `logs_cache/lock.hql` (and `logs/lock.hql`). A start with
/// consent must refuse before moving anything, and must not panic.
#[tokio::test(flavor = "multi_thread")]
async fn f126_a_live_node_is_refused_before_the_move() {
    let _serial = SERIAL.lock().await;
    for held in ["logs_cache", "logs"] {
        let dir = fresh(&format!("live-{held}"));
        legacy_cache(&dir);
        std::fs::create_dir_all(format!("{dir}/logs")).unwrap();
        let holder = hold(&format!("{dir}/{held}/lock.hql"));
        let before = tree(&dir);

        set_consent(Some("true"));
        let res = start(&dir).await;
        set_consent(None);
        let res = res.expect("no panic");
        if let Ok(client) = &res {
            let _ = client.shutdown().await;
        }
        let err = res
            .err()
            .unwrap_or_else(|| panic!("{held}: a live node must be refused"));
        assert!(matches!(err, Error::StorageInUse(_)), "{held}: got {err}");
        assert!(
            err.to_string().contains(&format!("{held}/lock.hql")),
            "{err}"
        );
        assert_only_owner_lock_added(&before, &tree(&dir));
        drop(holder);
    }
}

/// F-127: the previous run did not stop cleanly. Without `auto-heal` the marker is a refusal
/// before the move, and an error rather than a panic.
#[cfg(not(feature = "auto-heal"))]
#[tokio::test(flavor = "multi_thread")]
async fn f127_the_unclean_marker_is_refused_before_the_move() {
    let _serial = SERIAL.lock().await;
    let dir = fresh("marker");
    legacy_cache(&dir);
    std::fs::create_dir_all(format!("{dir}/logs")).unwrap();
    std::fs::create_dir_all(format!("{dir}/state_machine")).unwrap();
    std::fs::write(format!("{dir}/state_machine/lock"), b"").unwrap();
    let before = tree(&dir);

    set_consent(Some("true"));
    let res = start(&dir).await;
    set_consent(None);
    let err = res
        .expect("no panic")
        .err()
        .expect("an unclean marker must be refused");
    assert!(matches!(err, Error::Startup(_)), "got {err}");
    assert!(err.to_string().contains("state_machine/lock"), "{err}");
    assert_only_owner_lock_added(&before, &tree(&dir));
}

/// F-128: the refusal says what it created.
#[tokio::test(flavor = "multi_thread")]
async fn f128_a_refusal_names_what_it_created() {
    let _serial = SERIAL.lock().await;
    let dir = fresh("message");
    legacy_cache(&dir);
    let before = tree(&dir);

    set_consent(None);
    let err = start(&dir).await.expect("no panic").err().expect("refused");
    let msg = err.to_string();
    assert!(!msg.contains("Nothing was changed"), "{msg}");
    assert!(msg.contains("hiqlite-owner.lock"), "{msg}");
    assert_only_owner_lock_added(&before, &tree(&dir));

    // A second refusal finds the owner lock, and says so.
    let err = start(&dir).await.expect("no panic").err().expect("refused");
    assert!(err.to_string().contains("already existed"), "{err}");
}

/// F-130: the published build moved `logs_cache` first; a crash between its two renames left
/// the 0.14 snapshots in place and no legacy log beside them.
#[tokio::test(flavor = "multi_thread")]
async fn f130_an_interrupted_published_move_is_refused_then_finished() {
    let _serial = SERIAL.lock().await;
    let dir = fresh("interrupted");
    legacy_cache(&dir);
    std::fs::create_dir_all(format!("{dir}/pre-upgrade-100")).unwrap();
    std::fs::rename(
        format!("{dir}/logs_cache"),
        format!("{dir}/pre-upgrade-100/logs_cache"),
    )
    .unwrap();
    let before = tree(&dir);

    set_consent(None);
    let res = start(&dir).await.expect("no panic");
    if let Ok(client) = &res {
        let _ = client.shutdown().await;
    }
    let err = res
        .err()
        .expect("an interrupted move must not start without consent");
    assert!(err.to_string().contains("pre-upgrade-100"), "{err}");
    assert_only_owner_lock_added(&before, &tree(&dir));

    // With consent, the move is finished into the same directory.
    set_consent(Some("true"));
    let res = start(&dir).await;
    set_consent(None);
    let client = res.expect("no panic").expect("the move completes");
    client.shutdown().await.expect("a clean stop");
    assert_eq!(
        std::fs::read(format!(
            "{dir}/pre-upgrade-100/state_machine_cache/snapshots/s1"
        ))
        .unwrap(),
        b"legacy snapshot"
    );
    // A clean stop removed every WAL lock file.
    assert!(!Path::new(&format!("{dir}/logs/lock.hql")).exists());
    assert!(!Path::new(&format!("{dir}/logs_cache/lock.hql")).exists());
}

/// The ordinary consent move, then a repeated consent, which moves nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn the_consent_move_completes_once() {
    let _serial = SERIAL.lock().await;
    let dir = fresh("move");
    legacy_cache(&dir);
    set_consent(Some("true"));
    let res = start(&dir).await;
    let client = res.expect("no panic").expect("the move completes");
    client.shutdown().await.expect("a clean stop");

    let moved = pre_upgrade_dirs(&dir);
    assert_eq!(moved.len(), 1, "{moved:?}");
    let moved = &moved[0];
    assert!(!moved.ends_with(".partial"), "{moved}");
    assert_eq!(
        std::fs::read(format!("{dir}/{moved}/logs_cache/00000000000000000001.wal")).unwrap(),
        b"legacy wal"
    );
    assert_eq!(
        std::fs::read(format!("{dir}/{moved}/state_machine_cache/snapshots/s1")).unwrap(),
        b"legacy snapshot"
    );
    // Exactly the legacy entries moved: no lock file of this start's is left among them.
    assert!(!Path::new(&format!("{dir}/{moved}/logs_cache/lock.hql")).exists());

    let res = start(&dir).await;
    set_consent(None);
    let client = res.expect("no panic").expect("a marked directory starts");
    client.shutdown().await.expect("a clean stop");
    assert_eq!(
        pre_upgrade_dirs(&dir).len(),
        1,
        "a repeated consent moves nothing more"
    );
}

fn pre_upgrade_dirs(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("pre-upgrade-"))
        .collect()
}
