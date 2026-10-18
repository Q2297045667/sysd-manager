//! Runtime discovery of translation catalogs.
//!
//! gettext can only read binary catalogs from one directory, while this
//! application supports two sources at the same time:
//!
//! - the installed catalog directory (`/usr/share/locale`, the Flatpak locale
//!   directory, the development build's target directory or `TEXTDOMAINDIR`);
//! - a user drop-in directory (`$XDG_DATA_HOME/sysd-manager/translations`)
//!   holding flat `<language>.po` and `<language>.mo` files.
//!
//! When drop-in files exist the catalogs are staged into a cache directory
//! that gettext can bind to, so user files override installed catalogs for the
//! same language without hiding the other installed languages.

use super::catalog;
use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};
use tracing::{debug, warn};

const CATALOG_FILE: &str = "sysd-manager.mo";
const USER_DIRECTORY: &str = "sysd-manager/translations";
const CACHE_DIRECTORY: &str = "sysd-manager/locale";

/// Where a discovered catalog is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    System,
    User,
}

/// One discovered translation catalog.
#[derive(Debug, Clone)]
pub struct Language {
    /// Gettext language code, i.e. the catalog directory or drop-in file name.
    pub code: String,
    /// Human readable name declared by the catalog, or the code as fallback.
    pub name: String,
    pub origin: Origin,
}

impl Language {
    fn new(code: &str, header: &catalog::Header, origin: Origin) -> Self {
        let name = catalog::display_name(header)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| code.to_owned());
        Self {
            code: code.to_owned(),
            name,
            origin,
        }
    }

    /// Text shown in the language selector.
    pub fn label(&self) -> String {
        if self.name.eq_ignore_ascii_case(&self.code) {
            self.code.clone()
        } else {
            format!("{} — {}", self.name, self.code)
        }
    }
}

/// Whether a bound catalog directory provides a language.
pub fn has_catalog(directory: &Path, code: &str) -> bool {
    valid_code(code)
        && directory
            .join(code)
            .join("LC_MESSAGES")
            .join(CATALOG_FILE)
            .is_file()
}

/// Directory scanned for user supplied `.po`/`.mo` files.
pub fn user_translations_dir() -> Option<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share").map(|directory| directory.join(USER_DIRECTORY))
}

/// All catalogs available to the application, sorted by display name.
pub fn available_languages() -> Vec<Language> {
    available_languages_in(&super::locale_dir(), user_translations_dir().as_deref())
}

/// Catalog directory gettext should bind to.
///
/// Installed catalogs are used directly when no user drop-in file exists.
/// Otherwise every catalog is staged into the cache directory so that user
/// files can override installed ones while all other languages stay usable.
pub fn bind_dir() -> PathBuf {
    let system = super::locale_dir();
    let Some(user) = user_translations_dir() else {
        return system;
    };
    if user_catalog_files(&user).is_empty() {
        return system;
    }
    let Some(cache) =
        xdg_dir("XDG_CACHE_HOME", ".cache").map(|directory| directory.join(CACHE_DIRECTORY))
    else {
        warn!("No user cache directory; user translations cannot be loaded");
        return system;
    };
    match stage_catalogs(&system, Some(&user), &cache) {
        Ok(directory) => {
            debug!(
                "Staged translation catalogs from {} and {} into {}",
                system.display(),
                user.display(),
                directory.display()
            );
            directory
        }
        Err(error) => {
            warn!(
                "Unable to stage user translations from {}: {error}; using installed catalogs",
                user.display()
            );
            system
        }
    }
}

fn available_languages_in(system: &Path, user: Option<&Path>) -> Vec<Language> {
    let mut languages: BTreeMap<String, Language> = BTreeMap::new();
    for language in system_languages(system) {
        languages.insert(language.code.clone(), language);
    }
    if let Some(directory) = user {
        for language in user_languages(directory) {
            // A drop-in catalog overrides the installed catalog of the same
            // language, matching the staging precedence used by `bind_dir`.
            languages.insert(language.code.clone(), language);
        }
    }
    let mut languages: Vec<_> = languages.into_values().collect();
    languages.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.code.cmp(&right.code))
    });
    languages
}

