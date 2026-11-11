
use serde::{Deserialize, Serialize};


#[cfg(test)]
mod tests {
    use crate::services::respond::{JSendResponse, JSendStatus, error, fail, success};

    use super::*;
    use serde_json::{Value, json};
    
    #[derive(Deserialize, Serialize)]
    struct TestData {
        id: i32,
        name: String,
    }

    #[test]
    fn test_success_creates_correct_response() {
        let data = TestData {
            id: 1,
            name: "test".to_string(),
        };
        let response = success(data);

        assert!(matches!(response.status, JSendStatus::Success));
        assert!(response.data.is_some());
        assert!(response.message.is_none());
        assert!(response.code.is_none());
    }

    #[test]
    fn test_fail_creates_correct_response() {
        let data = TestData {
            id: 2,
            name: "failed".to_string(),
        };
        let response = fail(data);

        assert!(matches!(response.status, JSendStatus::Fail));
        assert!(response.data.is_some());
        assert!(response.message.is_none());
        assert!(response.code.is_none());
    }

    #[test]
    fn test_error_with_all_fields() {
        let data = TestData {
            id: 3,
            name: "error".to_string(),
        };
        let response = error("Something went wrong", Some(500), Some(data));

        assert!(matches!(response.status, JSendStatus::Error));
        assert!(response.data.is_some());
        assert_eq!(response.message, Some("Something went wrong".to_string()));
        assert_eq!(response.code, Some(500));
    }

    #[test]
    fn test_error_without_data() {
        let response: JSendResponse<TestData> = error("Error occurred", Some(404), None);

        assert!(matches!(response.status, JSendStatus::Error));
        assert!(response.data.is_none());
        assert_eq!(response.message, Some("Error occurred".to_string()));
        assert_eq!(response.code, Some(404));
    }

    #[test]
    fn test_error_without_code() {
        let response: JSendResponse<()> = error("Error without code", None, None);

        assert!(matches!(response.status, JSendStatus::Error));
        assert!(response.data.is_none());
        assert_eq!(response.message, Some("Error without code".to_string()));
        assert!(response.code.is_none());
    }

    #[test]
    fn test_success_serialization() {
        let data = TestData {
            id: 1,
            name: "test".to_string(),
        };
        let response = success(data);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["status"], "success");
        assert_eq!(json["data"]["id"], 1);
        assert_eq!(json["data"]["name"], "test");
        assert!(json.get("message").is_none());
        assert!(json.get("code").is_none());
    }

    #[test]
    fn test_fail_serialization() {
        let data = TestData {
            id: 2,
            name: "failed".to_string(),
        };
        let response = fail(data);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["status"], "fail");
        assert_eq!(json["data"]["id"], 2);
        assert_eq!(json["data"]["name"], "failed");
        assert!(json.get("message").is_none());
        assert!(json.get("code").is_none());
    }

    #[test]
    fn test_error_serialization_full() {
        let data = TestData {
            id: 3,
            name: "error".to_string(),
        };
        let response = error("Internal error", Some(500), Some(data));
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["status"], "error");
        assert_eq!(json["data"]["id"], 3);
        assert_eq!(json["message"], "Internal error");
        assert_eq!(json["code"], 500);
    }

    #[test]
    fn test_error_serialization_minimal() {
        let response: JSendResponse<()> = error("Error message", None, None);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["status"], "error");
        assert_eq!(json["message"], "Error message");
        assert!(json.get("data").is_none());
        assert!(json.get("code").is_none());
    }

    #[test]
    fn test_success_deserialization() {
        let json = json!({
            "status": "success",
            "data": {
                "id": 1,
                "name": "test"
            }
        });

        let response: JSendResponse<TestData> = serde_json::from_value(json).unwrap();
        assert!(matches!(response.status, JSendStatus::Success));
        assert_eq!(response.data.unwrap().id, 1);
    }

    #[test]
    fn test_fail_deserialization() {
        let json = json!({
            "status": "fail",
            "data": {
                "id": 2,
                "name": "failed"
            }
        });

        let response: JSendResponse<TestData> = serde_json::from_value(json).unwrap();
        assert!(matches!(response.status, JSendStatus::Fail));
        assert_eq!(response.data.unwrap().name, "failed");
    }

    #[test]
    fn test_error_deserialization() {
        let json = json!({
            "status": "error",
            "message": "Something failed",
            "code": 500
        });

        let response: JSendResponse<Value> = serde_json::from_value(json).unwrap();
        assert!(matches!(response.status, JSendStatus::Error));
        assert_eq!(response.message, Some("Something failed".to_string()));
        assert_eq!(response.code, Some(500));
    }

    #[test]
    fn test_status_enum_serialization() {
        assert_eq!(
            serde_json::to_string(&JSendStatus::Success).unwrap(),
            r#""success""#
        );
        assert_eq!(
            serde_json::to_string(&JSendStatus::Fail).unwrap(),
            r#""fail""#
        );
        assert_eq!(
            serde_json::to_string(&JSendStatus::Error).unwrap(),
            r#""error""#
        );
    }

    #[test]
    fn test_error_message_accepts_string() {
        let response: JSendResponse<()> = error("string slice".to_string(), None, None);
        assert_eq!(response.message, Some("string slice".to_string()));
    }

    #[test]
    fn test_error_message_accepts_str() {
        let response: JSendResponse<()> = error("string slice", None, None);
        assert_eq!(response.message, Some("string slice".to_string()));
    }

    #[test]
    fn test_with_unit_type() {
        let response = success(());
        assert!(matches!(response.status, JSendStatus::Success));

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["status"], "success");
        assert_eq!(json["data"], json!(null));
    }

    #[test]
    fn test_with_primitive_types() {
        let response = success(42);
        assert_eq!(response.data, Some(42));

        let response = success("hello");
        assert_eq!(response.data, Some("hello"));

        let response = success(vec![1, 2, 3]);
        assert_eq!(response.data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_roundtrip_success() {
        let original = success(TestData {
            id: 1,
            name: "test".to_string(),
        });
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: JSendResponse<TestData> = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized.status, JSendStatus::Success));
        assert_eq!(deserialized.data.unwrap().id, 1);
    }

    #[test]
    fn test_roundtrip_error() {
        let original: JSendResponse<TestData> = error("Test error", Some(400), None);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: JSendResponse<TestData> = serde_json::from_str(&json).unwrap();

        assert!(matches!(deserialized.status, JSendStatus::Error));
        assert_eq!(deserialized.message, Some("Test error".to_string()));
        assert_eq!(deserialized.code, Some(400));
    }
}
