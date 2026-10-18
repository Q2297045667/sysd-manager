//! In-process systemd fixture; the runner supplies a private system/session bus.
use enumflags2::BitFlag;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use zvariant::OwnedObjectPath;

pub const UNIT: &str = "demo.service";
pub const DESCRIPTION: &str = "中文服务 <用户数据> & English 🚀";
const PATH: &str = "/org/freedesktop/systemd1/unit/demo_2eservice";
const MANAGER: &str = "/org/freedesktop/systemd1";
type ListedUnit = (
    String,
    String,
    String,
    String,
    String,
    String,
    OwnedObjectPath,
    u32,
    String,
    OwnedObjectPath,
);

#[derive(Default)]
pub struct State {
    pub calls: Mutex<Vec<(String, String, String)>>,
    active: AtomicBool,
    enabled: AtomicBool,
    next_job: AtomicU32,
}

pub struct Mock {
    pub state: Arc<State>,
    _connection: zbus::Connection,
}
impl Mock {
    pub fn start() -> Self {
        assert_eq!(std::env::var("SYSD_UI_TEST_ISOLATED").as_deref(), Ok("1"));
        assert_eq!(
            std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap(),
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        );
        let state = Arc::new(State::default());
        let connection = systemd::runtime().block_on(async {
            zbus::connection::Builder::session()
                .unwrap()
                .name("org.freedesktop.systemd1")
                .unwrap()
                .serve_at(MANAGER, Manager(state.clone()))
                .unwrap()
                .serve_at(PATH, Unit(state.clone()))
                .unwrap()
                .serve_at(PATH, Service)
                .unwrap()
                .build()
                .await
                .unwrap()
        });
        Self {
            state,
            _connection: connection,
        }
    }

    pub fn verify_backend_operations(&self) {
        use base::enums::UnitDBusLevel::UserSession;
        use systemd::enums::{DisEnableFlags, StartStopMode};
        let runtime = systemd::runtime();
        for (action, expected) in [
            (systemd::ReStartStop::Start, "start"),
            (systemd::ReStartStop::Stop, "stop"),
            (systemd::ReStartStop::Restart, "restart"),
            (systemd::ReStartStop::ReloadUnit, "reload"),
        ] {
            let job = runtime
                .block_on(systemd::restartstop_unit(
                    UserSession,
                    UNIT,
                    StartStopMode::Replace,
                    action,
                ))
                .unwrap();
            assert!(job.starts_with("/org/freedesktop/systemd1/job/"));
            assert_eq!(
                self.state.calls.lock().unwrap().last().unwrap(),
                &(expected.to_owned(), UNIT.to_owned(), "replace".to_owned())
            );
        }
        let error = runtime
            .block_on(systemd::restartstop_unit(
                UserSession,
                "failed.service",
                StartStopMode::Fail,
                systemd::ReStartStop::Start,
            ))
            .unwrap_err();
        assert!(
            matches!(error, systemd::errors::SystemdErrors::JobRemoved(ref result) if result == "failed")
        );
        systemd::enable_unit_file(UserSession, UNIT, DisEnableFlags::empty()).unwrap();
        assert_eq!(
            self.state.calls.lock().unwrap().last().unwrap(),
            &("enable".to_owned(), UNIT.to_owned(), "0".to_owned())
        );
        assert_eq!(
            systemd::get_unit_file_state(UserSession, UNIT)
                .unwrap()
                .as_str(),
            "enabled"
        );
        systemd::disable_unit_file(UserSession, UNIT, DisEnableFlags::empty()).unwrap();
        assert_eq!(
            self.state.calls.lock().unwrap().last().unwrap(),
            &("disable".to_owned(), UNIT.to_owned(), "0".to_owned())
        );
        assert_eq!(
            systemd::get_unit_file_state(UserSession, UNIT)
                .unwrap()
                .as_str(),
            "disabled"
        );
        let unit = systemd::fetch_unit(UserSession, UNIT).unwrap();
        assert_eq!(unit.primary(), UNIT);
        assert_eq!(unit.description().as_deref(), Some(DESCRIPTION));
    }
}

