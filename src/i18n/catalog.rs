//! Portable Object parsing and Machine Object generation for runtime catalogs.
//!
//! Translation files dropped into the user translations directory are `.po`
//! (what Weblate produces) or already compiled `.mo` catalogs. gettext only
//! reads `.mo`, so `.po` files are parsed and compiled here without requiring
//! `msgfmt` to be installed on the user's machine.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MO_MAGIC: u32 = 0x9504_12de;
const MO_HEADER_SIZE: u32 = 28;
const MAX_PLURAL_FORMS: usize = 1000;
const DEFAULT_HEADER: &str = "Content-Type: text/plain; charset=UTF-8\n";

static TEMPORARY_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Catalog metadata, i.e. the fields of the header entry (`msgid ""`).
pub type Header = BTreeMap<String, String>;

/// Failure while reading, parsing or writing a translation catalog.
#[derive(Debug)]
pub enum CatalogError {
    /// The file could not be read or written.
    Io(std::io::Error),
    /// The file exists but is not a valid `.po` or `.mo` catalog.
    Invalid(String),
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::Io(error) => error.fmt(formatter),
            CatalogError::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for CatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CatalogError::Io(error) => Some(error),
            CatalogError::Invalid(_) => None,
        }
    }
}

impl From<std::io::Error> for CatalogError {
    fn from(error: std::io::Error) -> Self {
        CatalogError::Io(error)
    }
}

/// A message of a parsed PO catalog.
#[derive(Debug)]
struct Message {
    context: Option<String>,
    id: String,
    plural: Option<String>,
    forms: Vec<String>,
}

impl Message {
    /// Original string as stored in a MO file.
    ///
    /// gettext looks messages up with `strcmp`, so a context is joined with
    /// `\x04` and the plural form with a NUL byte.
    fn mo_key(&self) -> Vec<u8> {
        let mut key = Vec::new();
        if let Some(context) = &self.context {
            key.extend_from_slice(context.as_bytes());
            key.push(0x04);
        }
        key.extend_from_slice(self.id.as_bytes());
        if let Some(plural) = &self.plural {
            key.push(0);
            key.extend_from_slice(plural.as_bytes());
        }
        key
    }

    /// Translation as stored in a MO file; plural forms are NUL separated.
    fn mo_translation(&self) -> Vec<u8> {
        let mut translation = Vec::new();
        for (index, form) in self.forms.iter().enumerate() {
            if index > 0 {
                translation.push(0);
            }
            translation.extend_from_slice(form.as_bytes());
        }
        translation
    }
}

/// A parsed PO catalog.
#[derive(Debug)]
pub struct Catalog {
    header: Header,
    header_text: String,
    messages: Vec<Message>,
}

impl Catalog {
    /// Metadata declared by the catalog header, if the catalog has one.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Serialize the catalog as a little-endian revision 0 MO file.
    fn to_mo_bytes(&self) -> Result<Vec<u8>, CatalogError> {
        let header_text = if self.header_text.is_empty() {
            DEFAULT_HEADER.to_owned()
        } else {
            self.header_text.clone()
        };

        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(self.messages.len() + 1);
        entries.push((Vec::new(), header_text.into_bytes()));
        for message in &self.messages {
            entries.push((message.mo_key(), message.mo_translation()));
        }
        // gettext binary searches the original string table with strcmp. Rust
        // compares byte slices as unsigned bytes, which is the same order as
        // strcmp for the keys used here.
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        let count = to_u32(entries.len(), "the number of messages")?;
        let mut original_table = Vec::with_capacity(entries.len());
        let mut translation_table = Vec::with_capacity(entries.len());
        let mut original_pool = Vec::new();
        let mut translation_pool = Vec::new();

        for (original, translation) in &entries {
            let original_offset = to_u32(original_pool.len(), "the original strings")?;
            original_table.push((
                to_u32(original.len(), "an original string")?,
                original_offset,
            ));
            original_pool.extend_from_slice(original);
            original_pool.push(0);

            let translation_offset = to_u32(translation_pool.len(), "the translations")?;
            translation_table.push((
                to_u32(translation.len(), "a translation")?,
                translation_offset,
            ));
            translation_pool.extend_from_slice(translation);
            translation_pool.push(0);
        }

        let original_table_offset = MO_HEADER_SIZE;
        let translation_table_offset = add_offset(
            original_table_offset,
            table_size(count)?,
            "the original table",
        )?;
        let original_pool_offset = add_offset(
            translation_table_offset,
            table_size(count)?,
            "the translation table",
        )?;
        let translation_pool_offset = add_offset(
            original_pool_offset,
            to_u32(original_pool.len(), "the original strings")?,
            "the original strings",
        )?;
        // Offsets in the tables are absolute file offsets, not pool offsets.
        for (_, offset) in &mut original_table {
            *offset = add_offset(original_pool_offset, *offset, "an original string offset")?;
        }
        for (_, offset) in &mut translation_table {
            *offset = add_offset(
                translation_pool_offset,
                *offset,
                "a translation string offset",
            )?;
        }
        let total_size = add_offset(
            translation_pool_offset,
            to_u32(translation_pool.len(), "the translations")?,
            "the translations",
        )?;

        let mut bytes = Vec::with_capacity(total_size as usize);
        push_u32(&mut bytes, MO_MAGIC);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, count);
        push_u32(&mut bytes, original_table_offset);
        push_u32(&mut bytes, translation_table_offset);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        for (length, offset) in &original_table {
            push_u32(&mut bytes, *length);
            push_u32(&mut bytes, *offset);
        }
        for (length, offset) in &translation_table {
            push_u32(&mut bytes, *length);
            push_u32(&mut bytes, *offset);
        }
        bytes.extend_from_slice(&original_pool);
        bytes.extend_from_slice(&translation_pool);
        debug_assert_eq!(bytes.len(), total_size as usize);
        Ok(bytes)
    }
}

/// Parse a PO file into a catalog.
pub fn parse_po_file(path: &Path) -> Result<Catalog, CatalogError> {
    let bytes = fs::read(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|error| {
        CatalogError::Invalid(format!(
            "{}: the file is not valid UTF-8: {error}",
            path.display()
        ))
    })?;
    parse_po(text).map_err(|error| error_in_path(error, path))
}

