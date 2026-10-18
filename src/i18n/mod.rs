//! Gettext setup shared by Rust strings and GTK composite templates.
//!
//! Installed catalogs are compiled at build time. Users may additionally drop
//! `<language>.po` or `<language>.mo` files into
//! `$XDG_DATA_HOME/sysd-manager/translations`; those catalogs are discovered at
//! runtime, compiled when needed and offered by the preferences dialog.
//!
//! Keep locale setup at startup, before GTK, D-Bus or the async runtime starts.

use std::{env, io, path::PathBuf, sync::RwLock};

mod catalog;
mod discovery;

pub use discovery::{Language, Origin};

pub const DOMAIN: &str = "sysd-manager";

/// Language explicitly selected at startup; `None` means the system locale.
static APPLIED_LANGUAGE: RwLock<Option<String>> = RwLock::new(None);

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

/// Directory scanned for user supplied translation files.
pub fn translations_dir() -> Option<PathBuf> {
    discovery::user_translations_dir()
}

/// Translation catalogs currently available, installed and user supplied.
pub fn available_languages() -> Vec<Language> {
    discovery::available_languages()
}

/// Language code applied at startup, or `None` when the system locale is used.
pub fn applied_language() -> Option<String> {
    match APPLIED_LANGUAGE.read() {
        Ok(applied) => applied.clone(),
        Err(error) => {
            tracing::warn!("Applied language lock is poisoned: {error}");
            None
        }
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
#[allow(dead_code)] // used by the locale integration tests and the startup API
pub unsafe fn init() -> io::Result<PathBuf> {
    // SAFETY: forwarded from this function's contract.
    unsafe { init_with(None) }
}

/// Initialize gettext with an explicit interface language preference.
///
/// `preferred` is a gettext catalog code discovered by
/// [`available_languages`]. An empty value keeps the system locale, which is
/// what `init` does. A code whose catalog is not present also keeps the system
/// locale. Catalogs are bound from the staged catalog directory when user
/// drop-in files exist.
///
/// # Safety
///
/// The same single-threaded, startup-only contract as [`init`] applies. This
/// function also writes `LANGUAGE` in the process environment.
pub unsafe fn init_with(preferred: Option<&str>) -> io::Result<PathBuf> {
    let catalog_dir = discovery::bind_dir();
    let preferred = preferred
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .filter(|code| {
            let available = discovery::has_catalog(&catalog_dir, code);
            if !available {
                tracing::warn!(
                    "Selected interface language {code:?} is not available; using the system language"
                );
            }
            available
        });
    match APPLIED_LANGUAGE.write() {
        Ok(mut applied) => *applied = preferred.map(str::to_owned),
        Err(error) => tracing::warn!("Applied language lock is poisoned: {error}"),
    }

    if let Some(code) = preferred {
        // SAFETY: startup is single-threaded and `LANGUAGE` is read by gettext
        // on lookup, before any other thread can observe the environment.
        unsafe { env::set_var("LANGUAGE", code) };
    }

    // SAFETY: the caller guarantees exclusive access to the process locale.
    let valid = unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "") }.is_some();
    if !valid {
        // A failed LC_ALL call may have changed some categories. Reset all of
        // them before using source text, and still bind our own domain below.
        unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "C") };
    }

    if preferred.is_some() {
        // gettext ignores LANGUAGE in the C/POSIX message locale. Switch only
        // LC_MESSAGES to an installed UTF-8 locale so the explicit choice is
        // honored without changing date, number or monetary formats.
        ensure_message_locale_is_not_c();
    }

    let path = gettextrs::bindtextdomain(DOMAIN, &catalog_dir)?;
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

/// Make sure `LANGUAGE` is not ignored because the message locale is C.
fn ensure_message_locale_is_not_c() {
    let current = unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcMessages, "") };
    if !current.as_deref().is_none_or(is_c_message_locale) {
        return;
    }

    for candidate in ["en_US.UTF-8", "en_US.utf8", "en_GB.UTF-8", "en_GB.utf8"] {
        // SAFETY: startup is single-threaded; only LC_MESSAGES is changed.
        if unsafe { gettextrs::setlocale(gettextrs::LocaleCategory::LcMessages, candidate) }
            .is_some()
        {
            tracing::info!("Using message locale {candidate} for the selected interface language");
            return;
        }
    }
    tracing::warn!(
        "No UTF-8 message locale is installed; the selected interface language may fall back to English"
    );
}

fn is_c_message_locale(locale: &[u8]) -> bool {
    locale == b"C" || locale == b"POSIX" || locale.starts_with(b"C.")
}
