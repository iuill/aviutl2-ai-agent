use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    fn health_rejects_unknown_fields() {
        let json = r#"{"status":"ok","pluginVersion":"0.0.1","surprise":true}"#;
        assert!(serde_json::from_str::<Health>(json).is_err());
    }
}
