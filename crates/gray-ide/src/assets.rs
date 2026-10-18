use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct GrayAssets;

impl AssetSource for GrayAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/gray-logo.svg" => Some(include_bytes!("../assets/icons/gray-logo.svg")),
            "icons/claude.svg" => Some(include_bytes!("../assets/icons/claude.svg")),
            "icons/openai.svg" => Some(include_bytes!("../assets/icons/openai.svg")),
            _ => None,
        };
        if let Some(b) = bytes {
            return Ok(Some(Cow::Borrowed(b)));
        }
        gpui_kit::assets::Assets::new("").load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets::new("").list(path)
    }
}
