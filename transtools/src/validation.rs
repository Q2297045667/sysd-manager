//! Catalog validation shared by the CLI and its negative regression tests.
use quick_xml::{Reader, events::Event};
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs, io,
    path::Path,
    sync::LazyLock,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Counts = BTreeMap<String, usize>;
type Catalog = BTreeMap<(String, String), Entry>;

#[derive(Default, Debug)]
struct Entry {
    fields: BTreeMap<String, String>,
    fuzzy: bool,
}

fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

fn quoted_bytes(text: &str) -> Result<Vec<u8>> {
    let inner = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| invalid("invalid quoted PO string"))?;
    let mut chars = inner.chars().peekable();
    let mut bytes = Vec::new();
    while let Some(c) = chars.next() {
        if c == '"' {
            return Err(invalid("unescaped quote in PO string"));
        }
        if c != '\\' {
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| invalid("unfinished PO escape"))?;
        let value = match escaped {
            'n' => b'\n',
            'r' => b'\r',
            't' => b'\t',
            'a' => 7,
            'b' => 8,
            'f' => 12,
            'v' => 11,
            '\\' => b'\\',
            '"' => b'"',
            '\'' => b'\'',
            '0'..='7' | 'x' => {
                let radix = if escaped == 'x' { 16 } else { 8 };
                let mut digits = if escaped == 'x' {
                    String::new()
                } else {
                    escaped.to_string()
                };
                while chars.peek().is_some_and(|c| c.is_digit(radix))
                    && (radix == 16 || digits.len() < 3)
                {
                    digits.push(chars.next().unwrap());
                }
                u8::from_str_radix(&digits, radix).map_err(|_| invalid("invalid PO byte escape"))?
            }
            _ => return Err(invalid(format!("unsupported PO escape: \\{escaped}"))),
        };
        bytes.push(value);
    }
    Ok(bytes)
}

fn parse_catalog(contents: &str) -> Result<Catalog> {
    fn flush(raw: &mut BTreeMap<String, Vec<u8>>, fuzzy: bool, result: &mut Catalog) -> Result<()> {
        if raw.is_empty() {
            return Ok(());
        }
        let fields: BTreeMap<_, _> = std::mem::take(raw)
            .into_iter()
            .map(|(key, value)| String::from_utf8(value).map(|value| (key, value)))
            .collect::<std::result::Result<_, _>>()?;
        let id = fields
            .get("msgid")
            .ok_or_else(|| invalid("PO entry without msgid"))?;
        if id.is_empty() {
            return Ok(());
        } // Catalog header, checked by msgfmt.
        let key = (
            fields.get("msgctxt").cloned().unwrap_or_default(),
            id.clone(),
        );
        require(
            !result.contains_key(&key),
            format!("duplicate message: {key:?}"),
        )?;
        result.insert(key, Entry { fields, fuzzy });
        Ok(())
    }

    let mut result = Catalog::new();
    let mut raw = BTreeMap::<String, Vec<u8>>::new();
    let mut field = None::<String>;
    let mut fuzzy = false;
    for (number, line) in contents.lines().chain(std::iter::once("")).enumerate() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut raw, fuzzy, &mut result)?;
            field = None;
            fuzzy = false;
        } else if let Some(flags) = line.strip_prefix("#,") {
            fuzzy |= flags.split([',', ' ', '\t']).any(|flag| flag == "fuzzy");
        } else if line.starts_with('#') {
            continue;
        } else {
            let quoted = if line.starts_with('"') {
                line
            } else {
                let (name, quoted) = line
                    .split_once(char::is_whitespace)
                    .ok_or_else(|| invalid(format!("line {}: invalid PO field", number + 1)))?;
                let plural_form = name
                    .strip_prefix("msgstr[")
                    .and_then(|s| s.strip_suffix(']'));
                require(
                    matches!(name, "msgctxt" | "msgid" | "msgid_plural" | "msgstr")
                        || plural_form.is_some_and(|s| {
                            !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
                        }),
                    format!("line {}: unknown PO field {name}", number + 1),
                )?;
                require(!raw.contains_key(name), format!("duplicate field: {name}"))?;
                raw.insert(name.to_owned(), Vec::new());
                field = Some(name.to_owned());
                quoted.trim_start()
            };
            let name = field
                .as_ref()
                .ok_or_else(|| invalid("PO continuation without a field"))?;
            raw.get_mut(name).unwrap().extend(quoted_bytes(quoted)?);
        }
    }
    Ok(result)
}

