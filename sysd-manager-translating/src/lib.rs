use constcat::concat;
use std::{
    collections::HashSet,
    fs::{self, read_to_string},
    path::{Path, PathBuf},
    process::Command,
};
use tracing::{error, info};

use crate::error::TransError;
pub mod error;

pub const MAIN_PROG: &str = "sysd-manager";
pub const PO_DIR: &str = "./po";
pub const DESKTOP_DIR: &str = "./data/applications";
pub const DESKTOP_FILE: &str = "io.github.plrigaux.sysd-manager.desktop";
pub const DESKTOP_FILE_PATH: &str = concat!(DESKTOP_DIR, "/", DESKTOP_FILE);

pub const METAINFO_DIR: &str = "./data/metainfo";
pub const METAINFO_FILE: &str = "io.github.plrigaux.sysd-manager.metainfo.xml";
pub const METAINFO_FILE_PATH: &str = concat!(METAINFO_DIR, "/", METAINFO_FILE);
pub const POLICY_FILE: &str = "io.github.plrigaux.SysDManager.policy";
pub const POLICY_FILE_PATH: &str = concat!("./sysd-manager-proxy/data/", POLICY_FILE);

pub const PACK_FILE_DIR: &str = "target/loc";

/// Making the PO Template File
/// https://www.gnu.org/software/gettext/manual/html_node/xgettext-Invocation.html
pub fn xgettext(potfiles_file_path: &str, output_pot_file: &str) -> Result<(), TransError> {
    let mut command = Command::new("xgettext");

    for preset in glib_preset() {
        command.arg(preset);
    }

    let output = command
        .arg(format!("--files-from={potfiles_file_path}"))
        .arg(format!("--output={output_pot_file}"))
        .arg("--verbose")
        .output()
        .map_err(|error| TransError::create_command_error(command, error))?;

    display_output("XGETTEXT", output)?;

    Ok(())
}

