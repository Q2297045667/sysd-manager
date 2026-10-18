//! Locale mutation happens only in fresh, single-threaded subprocesses.
#[path = "../src/i18n.rs"]
mod i18n;

use gettextrs::{gettext, pgettext};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};
use systemd::enums::{NumMatchType, StrMatchType};
use test_base::TempDir;

fn main() {
    if env::args().nth(1).as_deref() == Some("--probe") {
        probe(&env::args().nth(2).expect("probe language"));
        return;
    }
    let fixture = TempDir::new().unwrap();
    let locale = TestLocales::new(fixture.path());
    let catalogs = PathBuf::from(env!("SYSD_LOCALE_DIR"));
    let mut count = 0;
    let mut run = |name: &str,
                   expected: &str,
                   variables: &[(&str, &str)],
                   directory: Option<&Path>,
                   cwd: Option<&Path>| {
        let mut command = Command::new(env::current_exe().unwrap());
        for (key, _) in env::vars_os() {
            if key.to_str().is_some_and(|key| {
                key.starts_with("LC_")
                    || ["LANG", "LANGUAGE", "LOCPATH", "TEXTDOMAINDIR"].contains(&key)
            }) {
                command.env_remove(key);
            }
        }
        if let Some(path) = &locale.path {
            command.env("LOCPATH", path);
        }
        command
            .arg("--probe")
            .arg(expected)
            .env("LANG", &locale.english);
        command
            .env("TEXTDOMAINDIR", &catalogs)
            .env("SYSD_EXPECTED_CATALOG_DIR", &catalogs);
        if let Some(directory) = directory {
            command
                .env("TEXTDOMAINDIR", directory)
                .env("SYSD_EXPECTED_CATALOG_DIR", directory);
        }
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        for (key, value) in variables {
            command.env(key, value);
        }
        let status = command.status().unwrap();
        assert!(status.success(), "locale probe failed: {name}");
        count += 1;
    };
    for (language, expected) in [
        ("zh_Hans", "zh"),
        ("zh_CN", "zh"),
        ("zh_SG", "zh"),
        ("zz:zh_CN:en", "zh"),
        ("en", "en"),
        ("zz", "en"),
        ("zh_TW:zh_HK:en", "en"),
    ] {
        run(
            language,
            expected,
            &[("LC_ALL", &locale.english), ("LANGUAGE", language)],
            None,
            None,
        );
    }
    run(
        "C disables LANGUAGE",
        "en",
        &[("LC_ALL", "C"), ("LANGUAGE", "zh_CN")],
        None,
        None,
    );
    for (language, expected) in [("zh_CN", "zh"), ("en", "en")] {
        run(
            "elapsed time across daylight-saving transitions",
            expected,
            &[("LANGUAGE", language), ("TZ", "CET-1CEST,M3.5.0,M10.5.0/3")],
            None,
            None,
        );
    }
    run(
        "native Chinese LANG",
        "zh",
        &[("LANG", &locale.chinese)],
        None,
        None,
    );
    run(
        "LC_MESSAGES precedes LANG",
        "zh",
        &[("LC_MESSAGES", &locale.chinese)],
        None,
        None,
    );
    run(
        "LC_ALL precedes LC_MESSAGES",
        "en",
        &[
            ("LC_ALL", &locale.english),
            ("LC_MESSAGES", &locale.chinese),
        ],
        None,
        None,
    );
    run(
        "LANGUAGE precedes message locale",
        "en",
        &[("LC_MESSAGES", &locale.chinese), ("LANGUAGE", "en")],
        None,
        None,
    );
    run(
        "empty LANGUAGE uses LANG",
        "zh",
        &[("LANG", &locale.chinese), ("LANGUAGE", "")],
        None,
        None,
    );
    run(
        "invalid locale uses English",
        "invalid",
        &[
            ("LC_ALL", "sysd_missing_LOCALE.UTF-8"),
            ("LANGUAGE", "zh_CN"),
        ],
        None,
        None,
    );
    run(
        "missing catalog",
        "en",
        &[("LANGUAGE", "zh_CN")],
        Some(&fixture.path().join("missing")),
        None,
    );
    run(
        "relative catalog from another cwd",
        "zh",
        &[("LANGUAGE", "zh_CN")],
        Some(Path::new(".")),
        Some(&catalogs),
    );
    let default_path = if cfg!(feature = "flatpak") {
        PathBuf::from("/app/share/locale")
    } else if cfg!(debug_assertions) && !cfg!(feature = "appimage") {
        catalogs.clone()
    } else {
        PathBuf::from("/usr/share/locale")
    };
    run(
        "empty override selects build default",
        "directory",
        &[
            ("TEXTDOMAINDIR", ""),
            ("SYSD_EXPECTED_CATALOG_DIR", default_path.to_str().unwrap()),
        ],
        None,
        None,
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let path = fixture
            .path()
            .join(std::ffi::OsString::from_vec(b"locale-\xff".to_vec()));
        let messages = path.join("zh_CN/LC_MESSAGES");
        fs::create_dir_all(&messages).unwrap();
        fs::copy(
            catalogs.join("zh_CN/LC_MESSAGES/sysd-manager.mo"),
            messages.join("sysd-manager.mo"),
        )
        .unwrap();
        run(
            "non-UTF-8 catalog path",
            "zh",
            &[("LANGUAGE", "zh_CN")],
            Some(&path),
            None,
        );
    }
    let source = fixture.path().join("plural.po");
    fs::write(&source, r#"msgid ""
msgstr ""
"Language: ru\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

msgctxt "time"
msgid "day"
msgid_plural "days"
msgstr[0] "день"
msgstr[1] "дня"
msgstr[2] "дней"

msgctxt "verb"
msgid "Open"
msgstr "Открыть"

msgctxt "noun"
msgid "Open"
msgstr "Открытие"
"#).unwrap();
    let directory = fixture.path().join("plural");
    let output = directory.join("ru/LC_MESSAGES/sysd-manager.mo");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    assert!(
        Command::new("msgfmt")
            .arg("-o")
            .arg(output)
            .arg(source)
            .status()
            .unwrap()
            .success()
    );
    run(
        "catalog-specific plural rules and contexts",
        "plural",
        &[("LANGUAGE", "ru")],
        Some(&directory),
        None,
    );
    println!("i18n: {count} isolated locale, path, plural and fallback probes passed");
}

struct TestLocales {
    english: String,
    chinese: String,
    path: Option<PathBuf>,
}
impl TestLocales {
    fn new(fixture: &Path) -> Self {
        let output = Command::new("locale")
            .arg("-a")
            .env_remove("LOCPATH")
            .output()
            .unwrap();
        let available = String::from_utf8(output.stdout).unwrap();
        let find = |prefix: &str| {
            available
                .lines()
                .find(|name| {
                    let name = name.to_ascii_lowercase().replace('-', "");
                    name.starts_with(prefix) && name.ends_with("utf8")
                })
                .map(str::to_owned)
        };
        if let (Some(english), Some(chinese)) = (find("en_us."), find("zh_cn.")) {
            return Self {
                english,
                chinese,
                path: None,
            };
        }
        let path = fixture.join("locales");
        fs::create_dir(&path).unwrap();
        for locale in ["en_US", "zh_CN"] {
            let status = Command::new("localedef")
                .args(["--no-archive", "-i", locale, "-f", "UTF-8"])
                .arg(path.join(format!("{locale}.UTF-8")))
                .status()
                .unwrap();
            assert!(status.success(), "generate test locale {locale}");
        }
        Self {
            english: "en_US.UTF-8".to_owned(),
            chinese: "zh_CN.UTF-8".to_owned(),
            path: Some(path),
        }
    }
}

fn probe(language: &str) {
    let start = Instant::now();
    // SAFETY: this is a fresh, single-threaded process.
    let result = unsafe { i18n::init() };
    if language == "invalid" {
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(gettext("Search"), "Search");
        return;
    }
    let path = result.expect("initialize gettext");
    let setup = start.elapsed();
    assert_eq!(path, env::var_os("SYSD_EXPECTED_CATALOG_DIR").unwrap());
    if language == "directory" {
        return;
    }
    if language == "plural" {
        for (days, expected) in [(2, "2 дня "), (5, "5 дней ")] {
            assert_eq!(
                systemd::time_handling::format_timestamp_relative_duration(
                    1_000_000,
                    1_000_000 + days * 86_400_000_000
                ),
                expected
            );
        }
        assert_eq!(pgettext("verb", "Open"), "Открыть");
        assert_eq!(pgettext("noun", "Open"), "Открытие");
        return;
    }

    let chinese = language == "zh";
    assert_eq!(gettext("Search"), if chinese { "搜索" } else { "Search" });
    assert_eq!(
        pgettext("controls", "Start"),
        if chinese { "启动" } else { "Start" }
    );
    // A missing context must not reuse the translation for another context.
    assert_eq!(pgettext("missing context", "Contains"), "Contains");
    assert_eq!(gettext("missing message"), "missing message");
    assert_eq!(
        systemd::format2!(pgettext("timer", "Add {}"), "OnBootSec"),
        if chinese {
            "添加 OnBootSec"
        } else {
            "Add OnBootSec"
        }
    );
    assert_eq!(
        StrMatchType::Contains.label(),
        if chinese { "包含" } else { "Contains" }
    );
    assert_eq!(
        NumMatchType::Greater.label(),
        if chinese { "大于" } else { "Greater than" }
    );
    // Localized display labels must not change filter persistence or selection.
    assert_eq!(StrMatchType::Contains.as_str(), "contains");
    assert_eq!(StrMatchType::from(0), StrMatchType::Contains);
    assert_eq!(NumMatchType::Greater.as_str(), "greater");
    assert_eq!(NumMatchType::from(1), NumMatchType::Greater);
    assert_eq!(
        systemd::time_handling::format_timespan(u64::MAX, 1000),
        if chinese { "无限" } else { "infinity" }
    );
    assert_eq!(
        systemd::time_handling::format_timestamp_relative_duration(1_000_000, 172_801_000_000),
        if chinese { "2 天 " } else { "2 days " }
    );
    // The civil clock jumps forward/backward, but both UTC intervals are 1h.
    for (begin, finish) in [
        (1_711_845_000_000_000, 1_711_848_600_000_000),
        (1_729_989_000_000_000, 1_729_992_600_000_000),
    ] {
        assert_eq!(
            systemd::time_handling::format_timestamp_relative_duration(begin, finish),
            "1h 0min "
        );
    }

    if env::var_os("SYSD_I18N_UI_TEST").is_some() {
        gtk::init().unwrap();
        let xml = r#"<interface>
                <object class="GtkLabel" id="label">
                    <property name="label" translatable="yes" context="controls">Start</property>
                </object>
            </interface>"#;
        for xml in [
            xml.to_owned(),
            xml.replace("<interface>", "<interface domain=\"sysd-manager\">"),
        ] {
            let builder = gtk::Builder::from_string(&xml);
            let label: gtk::Label = builder.object("label").unwrap();
            assert_eq!(label.label(), if chinese { "启动" } else { "Start" });
        }
    }

    if env::var_os("SYSD_I18N_BENCH").is_some() {
        // Warm gettext's catalog cache before measuring the existing lookup path.
        std::hint::black_box(pgettext("controls", "Start"));
        let start = Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(pgettext("controls", "Start"));
        }
        println!(
            "{language}: startup {setup:?}, 100000 cached lookups {:?}",
            start.elapsed()
        );
        let start = Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(systemd::time_handling::format_timestamp_relative_duration(
                std::hint::black_box(1_000_000),
                std::hint::black_box(172_801_000_000),
            ));
        }
        println!(
            "{language}: 100000 localized duration formats {:?}",
            start.elapsed()
        );
    }
}