struct SystemCatalog {
    code: String,
    path: PathBuf,
    header: catalog::Header,
    declared: Option<String>,
}

fn system_catalogs(directory: &Path) -> Vec<SystemCatalog> {
    let mut catalogs = Vec::new();
    let read_dir = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            debug!(
                "No installed translation catalog directory {}: {error}",
                directory.display()
            );
            return catalogs;
        }
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(code) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !valid_code(code) {
            continue;
        }
        let catalog_path = path.join("LC_MESSAGES").join(CATALOG_FILE);
        if !catalog_path.is_file() {
            continue;
        }
        let header = match catalog::read_mo_header(&catalog_path) {
            Ok(header) => header,
            Err(error) => {
                warn!(
                    "Unable to read metadata from {}: {error}",
                    catalog_path.display()
                );
                catalog::Header::new()
            }
        };
        let declared = catalog::header_language(&header);
        catalogs.push(SystemCatalog {
            code: code.to_owned(),
            path: catalog_path,
            header,
            declared,
        });
    }
    catalogs.sort_by(|left, right| left.code.cmp(&right.code));
    catalogs
}

/// The build stages shortcut locale names (for example zh_CN for zh_Hans) by
/// copying the canonical catalog. A shortcut whose content differs is a real
/// catalog and is listed separately.
fn is_shortcut(catalog: &SystemCatalog, catalogs: &[SystemCatalog]) -> bool {
    let Some(declared) = &catalog.declared else {
        return false;
    };
    if declared.eq_ignore_ascii_case(&catalog.code) {
        return false;
    }
    catalogs
        .iter()
        .find(|canonical| canonical.code.eq_ignore_ascii_case(declared))
        .is_some_and(|canonical| same_contents(&catalog.path, &canonical.path))
}

fn system_languages(directory: &Path) -> Vec<Language> {
    let catalogs = system_catalogs(directory);
    catalogs
        .iter()
        .filter(|catalog| !is_shortcut(catalog, &catalogs))
        .map(|catalog| Language::new(&catalog.code, &catalog.header, Origin::System))
        .collect()
}

/// Map a shortcut locale name to the language it aliases.
fn shortcut_languages(catalogs: &[SystemCatalog]) -> BTreeMap<String, String> {
    catalogs
        .iter()
        .filter(|catalog| is_shortcut(catalog, catalogs))
        .filter_map(|catalog| {
            catalog
                .declared
                .as_ref()
                .map(|declared| (catalog.code.clone(), declared.clone()))
        })
        .collect()
}

fn same_contents(left: &Path, right: &Path) -> bool {
    match (fs::read(left), fs::read(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn user_languages(directory: &Path) -> Vec<Language> {
    let mut languages = Vec::new();
    for path in user_catalog_files(directory) {
        let Some(code) = language_code(&path) else {
            continue;
        };
        let header = match path.extension().and_then(|extension| extension.to_str()) {
            Some("po") => catalog::parse_po_file(&path).map(|catalog| catalog.header().clone()),
            Some("mo") => catalog::read_mo_header(&path),
            _ => continue,
        };
        let header = match header {
            Ok(header) => header,
            Err(error) => {
                warn!("Ignoring translation file {}: {error}", path.display());
                continue;
            }
        };
        if let Some(declared) = catalog::header_language(&header)
            && !declared.eq_ignore_ascii_case(code)
        {
            debug!(
                "Translation file {} declares language {declared} but is named {code}",
                path.display()
            );
        }
        languages.push(Language::new(code, &header, Origin::User));
    }
    languages
}

fn user_catalog_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            debug!(
                "No user translation directory {}: {error}",
                directory.display()
            );
            return files;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let extension = path.extension().and_then(|extension| extension.to_str());
        if matches!(extension, Some("po" | "mo")) {
            files.push(path);
        }
    }
    files.sort();
    files
}

fn language_code(path: &Path) -> Option<&str> {
    let code = path.file_stem().and_then(|name| name.to_str())?.trim();
    valid_code(code).then_some(code)
}

/// Reject file names that cannot be used as a gettext language directory.
fn valid_code(code: &str) -> bool {
    // A leading dot would let a file such as "...po" name its language ".."
    // and escape the catalog directory when it is staged.
    !code.is_empty()
        && !code.starts_with('.')
        && code.len() <= 64
        && code.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '@' | '.')
        })
}

