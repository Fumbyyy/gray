// Real PTY terminal session (portable-pty + vt100). gpui-agnostic.
//
// A `Pty` owns a spawned child process attached to a pseudo-terminal, a
// background reader thread feeding output into a `vt100` screen parser,
// and a background waiter thread that records child exit.
//
// Rendering surface: `text()` / `styled()` / `title()`.
// Input surface: `write()` for raw bytes, plus the free function
// `key_to_bytes()` which maps UI key events to terminal input bytes.
//
// Scrollback: `SCROLLBACK` rows are retained by the vt100 parser. The
// parser's view offset is always restored to 0 (live screen); `text()`
// and `styled()` walk the scrollback explicitly so they return the full
// transcript, oldest row first. Rendered rows are cached and only
// recomputed when the reader thread feeds new output.
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, SlavePty, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

/// A run of screen text sharing one set of attributes.
///
/// `fg`/`bg` are packed 0xRRGGBB; `None` means "terminal default"
/// (vt100 `Color::Default`). The standard/bright/indexed 256-color
/// palette entries are resolved to concrete RGB values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub fg: Option<u32>,
    pub bg: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Lock a mutex, recovering the guard if a panicking thread poisoned it.
fn plock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// vt100 callback sink used to capture window-title escape sequences
/// (OSC 0 / OSC 2) that don't affect the screen grid itself.
struct TitleSink {
    title: Arc<Mutex<Option<String>>>,
}

impl vt100::Callbacks for TitleSink {
    fn set_window_title(&mut self, _screen: &mut vt100::Screen, title: &[u8]) {
        *plock(&self.title) = Some(String::from_utf8_lossy(title).into_owned());
    }
}

/// Rows of scrollback retained above the visible screen.
const SCROLLBACK: usize = 1000;

struct Shared {
    /// Write half of the master side; input to the child.
    writer: Mutex<Box<dyn Write + Send>>,
    /// vt100 screen model, fed by the reader thread.
    parser: Mutex<vt100::Parser<TitleSink>>,
    /// Rendered scrollback+screen rows; invalidated whenever the reader
    /// thread feeds the parser, so `text()`/`styled()` calls that share
    /// a frame don't each walk every cell.
    rendered: Mutex<Option<Vec<Vec<Span>>>>,
    /// Set whenever the screen (or exit state) changed since last
    /// `clear_dirty()`.
    dirty: AtomicBool,
    /// Set when the child exits or the output stream hits EOF/error.
    exited: AtomicBool,
    /// Last window title requested via OSC, if any.
    title: Arc<Mutex<Option<String>>>,
}

/// Terminates the child when the last `Pty` clone is dropped. Holds a
/// cloned killer so the child can be terminated while another thread is
/// blocked in `wait`.
struct KillGuard {
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
}

impl Drop for KillGuard {
    fn drop(&mut self) {
        if let Some(mut k) = plock(&self.killer).take() {
            let _ = k.kill();
        }
    }
}

/// A live terminal session. Cheap to clone: all state is shared.
///
/// Safe to move/share across threads: the master/slave handles sit behind
/// `Mutex` because portable-pty's `dyn *Pty` objects are `Send` but not
/// `Sync`. Dropping the last clone kills the child via `KillGuard`.
#[derive(Clone)]
pub struct Pty {
    /// Held to keep the pty open; dropping the master ends the session.
    #[allow(dead_code)]
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// On Windows/ConPTY the slave side must stay alive or the pseudo
    /// console is destroyed and the child sees EOF. On unix it is dropped
    /// right after spawn so the reader sees EOF when the child exits.
    #[allow(dead_code)]
    slave: Option<Arc<Mutex<Box<dyn SlavePty + Send>>>>,
    shared: Arc<Shared>,
    /// Held for its Drop impl, which kills the child process.
    #[allow(dead_code)]
    kill_guard: Arc<KillGuard>,
}