/// Compile a PO file into a gettext MO file, creating parent directories.
///
/// The MO file is written to a temporary sibling first and renamed into place,
/// so an interrupted compilation never leaves a half-written catalog behind.
pub fn compile_po_file(po_path: &Path, mo_path: &Path) -> Result<(), CatalogError> {
    let catalog = parse_po_file(po_path)?;
    let bytes = catalog.to_mo_bytes()?;
    write_mo_file(mo_path, &bytes)
}

/// Read the header fields of an existing MO catalog.
pub fn read_mo_header(mo_path: &Path) -> Result<Header, CatalogError> {
    let data = fs::read(mo_path)?;
    let catalog = MoCatalog::parse(&data).map_err(|error| error_in_path(error, mo_path))?;
    let text = catalog
        .header_text()
        .map_err(|error| error_in_path(error, mo_path))?;
    Ok(parse_header(text))
}

/// Human readable language name from a catalog header, if one is present.
///
/// `Language-Team` is the only stable metadata field carrying a name; the URL
/// that Weblate appends to it is removed.
pub fn display_name(header: &Header) -> Option<String> {
    let team = header_value(header, "Language-Team")?;
    let name = collapse_whitespace(&strip_angle_brackets(team));
    (!name.is_empty()).then_some(name)
}

/// Language code declared in a catalog header, if any.
pub fn header_language(header: &Header) -> Option<String> {
    let language = header_value(header, "Language")?.trim();
    (!language.is_empty()).then(|| language.to_owned())
}

/// Value of a header field; gettext header keywords are case insensitive.
fn header_value<'a>(header: &'a Header, name: &str) -> Option<&'a str> {
    if let Some(value) = header.get(name) {
        return Some(value);
    }
    header
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Remove `<...>` segments such as the URL Weblate appends to `Language-Team`.
fn strip_angle_brackets(value: &str) -> String {
    let mut stripped = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('<') {
        stripped.push_str(&remaining[..start]);
        match remaining[start..].find('>') {
            Some(end) => remaining = &remaining[start + end + 1..],
            None => {
                remaining = "";
                break;
            }
        }
    }
    stripped.push_str(remaining);
    stripped
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn error_in_path(error: CatalogError, path: &Path) -> CatalogError {
    match error {
        CatalogError::Invalid(message) => {
            CatalogError::Invalid(format!("{}: {message}", path.display()))
        }
        error => error,
    }
}

fn invalid(line: usize, message: impl Into<String>) -> CatalogError {
    CatalogError::Invalid(format!("line {}: {}", line.max(1), message.into()))
}

fn parse_header(text: &str) -> Header {
    let mut header = Header::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        header.insert(key.to_owned(), value.trim().to_owned());
    }
    header
}

/// Target of a PO string directive, used to append continuation lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Context,
    Id,
    Plural,
    Form(usize),
}

#[derive(Default)]
struct EntryBuilder {
    context: Option<Vec<u8>>,
    id: Option<Vec<u8>>,
    plural: Option<Vec<u8>>,
    forms: Vec<Option<Vec<u8>>>,
    indexed: Option<bool>,
    fuzzy: bool,
    line: usize,
    target: Option<Target>,
}

impl EntryBuilder {
    fn is_empty(&self) -> bool {
        self.context.is_none()
            && self.id.is_none()
            && self.plural.is_none()
            && self.forms.is_empty()
    }

    fn has_translation(&self) -> bool {
        self.forms.iter().any(Option::is_some)
    }

    fn touch(&mut self, line: usize) {
        if self.line == 0 {
            self.line = line;
        }
    }

    fn buffer(&mut self, line: usize) -> Result<&mut Vec<u8>, CatalogError> {
        let buffer = match self.target {
            Some(Target::Context) => self.context.as_mut(),
            Some(Target::Id) => self.id.as_mut(),
            Some(Target::Plural) => self.plural.as_mut(),
            Some(Target::Form(index)) => self.forms.get_mut(index).and_then(Option::as_mut),
            None => None,
        };
        buffer.ok_or_else(|| invalid(line, "continuation line without a message field"))
    }
}

/// Parse PO text. The caller is responsible for decoding the file as UTF-8.
fn parse_po(text: &str) -> Result<Catalog, CatalogError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut catalog = Catalog {
        header: Header::new(),
        header_text: String::new(),
        messages: Vec::new(),
    };
    let mut seen: BTreeMap<(Option<String>, String), usize> = BTreeMap::new();
    let mut builder = EntryBuilder::default();

    for (index, raw_line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.is_empty() {
            flush_entry(&mut builder, &mut catalog, &mut seen)?;
            continue;
        }

        if let Some(comment) = trimmed.strip_prefix('#') {
            // A comment after a complete entry belongs to the next entry.
            if builder.has_translation() {
                flush_entry(&mut builder, &mut catalog, &mut seen)?;
            }
            if let Some(flags) = comment.strip_prefix(',') {
                builder.fuzzy |= flags.split(',').any(|flag| flag.trim() == "fuzzy");
            }
            continue;
        }

        if trimmed.starts_with('"') {
            let bytes = decode_po_string(trimmed, line_number)?;
            builder.buffer(line_number)?.extend_from_slice(&bytes);
            continue;
        }

        let (keyword, rest) = split_directive(trimmed);
        match keyword {
            "msgctxt" => {
                if builder.context.is_some() {
                    return Err(invalid(line_number, "duplicate msgctxt"));
                }
                if builder.id.is_some() {
                    return Err(invalid(line_number, "msgctxt after msgid"));
                }
                builder.touch(line_number);
                builder.context = Some(Vec::new());
                set_directive_string(&mut builder, Target::Context, rest, line_number)?;
            }
            "msgid" => {
                if builder.id.is_some() && builder.has_translation() {
                    flush_entry(&mut builder, &mut catalog, &mut seen)?;
                }
                if builder.id.is_some() {
                    return Err(invalid(line_number, "duplicate msgid"));
                }
                builder.touch(line_number);
                builder.id = Some(Vec::new());
                set_directive_string(&mut builder, Target::Id, rest, line_number)?;
            }
            "msgid_plural" => {
                if builder.id.is_none() {
                    return Err(invalid(line_number, "msgid_plural before msgid"));
                }
                if builder.plural.is_some() {
                    return Err(invalid(line_number, "duplicate msgid_plural"));
                }
                if builder.has_translation() {
                    return Err(invalid(line_number, "msgid_plural after msgstr"));
                }
                builder.plural = Some(Vec::new());
                set_directive_string(&mut builder, Target::Plural, rest, line_number)?;
            }
            "msgstr" => open_form(&mut builder, None, rest, line_number)?,
            _ => match plural_directive_index(keyword) {
                Some(index) => open_form(&mut builder, Some(index), rest, line_number)?,
                None => {
                    return Err(invalid(
                        line_number,
                        format!("unexpected directive {keyword:?}"),
                    ));
                }
            },
        }
    }

    flush_entry(&mut builder, &mut catalog, &mut seen)?;
    Ok(catalog)
}

