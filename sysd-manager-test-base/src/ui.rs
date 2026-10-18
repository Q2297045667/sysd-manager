//! Own every test process and fixture so failures cannot leave desktop state.
use crate::TempDir;
use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs, io,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

type Environment = BTreeMap<OsString, OsString>;

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(program: impl AsRef<OsStr>, environment: &Environment) -> Command {
    let mut command = Command::new(program);
    command.env_clear().envs(environment);
    command
}

fn wait_until<T>(
    timeout: Duration,
    description: &str,
    mut poll: impl FnMut() -> io::Result<Option<T>>,
) -> io::Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(result) = poll()? {
            return Ok(result);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{description} timed out"),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn run(command: &mut Command, timeout: Duration, description: &str) -> io::Result<()> {
    let mut process = Process(command.spawn()?);
    let status = wait_until(timeout, description, || process.0.try_wait())?;
    require_success(status, description)
}

fn require_success(status: ExitStatus, description: &str) -> io::Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{description} failed: {status}")))
    }
}

fn environment(directory: &Path, catalogs: &Path) -> Environment {
    let mut result: Environment = env::vars_os()
        .filter(|(key, _)| {
            !key.to_str().is_some_and(|key| {
                key.starts_with("LC_")
                    || matches!(key, "LANGUAGE" | "LOCPATH" | "DISPLAY" | "WAYLAND_DISPLAY")
            })
        })
        .collect();
    for (key, value) in [
        ("SYSD_UI_TEST_ISOLATED", "1"),
        ("GDK_BACKEND", "broadway"),
        ("BROADWAY_DISPLAY", ":55"),
        ("GSK_RENDERER", "cairo"),
        ("G_DEBUG", "fatal-warnings"),
        ("GTK_A11Y", "none"),
        ("GIO_USE_VFS", "local"),
        ("GIO_USE_PROXY_RESOLVER", "dummy"),
        ("GSETTINGS_BACKEND", "memory"),
        ("LANG", "en_US.UTF-8"),
        ("LC_ALL", "en_US.UTF-8"),
    ] {
        result.insert(key.into(), value.into());
    }
    result.insert("XDG_RUNTIME_DIR".into(), directory.into());
    result.insert("TEXTDOMAINDIR".into(), catalogs.into());
    for (key, subdirectory) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        result.insert(key.into(), directory.join(subdirectory).into());
    }
    result
}

