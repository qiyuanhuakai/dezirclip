//! GPU-process suppression for the embedded WebView.
//!
//! Tauri uses WebView2 on Windows (Chromium multi-process) and WebKitGTK on
//! Linux. Both can spawn auxiliary GPU processes that survive `WebviewWindow`
//! destruction — on Windows the GPU process is a singleton managed by
//! WebView2; on Linux WebKitGTK keeps its compositing layer alive even after
//! the renderer is gone. Either way ~200-400 MB of resident memory is held
//! by processes the user has no visibility into.
//!
//! For a clipboard manager the GPU is pure overhead: no WebGL, no video, no
//! 3D transforms. We expose a single boolean (`app.disable_webview_gpu`)
//! that, when enabled, applies Chromium / WebKitGTK flags that:
//!
//!   * disable hardware acceleration,
//!   * skip the GPU compositor,
//!   * fall back to CPU rendering.
//!
//! On Windows this collapses the GPU process entirely. On Linux it forces
//! WebKitGTK into a non-compositing mode that runs entirely inside the
//! renderer process — which our existing idle-destroyer can then tear down.
//!
//! The setting is applied via environment variables so it is picked up by
//! every WebView2 / WebKitWebProcess the runtime spawns after the call.
//! Apply once at startup, and again before every webview recreate so
//! changes take effect on the next show without restarting the app.

#[cfg(target_os = "windows")]
const WEBVIEW2_GPU_DISABLE_FLAGS: &str =
    "--disable-gpu --disable-gpu-compositing --disable-features=VizDisplayCompositor --disable-software-rasterizer";

#[cfg(target_os = "linux")]
const WEBKIT_GPU_DISABLE_FLAGS: &[&str] = &[
    "WEBKIT_DISABLE_COMPOSITING_MODE=1",
    "WEBKIT_DISABLE_DMABUF_RENDERER=1",
];

#[cfg(target_os = "windows")]
fn merge_flag(existing: Option<String>, extra: &str) -> String {
    match existing {
        Some(v) if !v.trim().is_empty() => {
            if v.split_whitespace().any(|tok| tok == extra) {
                v
            } else {
                format!("{v} {extra}")
            }
        }
        _ => extra.to_string(),
    }
}

/// Apply (or remove) GPU-suppression environment variables based on
/// `disabled`. Idempotent — calling it twice with the same flag is a no-op.
/// Safe to invoke from any thread before a WebView is spawned.
pub fn apply_gpu_disable_env(disabled: bool) {
    if disabled {
        apply_disable_flags();
    } else {
        clear_disable_flags();
    }
}

#[cfg(target_os = "windows")]
fn apply_disable_flags() {
    let key = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
    for flag in WEBVIEW2_GPU_DISABLE_FLAGS.split_whitespace() {
        let merged = merge_flag(std::env::var(key).ok(), flag);
        std::env::set_var(key, merged);
    }
    crate::info!(
        "[gpu-switcher] GPU disabled; {}={:?}",
        key,
        std::env::var(key).ok()
    );
}

#[cfg(not(target_os = "windows"))]
fn apply_disable_flags() {
    for entry in WEBKIT_GPU_DISABLE_FLAGS {
        if let Some((k, v)) = entry.split_once('=') {
            std::env::set_var(k, v);
        }
    }
    crate::info!(
        "[gpu-switcher] GPU disabled; WebKitGTK env {:?}={:?}",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").ok()
    );
}

#[cfg(target_os = "windows")]
fn clear_disable_flags() {
    let key = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
    let current = match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return,
    };
    let disabled: Vec<&str> = WEBVIEW2_GPU_DISABLE_FLAGS.split_whitespace().collect();
    let kept: Vec<&str> = current
        .split_whitespace()
        .filter(|tok| !disabled.contains(tok))
        .collect();
    if kept.is_empty() {
        std::env::remove_var(key);
    } else {
        std::env::set_var(key, kept.join(" "));
    }
    crate::info!(
        "[gpu-switcher] GPU re-enabled; {}={:?}",
        key,
        std::env::var(key).ok()
    );
}

#[cfg(not(target_os = "windows"))]
fn clear_disable_flags() {
    for entry in WEBKIT_GPU_DISABLE_FLAGS {
        if let Some((k, _)) = entry.split_once('=') {
            std::env::remove_var(k);
        }
    }
    crate::info!("[gpu-switcher] GPU re-enabled; WebKitGTK env vars cleared");
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    /// We test only the merge helper because the env-mutating helpers touch
    /// process-global state and must not run concurrently with the real app.
    #[cfg(target_os = "windows")]
    #[test]
    fn merge_flag_appends_when_missing() {
        let r = merge_flag(Some("--foo".into()), "--disable-gpu");
        assert_eq!(r, "--foo --disable-gpu");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn merge_flag_dedupes_when_present() {
        let r = merge_flag(Some("--disable-gpu --foo".into()), "--disable-gpu");
        assert_eq!(r, "--disable-gpu --foo");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn merge_flag_replaces_empty() {
        let r = merge_flag(None, "--disable-gpu");
        assert_eq!(r, "--disable-gpu");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn merge_flag_preserves_other_args() {
        let r = merge_flag(
            Some("--custom-arg --disable-gpu --other".into()),
            "--disable-gpu",
        );
        assert_eq!(r, "--custom-arg --disable-gpu --other");
    }
}
