use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub status: HealthStatus,
    pub plugin_version: String,
    pub api_version: String,
    pub listener_address: String,
    pub process_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentScene {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    RouteNotFound,
    EditorBusy,
    EditorUnavailable,
    InternalError,
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
    fn phase1_responses_use_stable_field_names() {
        let status = Status {
            status: HealthStatus::Ok,
            plugin_version: "0.0.1".into(),
            api_version: "v1".into(),
            listener_address: "127.0.0.1:7890".into(),
            process_id: 42,
        };
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"status":"ok","pluginVersion":"0.0.1","apiVersion":"v1","listenerAddress":"127.0.0.1:7890","processId":42}"#
        );

        let scene = CurrentScene {
            name: "Scene 1".into(),
        };
        assert_eq!(
            serde_json::to_string(&scene).unwrap(),
            r#"{"name":"Scene 1"}"#
        );
    }

    #[test]
    fn api_error_uses_snake_case_code() {
        let error = ApiError {
            code: ErrorCode::EditorBusy,
            message: "EditorGate is busy".into(),
            retryable: true,
        };
        assert_eq!(
            serde_json::to_string(&error).unwrap(),
            r#"{"code":"editor_busy","message":"EditorGate is busy","retryable":true}"#
        );
    }

    #[test]
    fn phase1_responses_accept_unknown_fields() {
        let status = serde_json::from_str::<Status>(
            r#"{"status":"ok","pluginVersion":"0.0.1","apiVersion":"v1","listenerAddress":"127.0.0.1:7890","processId":42,"future":true}"#,
        )
        .unwrap();
        assert_eq!(status.process_id, 42);

        let scene =
            serde_json::from_str::<CurrentScene>(r#"{"name":"Root","future":true}"#).unwrap();
        assert_eq!(scene.name, "Root");
    }
}