pub fn run_isolated(binary: &Path, root: &Path, catalogs: &Path) -> io::Result<()> {
    let fixture = TempDir::new()?;
    let directory = fixture.path();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let mut environment = environment(directory, catalogs);
    let backend = if let Some(runtime) = env::var_os("SYSD_GTK_RUNTIME") {
        let runtime = Path::new(&runtime).canonicalize()?;
        let mut paths = vec![runtime.join("usr/lib")];
        if let Some(previous) = environment.get(OsStr::new("LD_LIBRARY_PATH")) {
            paths.extend(env::split_paths(previous));
        }
        environment.insert(
            "LD_LIBRARY_PATH".into(),
            env::join_paths(paths).map_err(io::Error::other)?,
        );
        runtime.join("usr/bin/gtk4-broadwayd").into_os_string()
    } else {
        "gtk4-broadwayd".into()
    };

    let available = command("locale", &environment)
        .env("LC_ALL", "C")
        .arg("-a")
        .output()?;
    require_success(available.status, "locale enumeration")?;
    let available = String::from_utf8_lossy(&available.stdout)
        .to_lowercase()
        .replace('-', "");
    if !available.lines().any(|locale| locale == "en_us.utf8") {
        let locales = directory.join("locales");
        fs::create_dir(&locales)?;
        run(
            command("localedef", &environment)
                .env("LC_ALL", "C")
                .args(["--no-archive", "-i", "en_US", "-f", "UTF-8"])
                .arg(locales.join("en_US.UTF-8")),
            Duration::from_secs(20),
            "UTF-8 locale generation",
        )?;
        environment.insert("LOCPATH".into(), locales.into());
    }
    let schemas = directory.join("schemas");
    fs::create_dir(&schemas)?;
    fs::copy(
        root.join("data/schemas/io.github.plrigaux.sysd-manager.gschema.xml"),
        schemas.join("io.github.plrigaux.sysd-manager.gschema.xml"),
    )?;
    run(
        command("glib-compile-schemas", &environment)
            .arg("--strict")
            .arg(&schemas),
        Duration::from_secs(10),
        "GSettings schema compilation",
    )?;
    environment.insert("GSETTINGS_SCHEMA_DIR".into(), schemas.into());

    let config = directory.join("bus.conf");
    let escaped_directory = directory
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;");
    fs::write(
        &config,
        format!(
            r#"<busconfig>
      <type>session</type><listen>unix:tmpdir={escaped_directory}</listen><auth>EXTERNAL</auth>
      <policy context="default"><allow own="*"/><allow send_destination="*"/>
      <allow receive_sender="*"/></policy>
    </busconfig>"#
        ),
    )?;
    let log_path = directory.join("display.log");
    let display_log = fs::File::create(&log_path)?;
    let mut display = Process(
        command(backend, &environment)
            .args(["--port=0", ":55"])
            .stdout(display_log.try_clone()?)
            .stderr(display_log)
            .spawn()?,
    );
    let address_path = directory.join("bus.address");
    let mut bus = Process(
        command("dbus-daemon", &environment)
            .args(["--nofork", "--print-address=1", "--config-file"])
            .arg(config)
            .stdout(fs::File::create(&address_path)?)
            .spawn()?,
    );
    let address = wait_until(Duration::from_secs(5), "private D-Bus startup", || {
        if let Some(status) = bus.0.try_wait()? {
            return Err(io::Error::other(format!("private D-Bus exited: {status}")));
        }
        let address = fs::read_to_string(&address_path)?.trim().to_owned();
        Ok((!address.is_empty()).then_some(address))
    })?;
    environment.insert("DBUS_SESSION_BUS_ADDRESS".into(), address.clone().into());
    environment.insert("DBUS_SYSTEM_BUS_ADDRESS".into(), address.into());
    wait_until(
        Duration::from_secs(5),
        "private GTK display startup",
        || {
            if let Some(status) = display.0.try_wait()? {
                return Err(io::Error::other(format!(
                    "private GTK display exited: {status}"
                )));
            }
            Ok(directory.join("broadway56.socket").exists().then_some(()))
        },
    )?;
    let result = (|| {
        environment.insert("LANGUAGE".into(), "en".into());
        run(
            command(binary, &environment).arg("--preflight"),
            Duration::from_secs(20),
            "GTK window/popover preflight before application tests; use a compatible runtime via SYSD_GTK_RUNTIME (fatal warnings stay enabled)",
        )?;
        for language in ["en", "zh_CN"] {
            environment.insert("LANGUAGE".into(), language.into());
            run(
                &mut command(binary, &environment),
                Duration::from_secs(45),
                language,
            )?;
        }
        Ok(())
    })();
    drop(bus);
    drop(display);
    let log = fs::read_to_string(log_path)?;
    if log.contains("ERROR") || log.contains("assertion") {
        return Err(io::Error::other(format!("GTK display failed:\n{log}")));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn child_exit_failures_are_reported() {
        let mut success = Command::new("sh");
        success.args(["-c", "exit 0"]);
        run(&mut success, Duration::from_secs(2), "successful child").unwrap();
        let mut failure = Command::new("sh");
        failure.args(["-c", "exit 7"]);
        assert!(
            run(&mut failure, Duration::from_secs(2), "failed child")
                .unwrap_err()
                .to_string()
                .contains("failed child failed")
        );
    }

    #[test]
    fn timed_out_children_are_killed_and_reaped() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 30"]).stdout(Stdio::null());
        let process = command.spawn().unwrap();
        let pid = process.id();
        let mut process = Process(process);
        let result = wait_until(Duration::from_millis(20), "test child", || {
            process.0.try_wait()
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        drop(process);
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }
}
