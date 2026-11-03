use base64::Engine;
use dzc_core::{classification, qr, transforms};
use dzc_protocol::wire::{self, Method, ToolClassifyParams, ToolQrParams, ToolTransformParams, WireErrorCode};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

fn parameters<T: DeserializeOwned>(value: Value) -> Result<T, WireErrorCode> {
    serde_json::from_value(value).map_err(|_| WireErrorCode::InvalidRequest)
}

pub(super) fn run(method: Method, params: Value) -> Result<Value, WireErrorCode> {
    let result = match method {
        Method::DesktopSettingsFonts => json!({ "fonts": dzc_platform::fonts::list_system_fonts() }),
        Method::DesktopToolsCatalog => json!({
            "content_kinds": classification::CONTENT_KINDS,
            "transforms": transforms::list_transform_kinds(),
        }),
        Method::DesktopToolsClassify => {
            let input: ToolClassifyParams = parameters(params)?;
            json!({ "kinds": classification::classify(&input.text) })
        }
        Method::DesktopToolsTransform => {
            let input: ToolTransformParams = parameters(params)?;
            let text = transforms::transform_text(&input.text, &input.kind)
                .map_err(|_| WireErrorCode::InvalidRequest)?;
            json!({ "text": text })
        }
        Method::DesktopToolsQr => {
            let input: ToolQrParams = parameters(params)?;
            let text = match input.format.as_str() {
                "svg" => qr::generate_qr_svg(&input.content),
                "png" => qr::generate_qr_png(&input.content, input.size_px.unwrap_or(256))
                    .map(|bytes| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))),
                _ => return Err(WireErrorCode::InvalidRequest),
            }.map_err(|_| WireErrorCode::InvalidRequest)?;
            json!({ "text": text })
        }
        _ => return Err(WireErrorCode::InvalidRequest),
    };
    // Leave room for the response envelope and the request ID after JSON escaping.
    if serde_json::to_vec(&result).map_err(|_| WireErrorCode::Internal)?.len() > wire::MAX_FRAME_BYTES - 4096 {
        return Err(WireErrorCode::FrameTooLarge);
    }
    Ok(result)
}
