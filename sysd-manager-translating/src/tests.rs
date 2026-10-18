use super::*;
use test_base::TempDir;

fn write_po(directory: &Path, language: &str, translation: &str) -> PathBuf {
    fs::create_dir_all(directory).unwrap();
    let path = directory.join(format!("{language}.po"));
    fs::write(
        &path,
        format!(
            r#"msgid ""
msgstr ""
"Project-Id-Version: tests 1\n"
"PO-Revision-Date: 2026-10-19 00:00+0800\n"
"Last-Translator: Test\n"
"Language-Team: Test\n"
"Language: {language}\n"
"MIME-Version: 1.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Content-Transfer-Encoding: 8bit\n"
"Plural-Forms: nplurals=1; plural=0;\n"

msgid "Service manager"
msgstr "{translation}"
"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn linguas_handles_whitespace_duplicates_and_inline_comments() {
    assert_eq!(
        parse_linguas("# comment\n zh_Hans\tde # notes\n\nde\r\nfr\n"),
        ["zh_Hans", "de", "fr"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
}

#[test]
fn cargo_catalog_paths_support_custom_profiles_targets_and_directories() {
    for (out, target, expected) in [
        (
            "target/debug/build/pkg/out",
            "x86_64-unknown-linux-gnu",
            "target/locale",
        ),
        (
            "target/release/build/pkg/out",
            "aarch64-unknown-linux-gnu",
            "target/locale",
        ),
        (
            "custom 构建/profiling/build/pkg/out",
            "x86_64-unknown-linux-gnu",
            "custom 构建/locale",
        ),
        (
            "target/aarch64-unknown-linux-gnu/release/build/pkg/out",
            "aarch64-unknown-linux-gnu",
            "target/locale",
        ),
        (
            "target/x86_64-unknown-linux-gnu/debug/build/pkg/out",
            "x86_64-unknown-linux-gnu",
            "target/locale",
        ),
    ] {
        assert_eq!(
            cargo_locale_dir(Path::new(out), target).unwrap(),
            Path::new(expected)
        );
    }
    assert!(cargo_locale_dir(Path::new("out"), "target").is_none());
}

#[test]
fn catalogs_and_aliases_are_generated_in_the_requested_directory() {
    let fixture = TempDir::new().unwrap();
    let source = fixture.path().join("翻译 源");
    let output = fixture.path().join("构建 目录/locale");
    write_po(&source, "zh_Hans", "中文服务管理器");
    generate_mo_in(&source, &output).unwrap();
    let catalog = fs::read(output.join("zh_Hans/LC_MESSAGES/sysd-manager.mo")).unwrap();
    assert_eq!(&catalog[..4], &[0xde, 0x12, 0x04, 0x95]);
    for alias in ["zh_CN", "zh_SG"] {
        assert_eq!(
            fs::read(output.join(alias).join("LC_MESSAGES/sysd-manager.mo")).unwrap(),
            catalog
        );
    }
}

#[test]
fn explicit_regional_translation_takes_priority_over_the_alias() {
    let fixture = TempDir::new().unwrap();
    let source = fixture.path().join("po");
    let output = fixture.path().join("locale");
    write_po(&source, "zh_Hans", "通用中文");
    write_po(&source, "zh_CN", "专用中文");
    generate_mo_in(&source, &output).unwrap();
    assert_ne!(
        fs::read(output.join("zh_CN/LC_MESSAGES/sysd-manager.mo")).unwrap(),
        fs::read(output.join("zh_Hans/LC_MESSAGES/sysd-manager.mo")).unwrap()
    );
    fs::remove_file(source.join("zh_CN.po")).unwrap();
    generate_mo_in(&source, &output).unwrap();
    assert_eq!(
        fs::read(output.join("zh_CN/LC_MESSAGES/sysd-manager.mo")).unwrap(),
        fs::read(output.join("zh_Hans/LC_MESSAGES/sysd-manager.mo")).unwrap()
    );
}

#[test]
fn invalid_catalog_does_not_replace_the_last_valid_catalog() {
    let fixture = TempDir::new().unwrap();
    let source = write_po(fixture.path(), "zh_Hans", "中文服务");
    let output = fixture.path().join("out/sysd-manager.mo");
    compile_catalog(&source, &output).unwrap();
    let before = fs::read(&output).unwrap();
    fs::write(&source, "msgid \"unterminated").unwrap();
    assert!(matches!(
        compile_catalog(&source, &output),
        Err(TransError::CommandFailed(_, _, _))
    ));
    assert_eq!(fs::read(&output).unwrap(), before);
    assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn missing_source_directory_is_not_silently_ignored() {
    let fixture = TempDir::new().unwrap();
    assert!(matches!(
        generate_mo_in(
            &fixture.path().join("missing"),
            &fixture.path().join("locale")
        ),
        Err(TransError::IoError(_))
    ));
}

#[test]
fn desktop_appstream_and_policy_metadata_include_chinese_region_aliases() {
    let fixture = TempDir::new().unwrap();
    let source = fixture.path().join("po");
    write_po(&source, "zh_Hans", "服务管理器");
    let desktop = fixture.path().join("test.desktop");
    fs::write(
        &desktop,
        "[Desktop Entry]\nType=Application\nName=Service manager\nExec=sysd-manager --user\n",
    )
    .unwrap();
    let output = fixture.path().join("pack/test.desktop");
    generate_template(&source, &desktop, &output, TemplateKind::Desktop).unwrap();
    let generated = fs::read_to_string(&output).unwrap();
    for language in ["zh_Hans", "zh_CN", "zh_SG"] {
        assert!(
            generated.contains(&format!("Name[{language}]=服务管理器")),
            "{generated}"
        );
    }
    assert!(generated.contains("Exec=sysd-manager --user"));
    let template = fixture.path().join("test.metainfo.xml");
    fs::write(
        &template,
        r#"<component type="desktop-application">
  <id>org.example.Test</id>
  <name>Service manager</name>
  <summary>Service manager</summary>
</component>"#,
    )
    .unwrap();
    let output = fixture.path().join("pack/test.metainfo.xml");
    generate_template(&source, &template, &output, TemplateKind::Xml).unwrap();
    let generated = fs::read_to_string(&output).unwrap();
    for (posix, bcp47) in [
        ("zh_Hans", "zh-Hans"),
        ("zh_CN", "zh-Hans-CN"),
        ("zh_SG", "zh-Hans-SG"),
    ] {
        assert!(
            [posix.to_owned(), posix.replace('_', "-"), bcp47.to_owned()]
                .iter()
                .any(|language| generated.contains(&format!("xml:lang=\"{language}\""))),
            "{generated}"
        );
    }
    assert!(
        quick_xml::escape::unescape(&generated)
            .unwrap()
            .contains("服务管理器")
    );

    let policy = fixture.path().join("test.policy");
    fs::write(
        &policy,
        r#"<policyconfig>
  <action id="org.example.Test">
    <message>Service manager</message>
    <defaults><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
</policyconfig>"#,
    )
    .unwrap();
    let output = fixture.path().join("pack/test.policy");
    generate_template(&source, &policy, &output, TemplateKind::Policy).unwrap();
    let generated = fs::read_to_string(&output).unwrap();
    for language in ["zh_Hans", "zh_CN", "zh_SG"] {
        assert!(
            generated.contains(&format!("xml:lang=\"{language}\"")),
            "{generated}"
        );
    }
    assert!(!generated.contains("xml:lang=\"zh-Hans"));
    assert!(
        quick_xml::escape::unescape(&generated)
            .unwrap()
            .contains("服务管理器")
    );
    assert!(generated.contains("id=\"org.example.Test\""));
    assert!(generated.contains("<allow_active>auth_admin_keep</allow_active>"));
}

#[test]
fn policy_locales_handle_old_and_new_gettext_xml_conventions() {
    let languages = ["zh_Hans", "zh_CN", "zh_SG", "pt_BR"].map(str::to_owned);
    for (source, expected) in [
        ("zh_Hans", "zh_Hans"),
        ("zh-Hans", "zh_Hans"),
        ("zh_CN", "zh_CN"),
        ("zh-CN", "zh_CN"),
        ("zh-Hans-CN", "zh_CN"),
        ("zh-SG", "zh_SG"),
        ("zh-Hans-SG", "zh_SG"),
        ("pt-BR", "pt_BR"),
    ] {
        let xml = format!("<message xml:lang=\"{source}\">认证 &amp; 授权</message>");
        assert_eq!(
            policy_locale_names(xml, &languages),
            format!("<message xml:lang=\"{expected}\">认证 &amp; 授权</message>")
        );
    }
}

#[test]
fn extraction_and_merge_failures_are_propagated() {
    let fixture = TempDir::new().unwrap();
    let missing = fixture.path().join("missing").to_str().unwrap().to_owned();
    let output = fixture
        .path()
        .join("output.po")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(matches!(
        xgettext(&missing, &output),
        Err(TransError::CommandFailed(_, _, _))
    ));
    assert!(matches!(
        msgmerge(&missing, &output),
        Err(TransError::CommandFailed(_, _, _))
    ));
    assert!(matches!(
        msginit(&missing, &output, "zh_CN"),
        Err(TransError::CommandFailed(_, _, _))
    ));
}

#[test]
fn failed_translation_commands_are_errors() {
    let output = Command::new("sh")
        .args(["-c", "printf 'invalid catalog' >&2; exit 7"])
        .output()
        .unwrap();
    match display_output("MSGFMT", output) {
        Err(TransError::CommandFailed(program, status, stderr)) => {
            assert_eq!(program, "MSGFMT");
            assert_eq!(status.code(), Some(7));
            assert_eq!(stderr, "invalid catalog");
        }
        other => panic!("expected failed command, got {other:?}"),
    }
}