fn split_directive(line: &str) -> (&str, &str) {
    match line.find(char::is_whitespace) {
        Some(index) => (&line[..index], line[index..].trim()),
        None => (line, ""),
    }
}

/// Parse `msgstr[N]` into its index.
fn plural_directive_index(keyword: &str) -> Option<usize> {
    let index = keyword.strip_prefix("msgstr[")?.strip_suffix(']')?;
    index.parse().ok()
}

fn set_directive_string(
    builder: &mut EntryBuilder,
    target: Target,
    text: &str,
    line: usize,
) -> Result<(), CatalogError> {
    let bytes = decode_po_string(text, line)?;
    builder.target = Some(target);
    builder.buffer(line)?.extend_from_slice(&bytes);
    Ok(())
}

fn open_form(
    builder: &mut EntryBuilder,
    index: Option<usize>,
    text: &str,
    line: usize,
) -> Result<(), CatalogError> {
    if builder.id.is_none() {
        return Err(invalid(line, "msgstr before msgid"));
    }
    match (builder.plural.is_some(), index) {
        (true, None) => return Err(invalid(line, "plural message requires msgstr[N]")),
        (false, Some(_)) => return Err(invalid(line, "msgstr[N] requires msgid_plural")),
        _ => {}
    }
    if let Some(indexed) = builder.indexed
        && indexed != index.is_some()
    {
        return Err(invalid(line, "cannot mix msgstr and msgstr[N]"));
    }
    builder.indexed = Some(index.is_some());

    let index = index.unwrap_or(0);
    if index >= MAX_PLURAL_FORMS {
        return Err(invalid(line, "plural form index is too large"));
    }
    if builder.forms.get(index).is_some_and(Option::is_some) {
        return Err(invalid(line, format!("duplicate msgstr[{index}]")));
    }
    builder.forms.resize(index + 1, None);
    let bytes = decode_po_string(text, line)?;
    builder.forms[index] = Some(bytes);
    builder.target = Some(Target::Form(index));
    Ok(())
}

fn flush_entry(
    builder: &mut EntryBuilder,
    catalog: &mut Catalog,
    seen: &mut BTreeMap<(Option<String>, String), usize>,
) -> Result<(), CatalogError> {
    let builder = std::mem::take(builder);
    if builder.is_empty() {
        return Ok(());
    }

    let line = builder.line.max(1);
    if builder.fuzzy {
        return Ok(());
    }

    let Some(id_bytes) = builder.id else {
        return Err(invalid(line, "message without msgid"));
    };
    if builder.forms.is_empty() {
        return Err(invalid(line, "message without msgstr"));
    }

    let id = decode_field(id_bytes, line, "msgid")?;
    let context = builder
        .context
        .map(|bytes| decode_field(bytes, line, "msgctxt"))
        .transpose()?;
    let plural = builder
        .plural
        .map(|bytes| decode_field(bytes, line, "msgid_plural"))
        .transpose()?;

    let mut forms = Vec::with_capacity(builder.forms.len());
    for (index, form) in builder.forms.into_iter().enumerate() {
        match form {
            Some(bytes) => forms.push(decode_field(bytes, line, "msgstr")?),
            None => return Err(invalid(line, format!("missing msgstr[{index}]"))),
        }
    }

    let key = (context, id);
    if let Some(first_line) = seen.get(&key) {
        return Err(invalid(
            line,
            format!(
                "duplicate message {} (first defined on line {first_line})",
                describe_key(&key)
            ),
        ));
    }
    seen.insert(key.clone(), line);

    if key.1.is_empty() && key.0.is_none() {
        if plural.is_some() {
            return Err(invalid(line, "the header entry cannot have a plural form"));
        }
        let header_text = forms.remove(0);
        if !header_text.is_empty() {
            catalog.header = parse_header(&header_text);
            catalog.header_text = header_text;
        }
        return Ok(());
    }

    if forms.iter().all(String::is_empty) {
        return Ok(());
    }

    catalog.messages.push(Message {
        context: key.0,
        id: key.1,
        plural,
        forms,
    });
    Ok(())
}

fn describe_key(key: &(Option<String>, String)) -> String {
    match &key.0 {
        Some(context) => format!("msgctxt {context:?} msgid {:?}", key.1),
        None => format!("msgid {:?}", key.1),
    }
}

fn decode_field(bytes: Vec<u8>, line: usize, name: &str) -> Result<String, CatalogError> {
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid(line, format!("{name} is not valid UTF-8")))?;
    if text.contains('\0') {
        return Err(invalid(line, format!("{name} contains a NUL byte")));
    }
    Ok(text)
}

