//! System tray (Tray-First UX). Tauri 2.10 API.
//! Native menu with quick actions; "Show Desktop" opens the main webview window.

use crate::TunnelState;
use tauri::{
    image::Image,
    menu::{Menu, MenuItemBuilder},
    tray::TrayIconBuilder,
    App, Manager, Runtime,
};

const MAIN_WINDOW_LABEL: &str = "main";
const WEB_DASHBOARD_URL: &str = "http://127.0.0.1:12358";

pub fn setup<R: Runtime>(app: &App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItemBuilder::with_id("show_desktop", "Show Desktop").build(app)?;
    let open_web_item = MenuItemBuilder::with_id("open_web", "Open Web Dashboard").build(app)?;
    let open_tunnel_item = MenuItemBuilder::with_id("open_tunnel", "Open Tunnel URL").build(app)?;
    let restart_item = MenuItemBuilder::with_id("restart", "Restart Server").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = Menu::with_items(
        app,
        &[&show_item, &open_web_item, &open_tunnel_item, &restart_item, &quit_item],
    )?;

    let icon_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png");
    let icon = Image::from_path(icon_path)?;

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .tooltip("VibeAround")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_desktop" => {
                if let Some(w) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "open_web" => {
                let _ = open::that(WEB_DASHBOARD_URL);
            }
            "open_tunnel" => {
                if let Some(state) = app.try_state::<TunnelState>() {
                    if let Ok(guard) = state.0.read() {
                        if let Some(ref url) = *guard {
                            let _ = open::that(url);
                        }
                    }
                }
            }
            "restart" => {
                eprintln!("[VibeAround] Restart requested via tray — exiting (use supervisor to auto-restart)");
                app.exit(0);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