impl Pty {
    /// Spawn `shell` (or a platform default) in `cwd` inside a pty of
    /// `cols`×`rows` cells.
    ///
    /// Shell resolution:
    /// - `shell` argument, if `Some`
    /// - Windows: `COMSPEC`, else `powershell.exe`
    /// - Unix: `SHELL`, else `sh`
    pub fn spawn(cwd: &Path, shell: Option<&str>, cols: u16, rows: u16) -> Result<Self, String> {
        let size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .map_err(|e| format!("openpty: {e}"))?;

        let program = shell
            .map(str::to_owned)
            .or_else(default_shell)
            .ok_or_else(|| "no shell found".to_string())?;

        let mut cmd = CommandBuilder::new(&program);
        cmd.cwd(cwd);
        // Advertise capabilities most TUIs expect.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn {program}: {e}"))?;
        let killer = child.clone_killer();
        // On unix the slave must be dropped so the reader sees EOF when
        // the child exits. On Windows/ConPTY dropping it destroys the
        // pseudo console and kills the session — keep it.
        #[cfg(unix)]
        let slave = {
            drop(pair.slave);
            None
        };
        #[cfg(windows)]
        let slave = Some(Arc::new(Mutex::new(pair.slave)));

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("pty writer: {e}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("pty reader: {e}"))?;

        let title = Arc::new(Mutex::new(None));
        let parser = vt100::Parser::new_with_callbacks(
            size.rows,
            size.cols,
            SCROLLBACK,
            TitleSink {
                title: Arc::clone(&title),
            },
        );

        let shared = Arc::new(Shared {
            writer: Mutex::new(writer),
            parser: Mutex::new(parser),
            rendered: Mutex::new(None),
            dirty: AtomicBool::new(true),
            exited: AtomicBool::new(false),
            title,
        });

        // Reader thread: pump master output into the vt100 parser.
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                // Carry the tail of the previous chunk so a device query
                // split across two reads is still seen and answered.
                let mut tail: Vec<u8> = Vec::new();
                let mut scan: Vec<u8> = Vec::with_capacity(8192 + 3);
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => {
                            scan.clear();
                            scan.extend_from_slice(&tail);
                            scan.extend_from_slice(&buf[..n]);
                            answer_device_queries(&shared, tail.len(), &scan);
                            tail.clear();
                            tail.extend_from_slice(&buf[n.saturating_sub(3)..n]);
                            {
                                let mut parser = plock(&shared.parser);
                                parser.process(&buf[..n]);
                                *plock(&shared.rendered) = None;
                            }
                            shared.dirty.store(true, Ordering::SeqCst);
                        }
                        Err(_) => break,
                    }
                }
                // Output stream ended: treat session as over.
                shared.exited.store(true, Ordering::SeqCst);
                shared.dirty.store(true, Ordering::SeqCst);
            });
        }

        // Waiter thread: record child exit status.
        {
            let shared = Arc::clone(&shared);
            thread::spawn(move || {
                let _ = child.wait();
                shared.exited.store(true, Ordering::SeqCst);
                shared.dirty.store(true, Ordering::SeqCst);
            });
        }

        Ok(Pty {
            master: Arc::new(Mutex::new(pair.master)),
            slave,
            shared,
            kill_guard: Arc::new(KillGuard {
                killer: Mutex::new(Some(killer)),
            }),
        })
    }

    /// Write raw input bytes to the child. Errors are swallowed: a dead
    /// session is reported via `is_exited()` instead.
    pub fn write(&self, data: &[u8]) {
        let mut w = plock(&self.shared.writer);
        let _ = w.write_all(data);
        let _ = w.flush();
    }

    /// Resize the pty and the vt100 screen model.
    #[allow(dead_code)]
    pub fn resize(&self, cols: u16, rows: u16) {
        let size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        plock(&self.shared.parser)
            .screen_mut()
            .set_size(size.rows, size.cols);
        *plock(&self.shared.rendered) = None;
        let _ = plock(&self.master).resize(size);
        self.shared.dirty.store(true, Ordering::SeqCst);
    }

    /// True once the child has exited or its output stream closed.
    pub fn is_exited(&self) -> bool {
        self.shared.exited.load(Ordering::SeqCst)
    }

    /// True if the screen (or exit state) changed since `clear_dirty()`.
    pub fn dirty(&self) -> bool {
        self.shared.dirty.load(Ordering::SeqCst)
    }

    /// Clear the dirty flag after rendering the current screen.
    pub fn clear_dirty(&self) {
        self.shared.dirty.store(false, Ordering::SeqCst);
    }

    /// Scrollback + visible screen rows as plain text, oldest first.
    /// Trailing blanks are trimmed per row and trailing all-blank rows
    /// are removed.
    pub fn text(&self) -> Vec<String> {
        self.rendered_rows()
            .iter()
            .map(|r| {
                r.iter()
                    .map(|s| s.text.as_str())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// Scrollback + visible screen rows as styled spans, oldest first.
    /// Consecutive cells with identical attributes are merged. Trailing
    /// unstyled blank cells are trimmed per row and trailing fully-empty
    /// rows are removed.
    pub fn styled(&self) -> Vec<Vec<Span>> {
        self.rendered_rows()
    }

    /// Rendered rows, recomputing only when the reader thread fed new
    /// output since the last call.
    fn rendered_rows(&self) -> Vec<Vec<Span>> {
        if let Some(rows) = plock(&self.shared.rendered).clone() {
            return rows;
        }
        let rows = {
            let mut parser = plock(&self.shared.parser);
            render_rows(parser.screen_mut())
        };
        *plock(&self.shared.rendered) = Some(rows.clone());
        rows
    }

    /// Last window title set by the child via OSC 0/2, if any.
    pub fn title(&self) -> Option<String> {
        plock(&self.shared.title).clone()
    }
}

/// Answer terminal device-status queries embedded in the output stream.
///
/// ConPTY emits `ESC[6n` (Device Status Report / cursor position) at
/// session start because portable-pty enables
/// `PSUEDOCONSOLE_INHERIT_CURSOR`, and stalls all child output until the
/// terminal replies `ESC[{row};{col}R`. vt100 parses queries but does
/// not answer them, so the reader must do so itself. Also answered:
/// `ESC[5n` (are-OK → `ESC[0n`), `ESC[c`/`ESC[0c` (primary Device
/// Attributes → `ESC[?1;2c`, VT100+AVO) and `ESC[>c` (secondary DA →
/// `ESC[>1;10;0c`), which many TUI programs probe at startup and block
/// on.
///
/// `tail_len` is how many leading bytes of `data` were carried over
/// from the previous read — a query fully inside that tail was already
/// answered then, so only matches that end past `tail_len` count.
fn answer_device_queries(shared: &Shared, tail_len: usize, data: &[u8]) {
    let fresh = |pat: &[u8]| {
        data.windows(pat.len())
            .enumerate()
            .any(|(i, w)| w == pat && i + pat.len() > tail_len)
    };
    let needs_dsr = fresh(b"\x1b[6n");
    let needs_ok = fresh(b"\x1b[5n");
    let needs_da = fresh(b"\x1b[c") || fresh(b"\x1b[0c");
    let needs_da2 = fresh(b"\x1b[>c");
    if !(needs_dsr || needs_ok || needs_da || needs_da2) {
        return;
    }
    let mut w = plock(&shared.writer);
    if needs_dsr {
        let (row, col) = {
            let parser = plock(&shared.parser);
            let (r, c) = parser.screen().cursor_position();
            (r.saturating_add(1), c.saturating_add(1))
        };
        let _ = w.write_all(format!("\x1b[{row};{col}R").as_bytes());
    }
    if needs_ok {
        let _ = w.write_all(b"\x1b[0n");
    }
    if needs_da {
        let _ = w.write_all(b"\x1b[?1;2c");
    }
    if needs_da2 {
        let _ = w.write_all(b"\x1b[>1;10;0c");
    }
    let _ = w.flush();
}

/// Render one screen row to styled spans, merging consecutive cells
/// with identical attributes and trimming trailing unstyled blanks.
fn row_spans(screen: &vt100::Screen, r: u16, cols: u16) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for c in 0..cols {
        let Some(cell) = screen.cell(r, c) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue; // width occupied by previous wide cell
        }
        let text = if cell.has_contents() {
            cell.contents().to_string()
        } else {
            " ".to_string()
        };
        let (mut fg, mut bg) = (color_u32(cell.fgcolor()), color_u32(cell.bgcolor()));
        if cell.inverse() {
            std::mem::swap(&mut fg, &mut bg);
        }
        let (bold, italic, underline) = (cell.bold(), cell.italic(), cell.underline());

        match spans.last_mut() {
            Some(s)
                if s.fg == fg
                    && s.bg == bg
                    && s.bold == bold
                    && s.italic == italic
                    && s.underline == underline =>
            {
                s.text.push_str(&text);
            }
            _ => spans.push(Span {
                text,
                fg,
                bg,
                bold,
                italic,
                underline,
            }),
        }
    }
    // Trim trailing blank, unstyled spans; keep the last span's
    // text right-trimmed if it only had padding spaces.
    while let Some(last) = spans.last_mut() {
        if last.bg.is_none()
            && !last.bold
            && !last.italic
            && !last.underline
            && last.text.trim_end().is_empty()
        {
            spans.pop();
        } else if last.bg.is_none() && !last.bold && !last.italic && !last.underline {
            let t = last.text.trim_end().to_string();
            last.text = t;
            break;
        } else {
            break;
        }
    }
    spans
}