fn placeholders(text: &str) -> Result<Counts> {
    let mut result = Counts::new();
    let mut automatic = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if (c == '{' || c == '}') && chars.peek().is_some_and(|(_, next)| *next == c) {
            chars.next();
            continue;
        }
        if c == '}' {
            return Err(invalid("unmatched closing format brace"));
        }
        if c != '{' {
            continue;
        }
        let mut depth = 0;
        let end = loop {
            let (index, c) = chars
                .next()
                .ok_or_else(|| invalid("unclosed format field"))?;
            match c {
                '{' => depth += 1,
                '}' if depth == 0 => break index,
                '}' => depth -= 1,
                _ => (),
            }
        };
        let raw = &text[start + 1..end];
        let split = raw.find([':', '!']).unwrap_or(raw.len());
        let field = if split == 0 {
            let field = automatic.to_string();
            automatic += 1;
            field
        } else {
            raw[..split].to_owned()
        };
        *result
            .entry(format!("{field}{}", &raw[split..]))
            .or_default() += 1;
    }
    Ok(result)
}

fn markup(text: &str) -> Result<Counts> {
    static TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[A-Za-z][^>]*>").unwrap());
    let mut result = Counts::new();
    if !TAGS.is_match(text) {
        return Ok(result);
    }
    let xml = format!("<root>{text}</root>");
    let mut reader = Reader::from_str(&xml);
    loop {
        match reader.read_event()? {
            event @ (Event::Start(_) | Event::Empty(_)) => {
                let (start, kind) = match event {
                    Event::Start(start) => (start, "start"),
                    Event::Empty(start) => (start, "empty"),
                    _ => unreachable!(),
                };
                let attributes: BTreeMap<_, _> = start
                    .attributes()
                    .map(|attribute| {
                        let attribute = attribute?;
                        Ok((
                            attribute.key.as_ref().to_owned(),
                            attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                                .into_owned(),
                        ))
                    })
                    .collect::<Result<_>>()?;
                *result
                    .entry(format!("{kind}:{}:{attributes:?}", start.name().as_ref()))
                    .or_default() += 1;
            }
            Event::End(end) => {
                *result
                    .entry(format!("end:{}", end.name().as_ref()))
                    .or_default() += 1
            }
            Event::GeneralRef(reference) => {
                require(
                    reference.resolve_char_ref()?.is_some()
                        || matches!(reference.as_ref(), "amp" | "lt" | "gt" | "quot" | "apos"),
                    "undefined XML entity",
                )?;
            }
            Event::Eof => break,
            _ => (),
        }
    }
    Ok(result)
}

fn mnemonics(text: &str) -> Counts {
    let mut result = Counts::new();
    let chars: Vec<_> = text.chars().collect();
    for (index, pair) in chars.windows(2).enumerate() {
        if pair[0] == '_'
            && pair[1].is_ascii_alphabetic()
            && (index == 0 || chars[index - 1] != '_')
        {
            *result
                .entry(pair[1].to_ascii_lowercase().to_string())
                .or_default() += 1;
        }
    }
    result
}

fn validate_message(source: &str, translated: &str) -> Result<()> {
    require(!translated.is_empty(), "untranslated message")?;
    require(
        placeholders(source)? == placeholders(translated)?,
        "format placeholders differ",
    )?;
    require(markup(source)? == markup(translated)?, "markup differs")?;
    require(
        mnemonics(source) == mnemonics(translated),
        "mnemonic differs",
    )
}

