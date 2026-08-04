// tray.rs — the menu bar / system tray icon and its menu.
//
// The tray is what keeps the app alive after the window is closed, which is
// what makes background sync and the daily digest possible at all.
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

pub const MENU_SHOW: &str = "show";
pub const MENU_SYNC: &str = "sync";
pub const MENU_AUTOSTART: &str = "autostart";
pub const MENU_QUIT: &str = "quit";

/// Bring the window back and focus it.
pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn toggle_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        match w.is_visible() {
            Ok(true) => {
                let _ = w.hide();
            }
            _ => show_window(app),
        }
    }
}

/// Build the tray icon and wire its menu.
///
/// `autostart_enabled` seeds the checkbox so it reflects reality on launch
/// rather than assuming a default.
pub fn build<R: Runtime>(app: &AppHandle<R>, autostart_enabled: bool) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Todo", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, MENU_SYNC, "Sync now", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        MENU_AUTOSTART,
        "Start on login",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &sync, &autostart, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("Todo")
        .menu(&menu)
        // The menu should only appear on right-click; a left-click toggles the
        // window, which is what people expect from a widget.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    // Without this the icon renders as a colored blob in the macOS menu bar
    // instead of adapting to light and dark.
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }

    builder.build(app)?;
    Ok(())
}
