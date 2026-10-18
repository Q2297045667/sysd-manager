#!/usr/bin/env python3
"""Validate Chinese catalogs and GTK extraction with Python's standard library."""

import ast
from collections import Counter
from pathlib import Path
import re
import string
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]


class ValidationError(ValueError):
    pass


def require(condition, message):
    # These checks must also run under python -O.
    if not condition:
        raise ValidationError(message)


def entries(path):
    result, entry, field = {}, {}, None
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines() + [""], 1):
        line = raw.strip()
        location = f"{path}:{number}"
        if not line:
            if entry.get("msgid"):
                key = (entry.get("msgctxt", ""), entry["msgid"])
                require(key not in result, f"{location}: duplicate message {key}")
                result[key] = entry
            entry, field = {}, None
        elif line.startswith("#,"):
            entry["fuzzy"] = entry.get("fuzzy", False) or "fuzzy" in line[2:].replace(",", " ").split()
        elif line.startswith("#"):
            continue
        else:
            if line.startswith('"'):
                require(field is not None, f"{location}: continuation without a field")
                value = line
            else:
                match = re.fullmatch(r'(msgctxt|msgid_plural|msgid|msgstr(?:\[\d+\])?)\s+(".*")', line)
                require(match is not None, f"{location}: invalid PO field")
                field, value = match.groups()
                require(field not in entry, f"{location}: duplicate field {field}")
                entry[field] = ""
            try:
                decoded = ast.literal_eval(value)
            except (SyntaxError, ValueError) as error:
                raise ValidationError(f"{location}: invalid quoted string") from error
            require(isinstance(decoded, str), f"{location}: expected a string")
            entry[field] += decoded
    return result


def placeholders(text):
    """Normalize automatic and explicit positional indexes, allowing reordering."""
    automatic = 0
    result = Counter()
    try:
        for _, field, spec, conversion in string.Formatter().parse(text):
            if field is None:
                continue
            if field == "":
                field = str(automatic)
                automatic += 1
            result[(field, spec, conversion)] += 1
    except ValueError as error:
        raise ValidationError(f"invalid format string: {text!r}") from error
    return result


def tags(text):
    return Counter(re.findall(r"</?[A-Za-z][^>]*>", text))


def validate_markup(text):
    if not tags(text):
        return
    try:
        ET.fromstring("<root>" + text + "</root>")
    except ET.ParseError as error:
        raise ValidationError(f"invalid markup: {text!r}") from error


def mnemonics(text):
    return Counter(key.lower() for key in re.findall(r"(?<!_)_([A-Za-z])", text))


def validate_message(source, translated, key):
    require(bool(translated), f"untranslated message: {key}")
    require(placeholders(source) == placeholders(translated), f"format placeholders differ: {key}")
    validate_markup(source)
    validate_markup(translated)
    require(tags(source) == tags(translated), f"markup differs: {key}")
    require(mnemonics(source) == mnemonics(translated), f"mnemonic differs: {key}")


def validate_catalog(template, chinese):
    require(template.keys() == chinese.keys(),
            f"Chinese catalog differs from POT: missing={template.keys() - chinese.keys()}, "
            f"extra={chinese.keys() - template.keys()}")
    for key, entry in chinese.items():
        require(not entry.get("fuzzy"), f"unreviewed translation: {key}")
        plural = template[key].get("msgid_plural")
        require(entry.get("msgid_plural") == plural, f"plural source differs: {key}")
        # Simplified Chinese has one plural form, including zero.
        fields = {name for name in entry if name.startswith("msgstr")}
        expected = {"msgstr[0]"} if plural is not None else {"msgstr"}
        require(fields == expected, f"translation forms differ: {key}: {fields}")
        validate_message(plural or entry["msgid"], entry[next(iter(expected))], key)


def validate_interfaces(directory):
    properties = {"label", "title", "subtitle", "tooltip-text", "tooltip-markup", "placeholder-text"}
    # Brand names and existing text replaced by code before the widget is shown.
    placeholders = {"SysD Manager", "test", "link", "Love", "Hello friend"}
    for path in directory.rglob("*.ui"):
        for element in ET.parse(path).iter():
            if element.tag == "property" and element.get("name") in properties:
                text = element.text or ""
                if text.strip() in placeholders or not any(c.isalpha() for c in text):
                    continue
                require(element.get("translatable") == "yes", f"unmarked UI text: {path}: {text!r}")


def check(root=ROOT):
    template = entries(root / "po/sysd-manager.pot")
    chinese = entries(root / "po/zh_Hans.po")
    validate_catalog(template, chinese)
    validate_interfaces(root / "data/interfaces")
    with tempfile.TemporaryDirectory(prefix="sysd-i18n-") as directory:
        scratch = Path(directory)
        subprocess.run(["msgfmt", "--check", "--statistics", "-o", str(scratch / "zh.mo"),
                        "po/zh_Hans.po"], cwd=root, check=True)
        subprocess.run(
            ["xgettext", "--from-code=UTF-8", "--keyword=_", "--keyword=N_",
             "--keyword=C_:1c,2", "--keyword=NC_:1c,2", "--keyword=g_dcgettext:2",
             "--keyword=g_dngettext:2,3", "--keyword=g_dpgettext2:2c,3",
             "--files-from=po/POTFILES", "--output=" + str(scratch / "source.pot")],
            cwd=root, check=True,
        )
        extracted = entries(scratch / "source.pot")
        require(extracted.keys() == template.keys(),
                "POT is stale; run cargo run -p transtools -- extract")
        for key, entry in extracted.items():
            require(entry.get("msgid_plural") == template[key].get("msgid_plural"),
                    f"POT plural source is stale: {key}")
    listed = {
        line.strip().removeprefix("./")
        for line in (root / "po/POTFILES").read_text().splitlines()
        if line.strip() and not line.startswith("#")
    }
    for directory, extension in [("src", "rs"), ("sysd-manager-comcontroler/src", "rs"),
                                 ("data/interfaces", "ui")]:
        for path in (root / directory).rglob("*." + extension):
            require(str(path.relative_to(root)) in listed, f"POTFILES missing: {path}")
    print(f"i18n: {len(chinese)} Chinese translations verified")


if __name__ == "__main__":
    check()
