//! Validate environment availability and X11 TCP fallback arithmetic before native probes.
//! The native library remains responsible for DISPLAY grammar and authentication.
use std::ffi::OsStr;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayUnavailable {
    NotSet,
    InvalidDisplay,
}
impl std::fmt::Display for DisplayUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotSet => "X11DisplayNotSet",
            Self::InvalidDisplay => "X11DisplayInvalid",
        })
    }
}
impl std::error::Error for DisplayUnavailable {}
pub fn validate_x11_display() -> Result<(), DisplayUnavailable> {
    validate_display_value(std::env::var_os("DISPLAY").as_deref())
}
fn validate_display_value(display: Option<&OsStr>) -> Result<(), DisplayUnavailable> {
    let display = display
        .ok_or(DisplayUnavailable::NotSet)?
        .to_str()
        .ok_or(DisplayUnavailable::InvalidDisplay)?;
    if display.is_empty() {
        return Err(DisplayUnavailable::InvalidDisplay);
    }
    // x11rb considers TCP fallback even for a local socket. Its port must fit in u16.
    if display
        .rsplit(':')
        .next()
        .and_then(|part| part.split('.').next())
        .and_then(|number| number.parse::<u32>().ok())
        .is_some_and(|number| number > u16::MAX as u32 - 6000)
    {
        return Err(DisplayUnavailable::InvalidDisplay);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn x11_tcp_fallback_port_does_not_overflow() {
        for display in [":0", ":99.0", "localhost:59535.0", "unix/:77"] {
            assert_eq!(validate_display_value(Some(OsStr::new(display))), Ok(()));
        }
        for display in ["", ":59536", ":65530", "host:65535.0", ":65536"] {
            assert_eq!(
                validate_display_value(Some(OsStr::new(display))),
                Err(DisplayUnavailable::InvalidDisplay)
            );
        }
        assert_eq!(
            validate_display_value(None),
            Err(DisplayUnavailable::NotSet)
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn non_unicode_display_is_a_normal_unavailable_result() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            validate_display_value(Some(OsStr::from_bytes(b":\xff"))),
            Err(DisplayUnavailable::InvalidDisplay)
        );
    }
}
