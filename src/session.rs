//! 表示サーバ種別（X11 / Wayland）。

use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
}

impl DisplayServer {
    pub fn name(self) -> &'static str {
        match self {
            Self::X11 => "x11",
            Self::Wayland => "wayland",
        }
    }
}

/// 環境変数から判定する。`XDG_SESSION_TYPE=wayland` または `WAYLAND_DISPLAY` があれば Wayland。
/// Wayland では Xwayland の DISPLAY があっても X11 に落とさない。
pub fn detect() -> Result<DisplayServer, String> {
    detect_from_env(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var("WAYLAND_DISPLAY").ok().as_deref(),
        env::var("DISPLAY").ok().as_deref(),
    )
}

pub fn detect_from_env(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> Result<DisplayServer, String> {
    let session = session_type.map(|s| s.trim().to_ascii_lowercase());
    if session.as_deref() == Some("wayland") {
        return Ok(DisplayServer::Wayland);
    }
    if wayland_display.map(|s| !s.is_empty()).unwrap_or(false) {
        return Ok(DisplayServer::Wayland);
    }
    if session.as_deref() == Some("x11") {
        return Ok(DisplayServer::X11);
    }
    if display.map(|s| !s.is_empty()).unwrap_or(false) {
        return Ok(DisplayServer::X11);
    }
    Err(
        "表示サーバを判定できません。X11（DISPLAY）または Wayland（WAYLAND_DISPLAY / XDG_SESSION_TYPE）が必要です"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_by_session_type() {
        assert_eq!(
            detect_from_env(Some("wayland"), None, Some(":0")).unwrap(),
            DisplayServer::Wayland
        );
    }

    #[test]
    fn wayland_by_wayland_display() {
        assert_eq!(
            detect_from_env(None, Some("wayland-0"), Some(":0")).unwrap(),
            DisplayServer::Wayland
        );
    }

    #[test]
    fn x11_by_display() {
        assert_eq!(
            detect_from_env(Some("x11"), None, Some(":0")).unwrap(),
            DisplayServer::X11
        );
        assert_eq!(
            detect_from_env(None, None, Some(":1")).unwrap(),
            DisplayServer::X11
        );
    }

    #[test]
    fn none_fails() {
        assert!(detect_from_env(None, None, None).is_err());
        assert!(detect_from_env(None, Some(""), Some("")).is_err());
    }
}