fn stage_catalogs(system: &Path, user: Option<&Path>, cache: &Path) -> io::Result<PathBuf> {
    if cache.exists() {
        fs::remove_dir_all(cache)?;
    }
    fs::create_dir_all(cache)?;

    // Stage every installed catalog, including shortcut locale names such as
    // zh_CN, because gettext resolves those names when LANGUAGE is set by the
    // environment or by the stored preference.
    let installed = system_catalogs(system);
    let shortcuts = shortcut_languages(&installed);
    for catalog in &installed {
        let destination = cache
            .join(&catalog.code)
            .join("LC_MESSAGES")
            .join(CATALOG_FILE);
        if let Err(error) = link_or_copy(&catalog.path, &destination) {
            warn!(
                "Unable to stage installed translation {}: {error}",
                catalog.path.display()
            );
        }
    }

    let Some(user) = user else {
        return Ok(cache.to_path_buf());
    };
    let mut user_catalogs = BTreeMap::<String, PathBuf>::new();
    for path in user_catalog_files(user) {
        let Some(code) = language_code(&path) else {
            continue;
        };
        let destination = cache.join(code).join("LC_MESSAGES").join(CATALOG_FILE);
        let installed = match path.extension().and_then(|extension| extension.to_str()) {
            Some("po") => match catalog::compile_po_file(&path, &destination) {
                Ok(()) => true,
                Err(error) => {
                    warn!(
                        "Unable to compile user translation {}: {error}",
                        path.display()
                    );
                    false
                }
            },
            Some("mo") => match copy_catalog(&path, &destination) {
                Ok(()) => true,
                Err(error) => {
                    warn!(
                        "Unable to copy user translation {}: {error}",
                        path.display()
                    );
                    false
                }
            },
            _ => false,
        };
        if installed {
            user_catalogs.insert(code.to_owned(), destination);
        }
    }

    // Apply a user override to the shortcut locale names that alias it, so a
    // zh_Hans.po override is also used by a zh_CN system locale.
    for (shortcut, canonical) in shortcuts {
        if user_catalogs.contains_key(&shortcut) {
            continue;
        }
        let Some(source) = user_catalogs.get(&canonical) else {
            continue;
        };
        let destination = cache.join(&shortcut).join("LC_MESSAGES").join(CATALOG_FILE);
        if let Err(error) = copy_catalog(source, &destination) {
            warn!("Unable to stage the user translation for {shortcut}: {error}");
        }
    }
    Ok(cache.to_path_buf())
}

fn copy_catalog(source: &Path, destination: &Path) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination).map(|_| ())
}

fn link_or_copy(source: &Path, destination: &Path) -> io::Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    {
        // A symlink keeps the cache small and follows catalog updates until the
        // next staging run. Copy when the target cannot be resolved.
        if let Ok(target) = fs::canonicalize(source) {
            match std::os::unix::fs::symlink(&target, destination) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    debug!(
                        "Unable to link {} into the catalog cache: {error}",
                        source.display()
                    );
                }
            }
        }
    }
    fs::copy(source, destination).map(|_| ())
}

