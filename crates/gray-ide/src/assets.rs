use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub struct GrayAssets;

impl AssetSource for GrayAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/gray-logo.svg" => Some(include_bytes!("../assets/icons/gray-logo.svg")),
            "icons/claude.svg" => Some(include_bytes!("../assets/icons/claude.svg")),
            "icons/openai.svg" => Some(include_bytes!("../assets/icons/openai.svg")),
            "bg/andromeda.png" => Some(include_bytes!("../assets/andromeda-dither.png")),
            "bg/bluemarble.png" => Some(include_bytes!("../assets/bluemarble-dither.png")),
            "icons/ai_anthropic.svg" => Some(include_bytes!("../assets/icons/ai_anthropic.svg")),
            "icons/ai_claude.svg" => Some(include_bytes!("../assets/icons/ai_claude.svg")),
            "icons/ai_deep_seek.svg" => Some(include_bytes!("../assets/icons/ai_deep_seek.svg")),
            "icons/ai_edit.svg" => Some(include_bytes!("../assets/icons/ai_edit.svg")),
            "icons/ai_gemini.svg" => Some(include_bytes!("../assets/icons/ai_gemini.svg")),
            "icons/ai_google.svg" => Some(include_bytes!("../assets/icons/ai_google.svg")),
            "icons/ai_mistral.svg" => Some(include_bytes!("../assets/icons/ai_mistral.svg")),
            "icons/ai_ollama.svg" => Some(include_bytes!("../assets/icons/ai_ollama.svg")),
            "icons/ai_open_ai.svg" => Some(include_bytes!("../assets/icons/ai_open_ai.svg")),
            "icons/ai_open_router.svg" => {
                Some(include_bytes!("../assets/icons/ai_open_router.svg"))
            }
            "icons/ai_x_ai.svg" => Some(include_bytes!("../assets/icons/ai_x_ai.svg")),
            "icons/ai_zed.svg" => Some(include_bytes!("../assets/icons/ai_zed.svg")),
            "icons/archive.svg" => Some(include_bytes!("../assets/icons/archive.svg")),
            "icons/arrow_left.svg" => Some(include_bytes!("../assets/icons/arrow_left.svg")),
            "icons/arrow_right.svg" => Some(include_bytes!("../assets/icons/arrow_right.svg")),
            "icons/arrow_up_right.svg" => {
                Some(include_bytes!("../assets/icons/arrow_up_right.svg"))
            }
            "icons/bell.svg" => Some(include_bytes!("../assets/icons/bell.svg")),
            "icons/book.svg" => Some(include_bytes!("../assets/icons/book.svg")),
            "icons/check.svg" => Some(include_bytes!("../assets/icons/check.svg")),
            "icons/chevron_down.svg" => Some(include_bytes!("../assets/icons/chevron_down.svg")),
            "icons/chevron_left.svg" => Some(include_bytes!("../assets/icons/chevron_left.svg")),
            "icons/chevron_right.svg" => Some(include_bytes!("../assets/icons/chevron_right.svg")),
            "icons/chevron_up.svg" => Some(include_bytes!("../assets/icons/chevron_up.svg")),
            "icons/circle_help.svg" => Some(include_bytes!("../assets/icons/circle_help.svg")),
            "icons/clock.svg" => Some(include_bytes!("../assets/icons/clock.svg")),
            "icons/close.svg" => Some(include_bytes!("../assets/icons/close.svg")),
            "icons/copy.svg" => Some(include_bytes!("../assets/icons/copy.svg")),
            "icons/dash.svg" => Some(include_bytes!("../assets/icons/dash.svg")),
            "icons/debug.svg" => Some(include_bytes!("../assets/icons/debug.svg")),
            "icons/diff.svg" => Some(include_bytes!("../assets/icons/diff.svg")),
            "icons/diff_split.svg" => Some(include_bytes!("../assets/icons/diff_split.svg")),
            "icons/diff_split_auto.svg" => {
                Some(include_bytes!("../assets/icons/diff_split_auto.svg"))
            }
            "icons/diff_unified.svg" => Some(include_bytes!("../assets/icons/diff_unified.svg")),
            "icons/download.svg" => Some(include_bytes!("../assets/icons/download.svg")),
            "icons/envelope.svg" => Some(include_bytes!("../assets/icons/envelope.svg")),
            "icons/eye.svg" => Some(include_bytes!("../assets/icons/eye.svg")),
            "icons/file-c.svg" => Some(include_bytes!("../assets/icons/file-c.svg")),
            "icons/file-cpp.svg" => Some(include_bytes!("../assets/icons/file-cpp.svg")),
            "icons/file-css.svg" => Some(include_bytes!("../assets/icons/file-css.svg")),
            "icons/file-go.svg" => Some(include_bytes!("../assets/icons/file-go.svg")),
            "icons/file-html.svg" => Some(include_bytes!("../assets/icons/file-html.svg")),
            "icons/file-javascript.svg" => {
                Some(include_bytes!("../assets/icons/file-javascript.svg"))
            }
            "icons/file-json.svg" => Some(include_bytes!("../assets/icons/file-json.svg")),
            "icons/file-markdown.svg" => Some(include_bytes!("../assets/icons/file-markdown.svg")),
            "icons/file-python.svg" => Some(include_bytes!("../assets/icons/file-python.svg")),
            "icons/file-rust.svg" => Some(include_bytes!("../assets/icons/file-rust.svg")),
            "icons/file-toml.svg" => Some(include_bytes!("../assets/icons/file-toml.svg")),
            "icons/file-typescript.svg" => {
                Some(include_bytes!("../assets/icons/file-typescript.svg"))
            }
            "icons/file-yaml.svg" => Some(include_bytes!("../assets/icons/file-yaml.svg")),
            "icons/file.svg" => Some(include_bytes!("../assets/icons/file.svg")),
            "icons/folder.svg" => Some(include_bytes!("../assets/icons/folder.svg")),
            "icons/folder_open.svg" => Some(include_bytes!("../assets/icons/folder_open.svg")),
            "icons/git_branch.svg" => Some(include_bytes!("../assets/icons/git_branch.svg")),
            "icons/git_commit.svg" => Some(include_bytes!("../assets/icons/git_commit.svg")),
            "icons/history_rerun.svg" => Some(include_bytes!("../assets/icons/history_rerun.svg")),
            "icons/info.svg" => Some(include_bytes!("../assets/icons/info.svg")),
            "icons/list_tree.svg" => Some(include_bytes!("../assets/icons/list_tree.svg")),
            "icons/lock.svg" => Some(include_bytes!("../assets/icons/lock.svg")),
            "icons/magnifying_glass.svg" => {
                Some(include_bytes!("../assets/icons/magnifying_glass.svg"))
            }
            "icons/person.svg" => Some(include_bytes!("../assets/icons/person.svg")),
            "icons/pin.svg" => Some(include_bytes!("../assets/icons/pin.svg")),
            "icons/play_filled.svg" => Some(include_bytes!("../assets/icons/play_filled.svg")),
            "icons/play_outlined.svg" => Some(include_bytes!("../assets/icons/play_outlined.svg")),
            "icons/plus.svg" => Some(include_bytes!("../assets/icons/plus.svg")),
            "icons/pull_request.svg" => Some(include_bytes!("../assets/icons/pull_request.svg")),
            "icons/queue_message.svg" => Some(include_bytes!("../assets/icons/queue_message.svg")),
            "icons/return.svg" => Some(include_bytes!("../assets/icons/return.svg")),
            "icons/rotate_cw.svg" => Some(include_bytes!("../assets/icons/rotate_cw.svg")),
            "icons/send.svg" => Some(include_bytes!("../assets/icons/send.svg")),
            "icons/settings.svg" => Some(include_bytes!("../assets/icons/settings.svg")),
            "icons/sparkle.svg" => Some(include_bytes!("../assets/icons/sparkle.svg")),
            "icons/stop.svg" => Some(include_bytes!("../assets/icons/stop.svg")),
            "icons/terminal.svg" => Some(include_bytes!("../assets/icons/terminal.svg")),
            "icons/terminal_alt.svg" => Some(include_bytes!("../assets/icons/terminal_alt.svg")),
            "icons/tool_copy.svg" => Some(include_bytes!("../assets/icons/tool_copy.svg")),
            "icons/tool_delete_file.svg" => {
                Some(include_bytes!("../assets/icons/tool_delete_file.svg"))
            }
            "icons/tool_diagnostics.svg" => {
                Some(include_bytes!("../assets/icons/tool_diagnostics.svg"))
            }
            "icons/tool_hammer.svg" => Some(include_bytes!("../assets/icons/tool_hammer.svg")),
            "icons/tool_notification.svg" => {
                Some(include_bytes!("../assets/icons/tool_notification.svg"))
            }
            "icons/tool_pencil.svg" => Some(include_bytes!("../assets/icons/tool_pencil.svg")),
            "icons/tool_search.svg" => Some(include_bytes!("../assets/icons/tool_search.svg")),
            "icons/tool_terminal.svg" => Some(include_bytes!("../assets/icons/tool_terminal.svg")),
            "icons/tool_think.svg" => Some(include_bytes!("../assets/icons/tool_think.svg")),
            "icons/tool_web.svg" => Some(include_bytes!("../assets/icons/tool_web.svg")),
            "icons/trash.svg" => Some(include_bytes!("../assets/icons/trash.svg")),
            "icons/warning.svg" => Some(include_bytes!("../assets/icons/warning.svg")),
            "icons/zed_agent.svg" => Some(include_bytes!("../assets/icons/zed_agent.svg")),
            "icons/zed_agent_two.svg" => Some(include_bytes!("../assets/icons/zed_agent_two.svg")),
            "icons/zed_assistant.svg" => Some(include_bytes!("../assets/icons/zed_assistant.svg")),
            "fonts/Brygada1918-Italic.ttf" => {
                Some(include_bytes!("../fonts/Brygada1918-Italic.ttf"))
            }
            "fonts/Brygada1918-Regular.ttf" => {
                Some(include_bytes!("../fonts/Brygada1918-Regular.ttf"))
            }
            "fonts/DepartureMono-Regular.ttf" => {
                Some(include_bytes!("../fonts/DepartureMono-Regular.ttf"))
            }
            "fonts/IBMPlexSans-Italic.ttf" => {
                Some(include_bytes!("../fonts/IBMPlexSans-Italic.ttf"))
            }
            "fonts/IBMPlexSans-Regular.ttf" => {
                Some(include_bytes!("../fonts/IBMPlexSans-Regular.ttf"))
            }
            "fonts/IBMPlexSans-SemiBold.ttf" => {
                Some(include_bytes!("../fonts/IBMPlexSans-SemiBold.ttf"))
            }
            "fonts/IBMPlexSans-SemiBoldItalic.ttf" => {
                Some(include_bytes!("../fonts/IBMPlexSans-SemiBoldItalic.ttf"))
            }
            "fonts/Lilex-Bold.ttf" => Some(include_bytes!("../fonts/Lilex-Bold.ttf")),
            "fonts/Lilex-Italic.ttf" => Some(include_bytes!("../fonts/Lilex-Italic.ttf")),
            "fonts/Lilex-Regular.ttf" => Some(include_bytes!("../fonts/Lilex-Regular.ttf")),
            "fonts/PlusJakartaSans-Variable.ttf" => {
                Some(include_bytes!("../fonts/PlusJakartaSans-Variable.ttf"))
            }
            _ => None,
        };
        if let Some(b) = bytes {
            return Ok(Some(Cow::Borrowed(b)));
        }
        // Full Lucide catalog — the default `Assets` bundle only embeds
        // ~106 icons and misses many (gauge, git-branch, bell-ring...).
        gpui_kit::assets::AllAssets::new("").load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::AllAssets::new("").list(path)
    }
}
