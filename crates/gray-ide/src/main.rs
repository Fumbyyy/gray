#![recursion_limit = "512"]
mod app;
mod assets;
mod git;
mod graycmds;
mod inbox;
mod ledger;
mod plugins;
mod provider;
mod pty;
mod sessions;
mod skills;
mod store;
mod usage;
mod workspace;

use gpui_kit::*;
use std::path::PathBuf;

/// Same precedence as `gray_core::paths::user_home` so the IDE resolves
/// the same home as the CLI on Windows (USERPROFILE) and unix (HOME).
/// `crate::skills::discover` uses this too so skill roots stay consistent.
fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("GRAY_IDE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".gray")
                .join("ide")
        })
}

fn main() {
    env_logger::init();
    let initial = std::env::args().nth(1).map(PathBuf::from).or_else(|| {
        if std::env::var_os("GRAY_IDE_NO_CWD").is_some() {
            None
        } else {
            std::env::current_dir().ok()
        }
    });
    let store = store::Store::open(&data_dir()).expect("open gray data directory");
    gpui_kit::application()
        .with_assets(assets::GrayAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            app::bind_keys(cx);
            cx.text_system()
                .add_fonts(vec![
                    std::borrow::Cow::Borrowed(include_bytes!(
                        "../fonts/PlusJakartaSans-Variable.ttf"
                    ) as &[u8]),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/DepartureMono-Regular.ttf") as &[u8],
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/Brygada1918-Regular.ttf") as &[u8]
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/Brygada1918-Italic.ttf") as &[u8]
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/IBMPlexSans-Regular.ttf") as &[u8]
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/IBMPlexSans-Italic.ttf") as &[u8]
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf") as &[u8]
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../fonts/IBMPlexSans-SemiBoldItalic.ttf") as &[u8]
                    ),
                    std::borrow::Cow::Borrowed(include_bytes!("../fonts/Lilex-Regular.ttf") as &[u8]),
                    std::borrow::Cow::Borrowed(include_bytes!("../fonts/Lilex-Bold.ttf") as &[u8]),
                    std::borrow::Cow::Borrowed(include_bytes!("../fonts/Lilex-Italic.ttf") as &[u8]),
                ])
                .expect("load gray fonts");
            app::apply_brand(cx);
            let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)));
            let opts = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Gray".into()),
                    ..Default::default()
                }),
                window_background: WindowBackgroundAppearance::Opaque,
                ..Default::default()
            };
            gpui_kit::open_window(opts, cx, move |window, cx| {
                cx.new(|cx| app::GrayApp::new(store, initial, window, cx))
            })
            .expect("open window");
            cx.activate(true);
        });
}
