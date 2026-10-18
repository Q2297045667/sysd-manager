//! Gettext setup shared by Rust strings and GTK composite templates.
//!
//! Catalogs are compiled at build time; gettext loads and caches them on demand.
//! Keep locale setup at startup, before GTK, D-Bus or the async runtime starts.

use std::{env, io, path::PathBuf};

pub const DOMAIN: &str = "sysd-manager";

fn locale_dir() -> PathBuf {
    if let Some(path) = env::var_os("TEXTDOMAINDIR").filter(|path| !path.is_empty()) {
        return path.into();
    }

    if cfg!(feature = "flatpak") {
        PathBuf::from("/app/share/locale")
    } else if cfg!(debug_assertions) && !cfg!(feature = "appimage") {
        PathBuf::from(env!("SYSD_LOCALE_DIR"))
    } else {
        PathBuf::from("/usr/share/locale")
    }
}

/// Initialize the process locale and bind the application's UTF-8 catalog.
///
/// `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` and `LANG` follow gettext's usual rules,
/// including English source text when no translation is available.
///
/// # Safety
///
/// Call once, on the main thread, before starting other threads or installing
/// signal handlers. Neither the environment nor the locale may change during
/// this call.
pub unsafe fn init() -> io::Result<PathBuf> {
    // SAFETY: the caller guarantees exclusive access to the process locale.
    let valid = unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "") }.is_some();
    if !valid {
        // A failed LC_ALL call may have changed some categories. Reset all of
        // them before using source text, and still bind our own domain below.
        unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "C") };
    }

    let path = gettextrs::bindtextdomain(DOMAIN, locale_dir())?;
    gettextrs::bind_textdomain_codeset(DOMAIN, "UTF-8")?;
    gettextrs::textdomain(DOMAIN)?;
    if valid {
        Ok(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The requested locale is unavailable; using English source text",
        ))
    }
}
