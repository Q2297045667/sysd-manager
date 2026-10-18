use std::sync::OnceLock;

/// An owned, isolated fixture directory. Never changes the working directory or
/// the user's environment, so fixtures can be used by parallel tests.
pub struct TempDir(std::path::PathBuf);

impl TempDir {
    pub fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "sysd-manager-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

use tracing::{level_filters::LevelFilter, warn};

pub const TEST_SERVICE: &str = "tiny_daemon.service";

//Too avoid duplicate init during series os tests
static TRACING: OnceLock<()> = OnceLock::new();

pub fn init_logs() {
    TRACING.get_or_init(|| {
        let timer =
            tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f".to_owned());

        if let Err(err) = tracing_subscriber::fmt()
            .with_timer(timer)
            .with_max_level(LevelFilter::DEBUG)
            .try_init()
        {
            warn!("Tracing Init Error:{:?}", err)
        }
    });
}
