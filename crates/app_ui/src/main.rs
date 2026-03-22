use anyhow::{Context, Result};
use reqwest::Client;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Runtime, WindowEvent};
use tauri_plugin_global_shortcut::{Builder as ShortcutBuilder, GlobalShortcutExt, ShortcutState};

const BACKEND_URL: &str = "http://127.0.0.1:8765";

struct AppTray {
    _icon: TrayIcon<tauri::Wry>,
}

fn main() {
    if let Err(error) = run() {
        panic!("failed to run the Soundmind desktop UI: {error:#}");
    }
}

fn run() -> Result<()> {
    tauri::Builder::default()
        .plugin(ShortcutBuilder::new().build())
        .setup(|app| {
            install_tray(app)?;
            install_global_shortcuts(app.handle());
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-window" => {
                let _ = show_main_window(app);
            }
            "hide-window" => {
                let _ = hide_main_window(app);
            }
            "start-capture" => spawn_action("Start"),
            "stop-capture" => spawn_action("Stop"),
            "pause-cloud" => spawn_action("PauseCloud"),
            "resume-cloud" => spawn_action("ResumeCloud"),
            "answer-question" => spawn_action("AnswerLastQuestion"),
            "summarise-minute" => spawn_action("SummariseLastMinute"),
            "comment-topic" => spawn_action("CommentCurrentTopic"),
            "quit-app" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|app, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = toggle_main_window(app);
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .context("failed to start tauri runtime")?;

    Ok(())
}

fn install_global_shortcuts<R: Runtime>(app: &AppHandle<R>) {
    register_shortcut(app, "CmdOrCtrl+Alt+Shift+M", |app| {
        let _ = toggle_main_window(app);
    });
    register_shortcut(app, "CmdOrCtrl+Alt+Shift+A", |_| {
        spawn_action("AnswerLastQuestion");
    });
    register_shortcut(app, "CmdOrCtrl+Alt+Shift+S", |_| {
        spawn_action("SummariseLastMinute");
    });
    register_shortcut(app, "CmdOrCtrl+Alt+Shift+C", |_| {
        spawn_action("CommentCurrentTopic");
    });
}

fn register_shortcut<R, F>(app: &AppHandle<R>, shortcut: &str, action: F)
where
    R: Runtime,
    F: Fn(&AppHandle<R>) + Send + Sync + 'static,
{
    if let Err(error) = app.global_shortcut().on_shortcut(shortcut, move |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            action(app);
        }
    }) {
        eprintln!("failed to register global shortcut {shortcut}: {error}");
    }
}

fn install_tray(app: &mut App<tauri::Wry>) -> Result<()> {
    let show_window =
        MenuItem::with_id(app, "show-window", "Show Window", true, Some("Ctrl+Alt+Shift+M"))?;
    let hide_window = MenuItem::with_id(app, "hide-window", "Hide Window", true, None::<&str>)?;
    let start_capture =
        MenuItem::with_id(app, "start-capture", "Start Capture", true, None::<&str>)?;
    let stop_capture = MenuItem::with_id(app, "stop-capture", "Stop Capture", true, None::<&str>)?;
    let pause_cloud = MenuItem::with_id(app, "pause-cloud", "Pause Cloud", true, None::<&str>)?;
    let resume_cloud = MenuItem::with_id(app, "resume-cloud", "Resume Cloud", true, None::<&str>)?;
    let answer_question = MenuItem::with_id(
        app,
        "answer-question",
        "Answer Last Question",
        true,
        Some("Ctrl+Alt+Shift+A"),
    )?;
    let summarise_minute = MenuItem::with_id(
        app,
        "summarise-minute",
        "Summarise Last Minute",
        true,
        Some("Ctrl+Alt+Shift+S"),
    )?;
    let comment_topic = MenuItem::with_id(
        app,
        "comment-topic",
        "Comment Current Topic",
        true,
        Some("Ctrl+Alt+Shift+C"),
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_app = MenuItem::with_id(app, "quit-app", "Quit Soundmind", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show_window,
            &hide_window,
            &separator,
            &start_capture,
            &stop_capture,
            &pause_cloud,
            &resume_cloud,
            &separator,
            &answer_question,
            &summarise_minute,
            &comment_topic,
            &separator,
            &quit_app,
        ],
    )?;

    let mut tray_builder = TrayIconBuilder::with_id("soundmind-tray")
        .menu(&menu)
        .tooltip("Soundmind")
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon().cloned() {
        tray_builder = tray_builder.icon(icon);
    }

    let tray = tray_builder.build(app)?;
    app.manage(AppTray { _icon: tray });
    Ok(())
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
    }
    Ok(())
}

fn hide_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide()?;
    }
    Ok(())
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible()? {
            window.hide()?;
        } else {
            window.show()?;
            window.unminimize()?;
            window.set_focus()?;
        }
    }
    Ok(())
}

fn spawn_action(action: &'static str) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = dispatch_backend_action(action).await {
            eprintln!("failed to send backend action {action}: {error:#}");
        }
    });
}

async fn dispatch_backend_action(action: &'static str) -> Result<()> {
    Client::new()
        .post(format!("{BACKEND_URL}/actions"))
        .json(action)
        .send()
        .await
        .context("failed to reach backend action endpoint")?
        .error_for_status()
        .context("backend rejected action request")?;
    Ok(())
}
