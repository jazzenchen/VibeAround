//! System tray and window visibility (Tray-First UX). Tauri 2.10 API.

use crate::TunnelState;
use tauri::{
    image::Image,
    menu::{Menu, MenuEvent, MenuItemBuilder},
    tray::TrayIconBuilder,
    App, AppHandle, Manager, Runtime,
};

const TRAY_WINDOW_LABEL: &str = "main";
const WEB_DASHBOARD_URL: &str = "http://127.0.0.1:5182";

pub fn setup<R: Runtime>(app: &App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItemBuilder::with_id("show", "Show").build(app)?;
    let open_web_item = MenuItemBuilder::with_id("open_web", "Open Web Dashboard").build(app)?;
    let open_tunnel_item = MenuItemBuilder::with_id("open_tunnel", "Open tunnel URL").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = Menu::with_items(app, &[&show_item, &open_web_item, &open_tunnel_item, &quit_item])?;

    let icon_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png");
    let icon = Image::from_path(icon_path)?;

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .tooltip("VibeAround")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event: MenuEvent| {
            let id = event.id().as_ref();
            if id == "quit" {
                app.exit(0);
            } else if id == "show" {
                show_tray_window(app);
            } else if id == "open_web" {
                let _ = open::that(WEB_DASHBOARD_URL);
            } else if id == "open_tunnel" {
                if let Some(state) = app.try_state::<TunnelState>() {
                    if let Ok(guard) = state.0.read() {
                        if let Some(ref url) = *guard {
                            let _ = open::that(url);
                        }
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn show_tray_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
