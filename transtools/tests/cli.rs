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
fn translation_commands_propagate_compiler_failures_without_partial_catalogs() {
    use std::os::unix::fs::PermissionsExt;
    let directory = TempDir::new().unwrap();
    let compiler = directory.path().join("msgfmt");
    fs::write(
        &compiler,
        "#!/bin/sh\nprintf 'fixture compiler failure' >&2\nexit 17\n",
    )
    .unwrap();
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755)).unwrap();
    for action in ["mo", "check"] {
        let result = Command::new(env!("CARGO_BIN_EXE_transtools"))
            .arg(action)
            .env("PATH", directory.path())
            .env("CARGO_TARGET_DIR", directory.path().join("build"))
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&result.stdout).contains("fixture compiler failure"));
    }
    assert!(
        !directory
            .path()
            .join("build/locale/cs/LC_MESSAGES/sysd-manager.mo")
            .exists()
    );
}

#[test]
fn catalog_check_works_from_an_unrelated_working_directory() {
    let directory = TempDir::new().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_transtools"))
        .arg("check")
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Chinese translations verified"));
}
