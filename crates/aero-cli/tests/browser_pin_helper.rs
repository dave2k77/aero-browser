use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::{Command, Output};

fn fixture(directory: &Path, python3: bool, report: &str, status: i32) {
    symlink("/usr/bin/dirname", directory.join("dirname")).unwrap();
    if python3 {
        symlink("/usr/bin/python3", directory.join("python3")).unwrap();
    }
    // Synthetic Cargo report tests the shell adapter, not actual browser readiness.
    let cargo = directory.join("cargo");
    fs::write(&cargo, format!(
        "#!/bin/sh\nprintf invoked > \"${{0%/*}}/cargo-invoked\"\nprintf '%s\\n' '{report}'\nexit {status}\n"
    )).unwrap();
    fs::set_permissions(cargo, fs::Permissions::from_mode(0o700)).unwrap();
}

fn run(directory: &Path) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    Command::new("/bin/bash")
        .arg(root.join("scripts/lock-browser.sh"))
        .arg("synthetic-browser")
        .env_clear()
        .env("PATH", directory)
        .output()
        .unwrap()
}

#[test]
fn browser_pin_helper_works_with_python3_only() {
    let directory = tempfile::tempdir().unwrap();
    fixture(
        directory.path(),
        true,
        r#"{"browser":{"available":true},"browser_lock":{"status":"matched_local_pin"}}"#,
        2,
    );
    let output = run(directory.path());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("sandbox remains unverified"));
}

#[test]
fn browser_pin_helper_checks_python3_before_cargo() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), false, "{}", 2);
    let output = run(directory.path());
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("python3"));
    assert!(!directory.path().join("cargo-invoked").exists());
}

#[test]
fn browser_pin_helper_rejects_mismatch_and_propagates_cargo_failure() {
    for (status, expected) in [(2, 1), (101, 101)] {
        let directory = tempfile::tempdir().unwrap();
        fixture(
            directory.path(),
            true,
            r#"{"browser":{"available":true},"browser_lock":{"status":"mismatch"}}"#,
            status,
        );
        assert_eq!(run(directory.path()).status.code(), Some(expected));
    }
}
