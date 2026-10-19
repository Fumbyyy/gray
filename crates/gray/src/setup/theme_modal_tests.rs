use super::*;
use ratatui::backend::TestBackend;

fn row(name: &str) -> Row {
    Row {
        name: name.to_string(),
        theme: theme::GRAY_UI_THEME,
    }
}

fn twelve() -> Vec<Row> {
    (0..12).map(|i| row(&format!("theme{i:02}"))).collect()
}

fn screen(w: u16, h: u16, rows: &[Row], sel: usize) -> Vec<String> {
    let filtered: Vec<usize> = (0..rows.len()).collect();
    let picker = picker(rows, sel, None);
    let list_h = crate::repl::picker::list_rows_for(h, filtered.len(), specimen_lines().len());
    let bg = BackgroundSnapshot::default_initial();
    let mut terminal = ratatui::Terminal::new(TestBackend::new(w, h)).expect("test terminal");
    terminal
        .draw(|frame| {
            crate::repl::picker::draw_list_picker(frame, &bg, &picker, &filtered, sel, "", list_h)
        })
        .expect("draw picker");
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

#[test]
fn specimen_rows_show_on_an_80x20_terminal() {
    let lines = screen(80, 20, &twelve(), 0);
    assert!(lines.iter().any(|l| l.contains("Aa bright")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("✓ success")), "{lines:#?}");
}

#[test]
fn specimen_rows_show_on_an_80x24_terminal() {
    let lines = screen(80, 24, &twelve(), 0);
    assert!(lines.iter().any(|l| l.contains("Aa bright")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("✓ success")), "{lines:#?}");
}

#[test]
fn list_scrolls_to_keep_the_selection_on_screen() {
    let lines = screen(80, 20, &twelve(), 11);
    assert!(lines.iter().any(|l| l.contains("theme11")), "{lines:#?}");
    assert!(!lines.iter().any(|l| l.contains("theme00")), "{lines:#?}");
}

#[test]
fn rows_start_with_the_builtin_theme() {
    let rows = rows();
    assert_eq!(rows[0].name, theme::BUILTIN_THEME_NAME);
}