/// Render the whole transcript — retained scrollback plus the visible
/// screen — as styled span rows, oldest first. `set_scrollback` moves
/// the parser's view window, so each scrollback row is rendered by
/// stepping the offset back one row at a time; the offset is always
/// restored to 0 (live screen) before returning.
fn render_rows(screen: &mut vt100::Screen) -> Vec<Vec<Span>> {
    let (rows, cols) = screen.size();
    // scrollback() reports the current view offset, clamped by
    // set_scrollback to the real backlog size — request the maximum to
    // learn how many scrolled-off rows exist.
    screen.set_scrollback(usize::MAX);
    let back = screen.scrollback();
    let mut out: Vec<Vec<Span>> = Vec::with_capacity(back + rows as usize);
    for i in 0..back {
        // View one row back each step: scrollback[i] lands on view row 0.
        screen.set_scrollback(back - i);
        out.push(row_spans(screen, 0, cols));
    }
    screen.set_scrollback(0);
    for r in 0..rows {
        out.push(row_spans(screen, r, cols));
    }
    while out.last().is_some_and(|r| r.is_empty()) {
        out.pop();
    }
    out
}

/// Platform default shell when the caller didn't specify one.
#[cfg(windows)]
fn default_shell() -> Option<String> {
    std::env::var("COMSPEC")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| Some("powershell.exe".to_string()))
}

