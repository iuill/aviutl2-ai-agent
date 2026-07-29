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
#[serde(rename_all = "camelCase")]
pub struct CurrentTimeline {
    pub width: usize,
    pub height: usize,
    pub frame_rate: FrameRate,
    pub cursor_frame: usize,
    pub object_end_frame: usize,
    pub highest_object_layer: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRate {
    pub numerator: i32,
    pub denominator: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentObjects {
    pub objects: Vec<TimelineObject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineObject {
    pub layer: usize,
    pub start_frame: usize,
    pub end_frame: usize,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveObjectRequest {
    pub expected_scene_name: String,
    pub target: TimelineObject,
    pub destination: MoveObjectDestination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveObjectDestination {
    pub layer: usize,
    pub start_frame: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveObjectResponse {
    pub object: TimelineObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteObjectRequest {
    pub expected_scene_name: String,
    pub target: TimelineObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteObjectResponse {
    pub deleted: TimelineObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTextObjectRequest {
    pub expected_scene_name: String,
    pub layer: usize,
    pub start_frame: usize,
    pub length: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTextObjectResponse {
    pub object: TimelineObject,
    pub text: String,
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
    ObjectNotFound,
    StateConflict,
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

        let timeline = CurrentTimeline {
            width: 1920,
            height: 1080,
            frame_rate: FrameRate {
                numerator: 30,
                denominator: 1,
            },
            cursor_frame: 12,
            object_end_frame: 99,
            highest_object_layer: 2,
        };
        assert_eq!(
            serde_json::to_string(&timeline).unwrap(),
            r#"{"width":1920,"height":1080,"frameRate":{"numerator":30,"denominator":1},"cursorFrame":12,"objectEndFrame":99,"highestObjectLayer":2}"#
        );

        let objects = CurrentObjects {
            objects: vec![TimelineObject {
                layer: 0,
                start_frame: 10,
                end_frame: 39,
                name: Some("Title".into()),
            }],
        };
        assert_eq!(
            serde_json::to_string(&objects).unwrap(),
            r#"{"objects":[{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"}]}"#
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
    fn move_request_uses_strict_camel_case_contract() {
        let json = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"destination":{"layer":2,"startFrame":100}}"#;
        let request = serde_json::from_str::<MoveObjectRequest>(json).unwrap();
        assert_eq!(request.expected_scene_name, "Root");
        assert_eq!(request.destination.layer, 2);
        assert_eq!(serde_json::to_string(&request).unwrap(), json);

        let with_unknown = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":"Title"},"destination":{"layer":2,"startFrame":100},"extra":true}"#;
        assert!(serde_json::from_str::<MoveObjectRequest>(with_unknown).is_err());
    }

    #[test]
    fn delete_request_uses_strict_camel_case_contract() {
        let json = r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":null}}"#;
        let request = serde_json::from_str::<DeleteObjectRequest>(json).unwrap();
        assert_eq!(request.target.start_frame, 10);
        assert_eq!(serde_json::to_string(&request).unwrap(), json);
        assert!(
            serde_json::from_str::<DeleteObjectRequest>(
                r#"{"expectedSceneName":"Root","target":{"layer":0,"startFrame":10,"endFrame":39,"name":null},"extra":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn create_text_request_uses_strict_camel_case_contract() {
        let json =
            r#"{"expectedSceneName":"Root","layer":1,"startFrame":100,"length":90,"text":"Hello"}"#;
        let request = serde_json::from_str::<CreateTextObjectRequest>(json).unwrap();
        assert_eq!(request.text, "Hello");
        assert_eq!(request.length, 90);
        assert_eq!(serde_json::to_string(&request).unwrap(), json);
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
