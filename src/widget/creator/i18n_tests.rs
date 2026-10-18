use super::{
    PageType, UnitCreatorWindow, unit_file::UnitFileData,
    unit_file_creator_page::UnitFileCreatorPage,
};
use gtk::prelude::*;

#[allow(dead_code)] // Also compiled by the separate threaded unit-test harness.
pub(crate) fn assert_file_editor(window: &UnitCreatorWindow) {
    let mut service = UnitFileData::new();
    service.set_description("中文说明 <用户数据> & English 🚀");
    service.set_exec_start("/usr/bin/printf '你好\\n'");
    service.set_environment(Some(&["GREETING=您好", "LANG=C.UTF-8"]));
    service.set_wanted_by("default.target");
    service.sort();
    let mut timer = UnitFileData::new();
    timer.set_description("定时任务");
    timer.add_timers(
        [("OnBootSec".to_owned(), vec!["30s".to_owned()])]
            .into_iter()
            .collect(),
    );
    timer.set_persistent(true);
    timer.sort();
    let mut mount = UnitFileData::new();
    mount.set_description("数据挂载");
    mount.set_what("/dev/disk/by-label/数据");
    mount.set_wherex("/mnt/数据");
    mount.sort();
    for (data, expected) in [
        (service, "ExecStart=/usr/bin/printf '你好\\n'"),
        (timer, "OnBootSec=30s"),
        (mount, "Where=/mnt/数据"),
    ] {
        let text = data.to_file();
        assert!(text.contains(expected), "{text}");
        let parsed = UnitFileData::from_content(&text).unwrap();
        assert_eq!(parsed.to_file(), text);
        let editor = UnitFileCreatorPage::new(window.downgrade(), PageType::ServiceFile);
        editor.update_view(&data);
        assert_eq!(editor.file_text(), text);
    }
}
