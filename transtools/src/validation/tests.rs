use super::*;

#[test]
fn positional_and_named_fields_can_be_reordered() {
    for (source, translated) in [
        ("{} has {}", "{1} 属于 {0}"),
        ("{name}: {count}", "{count}：{name}"),
        ("{{literal}} {}", "{} {{字面量}}"),
        ("{:>10} {1:.2}", "{1:.2} {0:>10}"),
    ] {
        validate_message(source, translated).unwrap();
    }
}

#[test]
fn missing_duplicated_and_changed_fields_are_rejected() {
    for translated in ["缺少", "{} {} {}", "{0} {0}", "{name} {1}", "{0:x} {1}"] {
        assert!(
            validate_message("{} {}", translated).is_err(),
            "{translated}"
        );
    }
}

#[test]
fn invalid_format_strings_are_rejected() {
    for text in ["{", "}", "{name", "{name:{width}"] {
        assert!(validate_message("{}", text).is_err(), "{text}");
    }
}

#[test]
fn markup_content_can_be_translated() {
    validate_message(
        "<b>Hello</b> <a href=\"https://example.com\">link</a>",
        "<b>您好</b> <a href=\"https://example.com\">链接</a>",
    )
    .unwrap();
    validate_message("<b>one &amp; two</b>", "<b>一 &amp; 二</b>").unwrap();
}

#[test]
fn balanced_tag_counts_do_not_hide_broken_nesting() {
    assert!(validate_message("<b><i>text</i></b>", "<b><i>文字</b></i>").is_err());
}

#[test]
fn changed_links_attributes_and_undefined_entities_are_rejected() {
    for translated in [
        "<b>文字</b>",
        "<a href=\"https://evil.example\">文字</a>",
        "<a href=\"https://example.com\" onclick=\"bad\">文字</a>",
    ] {
        assert!(validate_message("<a href=\"https://example.com\">text</a>", translated).is_err());
    }
    assert!(validate_message("<b>text</b>", "<b>&undefined;</b>").is_err());
    assert!(
        validate_message(
            "<a href=\"https://example.com/a>b\">link</a>",
            "<a href=\"https://example.com/a>evil\">链接</a>",
        )
        .is_err()
    );
}

#[test]
fn mnemonic_letters_are_preserved() {
    validate_message("_Start", "启动(_S)").unwrap();
    validate_message("__literal", "__字面值").unwrap();
    for translated in ["启动", "启动(_T)"] {
        assert!(validate_message("_Start", translated).is_err());
    }
}

#[test]
fn fuzzy_missing_empty_and_extra_messages_are_rejected() {
    let source = parse_catalog("msgctxt \"ctx\"\nmsgid \"Open\"\nmsgstr \"\"\n").unwrap();
    let good = "msgctxt \"ctx\"\nmsgid \"Open\"\nmsgstr \"打开\"\n";
    validate_catalog(&source, &parse_catalog(good).unwrap()).unwrap();
    for text in [
        String::new(),
        good.replace("打开", ""),
        format!("#, fuzzy\n{good}"),
        format!("{good}\nmsgid \"extra\"\nmsgstr \"额外\"\n"),
    ] {
        assert!(
            validate_catalog(&source, &parse_catalog(&text).unwrap()).is_err(),
            "{text}"
        );
    }
}

#[test]
fn chinese_plural_requires_exactly_one_form() {
    let source =
        parse_catalog("msgid \"{} item\"\nmsgid_plural \"{} items\"\nmsgstr[0] \"\"\n").unwrap();
    let good = "msgid \"{} item\"\nmsgid_plural \"{} items\"\nmsgstr[0] \"{} 项\"\n";
    validate_catalog(&source, &parse_catalog(good).unwrap()).unwrap();
    for text in [
        format!("{good}msgstr[1] \"{{}} 项\"\n"),
        good.replace("msgstr[0]", "msgstr"),
        good.replace("{} items", "{} entries"),
    ] {
        assert!(validate_catalog(&source, &parse_catalog(&text).unwrap()).is_err());
    }
}

#[test]
fn po_parser_handles_context_continuations_and_obsolete_entries() {
    let parsed = parse_catalog("msgctxt \"ctx\"\nmsgid \"a\"\n\"b\"\nmsgstr \"中文\"\n\n#~ msgid \"old\"\n#~ msgstr \"旧\"\n").unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[&("ctx".into(), "ab".into())].fields["msgstr"],
        "中文"
    );
}

#[test]
fn po_parser_rejects_duplicate_fields_messages_and_malformed_strings() {
    for text in [
        "msgid \"a\"\nmsgid \"b\"\n",
        "msgid \"a\"\nmsgstr \"b\"\n\nmsgid \"a\"\nmsgstr \"c\"\n",
        "\"orphan\"\n",
        "msgid \"unterminated\n",
        "msgid \"extra\" text\n",
        "msgid \"bad\\q\"\n",
        "msgid \"bad\"\nmsgstr[x] \"x\"\n",
    ] {
        assert!(parse_catalog(text).is_err(), "{text}");
    }
}

#[test]
fn po_byte_escapes_decode_utf8_across_continuations() {
    let parsed = parse_catalog(
        r#"msgid "escaped"
msgstr "\303"
"\251\n\t\"\\"
"#,
    )
    .unwrap();
    assert_eq!(
        parsed[&("".into(), "escaped".into())].fields["msgstr"],
        "é\n\t\"\\"
    );
}

#[test]
fn ui_text_requires_a_translation_marker() {
    let xml = "<interface><object class=\"GtkLabel\"><property name=\"label\">New text</property></object></interface>";
    assert!(validate_interface(xml).is_err());
    validate_interface(&xml.replace("name=\"label\"", "name=\"label\" translatable=\"yes\""))
        .unwrap();
}

#[test]
fn ui_entities_and_non_display_properties_are_handled() {
    validate_interface(
        r#"<interface><object class="GtkLabel">
      <property name="label" translatable="yes">&lt;b&gt;Start&lt;/b&gt;</property>
      <property name="name">raw-systemd-identifier</property>
      <property name="title">SysD Manager</property>
    </object></interface>"#,
    )
    .unwrap();
}
