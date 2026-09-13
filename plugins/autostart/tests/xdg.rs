// Copyright 2019-2026 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

#![cfg(any(target_os = "linux", target_os = "freebsd"))]

use std::{
    env, fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri_plugin_autostart::ManagerExt;

const TEST_NAME: &str = "xdg_autostart_uses_executable_and_preserves_arguments";
const CHILD_ENV: &str = "TAURI_AUTOSTART_TEST_HOME";

#[test]
fn xdg_autostart_uses_executable_and_preserves_arguments() {
    if let Some(home) = env::var_os(CHILD_ENV) {
        // The parent gives only this child a disposable home. Never mutate the
        // developer's autostart entries or process-wide environment in a test.
        assert_eq!(env::var_os("HOME"), Some(home.clone()));
        let home = std::path::PathBuf::from(home);
        let app = mock_builder()
            .plugin(
                tauri_plugin_autostart::Builder::new()
                    .app_name("Tauri Autostart Test")
                    .args(["--from-autostart", "--profile=test"])
                    .build(),
            )
            .build(mock_context(noop_assets()))
            .unwrap();
        let autostart = app.autolaunch();
        let entry = home.join(".config/autostart/Tauri Autostart Test.desktop");
        assert!(!autostart.is_enabled().unwrap());
        autostart.enable().unwrap();
        assert!(autostart.is_enabled().unwrap());
        let desktop = fs::read_to_string(&entry).unwrap();
        let expected = format!(
            "Exec={} --from-autostart --profile=test",
            env::current_exe().unwrap().display()
        );
        assert!(desktop.lines().any(|line| line == expected));
        assert!(desktop.lines().any(|line| line == "Type=Application"));
        assert!(!home.join(".config/systemd").exists());
        autostart.enable().unwrap();
        assert_eq!(fs::read_to_string(&entry).unwrap(), desktop);
        autostart.disable().unwrap();
        assert!(!autostart.is_enabled().unwrap());
        assert!(!entry.exists());
        autostart.disable().unwrap();
        return;
    }

    let home = tempfile::tempdir().unwrap();
    fs::create_dir(home.path().join(".config")).unwrap();
    let mut child = Command::new(env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "autostart test child failed");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("autostart test child timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