fn display_output(id: &str, output: std::process::Output) -> Result<(), TransError> {
    info!("{id}: {:?}", output.status);
    info!("{id}: {}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        error!("{id}: {}", String::from_utf8_lossy(&output.stderr));
        return Err(TransError::CommandFailed(
            id.to_owned(),
            output.status,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(())
}

/// Creating a New PO File
/// https://www.gnu.org/software/gettext/manual/html_node/msginit-Invocation.html
pub fn msginit(input_pot_file: &str, output_file: &str, lang: &str) -> Result<(), TransError> {
    let mut command = Command::new("msginit");

    let output = command
        .arg(format!("--input={input_pot_file}"))
        .arg(format!("--output-file={output_file}"))
        .arg(format!("--locale={lang}"))
        .output()
        .map_err(|error| TransError::create_command_error(command, error))?;

    display_output("MSGINIT", output)?;
    Ok(())
}

//   /usr/bin/msgmerge --update --quiet  --lang=pt_BR --previous pt_BR.po hello-rust.pot
// rm -f pt_BR.gmo && /usr/bin/msgmerge --for-msgfmt -o pt_BR.1po pt_BR.po hello-rust.pot && /usr/bin/msgfmt -c --statistics --verbose -o pt_BR.gmo pt_BR.1po && rm -f pt_BR.1po
// pt_BR.1po: 2 translated messages.

/// https://www.gnu.org/software/gettext/manual/html_node/msgmerge-Invocation.html
pub fn msgmerge(input_pot_file: &str, output_file: &str) -> Result<(), TransError> {
    let mut command = Command::new("msgmerge");

    let output = command
        .arg("-o")
        .arg(output_file)
        .arg(output_file)
        .arg(input_pot_file)
        .arg("--verbose")
        .output()
        .map_err(|error| TransError::create_command_error(command, error))?;

    display_output("MSGMERGE", output)?;
    Ok(())
}

pub fn generate_mo() -> Result<(), TransError> {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    generate_mo_in(Path::new(PO_DIR), &target.join("locale"))
}

/// Locate the shared catalog directory from Cargo's profile-specific OUT_DIR.
/// Explicit targets add a target-triple directory; packaging still expects
/// catalogs at the Cargo target root, alongside debug/release or that triple.
pub fn cargo_locale_dir(out_dir: &Path, target: &str) -> Option<PathBuf> {
    let mut root = out_dir.ancestors().nth(4)?;
    if root.file_name().is_some_and(|name| name == target) {
        root = root.parent()?;
    }
    Some(root.join("locale"))
}

fn po_files(po_dir: &Path) -> Result<Vec<PathBuf>, TransError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(po_dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "po") && path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn generate_mo_in(po_dir: &Path, locale_dir: &Path) -> Result<(), TransError> {
    for path in po_files(po_dir)? {
        let lang = path
            .file_stem()
            .ok_or_else(|| std::io::Error::other("missing language"))?;
        compile_catalog(
            &path,
            &locale_dir
                .join(lang)
                .join("LC_MESSAGES")
                .join("sysd-manager.mo"),
        )?;
    }
    if po_dir.join("zh_Hans.po").is_file() {
        for alias in ["zh_CN", "zh_SG"] {
            if po_dir.join(format!("{alias}.po")).is_file() {
                continue;
            }
            let directory = locale_dir.join(alias).join("LC_MESSAGES");
            fs::create_dir_all(&directory)?;
            fs::copy(
                locale_dir.join("zh_Hans/LC_MESSAGES/sysd-manager.mo"),
                directory.join("sysd-manager.mo"),
            )?;
        }
    }
    Ok(())
}

pub fn lingas_from_files() -> Result<HashSet<String>, TransError> {
    let linguas: HashSet<_> = po_files(Path::new(PO_DIR))?
        .into_iter()
        .filter_map(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();

    Ok(linguas)
}

pub fn lingas_from_lingua_file() -> Result<HashSet<String>, TransError> {
    let mut linguas_file = PathBuf::from(PO_DIR);
    linguas_file.push("LINGUAS");

    Ok(parse_linguas(&read_to_string(linguas_file)?))
}

fn parse_linguas(contents: &str) -> HashSet<String> {
    let mut linguas = HashSet::new();
    for line in contents
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
    {
        for lang in line.split_whitespace() {
            linguas.insert(lang.to_owned());
        }
    }

    linguas
}

pub fn generate_desktop() -> Result<(), TransError> {
    generate_packaged_template(DESKTOP_FILE_PATH, DESKTOP_FILE, TemplateKind::Desktop)
}

pub fn generate_metainfo() -> Result<(), TransError> {
    generate_packaged_template(METAINFO_FILE_PATH, METAINFO_FILE, TemplateKind::Xml)
}

pub fn generate_policy() -> Result<(), TransError> {
    generate_packaged_template(POLICY_FILE_PATH, POLICY_FILE, TemplateKind::Policy)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TemplateKind {
    Desktop,
    Xml,
    Policy,
}

fn generate_packaged_template(
    source: &str,
    filename: &str,
    kind: TemplateKind,
) -> Result<(), TransError> {
    generate_template(
        Path::new(PO_DIR),
        Path::new(source),
        &Path::new(PACK_FILE_DIR).join(filename),
        kind,
    )
}

// Stage locale aliases for metadata too: desktop entries and polkit do not
// resolve the script-only zh_Hans name from a zh_CN/zh_SG system locale.
fn generate_template(
    po_dir: &Path,
    template: &Path,
    output: &Path,
    kind: TemplateKind,
) -> Result<(), TransError> {
    let parent = output
        .parent()
        .ok_or_else(|| std::io::Error::other("missing output directory"))?;
    fs::create_dir_all(parent)?;
    let staged = parent.join(".catalogs");
    fs::create_dir_all(&staged)?;
    let mut languages = Vec::new();
    for source in po_files(po_dir)? {
        let name = source.file_name().unwrap();
        fs::copy(&source, staged.join(name))?;
        languages.push(source.file_stem().unwrap().to_string_lossy().into_owned());
    }
    if po_dir.join("zh_Hans.po").is_file() {
        for alias in ["zh_CN", "zh_SG"] {
            if !po_dir.join(format!("{alias}.po")).is_file() {
                fs::copy(
                    po_dir.join("zh_Hans.po"),
                    staged.join(format!("{alias}.po")),
                )?;
                languages.push(alias.to_owned());
            }
        }
    }
    languages.sort();
    fs::write(staged.join("LINGUAS"), languages.join("\n"))?;
    let mut command = Command::new("msgfmt");
    command.env("GETTEXTDATADIRS", po_dir.canonicalize()?);
    let command_output = command
        .arg("--check")
        .arg("--statistics")
        .arg("--verbose")
        .arg(if kind == TemplateKind::Desktop {
            "--desktop"
        } else {
            "--xml"
        })
        .arg("--template")
        .arg(template)
        .arg("-d")
        .arg(staged)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|error| TransError::create_command_error(command, error))?;

    display_output("MSGFMT", command_output)?;

    if kind == TemplateKind::Policy {
        // XML catalogs use BCP 47 names, whereas polkit looks up POSIX locale
        // names verbatim and then uses g_get_locale_variants(). AppStream must
        // retain BCP 47, so only adapt attributes in the policy output.
        let xml = fs::read_to_string(output)?;
        fs::write(output, policy_locale_names(xml, &languages))?;
    }

    Ok(())
}

fn policy_locale_names(mut xml: String, languages: &[String]) -> String {
    for language in languages {
        let tag = language.replace('_', "-");
        // Gettext versions differ in whether they add an implicit script.
        let canonical = match language.as_str() {
            "zh_CN" => "zh-Hans-CN",
            "zh_SG" => "zh-Hans-SG",
            _ => &tag,
        };
        let target = format!("xml:lang=\"{language}\"");
        for source in [&tag, canonical] {
            if source == language {
                continue;
            }
            let source = format!("xml:lang=\"{source}\"");
            if xml.contains(&source) {
                xml = xml.replace(&source, &target);
            }
        }
    }
    xml
}

// /usr/bin/msgfmt -c --statistics --verbose -o pt_BR.gmo pt_BR.1po && rm -f pt_BR.1po
/// Generates a binary message catalog from a textual translation description.
/// https://www.gnu.org/software/gettext/manual/html_node/msgfmt-Invocation.html
pub fn msgfmt(po_file: &str, lang: &str, domain_name: &str) -> Result<(), TransError> {
    compile_catalog(
        Path::new(po_file),
        &PathBuf::from("target/locale")
            .join(lang)
            .join("LC_MESSAGES")
            .join(format!("{domain_name}.mo")),
    )
}

pub fn compile_catalog(po_file: &Path, output: &Path) -> Result<(), TransError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut command = Command::new("msgfmt");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension(format!(
        "mo.tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let command_output = command
        .arg("--check")
        .arg("--statistics")
        .arg("--verbose")
        .arg("-o")
        .arg(&temporary)
        .arg(po_file)
        .output()
        .map_err(|error| TransError::create_command_error(command, error))?;

    let result = display_output("MSGFMT", command_output)
        .and_then(|()| fs::rename(&temporary, output).map_err(TransError::from));
    let _ = fs::remove_file(temporary);
    result
}

fn glib_preset() -> Vec<&'static str> {
    let v = vec![
        "--from-code=UTF-8",
        "--add-comments",
        // # https://developer.gnome.org/glib/stable/glib-I18N.html
        "--keyword=_",
        "--keyword=N_",
        "--keyword=C_:1c,2",
        "--keyword=NC_:1c,2",
        "--keyword=g_dcgettext:2",
        "--keyword=g_dngettext:2,3",
        "--keyword=g_dpgettext2:2c,3",
        "--flag=N_:1:pass-c-format",
        "--flag=C_:2:pass-c-format",
        "--flag=NC_:2:pass-c-format",
        "--flag=g_dngettext:2:pass-c-format",
        "--flag=g_strdup_printf:1:c-format",
        "--flag=g_string_printf:2:c-format",
        "--flag=g_string_append_printf:2:c-format",
        "--flag=g_error_new:3:c-format",
        "--flag=g_set_error:4:c-format",
        "--flag=g_markup_printf_escaped:1:c-format",
        "--flag=g_log:3:c-format",
        "--flag=g_print:1:c-format",
        "--flag=g_printerr:1:c-format",
        "--flag=g_printf:1:c-format",
        "--flag=g_fprintf:2:c-format",
        "--flag=g_sprintf:2:c-format",
        "--flag=g_snprintf:3:c-format",
    ];
    v
}

#[cfg(test)]
mod tests;
