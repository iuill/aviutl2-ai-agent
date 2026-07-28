//! Phase 0 plugin spike.
//!
//! The HTTP server is deliberately transport-only for now. SDK calls will be
//! added as individual probes after plugin lifecycle behavior is verified on
//! a Windows host.

mod server;

pub use server::{HealthServer, ServerError};

#[cfg(windows)]
mod windows_plugin {
    use std::{
        fs::OpenOptions,
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    use aviutl2::generic::{GenericPlugin, GenericPluginTable, GlobalEditHandle, HostAppHandle};
    use serde_json::json;

    use crate::HealthServer;

    const LIFECYCLE_LOG_ENV: &str = "AVIUTL2_AI_AGENT_PHASE0_LIFECYCLE_LOG";

    pub(super) static EDIT_HANDLE: GlobalEditHandle = GlobalEditHandle::new();

    #[aviutl2::plugin(GenericPlugin)]
    struct AgentPlugin {
        health_server: Option<HealthServer>,
    }

    impl GenericPlugin for AgentPlugin {
        fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
            let health_server = HealthServer::start("127.0.0.1:7890", 4)?;
            Ok(Self {
                health_server: Some(health_server),
            })
        }

        fn plugin_info(&self) -> GenericPluginTable {
            GenericPluginTable {
                name: "aviutl2-ai-agent Phase 0".to_owned(),
                information: format!(
                    "aviutl2-ai-agent {} — SDK fact-finding probe",
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
            let observation = self
                .health_server
                .as_mut()
                .map(HealthServer::shutdown)
                .unwrap_or(crate::server::ShutdownObservation {
                    worker_count: 0,
                    join_panics: 0,
                });
            write_lifecycle_event(
                "http_workers_joined",
                Some((observation.worker_count, observation.join_panics)),
            );
            self.health_server.take();
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
pub extern "C" fn aviutl2_ai_agent_phase0_placeholder() {}