struct Manager(Arc<State>);
impl Manager {
    fn job(
        &self,
        kind: &str,
        unit: &str,
        mode: &str,
        connection: &zbus::Connection,
    ) -> OwnedObjectPath {
        self.0
            .calls
            .lock()
            .unwrap()
            .push((kind.to_owned(), unit.to_owned(), mode.to_owned()));
        if unit != "failed.service" {
            self.0
                .active
                .store(kind == "start" || kind == "restart", Ordering::Relaxed);
        }
        let id = self.0.next_job.fetch_add(1, Ordering::Relaxed) + 1;
        let job = OwnedObjectPath::try_from(format!("/org/freedesktop/systemd1/job/{id}")).unwrap();
        let returned = job.clone();
        let connection = connection.clone();
        let unit = unit.to_owned();
        tokio::spawn(async move {
            // A different job must not accidentally complete the requested one.
            connection
                .emit_signal(
                    None::<&str>,
                    MANAGER,
                    "org.freedesktop.systemd1.Manager",
                    "JobRemoved",
                    &(id + 1000, job.clone(), unit.clone(), "done"),
                )
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let result = if unit == "failed.service" {
                "failed"
            } else {
                "done"
            };
            connection
                .emit_signal(
                    None::<&str>,
                    MANAGER,
                    "org.freedesktop.systemd1.Manager",
                    "JobRemoved",
                    &(id, job, unit, result),
                )
                .await
                .unwrap();
        });
        returned
    }
    fn units(&self) -> Vec<ListedUnit> {
        let active = if self.0.active.load(Ordering::Relaxed) {
            "active"
        } else {
            "inactive"
        };
        vec![(
            UNIT.into(),
            DESCRIPTION.into(),
            "loaded".into(),
            active.into(),
            "dead".into(),
            "".into(),
            PATH.try_into().unwrap(),
            0,
            "".into(),
            "/".try_into().unwrap(),
        )]
    }
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    fn subscribe(&self) {}
    fn unsubscribe(&self) {}
    fn list_units(&self) -> Vec<ListedUnit> {
        self.units()
    }
    fn list_units_by_patterns(
        &self,
        _states: Vec<String>,
        _patterns: Vec<String>,
    ) -> Vec<ListedUnit> {
        self.units()
    }
    fn list_unit_files(&self) -> Vec<(String, String)> {
        vec![("/tmp/demo.service".into(), "disabled".into())]
    }
    fn list_unit_files_by_patterns(
        &self,
        _states: Vec<String>,
        _patterns: Vec<String>,
    ) -> Vec<(String, String)> {
        self.list_unit_files()
    }
    fn get_unit(&self, _name: &str) -> OwnedObjectPath {
        PATH.try_into().unwrap()
    }
    fn get_unit_file_state(&self, _name: &str) -> String {
        if self.0.enabled.load(Ordering::Relaxed) {
            "enabled"
        } else {
            "disabled"
        }
        .into()
    }
    fn start_unit(
        &self,
        unit: &str,
        mode: &str,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> OwnedObjectPath {
        self.job("start", unit, mode, connection)
    }
    fn stop_unit(
        &self,
        unit: &str,
        mode: &str,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> OwnedObjectPath {
        self.job("stop", unit, mode, connection)
    }
    fn restart_unit(
        &self,
        unit: &str,
        mode: &str,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> OwnedObjectPath {
        self.job("restart", unit, mode, connection)
    }
    fn reload_unit(
        &self,
        unit: &str,
        mode: &str,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> OwnedObjectPath {
        self.job("reload", unit, mode, connection)
    }
    fn enable_unit_files_with_flags(
        &self,
        files: Vec<String>,
        flags: u64,
    ) -> (bool, Vec<(String, String, String)>) {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(("enable".into(), files.join(","), flags.to_string()));
        self.0.enabled.store(true, Ordering::Relaxed);
        (true, Vec::new())
    }
    fn disable_unit_files_with_flags_and_install_info(
        &self,
        files: Vec<String>,
        flags: u64,
    ) -> (bool, Vec<(String, String, String)>) {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(("disable".into(), files.join(","), flags.to_string()));
        self.0.enabled.store(false, Ordering::Relaxed);
        (true, Vec::new())
    }
}

struct Unit(Arc<State>);
#[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
impl Unit {
    #[zbus(property)]
    fn id(&self) -> &str {
        UNIT
    }
    #[zbus(property)]
    fn description(&self) -> &str {
        DESCRIPTION
    }
    #[zbus(property)]
    fn load_state(&self) -> &str {
        "loaded"
    }
    #[zbus(property)]
    fn active_state(&self) -> &str {
        if self.0.active.load(Ordering::Relaxed) {
            "active"
        } else {
            "inactive"
        }
    }
    #[zbus(property)]
    fn sub_state(&self) -> &str {
        "dead"
    }
    #[zbus(property)]
    fn following(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn fragment_path(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn unit_file_state(&self) -> &str {
        if self.0.enabled.load(Ordering::Relaxed) {
            "enabled"
        } else {
            "disabled"
        }
    }
    #[zbus(property)]
    fn unit_file_preset(&self) -> &str {
        "disabled"
    }
    #[zbus(property)]
    fn drop_in_paths(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn documentation(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn control_group(&self) -> &str {
        ""
    }
    #[zbus(property)]
    fn transient(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_reload(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn memory_current(&self) -> u64 {
        4096
    }
}
struct Service;
#[zbus::interface(name = "org.freedesktop.systemd1.Service")]
impl Service {
    #[zbus(property)]
    fn main_pid(&self) -> u32 {
        0
    }
    #[zbus(property)]
    fn status_text(&self) -> &str {
        "用户提供的状态 <>& 不应被翻译"
    }
}
