use std::sync::mpsc;
use std::time::Duration;

use webview2_com::BrowserProcessExitedEventHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment5;
use windows::core::Interface;

pub fn watch_main_browser_process_exit(window: &tauri::WebviewWindow) -> bool {
    let (tx, rx) = mpsc::channel();
    let result = window.with_webview(move |webview| {
        let registered = unsafe {
            webview
                .environment()
                .cast::<ICoreWebView2Environment5>()
                .and_then(|environment| {
                    let mut token = 0;
                    environment.add_BrowserProcessExited(
                        BrowserProcessExitedEventHandler::create(Box::new(|_, args| {
                            if let Some(args) = args {
                                let mut pid = 0;
                                let _ = unsafe { args.BrowserProcessId(&mut pid) };
                                crate::info!(
                                    "[webview-environment] WebView2 browser process exited: pid={}",
                                    pid
                                );
                            } else {
                                crate::info!("[webview-environment] WebView2 browser process exited");
                            }
                            crate::infrastructure::webview_environment::mark_main_browser_process_exited();
                            Ok(())
                        })),
                        &mut token,
                    )
                })
        };

        match registered {
            Ok(()) => {
                let _ = tx.send(true);
            }
            Err(err) => {
                crate::warn!(
                    "[webview-environment] Failed to watch WebView2 browser process exit: {:?}",
                    err
                );
                let _ = tx.send(false);
            }
        }
    });

    if let Err(err) = result {
        crate::warn!(
            "[webview-environment] Failed to access WebView2 environment before destroy: {}",
            err
        );
        return false;
    }

    rx.recv_timeout(Duration::from_millis(500)).unwrap_or(false)
}
