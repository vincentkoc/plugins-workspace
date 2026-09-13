// Copyright 2019-2026 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_deep_link::DeepLinkExt;

const URL: &str =
    "tauri-gio-test://receive/path%20with%20spaces?value=one%26two&literal=%24HOME#fragment";
const ROOT_ENV: &str = "TAURI_GIO_PROOF_ROOT";

struct Sandbox(PathBuf);
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(Child);
impl OwnedChild {
    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "proof child failed: {status}");
                return;
            }
            assert!(Instant::now() < deadline, "proof child timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn executable(root: &Path) -> PathBuf {
    root.join("app directory/test-app")
}

fn main() {
    let args: Vec<_> = env::args().collect();
    if let Some(root) = env::var_os(ROOT_ENV) {
        let root = PathBuf::from(root);
        if args.get(1).map(String::as_str) == Some("--register") {
            assert_eq!(args.len(), 2);
            register_and_launch(&root);
        } else {
            // This is the process started by GIO from the unchanged desktop entry.
            assert_eq!(
                args,
                [
                    executable(&root).to_string_lossy().into_owned(),
                    URL.to_string()
                ]
            );
            let pending = root.join("receipt.pending");
            let mut receipt = fs::File::options()
                .write(true)
                .create_new(true)
                .open(&pending)
                .unwrap();
            serde_json::to_writer(&mut receipt, &args).unwrap();
            drop(receipt);
            // Keep the exclusive pending file so a duplicate launch cannot replace its receipt.
            fs::hard_link(pending, root.join("receipt.json")).unwrap();
        }
        return;
    }

    let root = env::temp_dir().join(format!(
        "tauri-gio-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let sandbox = Sandbox(root);
    for directory in [
        "config",
        "data",
        "runtime",
        "system-config",
        "system-data",
        "app directory",
    ] {
        fs::create_dir(sandbox.0.join(directory)).unwrap();
    }
    fs::set_permissions(sandbox.0.join("runtime"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::copy(env::current_exe().unwrap(), executable(&sandbox.0)).unwrap();
    let mut child = OwnedChild(
        Command::new(executable(&sandbox.0))
            .arg("--register")
            .env_clear()
            .env("PATH", env::var_os("PATH").unwrap())
            .env("HOME", &sandbox.0)
            .env("XDG_CONFIG_HOME", sandbox.0.join("config"))
            .env("XDG_DATA_HOME", sandbox.0.join("data"))
            .env("XDG_RUNTIME_DIR", sandbox.0.join("runtime"))
            .env("XDG_CONFIG_DIRS", sandbox.0.join("system-config"))
            .env("XDG_DATA_DIRS", sandbox.0.join("system-data"))
            .env("XDG_CURRENT_DESKTOP", "X-Generic")
            .env(ROOT_ENV, &sandbox.0)
            .spawn()
            .unwrap(),
    );
    child.wait();
    println!("GIO_LAUNCH_PROOF_COMPLETE: exact quoted executable and URL argv received");
}

fn register_and_launch(root: &Path) {
    let app = mock_builder()
        .plugin(tauri_plugin_deep_link::init())
        .build(mock_context(noop_assets()))
        .unwrap();
    app.deep_link().register("tauri-gio-test").unwrap();
    let desktop = root.join("data/applications/test-app-handler.desktop");
    let original = fs::read_to_string(&desktop).unwrap();
    assert_eq!(
        original.lines().find(|line| line.starts_with("Exec=")),
        Some(format!("Exec=\"{}\" %u", executable(root).display()).as_str())
    );
    OwnedChild(Command::new("gio").args(["open", URL]).spawn().unwrap()).wait();
    let deadline = Instant::now() + Duration::from_secs(5);
    let receipt = root.join("receipt.json");
    while !receipt.try_exists().unwrap() {
        assert!(Instant::now() < deadline, "GIO did not deliver the URL");
        thread::sleep(Duration::from_millis(20));
    }
    let delivered: Vec<String> = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    assert_eq!(
        delivered,
        [
            executable(root).to_string_lossy().into_owned(),
            URL.to_string()
        ]
    );
    assert_eq!(fs::read_to_string(desktop).unwrap(), original);
}