/// Platform default shell when the caller didn't specify one.
#[cfg(unix)]
fn default_shell() -> Option<String> {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| Some("sh".to_string()))
}

#[cfg(not(any(windows, unix)))]
fn default_shell() -> Option<String> {
    None
}

/// Resolve a vt100 color to packed 0xRRGGBB; `Default` maps to `None`.
fn color_u32(c: vt100::Color) -> Option<u32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(palette256(i)),
        vt100::Color::Rgb(r, g, b) => Some(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
    }
}

/// xterm 256-color palette entry for an index color.
fn palette256(i: u8) -> u32 {
    const ANSI16: [u32; 16] = [
        0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
        0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ];
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let v = i as u32 - 16;
            let comp = |n: u32| if n == 0 { 0 } else { 55 + 40 * n };
            (comp(v / 36) << 16) | (comp((v / 6) % 6) << 8) | comp(v % 6)
        }
        _ => {
            let g = 8 + (i as u32 - 232) * 10;
            (g << 16) | (g << 8) | g
        }
    }
}

/// Xterm-style modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn mod_param(ctrl: bool, alt: bool, shift: bool) -> u8 {
    1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8
}

/// Build a CSI final-byte sequence honoring modifiers.
/// No modifiers → `\x1b[<final>`; with modifiers → `\x1b[1;<m><final>`.
fn csi_key(final_byte: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    if ctrl || alt || shift {
        format!(
            "\x1b[1;{}{}",
            mod_param(ctrl, alt, shift),
            final_byte as char
        )
        .into_bytes()
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

/// Build a `...~` CSI sequence honoring modifiers.
/// No modifiers → `\x1b[<n>~`; with modifiers → `\x1b[<n>;<m>~`.
fn csi_tilde(n: u8, ctrl: bool, alt: bool, shift: bool) -> Vec<u8> {
    if ctrl || alt || shift {
        format!("\x1b[{};{}~", n, mod_param(ctrl, alt, shift)).into_bytes()
    } else {
        format!("\x1b[{}~", n).into_bytes()
    }
}

/// Map a UI key description to terminal input bytes.
///
/// `key` is either a named key (case-insensitive) — "enter", "backspace",
/// "tab", "escape", "space", "up"/"down"/"right"/"left", "home", "end",
/// "pageup", "pagedown", "insert", "delete", "f1".."f12" — or a single
/// character. `ctrl`, `alt`, `shift` are modifier states.
///
/// Conventions: enter→`\r`, backspace→`\x7f`, arrows/home/end use xterm
/// modifier encoding when modified, ctrl+letter → C0 control byte
/// (`@`..`_` and `a`..`z` map via `& 0x1f`), alt prefixes `\x1b`, shift
/// uppercases plain characters, shift+tab sends backtab `\x1b[Z`.
/// Returns `None` for keys with no terminal encoding.
pub fn key_to_bytes(key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<Vec<u8>> {
    // Prefix ESC for raw-byte keys when alt is held (alt+enter etc).
    let alt_prefix = |mut v: Vec<u8>| -> Vec<u8> {
        if alt {
            v.insert(0, 0x1b);
        }
        v
    };

    let named = |s: &str| -> Option<Vec<u8>> {
        match s {
            "enter" | "return" => Some(alt_prefix(b"\r".to_vec())),
            "backspace" => Some(alt_prefix(b"\x7f".to_vec())),
            "tab" if shift => Some(alt_prefix(b"\x1b[Z".to_vec())),
            "tab" => Some(alt_prefix(b"\t".to_vec())),
            "escape" | "esc" => Some(b"\x1b".to_vec()),
            "space" if ctrl => Some(vec![0x00]),
            "space" => Some(alt_prefix(b" ".to_vec())),
            "up" => Some(csi_key(b'A', ctrl, alt, shift)),
            "down" => Some(csi_key(b'B', ctrl, alt, shift)),
            "right" => Some(csi_key(b'C', ctrl, alt, shift)),
            "left" => Some(csi_key(b'D', ctrl, alt, shift)),
            "home" => Some(csi_key(b'H', ctrl, alt, shift)),
            "end" => Some(csi_key(b'F', ctrl, alt, shift)),
            "pageup" => Some(csi_tilde(5, ctrl, alt, shift)),
            "pagedown" => Some(csi_tilde(6, ctrl, alt, shift)),
            "insert" => Some(csi_tilde(2, ctrl, alt, shift)),
            "delete" => Some(csi_tilde(3, ctrl, alt, shift)),
            "f1" => Some(alt_prefix(b"\x1bOP".to_vec())),
            "f2" => Some(alt_prefix(b"\x1bOQ".to_vec())),
            "f3" => Some(alt_prefix(b"\x1bOR".to_vec())),
            "f4" => Some(alt_prefix(b"\x1bOS".to_vec())),
            "f5" => Some(csi_tilde(15, ctrl, alt, shift)),
            "f6" => Some(csi_tilde(17, ctrl, alt, shift)),
            "f7" => Some(csi_tilde(18, ctrl, alt, shift)),
            "f8" => Some(csi_tilde(19, ctrl, alt, shift)),
            "f9" => Some(csi_tilde(20, ctrl, alt, shift)),
            "f10" => Some(csi_tilde(21, ctrl, alt, shift)),
            "f11" => Some(csi_tilde(23, ctrl, alt, shift)),
            "f12" => Some(csi_tilde(24, ctrl, alt, shift)),
            _ => None,
        }
    };

    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        // Single-character key.
        if ctrl {
            let lc = c.to_ascii_lowercase();
            if lc.is_ascii_lowercase() || ('@'..='_').contains(&lc) {
                let mut v = vec![(lc as u8) & 0x1f];
                if alt {
                    v.insert(0, 0x1b);
                }
                return Some(v);
            }
            return None;
        }
        let c = if shift { c.to_ascii_uppercase() } else { c };
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        let mut v = s.as_bytes().to_vec();
        if alt {
            v.insert(0, 0x1b);
        }
        return Some(v);
    }

    named(&key.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_smoke() {
        let cwd = std::env::temp_dir();
        let p = Pty::spawn(&cwd, None, 80, 24).expect("spawn");
        for _ in 0..40 {
            if !p.text().is_empty() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(250));
        }
        p.write(b"echo MARKER_XYZZY\r");
        let mut found = false;
        for _ in 0..40 {
            let t = p.text();
            if t.iter().any(|l| l.contains("MARKER_XYZZY")) {
                found = true;
                break;
            }
            thread::sleep(std::time::Duration::from_millis(250));
        }
        assert!(found, "marker never appeared; screen={:?}", p.text());
    }

    /// Scrollback rows are rendered ahead of the live screen, oldest
    /// first, and the view offset is restored to the bottom.
    #[test]
    fn render_rows_includes_scrollback() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n");
        let rows = render_rows(parser.screen_mut());
        let text: Vec<String> = rows
            .iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();
        assert_eq!(text, vec!["1", "2", "3", "4", "5", "6"]);
        assert_eq!(parser.screen().scrollback(), 0);
    }

    /// A one-shot command should produce output and exit (exercises the
    /// waiter thread). hostname.exe is a stock Windows console binary.
    #[test]
    #[cfg(windows)]
    fn pty_oneshot() {
        let cwd = std::env::temp_dir();
        let p = Pty::spawn(&cwd, Some("hostname.exe"), 80, 24).expect("spawn");
        let mut saw_output = false;
        for _ in 0..40 {
            if !p.text().is_empty() {
                saw_output = true;
            }
            if saw_output && p.is_exited() {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(250));
        }
        assert!(
            saw_output && p.is_exited(),
            "one-shot command produced no output or never exited; screen={:?}",
            p.text()
        );
    }
}
