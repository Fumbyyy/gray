mod app;
mod composer;
mod theme;

use gray_ide::bridge;

use std::path::PathBuf;

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::GrayIde;

fn main() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (handle, events) = bridge::spawn(cwd);

    Application::new().run(|cx: &mut App| {
        cx.bind_keys(composer::keybindings());
        cx.bind_keys([
            KeyBinding::new("escape", app::CancelTurn, None),
            KeyBinding::new("ctrl-n", app::NewSession, None),
            KeyBinding::new("cmd-n", app::NewSession, None),
        ]);

        let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(600.))),
                app_id: Some("id.alignment.gray-ide".into()),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("gray")),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            move |window, cx| {
                let events = events.clone();
                cx.new(|cx| GrayIde::new(handle.clone(), events, window, cx))
            },
        ) {
            eprintln!("failed to open gray-ide window: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
