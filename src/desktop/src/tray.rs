//! System tray (Tray-First UX). Tauri 2.10 API.
//! Native menu with quick actions; "Show Window" opens the main webview window.
//! During onboarding, service-related items are disabled until `onboarding-complete` fires.

use std::sync::atomic::Ordering;

use tauri::{
    image::Image,
    menu::{Menu, MenuItemBuilder},
    tray::TrayIconBuilder,
    App, Listener, Manager, Runtime,
};

use crate::{AppServiceManager, OnboardingActive};

const MAIN_WINDOW_LABEL: &str = "main";
const LOCAL_DASHBOARD_URL: &str = "http://127.0.0.1:12358";

pub fn setup<R: Runtime>(app: &App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let is_onboarding = app
        .try_state::<OnboardingActive>()
        .map(|s| s.0.load(Ordering::Relaxed))
        .unwrap_or(false);

    let show_item = MenuItemBuilder::with_id("show_window", "Show Window").build(app)?;
    let open_local_item = MenuItemBuilder::with_id("open_local", "Open Local Dashboard")
        .enabled(!is_onboarding)
        .build(app)?;
    let open_tunnel_item = MenuItemBuilder::with_id("open_tunnel", "Open Tunnel")
        .enabled(false)
        .build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = Menu::with_items(app, &[&show_item, &open_local_item, &open_tunnel_item, &quit_item])?;

    let icon_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png");
    let icon = Image::from_path(icon_path)?;

    // Clone items for the async watchers
    let tunnel_item_clone = open_tunnel_item.clone();
    let open_local_clone = open_local_item.clone();

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("VibeAround")
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show_window" => {
                if let Some(w) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "open_local" => {
                let _ = open::that(LOCAL_DASHBOARD_URL);
            }
            "open_tunnel" => {
                if let Some(state) = app.try_state::<AppServiceManager>() {
                    if let Some(url) = state.0.get_tunnel_url() {
                        let _ = open::that(&url);
                    }
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    // Listen for onboarding-complete → re-enable menu items
    let open_local_onboard = open_local_clone.clone();
    app.listen("onboarding-complete", move |_| {
        let _ = open_local_onboard.set_enabled(true);
    });

    // Watch for tunnel state changes → enable/disable "Open Tunnel" menu item
    let app_handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app_handle.try_state::<AppServiceManager>() else { return };
        let sm = &state.0;
        let mut rx = sm.subscribe_changes();
        loop {
            let has_url = sm.has_tunnel_url();
            let _ = tunnel_item_clone.set_enabled(has_url);

            // Wait for next change notification
            if rx.recv().await.is_err() { break; }
        }
    });

    Ok(())
}
