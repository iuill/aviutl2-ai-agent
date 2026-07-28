//! Phase 0 plugin spike.
//!
//! The HTTP server is deliberately transport-only for now. SDK calls will be
//! added as individual probes after plugin lifecycle behavior is verified on
//! a Windows host.

mod server;

pub use server::{HealthServer, ServerError};

#[cfg(windows)]
mod windows_plugin {
    use aviutl2::generic::{GenericPlugin, GenericPluginTable, GlobalEditHandle, HostAppHandle};

    use crate::HealthServer;

    pub(super) static EDIT_HANDLE: GlobalEditHandle = GlobalEditHandle::new();

    #[aviutl2::plugin(GenericPlugin)]
    struct AgentPlugin {
        _health_server: HealthServer,
    }

    impl GenericPlugin for AgentPlugin {
        fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
            let health_server = HealthServer::start("127.0.0.1:7890", 4)?;
            Ok(Self {
                _health_server: health_server,
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

    aviutl2::register_generic_plugin!(AgentPlugin);
}

// Keep Linux workspace checks useful while the real entry point is Windows-only.
#[cfg(not(windows))]
#[unsafe(no_mangle)]
pub extern "C" fn aviutl2_ai_agent_phase0_placeholder() {}
