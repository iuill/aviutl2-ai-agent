use serde::{Deserialize, Serialize};

// Temporary source-change marker for CI cache measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub status: HealthStatus,
    pub plugin_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HealthStatus {
    Ok,
    ShuttingDown,
    Degraded,
}

/// Phase 0 only: observation from invoking `call_read_section` on an HTTP worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadSectionProbe {
    pub success: bool,
    pub worker_thread: String,
    pub callback_thread: Option<String>,
    pub elapsed_micros: u64,
    pub scene_name: Option<String>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_round_trip_uses_camel_case() {
        let health = Health {
            status: HealthStatus::Ok,
            plugin_version: "0.0.1".into(),
        };
        let json = serde_json::to_string(&health).unwrap();
        assert_eq!(json, r#"{"status":"ok","pluginVersion":"0.0.1"}"#);
        assert_eq!(serde_json::from_str::<Health>(&json).unwrap(), health);
    }

    #[test]
    fn health_accepts_unknown_fields_for_forward_compatibility() {
        let json = r#"{"status":"ok","pluginVersion":"0.0.1","surprise":true}"#;
        let health = serde_json::from_str::<Health>(json).unwrap();
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.plugin_version, "0.0.1");
    }

    #[test]
    fn read_section_probe_uses_camel_case() {
        let probe = ReadSectionProbe {
            success: true,
            worker_thread: "ThreadId(2)".into(),
            callback_thread: Some("ThreadId(2)".into()),
            elapsed_micros: 42,
            scene_name: Some("Scene 1".into()),
            error: None,
        };
        let json = serde_json::to_string(&probe).unwrap();
        assert!(json.contains(r#""workerThread":"ThreadId(2)""#));
        assert!(json.contains(r#""elapsedMicros":42"#));
        assert_eq!(
            serde_json::from_str::<ReadSectionProbe>(&json).unwrap(),
            probe
        );
    }
}