/// Decode the quoted string of a PO line, including continuation content.
fn decode_po_string(text: &str, line: usize) -> Result<Vec<u8>, CatalogError> {
    let text = text.trim();
    let Some(inner) = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Err(invalid(line, "expected a double quoted string"));
    };

    let bytes = inner.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte != b'\\' {
            let Some(character) = inner[index..].chars().next() else {
                return Err(invalid(line, "invalid UTF-8 in string"));
            };
            if character == '"' {
                return Err(invalid(line, "unescaped quote in string"));
            }
            if character == '\0' {
                return Err(invalid(line, "NUL byte in string"));
            }
            let mut buffer = [0; 4];
            decoded.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            index += character.len_utf8();
            continue;
        }

        index += 1;
        let Some(&escape) = bytes.get(index) else {
            return Err(invalid(line, "trailing backslash in string"));
        };
        index += 1;
        match escape {
            b'n' => decoded.push(b'\n'),
            b't' => decoded.push(b'\t'),
            b'r' => decoded.push(b'\r'),
            b'a' => decoded.push(0x07),
            b'b' => decoded.push(0x08),
            b'f' => decoded.push(0x0c),
            b'v' => decoded.push(0x0b),
            b'\\' => decoded.push(b'\\'),
            b'"' => decoded.push(b'"'),
            b'\'' => decoded.push(b'\''),
            b'?' => decoded.push(b'?'),
            b'x' | b'X' => {
                let (value, digits) = take_digits(bytes, &mut index, 16, 2);
                if digits == 0 {
                    return Err(invalid(line, "invalid hexadecimal escape"));
                }
                decoded.push(
                    u8::try_from(value)
                        .map_err(|_| invalid(line, "hexadecimal escape is out of range"))?,
                );
            }
            b'0'..=b'7' => {
                let mut value = u32::from(escape - b'0');
                let mut digits = 1;
                while digits < 3 {
                    let Some(&next) = bytes.get(index) else {
                        break;
                    };
                    if !(b'0'..=b'7').contains(&next) {
                        break;
                    }
                    value = value * 8 + u32::from(next - b'0');
                    index += 1;
                    digits += 1;
                }
                decoded.push(
                    u8::try_from(value)
                        .map_err(|_| invalid(line, "octal escape is out of range"))?,
                );
            }
            _ => {
                return Err(invalid(
                    line,
                    format!("invalid escape sequence \\{}", char::from(escape)),
                ));
            }
        }
    }
    Ok(decoded)
}

fn take_digits(bytes: &[u8], index: &mut usize, radix: u32, limit: usize) -> (u32, usize) {
    let mut value = 0;
    let mut digits = 0;
    while digits < limit {
        let Some(&byte) = bytes.get(*index) else {
            break;
        };
        let Some(digit) = char::from(byte).to_digit(radix) else {
            break;
        };
        value = value * radix + digit;
        *index += 1;
        digits += 1;
    }
    (value, digits)
}

fn to_u32(value: usize, what: &str) -> Result<u32, CatalogError> {
    u32::try_from(value)
        .map_err(|_| CatalogError::Invalid(format!("{what} are too large for the MO format")))
}

fn table_size(count: u32) -> Result<u32, CatalogError> {
    count
        .checked_mul(8)
        .ok_or_else(|| CatalogError::Invalid("the MO string table is too large".to_owned()))
}

fn add_offset(left: u32, right: u32, what: &str) -> Result<u32, CatalogError> {
    left.checked_add(right)
        .ok_or_else(|| CatalogError::Invalid(format!("{what} are too large for the MO format")))
}

fn push_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn temporary_path(mo_path: &Path) -> PathBuf {
    let counter = TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = mo_path.file_name().map_or_else(
        || "catalog.mo".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    mo_path.with_file_name(format!(".{name}.{}.{counter}.tmp", std::process::id()))
}

fn write_mo_file(mo_path: &Path, contents: &[u8]) -> Result<(), CatalogError> {
    let parent = mo_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let temporary = temporary_path(mo_path);
    let result = fs::write(&temporary, contents).and_then(|()| fs::rename(&temporary, mo_path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(CatalogError::Io)
}

/// Byte order of an existing MO file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endian {
    Little,
    Big,
}

/// Structural reader for MO files, used for the header and by the tests.
struct MoCatalog<'a> {
    data: &'a [u8],
    originals: Vec<(u32, u32)>,
    translations: Vec<(u32, u32)>,
}

impl<'a> MoCatalog<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, CatalogError> {
        if data.len() < MO_HEADER_SIZE as usize {
            return Err(CatalogError::Invalid("truncated MO header".to_owned()));
        }

        let endian = match data.get(..4) {
            Some([0xde, 0x12, 0x04, 0x95]) => Endian::Little,
            Some([0x95, 0x04, 0x12, 0xde]) => Endian::Big,
            _ => {
                return Err(CatalogError::Invalid(
                    "not a gettext MO file: bad magic number".to_owned(),
                ));
            }
        };

        let revision = read_word(data, 4, endian)?;
        if revision > 1 {
            return Err(CatalogError::Invalid(format!(
                "unsupported MO revision {revision}"
            )));
        }

        let count = read_word(data, 8, endian)?;
        let original_table = read_word(data, 12, endian)?;
        let translation_table = read_word(data, 16, endian)?;
        validate_table(data, original_table, count)?;
        validate_table(data, translation_table, count)?;

        let count = usize::try_from(count)
            .map_err(|_| CatalogError::Invalid("MO string table is too large".to_owned()))?;
        let mut originals = Vec::with_capacity(count);
        let mut translations = Vec::with_capacity(count);

        for index in 0..count {
            let record = original_table as usize + index * 8;
            let length = read_word(data, record, endian)?;
            let offset = read_word(data, record + 4, endian)?;
            check_string_range(data.len(), offset, length)?;
            originals.push((length, offset));
        }
        for index in 0..count {
            let record = translation_table as usize + index * 8;
            let length = read_word(data, record, endian)?;
            let offset = read_word(data, record + 4, endian)?;
            check_string_range(data.len(), offset, length)?;
            translations.push((length, offset));
        }

        Ok(Self {
            data,
            originals,
            translations,
        })
    }

    /// Header text: the translation of the entry whose original string is empty.
    fn header_text(&self) -> Result<&'a str, CatalogError> {
        let index = self
            .originals
            .iter()
            .position(|(length, _)| *length == 0)
            .ok_or_else(|| CatalogError::Invalid("MO file has no header entry".to_owned()))?;
        let (length, offset) = self.translations[index];
        let start = offset as usize;
        let end = start + length as usize;
        let bytes = self.data.get(start..end).ok_or_else(|| {
            CatalogError::Invalid("MO header translation is outside the file".to_owned())
        })?;
        std::str::from_utf8(bytes)
            .map_err(|_| CatalogError::Invalid("MO header is not valid UTF-8".to_owned()))
    }

    /// All original/translation pairs in table order.
    #[cfg(test)]
    fn entries(&self) -> Vec<(&'a [u8], &'a [u8])> {
        self.originals
            .iter()
            .zip(&self.translations)
            .map(
                |(
                    &(original_length, original_offset),
                    &(translation_length, translation_offset),
                )| {
                    let original_start = original_offset as usize;
                    let translation_start = translation_offset as usize;
                    (
                        &self.data[original_start..original_start + original_length as usize],
                        &self.data
                            [translation_start..translation_start + translation_length as usize],
                    )
                },
            )
            .collect()
    }
}

