use std::{path::PathBuf, process::Command};

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("mooduck-cli-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn run_without_models_fails_locally_before_opening_audio_devices() {
    let directory = Directory::new();
    let output = Command::new(env!("CARGO_BIN_EXE_mooduck_listener"))
        .current_dir(&directory.0)
        .args(["run", "--model-dir", "not-downloaded"])
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("HTTPS_PROXY", "http://127.0.0.1:1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("setup-model"));
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn startup_requires_an_explicit_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_mooduck_listener"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn all_three_modes_are_documented_and_invalid_modes_fail_early() {
    let output = Command::new(env!("CARGO_BIN_EXE_mooduck_listener"))
        .args(["run", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("listen, support, mirror"));
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_mooduck_listener"))
            .args(["run", "--mode", "invalid"])
            .output()
            .unwrap()
            .status
            .success()
    );
}