fn xdg_dir(variable: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(path) = env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Some(path);
    }
    env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use test_base::TempDir;

    fn po(code: &str, team: &str, search: &str) -> String {
        format!(
            "msgid \"\"\n\
             msgstr \"\"\n\
             \"Language: {code}\\n\"\n\
             \"Language-Team: {team} <https://example.invalid/{code}/>\\n\"\n\
             \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
             \n\
             msgid \"Search\"\n\
             msgstr \"{search}\"\n"
        )
    }

    fn install(directory: &Path, code: &str, source: &str) {
        let path = directory.join("source.po");
        fs::create_dir_all(directory).unwrap();
        fs::write(&path, source).unwrap();
        catalog::compile_po_file(
            &path,
            &directory.join(code).join("LC_MESSAGES").join(CATALOG_FILE),
        )
        .unwrap();
    }

    #[test]
    fn system_and_user_catalogs_are_listed() {
        let fixture = TempDir::new().unwrap();
        let system = fixture.path().join("system");
        install(&system, "de", &po("de", "German", "Suchen"));
        install(&system, "fr", &po("fr", "French", "Rechercher"));

        let user = fixture.path().join("user");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("it.po"), po("it", "Italian", "Cerca")).unwrap();

        let languages = available_languages_in(&system, Some(&user));
        let codes: Vec<_> = languages
            .iter()
            .map(|language| language.code.as_str())
            .collect();
        assert_eq!(codes, ["fr", "de", "it"]);
        assert_eq!(languages[2].origin, Origin::User);
        assert_eq!(languages[2].name, "Italian");
    }

    #[test]
    fn user_catalog_overrides_the_system_name() {
        let fixture = TempDir::new().unwrap();
        let system = fixture.path().join("system");
        install(&system, "de", &po("de", "German", "Suchen"));

        let user = fixture.path().join("user");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("de.po"), po("de", "Deutsch", "Suchen")).unwrap();

        let languages = available_languages_in(&system, Some(&user));
        assert_eq!(languages.len(), 1);
        assert_eq!(languages[0].name, "Deutsch");
        assert_eq!(languages[0].origin, Origin::User);
        assert_eq!(languages[0].label(), "Deutsch — de");
    }

    #[test]
    fn staging_merges_installed_and_user_catalogs() {
        let fixture = TempDir::new().unwrap();
        let system = fixture.path().join("system");
        install(&system, "de", &po("de", "German", "Suchen"));

        let user = fixture.path().join("user");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("ja.po"), po("ja", "Japanese", "検索")).unwrap();
        fs::write(user.join("de.po"), po("de", "Deutsch", "Suchen")).unwrap();

        // An already compiled drop-in catalog is copied as well.
        let compiled = fixture.path().join("compiled/es/LC_MESSAGES");
        fs::create_dir_all(&compiled).unwrap();
        let es_po = fixture.path().join("es.po");
        fs::write(&es_po, po("es", "Spanish", "Buscar")).unwrap();
        catalog::compile_po_file(&es_po, &compiled.join(CATALOG_FILE)).unwrap();
        fs::copy(compiled.join(CATALOG_FILE), user.join("es.mo")).unwrap();

        let cache = fixture.path().join("cache");
        stage_catalogs(&system, Some(&user), &cache).unwrap();

        for code in ["de", "ja", "es"] {
            let path = cache.join(code).join("LC_MESSAGES").join(CATALOG_FILE);
            assert!(path.is_file(), "missing staged catalog for {code}");
        }
        let header =
            catalog::read_mo_header(&cache.join("de").join("LC_MESSAGES").join(CATALOG_FILE))
                .unwrap();
        assert_eq!(catalog::display_name(&header).as_deref(), Some("Deutsch"));
    }

    #[test]
    fn copied_locale_shortcuts_are_not_listed_twice() {
        let fixture = TempDir::new().unwrap();
        let system = fixture.path().join("system");
        let canonical = po("zh_Hans", "Chinese (Simplified Han script)", "搜索");
        install(&system, "zh_Hans", &canonical);
        fs::create_dir_all(system.join("zh_CN/LC_MESSAGES")).unwrap();
        fs::copy(
            system.join("zh_Hans/LC_MESSAGES").join(CATALOG_FILE),
            system.join("zh_CN/LC_MESSAGES").join(CATALOG_FILE),
        )
        .unwrap();

        let languages = available_languages_in(&system, None);
        assert_eq!(languages.len(), 1);
        assert_eq!(languages[0].code, "zh_Hans");

        // Staging keeps the shortcut so LANGUAGE=zh_CN still resolves.
        let cache = fixture.path().join("cache");
        stage_catalogs(&system, None, &cache).unwrap();
        assert!(cache.join("zh_CN/LC_MESSAGES").join(CATALOG_FILE).is_file());

        // A shortcut with a distinct translation is a real catalog.
        install(
            &system,
            "zh_CN",
            &po("zh_Hans", "Chinese (Simplified Han script)", "查找"),
        );
        let languages = available_languages_in(&system, None);
        assert_eq!(languages.len(), 2);
    }

    #[test]
    fn user_override_reaches_shortcut_locale_names() {
        let fixture = TempDir::new().unwrap();
        let system = fixture.path().join("system");
        install(
            &system,
            "zh_Hans",
            &po("zh_Hans", "Chinese (Simplified Han script)", "搜索"),
        );
        fs::create_dir_all(system.join("zh_CN/LC_MESSAGES")).unwrap();
        fs::copy(
            system.join("zh_Hans/LC_MESSAGES").join(CATALOG_FILE),
            system.join("zh_CN/LC_MESSAGES").join(CATALOG_FILE),
        )
        .unwrap();

        let user = fixture.path().join("user");
        fs::create_dir_all(&user).unwrap();
        fs::write(
            user.join("zh_Hans.po"),
            po("zh_Hans", "Chinese (Simplified Han script)", "查找"),
        )
        .unwrap();

        let cache = fixture.path().join("cache");
        stage_catalogs(&system, Some(&user), &cache).unwrap();
        let staged_hans = fs::read(cache.join("zh_Hans/LC_MESSAGES").join(CATALOG_FILE)).unwrap();
        let staged_shortcut = fs::read(cache.join("zh_CN/LC_MESSAGES").join(CATALOG_FILE)).unwrap();
        let installed = fs::read(system.join("zh_Hans/LC_MESSAGES").join(CATALOG_FILE)).unwrap();
        assert_eq!(staged_hans, staged_shortcut);
        assert_ne!(staged_hans, installed);
    }

    #[test]
    fn rejects_path_like_language_codes() {
        for code in ["", ".", "..", "...", ".hidden", "../etc", "a/b"] {
            assert!(!valid_code(code), "{code:?} must be rejected");
        }
        for code in ["de", "zh_Hans", "pt_BR", "sr@latin", "a.b"] {
            assert!(valid_code(code), "{code:?} must be accepted");
        }
    }

    #[test]
    fn staging_ignores_path_like_codes() {
        let fixture = TempDir::new().unwrap();
        let user = fixture.path().join("user");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("...po"), po("..", "Invalid", "X")).unwrap();
        fs::write(user.join("..po"), po(".", "Invalid", "X")).unwrap();

        let cache = fixture.path().join("cache");
        stage_catalogs(&fixture.path().join("system"), Some(&user), &cache).unwrap();
        assert!(!cache.join("LC_MESSAGES").is_dir());
        assert!(!fixture.path().join("LC_MESSAGES").exists());
    }

    #[test]
    fn invalid_user_files_are_ignored() {
        let fixture = TempDir::new().unwrap();
        let user = fixture.path().join("user");
        fs::create_dir_all(&user).unwrap();
        fs::write(user.join("broken.po"), "msgid \"Search\"\n").unwrap();
        fs::write(user.join("bad name.po"), po("bad name", "Invalid", "X")).unwrap();
        fs::write(user.join("...po"), po("..", "Invalid", "X")).unwrap();
        fs::write(user.join("..po"), po(".", "Invalid", "X")).unwrap();
        fs::write(user.join("notes.txt"), "not a catalog").unwrap();
        fs::write(user.join("xx.po"), po("xx", "Test", "Search")).unwrap();

        let languages = available_languages_in(&fixture.path().join("empty"), Some(&user));
        assert_eq!(languages.len(), 1);
        assert_eq!(languages[0].code, "xx");
        assert_eq!(languages[0].name, "Test");
    }

    #[test]
    fn staging_failure_leaves_the_directory_unusable() {
        let fixture = TempDir::new().unwrap();
        let cache = fixture.path().join("cache");
        fs::write(&cache, "not a directory").unwrap();
        assert!(stage_catalogs(&fixture.path().join("system"), None, &cache).is_err());
    }
}
