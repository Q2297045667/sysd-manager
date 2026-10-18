use std::{fs, process::Command};
use test_base::TempDir;

#[test]
fn invalid_language_is_a_failure_exit() {
    let result = Command::new(env!("CARGO_BIN_EXE_transtools"))
        .args(["po", "--lang", "sysd_unknown_language"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stdout).contains("LanguageNotSet"));
}

#[cfg(unix)]
#[test]
fn catalog_compiler_failure_is_a_failure_exit_without_partial_catalogs() {
    use std::os::unix::fs::PermissionsExt;
    let directory = TempDir::new().unwrap();
    let compiler = directory.path().join("msgfmt");
    fs::write(
        &compiler,
        "#!/bin/sh\nprintf 'fixture compiler failure' >&2\nexit 17\n",
    )
    .unwrap();
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_transtools"))
        .arg("mo")
        .env("PATH", directory.path())
        .env("CARGO_TARGET_DIR", directory.path().join("build"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stdout).contains("fixture compiler failure"));
    assert!(
        !directory
            .path()
            .join("build/locale/cs/LC_MESSAGES/sysd-manager.mo")
            .exists()
    );
}