fn read_word(data: &[u8], offset: usize, endian: Endian) -> Result<u32, CatalogError> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .ok_or_else(|| CatalogError::Invalid("truncated MO file".to_owned()))?
        .try_into()
        .map_err(|_| CatalogError::Invalid("truncated MO file".to_owned()))?;
    Ok(match endian {
        Endian::Little => u32::from_le_bytes(bytes),
        Endian::Big => u32::from_be_bytes(bytes),
    })
}

fn validate_table(data: &[u8], offset: u32, count: u32) -> Result<(), CatalogError> {
    let end = u64::from(offset)
        .checked_add(u64::from(count) * 8)
        .ok_or_else(|| CatalogError::Invalid("MO offset table overflows".to_owned()))?;
    if end > data.len() as u64 {
        return Err(CatalogError::Invalid(
            "MO offset table lies outside the file".to_owned(),
        ));
    }
    Ok(())
}

fn check_string_range(file_len: usize, offset: u32, length: u32) -> Result<(), CatalogError> {
    let end = u64::from(offset) + u64::from(length);
    if end > file_len as u64 {
        return Err(CatalogError::Invalid(
            "MO string lies outside the file".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        path::PathBuf,
        process::{Command, Stdio},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    const FULL_PO: &str = r#"msgid ""
    msgstr ""
    "Project-Id-Version: sysd-manager 2.24.0\n"
    "Language-Team: French <https://hosted.weblate.org/projects/sysd-manager/translation/fr/>\n"
    "Language: fr\n"
    "MIME-Version: 1.0\n"
    "Content-Type: text/plain; charset=UTF-8\n"
    "Content-Transfer-Encoding: 8bit\n"
    "Plural-Forms: nplurals=2; plural=(n > 1);\n"

    #. A normal message
    #: src/main.rs:10
    msgid "Start"
    msgstr "Démarrer"

    msgctxt "menu"
    msgid "Open"
    msgstr "Ouvrir"

    msgctxt "verb"
    msgid "Open"
    msgstr "Ouvrir le fichier"

    msgid "one file"
    msgid_plural "many files"
    msgstr[0] "un fichier"
    msgstr[1] "des fichiers"
    "#;

    const PLURAL_PO: &str = r#"msgid ""
    msgstr ""
    "Language: ru\n"
    "Content-Type: text/plain; charset=UTF-8\n"
    "Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\n"

    msgid "file"
    msgid_plural "files"
    msgstr[0] "fajl"
    msgstr[1] "fajla"
    msgstr[2] "fajlov"
    "#;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = env::temp_dir().join(format!(
                "sysd-manager-catalog-{}-{nanos}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create a temporary directory");
            Self { path }
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            self.write_bytes(name, contents.as_bytes())
        }

        fn write_bytes(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.path.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create temporary directories");
            }
            fs::write(&path, contents).expect("write a temporary file");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn compile(po: &str) -> Vec<u8> {
        let directory = TempDir::new();
        let po_path = directory.write("catalog.po", po);
        let mo_path = directory.path.join("catalog.mo");
        compile_po_file(&po_path, &mo_path).expect("compile the PO file");
        fs::read(&mo_path).expect("read the compiled MO file")
    }

    fn entries_of(data: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
        MoCatalog::parse(data)
            .expect("parse the generated MO file")
            .entries()
            .into_iter()
            .map(|(original, translation)| (original.to_vec(), translation.to_vec()))
            .collect()
    }

    fn translation_map(data: &[u8]) -> BTreeMap<Vec<u8>, Vec<u8>> {
        entries_of(data).into_iter().collect()
    }

    fn error_message(po: &str) -> String {
        match parse_po(po) {
            Err(CatalogError::Invalid(message)) => message,
            Err(error) => panic!("expected an invalid catalog, got {error:?}"),
            Ok(_) => panic!("expected the PO file to be rejected"),
        }
    }

    fn command_works(program: &str) -> bool {
        Command::new(program)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn parses_header_and_basic_messages() {
        let directory = TempDir::new();
        let po_path = directory.write("fr.po", FULL_PO);
        let catalog = parse_po_file(&po_path).expect("parse the PO file");

        assert_eq!(
            catalog.header().get("Language").map(String::as_str),
            Some("fr")
        );
        assert_eq!(
            catalog.header().get("Plural-Forms").map(String::as_str),
            Some("nplurals=2; plural=(n > 1);")
        );
        assert_eq!(
            catalog
                .header()
                .get("Content-Transfer-Encoding")
                .map(String::as_str),
            Some("8bit")
        );
        assert_eq!(display_name(catalog.header()).as_deref(), Some("French"));
        assert_eq!(header_language(catalog.header()).as_deref(), Some("fr"));

        let mo = compile(FULL_PO);
        let messages = translation_map(&mo);
        assert_eq!(
            messages.get(b"Start".as_slice()).map(Vec::as_slice),
            Some("Démarrer".as_bytes())
        );
        let header_text = messages
            .get(b"".as_slice())
            .expect("the MO file has a header entry");
        let header_text = String::from_utf8(header_text.clone()).expect("the header is UTF-8");
        assert!(header_text.contains("Content-Type: text/plain; charset=UTF-8\n"));
        assert!(header_text.contains("Plural-Forms: nplurals=2; plural=(n > 1);\n"));
        assert!(header_text.contains("Language-Team: French <https://"));
    }

    #[test]
    fn encodes_contexts_and_plural_forms_for_gettext() {
        let mo = compile(FULL_PO);
        let messages = translation_map(&mo);

        assert_eq!(
            messages.get(b"menu\x04Open".as_slice()).map(Vec::as_slice),
            Some("Ouvrir".as_bytes())
        );
        assert_eq!(
            messages.get(b"verb\x04Open".as_slice()).map(Vec::as_slice),
            Some("Ouvrir le fichier".as_bytes())
        );
        assert_eq!(
            messages
                .get(b"one file\0many files".as_slice())
                .map(Vec::as_slice),
            Some("un fichier\0des fichiers".as_bytes())
        );
    }

    #[test]
    fn supports_three_plural_forms() {
        let mo = compile(PLURAL_PO);
        let messages = translation_map(&mo);
        assert_eq!(
            messages.get(b"file\0files".as_slice()).map(Vec::as_slice),
            Some(b"fajl\0fajla\0fajlov".as_slice())
        );
        let header_text = messages
            .get(b"".as_slice())
            .expect("the MO file has a header entry");
        let header_text = String::from_utf8(header_text.clone()).expect("the header is UTF-8");
        assert!(header_text.contains("nplurals=3"));
    }

    #[test]
    fn writes_strcmp_ordered_tables() {
        let mo = compile(FULL_PO);
        let entries = entries_of(&mo);
        assert!(
            entries
                .first()
                .is_some_and(|(original, _)| original.is_empty())
        );
        for pair in entries.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "MO original strings are not sorted: {:?} >= {:?}",
                pair[0].0,
                pair[1].0
            );
        }
    }

    #[test]
    fn decodes_escapes_and_continuations() {
        let po = r#"msgid ""
        msgstr ""
        "Content-Type: text/plain; charset=UTF-8\n"

        msgid "line\nbreak\ttab \"quote\" back\\slash oct\101 hex\x41"
        msgstr "ok"

        msgid "con"
        "tinued"
        msgstr "con"
        "tinue"
        "#;

        let catalog = parse_po(po).expect("parse escaped strings");
        assert_eq!(
            catalog.messages[0].id,
            "line\nbreak\ttab \"quote\" back\\slash octA hexA"
        );
        assert_eq!(catalog.messages[0].forms[0], "ok");
        assert_eq!(catalog.messages[1].id, "continued");
        assert_eq!(catalog.messages[1].forms[0], "continue");
    }

    #[test]
    fn skips_fuzzy_and_empty_translations() {
        let po = r#"msgid ""
        msgstr ""
        "Content-Type: text/plain; charset=UTF-8\n"

        #, fuzzy
        msgid "Draft"
        msgstr "Brouillon"

        #, fuzzy, c-format
        msgid "Draft %s"
        msgstr "Brouillon %s"

        msgid "Empty"
        msgstr ""

        msgid "Kept"
        msgstr "Gardé"
        "#;

        let mo = compile(po);
        let messages = translation_map(&mo);
        assert_eq!(messages.len(), 2);
        assert!(messages.contains_key(b"Kept".as_slice()));
        assert!(!messages.contains_key(b"Draft".as_slice()));
        assert!(!messages.contains_key(b"Empty".as_slice()));
    }

    #[test]
    fn reports_syntax_errors_with_line_numbers() {
        let cases = [
            ("msgid \"unterminated\n", "line 1"),
            (
                "msgid \"\"\nmsgstr \"\"\n\nmsgid \"bad\\qescape\"\nmsgstr \"x\"\n",
                "line 4",
            ),
            ("msgid \"no translation\"\n", "line 1"),
            (
                "msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"a\"\nmsgstr \"c\"\n",
                "line 4",
            ),
            ("msgid \"a\"\nmsgctxt \"late\"\nmsgstr \"b\"\n", "line 2"),
            (
                "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\nmsgid \"a\"\nmsgstr[0] \"b\"\n",
                "line 6",
            ),
            (
                "msgid \"a\"\nmsgid_plural \"as\"\nmsgstr[0] \"b\"\nmsgstr[2] \"d\"\n",
                "line 1",
            ),
        ];

        for (po, expected) in cases {
            let message = error_message(po);
            assert!(
                message.contains(expected),
                "expected {expected:?} in {message:?}"
            );
        }
    }

    #[test]
    fn round_trips_po_through_mo_header() {
        let directory = TempDir::new();
        let po_path = directory.write("fr.po", FULL_PO);
        let mo_path = directory.path.join("fr.mo");
        compile_po_file(&po_path, &mo_path).expect("compile the PO file");

        let parsed = parse_po_file(&po_path).expect("parse the PO file");
        let header = read_mo_header(&mo_path).expect("read the MO header");
        assert_eq!(&header, parsed.header());
        assert_eq!(
            header.get("Content-Type").map(String::as_str),
            Some("text/plain; charset=UTF-8")
        );
        assert_eq!(
            header.get("Plural-Forms").map(String::as_str),
            Some("nplurals=2; plural=(n > 1);")
        );
    }

    #[test]
    fn creates_parent_directories_and_synthesizes_a_header() {
        let directory = TempDir::new();
        let po_path = directory.write("plain.po", "msgid \"Hello\"\nmsgstr \"Bonjour\"\n");
        let mo_path = directory.path.join("a/b/c/catalog.mo");
        compile_po_file(&po_path, &mo_path).expect("compile into nested directories");

        assert!(mo_path.is_file());
        let siblings = fs::read_dir(mo_path.parent().expect("the MO file has a parent"))
            .expect("list the catalog directory")
            .count();
        assert_eq!(siblings, 1, "temporary files must not be left behind");

        let header = read_mo_header(&mo_path).expect("read the synthesized header");
        assert!(
            header
                .get("Content-Type")
                .is_some_and(|value| value.contains("charset=UTF-8"))
        );
        let messages = translation_map(&fs::read(&mo_path).expect("read the MO file"));
        assert_eq!(
            messages.get(b"Hello".as_slice()).map(Vec::as_slice),
            Some("Bonjour".as_bytes())
        );
    }

    #[test]
    fn accepts_byte_order_mark_and_crlf() {
        let po = concat!(
            "\u{feff}msgid \"\"\r\n",
            "msgstr \"\"\r\n",
            "\"Content-Type: text/plain; charset=UTF-8\\n\"\r\n",
            "\r\n",
            "msgid \"Hello\"\r\n",
            "msgstr \"Bonjour\"\r\n",
        );
        let messages = translation_map(&compile(po));
        assert_eq!(
            messages.get(b"Hello".as_slice()).map(Vec::as_slice),
            Some("Bonjour".as_bytes())
        );
    }

    #[test]
    fn formats_language_metadata() {
        let mut header = Header::new();
        assert_eq!(display_name(&header), None);
        assert_eq!(header_language(&header), None);

        header.insert(
            "Language-Team".to_owned(),
            "French <https://hosted.weblate.org/projects/sysd-manager/translation/fr/>".to_owned(),
        );
        assert_eq!(display_name(&header).as_deref(), Some("French"));
        header.insert(
            "Language-Team".to_owned(),
            "  German   translation  team ".to_owned(),
        );
        assert_eq!(
            display_name(&header).as_deref(),
            Some("German translation team")
        );
        header.insert("Language-Team".to_owned(), "  ".to_owned());
        assert_eq!(display_name(&header), None);

        header.remove("Language-Team");
        header.insert("language-team".to_owned(), "Lowercase Team".to_owned());
        assert_eq!(display_name(&header).as_deref(), Some("Lowercase Team"));

        header.insert("Language".to_owned(), " fr ".to_owned());
        assert_eq!(header_language(&header).as_deref(), Some("fr"));
        header.insert("Language".to_owned(), "   ".to_owned());
        assert_eq!(header_language(&header), None);
    }

    #[test]
    fn reads_big_endian_revision_one_catalogs() {
        let header_text = b"Language: fr\nContent-Type: text/plain; charset=UTF-8\n";
        let mo = build_mo(
            &[
                (b"".as_slice(), header_text.as_slice()),
                (b"Hello", b"Bonjour"),
            ],
            Endian::Big,
            1,
        );
        let directory = TempDir::new();
        let mo_path = directory.write_bytes("big-endian.mo", &mo);
        let header = read_mo_header(&mo_path).expect("read the big endian MO file");
        assert_eq!(header.get("Language").map(String::as_str), Some("fr"));
        assert_eq!(
            header.get("Content-Type").map(String::as_str),
            Some("text/plain; charset=UTF-8")
        );
    }

    #[test]
    fn rejects_corrupted_mo_files() {
        let valid = build_mo(
            &[(b"".as_slice(), b"Language: fr\n"), (b"Hello", b"Bonjour")],
            Endian::Little,
            0,
        );
        let directory = TempDir::new();

        let mut bad_magic = valid.clone();
        bad_magic[..4].copy_from_slice(&[0, 1, 2, 3]);
        assert_mo_error(&directory, "magic.mo", &bad_magic, "magic");

        let mut bad_revision = valid.clone();
        bad_revision[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert_mo_error(&directory, "revision.mo", &bad_revision, "revision");

        let mut bad_table = valid.clone();
        bad_table[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_mo_error(&directory, "table.mo", &bad_table, "offset table");

        let mut bad_string = valid;
        bad_string[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_mo_error(&directory, "string.mo", &bad_string, "outside the file");

        let invalid_utf8 = build_mo(&[(b"".as_slice(), &[0xff, 0xfe][..])], Endian::Little, 0);
        assert_mo_error(&directory, "utf8.mo", &invalid_utf8, "UTF-8");

        let no_header = build_mo(&[(b"Hello", b"Bonjour")], Endian::Little, 0);
        assert_mo_error(&directory, "header.mo", &no_header, "header");

        assert_mo_error(
            &directory,
            "short.mo",
            &[0xde, 0x12, 0x04, 0x95],
            "truncated",
        );
        assert_mo_error(&directory, "empty.mo", b"", "truncated");
    }

    #[test]
    fn matches_msgfmt_output_when_available() {
        if !command_works("msgfmt") {
            eprintln!("msgfmt is not installed; skipping the cross-check");
            return;
        }

        let directory = TempDir::new();
        let po_path = directory.write("compat.po", FULL_PO);
        let ours = directory.path.join("ours.mo");
        compile_po_file(&po_path, &ours).expect("compile with the pure Rust codec");
        let theirs = directory.path.join("msgfmt.mo");
        let output = Command::new("msgfmt")
            .arg("-o")
            .arg(&theirs)
            .arg(&po_path)
            .output()
            .expect("run msgfmt");
        assert!(
            output.status.success(),
            "msgfmt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let ours_bytes = fs::read(&ours).expect("read our MO file");
        let theirs_bytes = fs::read(&theirs).expect("read the msgfmt MO file");
        assert_eq!(
            entries_of(&ours_bytes),
            entries_of(&theirs_bytes),
            "our MO entries differ from msgfmt"
        );

        if command_works("msgunfmt") {
            let our_text = Command::new("msgunfmt")
                .arg(&ours)
                .output()
                .expect("run msgunfmt on our catalog");
            let other_text = Command::new("msgunfmt")
                .arg(&theirs)
                .output()
                .expect("run msgunfmt on the msgfmt catalog");
            assert!(
                our_text.status.success(),
                "msgunfmt rejected our catalog: {}",
                String::from_utf8_lossy(&our_text.stderr)
            );
            assert!(other_text.status.success());
            assert_eq!(
                our_text.stdout, other_text.stdout,
                "gettext reads our catalog differently"
            );
        }
    }

    #[test]
    fn gettext_loads_the_compiled_catalog() {
        if !command_works("gettext") {
            eprintln!("gettext is not installed; skipping the runtime lookup test");
            return;
        }
        let Some((locale, directory_name)) = available_utf8_locale() else {
            eprintln!("no UTF-8 locale is installed; skipping the runtime lookup test");
            return;
        };

        let po = r#"msgid ""
        msgstr ""
        "Language: fr\n"
        "Content-Type: text/plain; charset=UTF-8\n"
        "Plural-Forms: nplurals=2; plural=n != 1;\n"

        msgid "Start"
        msgstr "Démarrer"

        msgctxt "menu"
        msgid "Open"
        msgstr "Ouvrir"

        msgid "one file"
        msgid_plural "many files"
        msgstr[0] "un fichier"
        msgstr[1] "des fichiers"
        "#;

        let directory = TempDir::new();
        let po_path = directory.write("runtime.po", po);
        let locale_root = directory.path.join("locale");
        let mo_path = locale_root
            .join(&directory_name)
            .join("LC_MESSAGES")
            .join("sysd-manager-catalog-test.mo");
        compile_po_file(&po_path, &mo_path).expect("compile the runtime catalog");

        assert_eq!(
            run_gettext_tool(
                "gettext",
                &["-d", "sysd-manager-catalog-test", "-e", "Start"],
                &locale_root,
                &locale,
            ),
            "Démarrer"
        );
        assert_eq!(
            run_gettext_tool(
                "gettext",
                &[
                    "-d",
                    "sysd-manager-catalog-test",
                    "-c",
                    "menu",
                    "-e",
                    "Open"
                ],
                &locale_root,
                &locale,
            ),
            "Ouvrir"
        );
        assert_eq!(
            run_gettext_tool(
                "gettext",
                &["-d", "sysd-manager-catalog-test", "-e", "Untranslated"],
                &locale_root,
                &locale,
            ),
            "Untranslated"
        );

        if command_works("ngettext") {
            assert_eq!(
                run_gettext_tool(
                    "ngettext",
                    &[
                        "-d",
                        "sysd-manager-catalog-test",
                        "one file",
                        "many files",
                        "1",
                    ],
                    &locale_root,
                    &locale,
                ),
                "un fichier"
            );
            assert_eq!(
                run_gettext_tool(
                    "ngettext",
                    &[
                        "-d",
                        "sysd-manager-catalog-test",
                        "one file",
                        "many files",
                        "2",
                    ],
                    &locale_root,
                    &locale,
                ),
                "des fichiers"
            );
        }
    }

    fn build_mo(entries: &[(&[u8], &[u8])], endian: Endian, revision: u32) -> Vec<u8> {
        let count = u32::try_from(entries.len()).expect("a small catalog");
        let original_table_offset = MO_HEADER_SIZE;
        let translation_table_offset = original_table_offset + 8 * count;
        let original_pool_offset = translation_table_offset + 8 * count;

        let mut original_pool = Vec::new();
        let mut translation_pool = Vec::new();
        let mut original_records = Vec::new();
        let mut translation_records = Vec::new();
        for (original, translation) in entries {
            original_records.push((
                u32::try_from(original.len()).expect("a small original"),
                u32::try_from(original_pool.len()).expect("a small pool"),
            ));
            original_pool.extend_from_slice(original);
            original_pool.push(0);
            translation_records.push((
                u32::try_from(translation.len()).expect("a small translation"),
                u32::try_from(translation_pool.len()).expect("a small pool"),
            ));
            translation_pool.extend_from_slice(translation);
            translation_pool.push(0);
        }
        let translation_pool_offset =
            original_pool_offset + u32::try_from(original_pool.len()).expect("a small pool");

        let word = |value: u32| match endian {
            Endian::Little => value.to_le_bytes(),
            Endian::Big => value.to_be_bytes(),
        };
        let mut bytes = Vec::new();
        for value in [
            MO_MAGIC,
            revision,
            count,
            original_table_offset,
            translation_table_offset,
            0,
            0,
        ] {
            bytes.extend_from_slice(&word(value));
        }
        for (length, offset) in &original_records {
            bytes.extend_from_slice(&word(*length));
            bytes.extend_from_slice(&word(original_pool_offset + offset));
        }
        for (length, offset) in &translation_records {
            bytes.extend_from_slice(&word(*length));
            bytes.extend_from_slice(&word(translation_pool_offset + offset));
        }
        bytes.extend_from_slice(&original_pool);
        bytes.extend_from_slice(&translation_pool);
        bytes
    }

    fn assert_mo_error(directory: &TempDir, name: &str, contents: &[u8], expected: &str) {
        let path = directory.write_bytes(name, contents);
        match read_mo_header(&path) {
            Err(CatalogError::Invalid(message)) => assert!(
                message.contains(expected),
                "expected {expected:?} in {message:?}"
            ),
            Err(error) => panic!("expected an invalid MO file, got {error:?}"),
            Ok(header) => panic!("expected the MO file to be rejected, got {header:?}"),
        }
    }

    fn available_utf8_locale() -> Option<(String, String)> {
        let output = Command::new("locale").arg("-a").output().ok()?;
        if !output.status.success() {
            return None;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let locale = line.trim();
            let Some((language, codeset)) = locale.split_once('.') else {
                continue;
            };
            let language = language.split('@').next().unwrap_or(language);
            if language.is_empty() || matches!(language, "C" | "POSIX") {
                continue;
            }
            if !codeset.to_ascii_lowercase().contains("utf") {
                continue;
            }
            return Some((locale.to_owned(), language.to_owned()));
        }
        None
    }

    fn run_gettext_tool(
        program: &str,
        arguments: &[&str],
        locale_root: &Path,
        locale: &str,
    ) -> String {
        let output = Command::new(program)
            .env("TEXTDOMAINDIR", locale_root)
            .env("LC_ALL", locale)
            .env("LANG", locale)
            .env_remove("LANGUAGE")
            .env_remove("LC_MESSAGES")
            .args(arguments)
            .output()
            .unwrap_or_else(|error| panic!("run {program}: {error}"));
        assert!(
            output.status.success(),
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("the gettext tools print UTF-8")
    }
}
