//! Local structured API plugin for AviUtl2.

mod editor;
mod mutation;
mod server;

pub use server::{ApiServer, ServerError};

#[cfg_attr(not(windows), allow(dead_code))]
fn build_label(version: &str, commit: Option<&str>) -> String {
    match commit.filter(|commit| !commit.is_empty()) {
        Some(commit) => format!(
            "{version} ({})",
            commit.chars().take(12).collect::<String>()
        ),
        None => version.to_owned(),
    }
}

#[cfg(windows)]
mod windows_plugin {
    use std::{
        fs::OpenOptions,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    use aviutl2::generic::{GenericPlugin, GenericPluginTable, GlobalEditHandle, HostAppHandle};
    use serde_json::json;

    use crate::ApiServer;

    const LIFECYCLE_LOG_ENV: &str = "AVIUTL2_AI_AGENT_LIFECYCLE_LOG";
    const EVENT_OBSERVATION_LOG_ENV: &str = "AVIUTL2_AI_AGENT_EVENT_OBSERVATION_LOG";

    pub(super) static EDIT_HANDLE: GlobalEditHandle = GlobalEditHandle::new();
    pub(super) static PROJECT_GENERATION: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(1);

    #[aviutl2::plugin(GenericPlugin)]
    struct AgentPlugin {
        api_server: Option<ApiServer>,
        api_start_error: Option<String>,
    }

    impl GenericPlugin for AgentPlugin {
        fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
            let (api_server, api_start_error) = match ApiServer::start("127.0.0.1:7890", 4) {
                Ok(server) => (Some(server), None),
                Err(error) => {
                    let error = error.to_string();
                    write_lifecycle_event("api_start_failed", None, Some(&error));
                    (None, Some(error))
                }
            };
            Ok(Self {
                api_server,
                api_start_error,
            })
        }

        fn plugin_info(&self) -> GenericPluginTable {
            let build = crate::build_label(
                env!("CARGO_PKG_VERSION"),
                option_env!("AVIUTL2_AI_AGENT_BUILD_COMMIT"),
            );
            let information = match &self.api_start_error {
                Some(error) => {
                    format!("aviutl2-ai-agent {build} — local API unavailable: {error}")
                }
                None => format!("aviutl2-ai-agent {build} — local structured API"),
            };
            GenericPluginTable {
                name: "aviutl2-ai-agent".to_owned(),
                information,
            }
        }

        fn register(&mut self, registry: &mut HostAppHandle) {
            EDIT_HANDLE.init(registry.create_edit_handle());
        }

        fn on_project_load(&mut self, _project: &mut aviutl2::generic::ProjectFile) {
            PROJECT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            write_observation_event("project_load");
        }

        fn event_update_object_info(&mut self) {
            write_observation_event("update_object");
        }

        fn event_change_edit_frame(&mut self) {
            write_observation_event("change_edit_frame");
        }

        fn event_change_scene_info(&mut self) {
            write_observation_event("change_edit_scene");
        }

        fn event_change_focus_object(&mut self) {
            write_observation_event("change_focus_object");
        }
    }

    impl Drop for AgentPlugin {
        fn drop(&mut self) {
            write_lifecycle_event("plugin_drop_started", None, None);
            let observation = self.api_server.as_mut().map(ApiServer::shutdown).unwrap_or(
                crate::server::ShutdownObservation {
                    worker_count: 0,
                    join_panics: 0,
                },
            );
            write_lifecycle_event(
                "http_workers_joined",
                Some((observation.worker_count, observation.join_panics)),
                None,
            );
            self.api_server.take();
            write_lifecycle_event("plugin_drop_completed", None, None);
        }
    }

    fn write_lifecycle_event(event: &str, shutdown: Option<(usize, usize)>, error: Option<&str>) {
        let Ok(path) = std::env::var(LIFECYCLE_LOG_ENV) else {
            return;
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let mut record = json!({
            "event": event,
            "timestampMillis": timestamp_millis,
            "thread": format!("{:?}", std::thread::current().id()),
        });
        if let Some((worker_count, join_panics)) = shutdown {
            record["workerCount"] = worker_count.into();
            record["joinPanics"] = join_panics.into();
        }
        if let Some(error) = error {
            record["error"] = error.into();
        }
        let _ = writeln!(file, "{record}");
    }

    fn write_observation_event(event: &str) {
        let Ok(path) = std::env::var(EVENT_OBSERVATION_LOG_ENV) else {
            return;
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let record = json!({
            "event": event,
            "timestampMillis": timestamp_millis,
            "thread": format!("{:?}", std::thread::current().id()),
        });
        let _ = writeln!(file, "{record}");
    }

    aviutl2::register_generic_plugin!(AgentPlugin);
}

// Keep Linux workspace checks useful while the real entry point is Windows-only.
#[cfg(not(windows))]
#[unsafe(no_mangle)]
pub extern "C" fn aviutl2_ai_agent_placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn build_label_keeps_semver_and_adds_a_short_commit_when_available() {
        assert_eq!(super::build_label("0.1.0", None), "0.1.0");
        assert_eq!(super::build_label("0.1.0", Some("")), "0.1.0");
        assert_eq!(
            super::build_label("0.1.0", Some("0123456789abcdef")),
            "0.1.0 (0123456789ab)"
        );
    }
}
