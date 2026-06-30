use std::sync::OnceLock;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{MenuItem, Status, Tray};

pub struct TrayCallbacks {
    pub on_left_click: Box<dyn Fn() + Send + Sync + 'static>,
    pub on_show_hide: Box<dyn Fn() + Send + Sync + 'static>,
    pub on_settings: Box<dyn Fn() + Send + Sync + 'static>,
    pub on_quit: Box<dyn Fn() + Send + Sync + 'static>,
}

struct DezirClipTray {
    hidden: bool,
    callbacks: TrayCallbacks,
}

static LINUX_TRAY_HANDLE: OnceLock<Handle<DezirClipTray>> = OnceLock::new();

pub fn setup_status_icon(hide_tray: bool, callbacks: TrayCallbacks) {
    let tray = DezirClipTray {
        hidden: hide_tray,
        callbacks,
    };

    let in_flatpak = std::path::Path::new("/.flatpak-info").exists();
    match tray
        .disable_dbus_name(in_flatpak)
        .assume_sni_available(true)
        .spawn()
    {
        Ok(handle) => {
            let _ = LINUX_TRAY_HANDLE.set(handle);
        }
        Err(error) => crate::warn!("Failed to create Linux status notifier tray: {}", error),
    }
}

pub fn set_tray_visible(visible: bool) {
    if let Some(handle) = LINUX_TRAY_HANDLE.get() {
        let _ = handle.update(|tray| tray.hidden = !visible);
    }
}

impl Tray for DezirClipTray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "dezirclip".to_string()
    }

    fn title(&self) -> String {
        "DezirClip".to_string()
    }

    fn icon_name(&self) -> String {
        "dezirclip".to_string()
    }

    fn status(&self) -> Status {
        if self.hidden {
            Status::Passive
        } else {
            Status::Active
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.callbacks.on_left_click)();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "显示/隐藏".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.callbacks.on_show_hide)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "设置".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.callbacks.on_settings)()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "退出 DezirClip".to_string(),
                activate: Box::new(|tray: &mut Self| (tray.callbacks.on_quit)()),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_callbacks() -> TrayCallbacks {
        TrayCallbacks {
            on_left_click: Box::new(|| {}),
            on_show_hide: Box::new(|| {}),
            on_settings: Box::new(|| {}),
            on_quit: Box::new(|| {}),
        }
    }

    #[test]
    fn linux_tray_left_click_does_not_open_menu() {
        assert!(!DezirClipTray::MENU_ON_ACTIVATE);
    }

    #[test]
    fn linux_tray_menu_contains_expected_actions() {
        let tray = DezirClipTray {
            hidden: false,
            callbacks: noop_callbacks(),
        };
        let labels = tray
            .menu()
            .into_iter()
            .map(|item| match item {
                MenuItem::Standard(item) => item.label,
                _ => String::new(),
            })
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["显示/隐藏", "设置", "退出 DezirClip"]);
    }

    #[test]
    fn linux_tray_hidden_state_maps_to_passive_status() {
        let tray = DezirClipTray {
            hidden: true,
            callbacks: noop_callbacks(),
        };

        assert_eq!(tray.status(), Status::Passive);
    }
}
