//! Local IPC values validated against the single source in `protocol/schema`.
//! Optional fields are forward compatible. Unknown methods and required capabilities
//! are rejected; neither role names nor values in these messages grant authority.
mod generated;
pub use generated::*;

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::OnceLock};

pub const VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
pub const SCHEMA: &str = include_str!("schema.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidMessage;

/// Validators have only local fragment references. Network and file resolvers are
/// disabled in the dependency, and no peer-supplied schema is ever compiled.
fn validators() -> &'static Result<HashMap<String, jsonschema::Validator>, InvalidMessage> {
    static VALIDATORS: OnceLock<Result<HashMap<String, jsonschema::Validator>, InvalidMessage>> = OnceLock::new();
    VALIDATORS.get_or_init(|| {
        let schema: Value = serde_json::from_str(SCHEMA).map_err(|_| InvalidMessage)?;
        let definitions = schema["$defs"].as_object().ok_or(InvalidMessage)?;
        let mut validators = HashMap::new();
        for name in definitions.keys() {
            let mut root = schema.clone();
            root["$ref"] = Value::String(format!("#/$defs/{name}"));
            validators.insert(name.clone(), jsonschema::validator_for(&root).map_err(|_| InvalidMessage)?);
        }
        Ok(validators)
    })
}

pub fn validate(name: &str, value: &Value) -> Result<(), InvalidMessage> {
    fn within_depth(value: &Value, remaining: usize) -> bool {
        if remaining == 0 { return false; }
        match value {
            Value::Array(values) => values.iter().all(|value| within_depth(value, remaining - 1)),
            Value::Object(values) => values.values().all(|value| within_depth(value, remaining - 1)),
            _ => true,
        }
    }
    if !within_depth(value, MAX_JSON_DEPTH) { return Err(InvalidMessage); }
    let validator = validators().as_ref().map_err(|_| InvalidMessage)?.get(name).ok_or(InvalidMessage)?;
    if validator.is_valid(value) { Ok(()) } else { Err(InvalidMessage) }
}

pub fn decode<T: DeserializeOwned>(name: &str, bytes: &[u8]) -> Result<T, InvalidMessage> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES { return Err(InvalidMessage); }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| InvalidMessage)?;
    validate(name, &value)?;
    serde_json::from_value(value).map_err(|_| InvalidMessage)
}

pub fn encode<T: Serialize>(name: &str, message: &T) -> Result<Vec<u8>, InvalidMessage> {
    let value = serde_json::to_value(message).map_err(|_| InvalidMessage)?;
    validate(name, &value)?;
    let bytes = serde_json::to_vec(&value).map_err(|_| InvalidMessage)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES { return Err(InvalidMessage); }
    Ok(bytes)
}

impl WireError {
    pub fn fixed(code: WireErrorCode) -> Self {
        Self { code, message: "Local request failed".into() }
    }
}

impl Response {
    pub fn success(request_id: String, result: Value) -> Self {
        Self { request_id, result: Some(result), error: None }
    }
    pub fn failure(request_id: String, code: WireErrorCode) -> Self {
        Self { request_id, result: None, error: Some(WireError::fixed(code)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_rust_go_fixtures_obey_schema_and_round_trip() {
        let fixtures: Value = serde_json::from_str(include_str!("../../../../protocol/fixtures/messages.json")).unwrap();
        for fixture in fixtures.as_array().unwrap() {
            let name = fixture["schema"].as_str().unwrap();
            if fixture.get("raw").is_some() || fixture.get("raw_hex").is_some() {
                let raw = if let Some(raw) = fixture["raw"].as_str() { raw.as_bytes().to_vec() } else {
                    fixture["raw_hex"].as_str().unwrap().as_bytes().chunks_exact(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
                };
                assert_eq!(decode::<Value>(name, &raw).is_ok(), fixture["valid"].as_bool().unwrap(), "{}", fixture["name"]);
                continue;
            }
            assert_eq!(validate(name, &fixture["value"]).is_ok(), fixture["valid"].as_bool().unwrap(), "{}", fixture["name"]);
            if let Some(expected) = fixture.get("expected") {
                let bytes = serde_json::to_vec(&fixture["value"]).unwrap();
                let actual = match name {
                    "PrivacySetParams" => serde_json::to_value(decode::<PrivacySetParams>(name, &bytes).unwrap()).unwrap(),
                    "HelloParams" => serde_json::to_value(decode::<HelloParams>(name, &bytes).unwrap()).unwrap(),
                    "WireError" => serde_json::to_value(decode::<WireError>(name, &bytes).unwrap()).unwrap(),
                    "HistoryResult" => serde_json::to_value(decode::<HistoryResult>(name, &bytes).unwrap()).unwrap(),
                    _ => panic!("unsupported typed fixture"),
                };
                assert_eq!(&actual, expected, "{}", fixture["name"]);
            }
            if fixture["valid"] == true && name == "Request" {
                let bytes = serde_json::to_vec(&fixture["value"]).unwrap();
                let request: Request = decode(name, &bytes).unwrap();
                assert_eq!(serde_json::from_slice::<Value>(&encode(name, &request).unwrap()).unwrap(), fixture["value"]);
            }
        }
    }

    #[test]
    fn wire_rejects_non_json_and_frames_over_budget_without_reflection() {
        assert!(decode::<Request>("Request", b"not-json").is_err());
        assert!(decode::<Request>("Request", &vec![b' '; MAX_FRAME_BYTES + 1]).is_err());
        assert!(validate("UnknownSchema", &serde_json::json!({})).is_err());
        let error = serde_json::to_value(WireError::fixed(WireErrorCode::PermissionDenied)).unwrap();
        assert_eq!(error["code"], "permission_denied");
        assert_eq!(error["message"], "Local request failed");
    }
    #[test]
    fn tag_page_requests_require_bounded_owner_and_epoch_metadata() {
        let valid = serde_json::json!({"request_id":"tag-fixture","method":"desktop.tags.page","params":{
            "tag":"密码","query":"","sort_by":"time","limit":60,"offset":0,"service_instance_id":"original-owner","expected_epoch":3}});
        assert!(validate("Request", &valid).is_ok());
        for (field, value) in [("limit", serde_json::json!(101)), ("offset", serde_json::json!(-1)),
            ("tag", serde_json::json!("")), ("sort_by", serde_json::json!("body")), ("reveal", serde_json::json!(true))] {
            let mut invalid = valid.clone(); invalid["params"][field] = value;
            assert!(validate("Request", &invalid).is_err(), "{field}");
        }
        for field in ["expected_epoch", "service_instance_id"] {
            let mut invalid = valid.clone(); invalid["params"].as_object_mut().unwrap().remove(field);
            assert!(validate("Request", &invalid).is_err());
        }
    }
}
