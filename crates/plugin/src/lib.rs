//! Phase 1 read-only API plugin.

mod editor;
mod server;

pub use server::{ApiServer, ServerError};

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

    const LIFECYCLE_LOG_ENV: &str = "AVIUTL2_AI_AGENT_PHASE1_LIFECYCLE_LOG";

    pub(super) static EDIT_HANDLE: GlobalEditHandle = GlobalEditHandle::new();

    #[aviutl2::plugin(GenericPlugin)]
    struct AgentPlugin {
        api_server: Option<ApiServer>,
    }

    impl GenericPlugin for AgentPlugin {
        fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
            let api_server = ApiServer::start("127.0.0.1:7890", 4)?;
            Ok(Self {
                api_server: Some(api_server),
            })
        }

        fn plugin_info(&self) -> GenericPluginTable {
            GenericPluginTable {
                name: "aviutl2-ai-agent Phase 1".to_owned(),
                information: format!(
                    "aviutl2-ai-agent {} — local read-only API",
                    env!("CARGO_PKG_VERSION")
                ),
            }
        }

        fn register(&mut self, registry: &mut HostAppHandle) {
            EDIT_HANDLE.init(registry.create_edit_handle());
        }
    }

    impl Drop for AgentPlugin {
        fn drop(&mut self) {
            write_lifecycle_event("plugin_drop_started", None);
            let observation = self.api_server.as_mut().map(ApiServer::shutdown).unwrap_or(
                crate::server::ShutdownObservation {
                    worker_count: 0,
                    join_panics: 0,
                },
            );
            write_lifecycle_event(
                "http_workers_joined",
                Some((observation.worker_count, observation.join_panics)),
            );
            self.api_server.take();
            write_lifecycle_event("plugin_drop_completed", None);
        }
    }

    fn write_lifecycle_event(event: &str, shutdown: Option<(usize, usize)>) {
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
        let _ = writeln!(file, "{record}");
    }

    aviutl2::register_generic_plugin!(AgentPlugin);
}

// Keep Linux workspace checks useful while the real entry point is Windows-only.
#[cfg(not(windows))]
#[unsafe(no_mangle)]
pub extern "C" fn aviutl2_ai_agent_placeholder() {}
