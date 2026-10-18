//! Real composite widgets on a private display and D-Bus, in both languages.
// The source modules also contain unit-test imports; this executable uses its
// own single-threaded harness rather than Rust's threaded unit-test harness.
#![allow(dead_code, unused_imports)]
extern crate gtk;
extern crate systemd;
extern crate tracing;

#[path = "../src/analyze/mod.rs"]
mod analyze;
#[path = "../src/consts.rs"]
mod consts;
#[path = "../src/errors.rs"]
mod errors;
#[path = "../src/i18n.rs"]
mod i18n;
#[path = "../src/systemd_gui.rs"]
mod systemd_gui;
#[path = "../src/utils/mod.rs"]
mod utils;
#[path = "../src/widget/mod.rs"]
mod widget;

#[path = "support/systemd_mock.rs"]
mod systemd_mock;

use adw::prelude::*;
use gtk::{gio, glib};
use std::time::{Duration, Instant};

fn main() {
    if std::env::var_os("SYSD_UI_TEST_ISOLATED").is_none() {
        test_base::ui::run_isolated(
            &std::env::current_exe().unwrap(),
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
            std::path::Path::new(env!("SYSD_LOCALE_DIR")),
        )
        .expect("isolated GTK tests failed");
        return;
    }
    // SAFETY: a fresh single-threaded subprocess, before GTK or async startup.
    unsafe { i18n::init() }.unwrap();
    adw::init().unwrap();
    if std::env::args().any(|arg| arg == "--preflight") {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&gtk::Label::new(Some("系统服务 GTK preflight")));
        let popover = gtk::Popover::builder()
            .child(&gtk::Label::new(Some("Popover")))
            .build();
        let button = gtk::MenuButton::builder()
            .popover(&popover)
            .label("Open")
            .build();
        content.append(&button);
        let window = adw::Window::builder()
            .default_width(640)
            .default_height(480)
            .content(&content)
            .build();
        window.present();
        pump(Duration::from_millis(100));
        assert!(window.is_mapped());
        for _ in 0..2 {
            popover.popup();
            pump(Duration::from_millis(100));
            popover.popdown();
            pump(Duration::from_millis(100));
        }
        let dialog = adw::PreferencesDialog::new();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        group.add(
            &adw::SwitchRow::builder()
                .title("Generic GTK preflight")
                .build(),
        );
        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(&window));
        pump(Duration::from_millis(350));
        dialog.close();
        pump(Duration::from_millis(350));
        window.destroy();
        println!(
            "ui: GTK {}.{}.{}, libadwaita {}.{}.{} preflight passed",
            gtk::major_version(),
            gtk::minor_version(),
            gtk::micro_version(),
            adw::major_version(),
            adw::minor_version(),
            adw::micro_version()
        );
        return;
    }
    gio::resources_register_include!("sysd-manager.gresource").unwrap();
    let mock = systemd_mock::Mock::start();
    mock.verify_backend_operations();
    let backend_calls = mock.state.calls.lock().unwrap().len();
    widget::preferences::data::PREFERENCES.set_and_save_dbus_level(
        widget::preferences::data::DbusLevel::UserSession,
        &systemd_gui::new_settings(),
    );
    let app = adw::Application::builder()
        .application_id("io.github.plrigaux.sysd-manager.tests")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    widget::app_window::menu::on_startup(&app);
    let window = widget::app_window::AppWindow::new(&app);
    window.present();
    pump(Duration::from_millis(150));
    assert!(window.is_mapped());
    let chinese = std::env::var("LANGUAGE").unwrap() == "zh_CN";
    assert_eq!(
        gettextrs::gettext("Search"),
        if chinese { "搜索" } else { "Search" }
    );
    let unit =
        systemd::fetch_unit(base::enums::UnitDBusLevel::UserSession, systemd_mock::UNIT).unwrap();
    window.set_unit(Some(&unit));
    pump(Duration::from_millis(100));
    let status: widget::unit_status::UnitStatusPanel = find_widget(window.upcast_ref()).unwrap();
    let buffer = status.main_text_view().buffer();
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    assert!(text.contains(systemd_mock::UNIT));
    assert!(text.contains(systemd_mock::DESCRIPTION));
    assert!(
        text.contains(if chinese { "内存：" } else { "Memory:" }),
        "{text}"
    );
    assert!(text.contains("用户提供的状态 <>& 不应被翻译"));
    assert_eq!(mock.state.calls.lock().unwrap().len(), backend_calls);

    // Exercise the real window actions as well as the backend. Their localized
    // labels must leave the selected unit, action identifier and mode intact.
    for (action, expected) in [
        (consts::ACTION_WIN_START_UNIT, "start"),
        (consts::ACTION_WIN_STOP_UNIT, "stop"),
        (consts::ACTION_WIN_RESTART_UNIT, "restart"),
        (consts::ACTION_WIN_RELOAD_UNIT, "reload"),
    ] {
        let previous = mock.state.calls.lock().unwrap().len();
        gtk::prelude::WidgetExt::activate_action(&window, action, None).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            pump(Duration::from_millis(10));
            if mock.state.calls.lock().unwrap().len() == previous + 1
                && window.lookup_action(&action[4..]).unwrap().is_enabled()
            {
                break;
            }
            assert!(Instant::now() < until, "window action timed out: {action}");
        }
        assert_eq!(
            mock.state.calls.lock().unwrap().last().unwrap(),
            &(
                expected.to_owned(),
                systemd_mock::UNIT.to_owned(),
                "fail".to_owned()
            )
        );
    }

    let preferences = widget::preferences::PreferencesDialog::new(Some(&window));
    preferences.present(Some(&window));
    pump(Duration::from_millis(100));
    assert_text(
        &preferences,
        if chinese {
            "首选配色方案"
        } else {
            "Preferred Color Scheme"
        },
    );
    preferences.close();
    pump(Duration::from_millis(100));

    let list: widget::unit_list::UnitListPanel = find_widget(window.upcast_ref()).unwrap();
    let columns = widget::unit_properties_selector::UnitPropertiesSelectorDialog::new(&list, None);
    columns.present();
    pump(Duration::from_millis(100));
    assert_text(
        &columns,
        if chinese {
            "单元属性选择器"
        } else {
            "Unit Property Selector"
        },
    );
    assert_text(&columns, if chinese { "应用" } else { "Apply" });
    columns.destroy();
    let filters = widget::unit_list::UnitListFilterWindow::new(None, &list);
    filters.construct_filter_dialog();
    filters.present();
    pump(Duration::from_millis(100));
    assert_text(
        &filters,
        if chinese {
            "匹配方式"
        } else {
            "Match Type"
        },
    );
    filters.destroy();

    let creator = widget::creator::UnitCreatorWindow::new(&window);
    creator.set_creation_type(widget::creator::UnitCreateType::Service);
    creator.present();
    pump(Duration::from_millis(100));
    assert_text(
        &creator,
        if chinese {
            "单元名称前缀"
        } else {
            "Unit Name Prefix"
        },
    );
    widget::creator::i18n_tests::assert_file_editor(&creator);
    let navigation: adw::NavigationView = find_widget(creator.upcast_ref()).unwrap();
    for (creation, tag, chinese_text, english_text) in [
        (
            widget::creator::UnitCreateType::Service,
            widget::creator::PAGE_SERVICE,
            "启动命令（ExecStart）",
            "ExecStart",
        ),
        (
            widget::creator::UnitCreateType::Timer,
            widget::creator::PAGE_TIMER,
            "单调时钟定时器",
            "Monotonic Timer",
        ),
        (
            widget::creator::UnitCreateType::Mount,
            widget::creator::PAGE_MOUNT,
            "挂载源",
            "What",
        ),
    ] {
        navigation.pop_to_tag(widget::creator::PAGE_FIRST);
        pump(Duration::from_millis(50));
        creator.set_creation_type(creation);
        navigation.push_by_tag(tag);
        pump(Duration::from_millis(100));
        assert_text(&creator, if chinese { chinese_text } else { english_text });
        assert_eq!(creator.creation_type(), creation);
    }
    creator.destroy();
    let signals = widget::signals_dialog::SignalsWindow::new(&window);
    signals.present();
    pump(Duration::from_millis(100));
    assert_text(
        &signals,
        if chinese {
            "监视信号"
        } else {
            "Watch Signals"
        },
    );
    signals.destroy();
    window.destroy();
    pump(Duration::from_millis(100));
    println!(
        "ui: {} real windows and dialogs passed",
        if chinese { "Chinese" } else { "English" }
    );
}

fn find_widget<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<T> {
    if let Ok(widget) = root.clone().downcast::<T>() {
        return Some(widget);
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_widget(&widget) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

fn pump(duration: Duration) {
    let until = Instant::now() + duration;
    let context = glib::MainContext::default();
    while Instant::now() < until {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_text(root: &impl IsA<gtk::Widget>, expected: &str) {
    fn collect(widget: &gtk::Widget, result: &mut String) {
        for name in ["label", "title", "subtitle", "tooltip-text"] {
            if let Some(property) = widget.find_property(name)
                && property.flags().contains(glib::ParamFlags::READABLE)
                && property.value_type() == String::static_type()
                && let Ok(Some(text)) = widget.property_value(name).get::<Option<String>>()
            {
                result.push_str(&text);
                result.push('\n');
            }
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            collect(&widget, result);
            child = widget.next_sibling();
        }
    }
    let mut text = String::new();
    collect(root.as_ref(), &mut text);
    assert!(
        text.contains(expected),
        "missing {expected:?} in widget tree:\n{text}"
    );
}
