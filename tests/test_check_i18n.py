"""Regression tests for catalog corruption and extraction checks."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("check_i18n", Path(__file__).resolve().parents[1] / "scripts/check_i18n.py")
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class CatalogValidationTests(unittest.TestCase):
    def validate(self, source, target):
        CHECK.validate_message(source, target, ("test", source))

    def test_reordered_positional_and_named_fields_are_valid(self):
        self.validate("{} has {}", "{1} 属于 {0}")
        self.validate("{name}: {count}", "{count}：{name}")
        self.validate("{{literal}} {}", "{} {{字面量}}")

    def test_missing_duplicated_and_changed_format_fields_are_rejected(self):
        for target in ["缺少", "{} {} {}", "{0} {0}", "{name} {1}", "{0:x} {1}"]:
            with self.subTest(target=target), self.assertRaises(CHECK.ValidationError):
                self.validate("{} {}", target)

    def test_invalid_format_string_is_rejected(self):
        with self.assertRaises(CHECK.ValidationError):
            self.validate("{}", "{")

    def test_markup_content_can_be_translated(self):
        self.validate('<b>Hello</b> <a href="https://example.com">link</a>',
                      '<b>您好</b> <a href="https://example.com">链接</a>')

    def test_balanced_counts_do_not_hide_broken_nesting(self):
        with self.assertRaises(CHECK.ValidationError):
            self.validate("<b><i>text</i></b>", "<b><i>文字</b></i>")

    def test_markup_and_hyperlink_injection_are_rejected(self):
        for target in ['<b>文字</b>', '<a href="https://evil.example">文字</a>',
                       '<a href="https://example.com" onclick="bad">文字</a>']:
            with self.subTest(target=target), self.assertRaises(CHECK.ValidationError):
                self.validate('<a href="https://example.com">text</a>', target)

    def test_mnemonic_letter_is_preserved(self):
        self.validate("_Start", "启动(_S)")
        for target in ["启动", "启动(_T)"]:
            with self.subTest(target=target), self.assertRaises(CHECK.ValidationError):
                self.validate("_Start", target)

    def test_fuzzy_missing_empty_and_extra_translations_are_rejected(self):
        key = ("ctx", "Open")
        source = {key: {"msgid": "Open"}}
        for target in [{}, {key: {"msgid": "Open", "msgstr": ""}},
                       {key: {"msgid": "Open", "msgstr": "打开", "fuzzy": True}},
                       {**{key: {"msgid": "Open", "msgstr": "打开"}}, ("", "extra"): {}}]:
            with self.subTest(target=target), self.assertRaises(CHECK.ValidationError):
                CHECK.validate_catalog(source, target)

    def test_chinese_plural_requires_exactly_one_form(self):
        key = ("", "{} item")
        source = {key: {"msgid": "{} item", "msgid_plural": "{} items"}}
        target = {key: {"msgid": "{} item", "msgid_plural": "{} items", "msgstr[0]": "{} 项"}}
        CHECK.validate_catalog(source, target)
        target[key]["msgstr[1]"] = "{} 项"
        with self.assertRaises(CHECK.ValidationError):
            CHECK.validate_catalog(source, target)

    def parse(self, text):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.po"
            path.write_text(text, encoding="utf-8")
            return CHECK.entries(path)

    def test_parser_handles_context_multiline_and_obsolete_entries(self):
        result = self.parse('msgctxt "ctx"\nmsgid "a"\n"b"\nmsgstr "中文"\n\n#~ msgid "old"\n#~ msgstr "旧"\n')
        self.assertEqual(result[("ctx", "ab")]["msgstr"], "中文")
        self.assertEqual(len(result), 1)

    def test_parser_rejects_duplicate_fields_and_messages(self):
        for text in ['msgid "a"\nmsgid "b"\n',
                     'msgid "a"\nmsgstr "b"\n\nmsgid "a"\nmsgstr "c"\n',
                     '"orphan"\n', 'msgid "unterminated\n']:
            with self.subTest(text=text), self.assertRaises(CHECK.ValidationError):
                self.parse(text)

    def test_ui_text_requires_a_translation_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.ui"
            path.write_text('<interface><object class="GtkLabel"><property name="label">New text</property></object></interface>')
            with self.assertRaises(CHECK.ValidationError):
                CHECK.validate_interfaces(Path(directory))
            path.write_text(path.read_text().replace('name="label"', 'name="label" translatable="yes"'))
            CHECK.validate_interfaces(Path(directory))

    def test_optimized_python_does_not_disable_validation(self):
        code = f'import runpy; m=runpy.run_path({str(SPEC.origin)!r}); m["validate_message"]("{{}}", "缺失", "test")'
        result = subprocess.run([sys.executable, "-O", "-c", code], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"ValidationError", result.stderr)


if __name__ == "__main__":
    unittest.main()