fn validate_catalog(template: &Catalog, chinese: &Catalog) -> Result<()> {
    require(
        template.keys().eq(chinese.keys()),
        "Chinese catalog differs from POT",
    )?;
    for (key, entry) in chinese {
        let result = (|| {
            require(!entry.fuzzy, "unreviewed translation")?;
            let plural = template[key].fields.get("msgid_plural");
            require(
                entry.fields.get("msgid_plural") == plural,
                "plural source differs",
            )?;
            let expected = if plural.is_some() {
                "msgstr[0]"
            } else {
                "msgstr"
            };
            let fields: Vec<_> = entry
                .fields
                .keys()
                .filter(|name| name.starts_with("msgstr"))
                .collect();
            require(fields == [expected], "translation forms differ")?;
            validate_message(plural.unwrap_or(&key.1), &entry.fields[expected])
        })();
        result.map_err(|error| invalid(format!("{key:?}: {error}")))?;
    }
    Ok(())
}

fn validate_interface(xml: &str) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event()? {
            Event::Start(start) if start.name().as_ref() == "property" => {
                let attributes: BTreeMap<_, _> = start
                    .attributes()
                    .map(|attribute| {
                        let attribute = attribute?;
                        Ok((
                            attribute.key.as_ref().to_owned(),
                            attribute
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                                .into_owned(),
                        ))
                    })
                    .collect::<Result<_>>()?;
                if !attributes.get("name").is_some_and(|name| {
                    matches!(
                        name.as_str(),
                        "label"
                            | "title"
                            | "subtitle"
                            | "tooltip-text"
                            | "tooltip-markup"
                            | "placeholder-text"
                    )
                }) {
                    continue;
                }
                let text = reader.read_text(start.name())?;
                let text = quick_xml::escape::unescape(&text)?;
                if matches!(
                    text.trim(),
                    "SysD Manager" | "test" | "link" | "Love" | "Hello friend"
                ) || !text.chars().any(char::is_alphabetic)
                {
                    continue;
                }
                require(
                    attributes
                        .get("translatable")
                        .is_some_and(|value| value == "yes"),
                    format!("unmarked UI text: {text:?}"),
                )?;
            }
            Event::Eof => break,
            _ => (),
        }
    }
    Ok(())
}

pub(super) fn check(root: &Path) -> Result<usize> {
    let template = parse_catalog(&fs::read_to_string(root.join("po/sysd-manager.pot"))?)?;
    let chinese = parse_catalog(&fs::read_to_string(root.join("po/zh_Hans.po"))?)?;
    validate_catalog(&template, &chinese)?;
    for path in super::list_files(root.join("data/interfaces"), "ui")? {
        validate_interface(&fs::read_to_string(&path)?)
            .map_err(|error| invalid(format!("{}: {error}", path.display())))?;
    }
    let scratch = tempfile::tempdir()?;
    translating::compile_catalog(&root.join("po/zh_Hans.po"), &scratch.path().join("zh.mo"))
        .map_err(|error| invalid(format!("{error:?}")))?;
    let output = scratch.path().join("source.pot");
    translating::xgettext_in(root, Path::new("po/POTFILES"), &output)
        .map_err(|error| invalid(format!("{error:?}")))?;
    let extracted = parse_catalog(&fs::read_to_string(output)?)?;
    require(
        extracted.keys().eq(template.keys()),
        "POT is stale; run cargo run -p transtools -- extract",
    )?;
    for (key, entry) in extracted {
        require(
            entry.fields.get("msgid_plural") == template[&key].fields.get("msgid_plural"),
            format!("POT plural source is stale: {key:?}"),
        )?;
    }
    let listed: BTreeSet<_> = fs::read_to_string(root.join("po/POTFILES"))?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.strip_prefix("./").unwrap_or(line).to_owned())
        .collect();
    for (directory, extension) in [
        ("src", "rs"),
        ("sysd-manager-comcontroler/src", "rs"),
        ("data/interfaces", "ui"),
    ] {
        for path in super::list_files(root.join(directory), extension)? {
            let relative = path.strip_prefix(root)?;
            require(
                listed.contains(
                    relative
                        .to_str()
                        .ok_or_else(|| invalid("non-UTF-8 source path"))?,
                ),
                format!("POTFILES missing: {}", relative.display()),
            )?;
        }
    }
    Ok(chinese.len())
}

#[cfg(test)]
mod tests;
