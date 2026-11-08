use crate::{Host, Session};
use dzc_protocol::wire::*;
use serde_json::Value;

impl Host {
    pub(crate) fn system_settings_request(&self, session: &Session, method: Method, params: Value) -> Result<Value, WireErrorCode> {
        self.desktop_permission(session, self.instance_id())?;
        session.require(Permission::PrivacyWrite)?;
        let mut pending = session.system_change.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let value = match method {
            Method::DesktopSystemExplorerRestart => {
                dzc_platform::desktop_system::restart_explorer().map_err(system_error)?;
                true
            }
            Method::DesktopSystemWinVGet => dzc_platform::desktop_system::win_v_optimized().map_err(system_error)?,
            Method::DesktopSystemWinVBegin => {
                if pending.is_some() { return Err(WireErrorCode::Busy); }
                let input: WindowsSettingParams = serde_json::from_value(params).map_err(|_| WireErrorCode::InvalidRequest)?;
                let (value, change) = dzc_platform::desktop_system::WinVChange::begin(input.enabled).map_err(system_error)?;
                let token = change.token().to_owned();
                *pending = Some(change);
                return serde_json::to_value(WindowsSettingResult { value, token }).map_err(|_| WireErrorCode::Unavailable);
            }
            Method::DesktopSystemWinVFinish => {
                let input: WindowsSettingFinishParams = serde_json::from_value(params).map_err(|_| WireErrorCode::InvalidRequest)?;
                let change = pending.as_mut().filter(|change| change.token() == input.token).ok_or(WireErrorCode::NotFound)?;
                let value = change.finish(input.rollback).map_err(system_error)?;
                pending.take();
                value
            }
            Method::DesktopSystemClipboardHistorySet => {
                if pending.is_some() { return Err(WireErrorCode::Busy); }
                let input: WindowsSettingParams = serde_json::from_value(params).map_err(|_| WireErrorCode::InvalidRequest)?;
                dzc_platform::desktop_system::set_clipboard_history(input.enabled).map_err(system_error)?;
                true
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        serde_json::to_value(WindowsSettingResult { value, token: String::new() }).map_err(|_| WireErrorCode::Unavailable)
    }
}

fn system_error(error: std::io::Error) -> WireErrorCode {
    match error.kind() {
        std::io::ErrorKind::Unsupported => WireErrorCode::UnsupportedCapability,
        std::io::ErrorKind::PermissionDenied => WireErrorCode::PermissionDenied,
        std::io::ErrorKind::WouldBlock => WireErrorCode::Busy,
        _ => WireErrorCode::Unavailable,
    }
}
