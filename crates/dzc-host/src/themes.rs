use crate::{Host, Session};
use dzc_core::api::{CoreError, CoreErrorCode};
use dzc_protocol::wire::*;
use serde_json::{json, Value};
use std::path::Path;

fn theme_error(error: CoreError) -> WireErrorCode {
    match error.to_string().as_str() {
        "custom theme not found" => WireErrorCode::NotFound,
        _ if error.code == CoreErrorCode::Validation => WireErrorCode::InvalidRequest,
        _ => WireErrorCode::Unavailable,
    }
}

impl Host {
    pub(crate) fn theme_request(
        &self,
        session: &Session,
        method: Method,
        params: Value,
    ) -> Result<Value, WireErrorCode> {
        let string = |key: &str| params[key].as_str().ok_or(WireErrorCode::InvalidRequest);
        self.check_library_state(
            session,
            string("service_instance_id")?,
            params["expected_epoch"]
                .as_u64()
                .ok_or(WireErrorCode::InvalidRequest)?,
        )?;
        let path = || {
            let path = Path::new(string("path")?);
            path.is_absolute()
                .then_some(path)
                .ok_or(WireErrorCode::InvalidRequest)
        };
        let value = match method {
            Method::DesktopThemesList => json!(ThemeIndex {
                ids: self.history.custom_theme_ids().map_err(theme_error)?
            }),
            Method::DesktopThemesGet => json!(self
                .history
                .custom_theme(string("id")?)
                .map_err(theme_error)?),
            Method::DesktopThemesImport => json!(self
                .history
                .import_theme_file(path()?)
                .map_err(theme_error)?),
            Method::DesktopThemesRemove => json!(ThemeRemoved {
                active_reset: self
                    .history
                    .remove_theme(string("id")?)
                    .map_err(theme_error)?,
            }),
            Method::DesktopThemesExport => {
                self.history
                    .export_theme_file(string("id")?, path()?)
                    .map_err(theme_error)?;
                json!(OperationResult { performed: true })
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        if matches!(
            method,
            Method::DesktopThemesImport | Method::DesktopThemesRemove
        ) {
            self.publish(
                EventKind::ServiceStateChanged,
                EventData {
                    entry_id: None,
                    status: Some("custom_themes_changed".into()),
                },
            )?;
        }
        Ok(value)
    }
}
