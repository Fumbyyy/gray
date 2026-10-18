use super::*;
use gray_core::agent::PickerItem;
use ratatui::backend::TestBackend;

fn item(id: &str, label: &str, description: Option<&str>) -> PickerItem {
    PickerItem {
        id: id.to_string(),
        label: label.to_string(),
        description: description.map(str::to_string),
    }
}

fn items(n: usize) -> Vec<PickerItem> {
    (0..n)
        .map(|i| item(&format!("id{i:02}"), &format!("item{i:02}"), None))
        .collect()
}

fn picker<'a>(items: &'a [PickerItem], sel: usize) -> ListPicker<'a, PickerItem> {
    ListPicker {
        title: "Things",
        items,
        label: Box::new(|i: &PickerItem| i.label.as_str()),
        trailing: Some(Box::new(|i: &PickerItem| {
            i.description
                .as_deref()
                .map(|d| vec![Span::styled(d.to_string(), Style::default())])
                .unwrap_or_default()
        })),
        footer: None,
        on_highlight: None,
        selected: sel,
    }
}

fn screen(
    w: u16,
    h: u16,
    picker: &ListPicker<'_, PickerItem>,
    filtered: &[usize],
    sel: usize,
    query: &str,
) -> Vec<String> {
    let footer_n = picker.footer.as_ref().map(|f| f().len()).unwrap_or(0);
    let list_h = list_rows_for(h, filtered.len(), footer_n);
    let bg = BackgroundSnapshot::default_initial();
    let mut terminal = ratatui::Terminal::new(TestBackend::new(w, h)).expect("test terminal");
    terminal
        .draw(|frame| draw_list_picker(frame, &bg, picker, filtered, sel, query, list_h))
        .expect("draw picker");
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect()
}

#[test]
fn title_counter_and_rows_render() {
    let items = items(3);
    let picker = picker(&items, 0);
    let filtered: Vec<usize> = (0..items.len()).collect();
    let lines = screen(80, 20, &picker, &filtered, 0, "");
    assert!(lines.iter().any(|l| l.contains("Things")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("1/3")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("› item00")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("item02")), "{lines:#?}");
}

#[test]
fn description_renders_as_trailing_text() {
    let items = vec![
        item("a", "alpha", Some("first item")),
        item("b", "beta", None),
    ];
    let picker = picker(&items, 0);
    let filtered: Vec<usize> = (0..items.len()).collect();
    let lines = screen(80, 20, &picker, &filtered, 0, "");
    assert!(lines.iter().any(|l| l.contains("first item")), "{lines:#?}");
}

#[test]
fn list_scrolls_to_keep_the_selection_on_screen() {
    let items = items(12);
    let picker = picker(&items, 11);
    let filtered: Vec<usize> = (0..items.len()).collect();
    let lines = screen(80, 20, &picker, &filtered, 11, "");
    assert!(lines.iter().any(|l| l.contains("item11")), "{lines:#?}");
    assert!(!lines.iter().any(|l| l.contains("item00")), "{lines:#?}");
}

#[test]
fn list_rows_respect_the_footer_rows() {
    assert_eq!(list_rows_for(20, 12, 2), 9);
    assert_eq!(list_rows_for(24, 12, 2), 12);
    assert_eq!(list_rows_for(20, 0, 2), 1);
    assert_eq!(list_rows_for(8, 12, 2), 1);
    assert_eq!(list_rows_for(20, 12, 0), 11);
}

#[test]
fn empty_filter_shows_no_match() {
    let items = items(3);
    let picker = picker(&items, 0);
    let lines = screen(80, 20, &picker, &[], 0, "zzz");
    assert!(lines.iter().any(|l| l.contains("no match")), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("0/0")), "{lines:#?}");
}

#[test]
fn footer_lines_render_below_the_list() {
    let items = items(3);
    let mut picker = picker(&items, 0);
    picker.footer = Some(Box::new(|| vec![Line::from("FOOTLINE")]));
    let filtered: Vec<usize> = (0..items.len()).collect();
    let lines = screen(80, 20, &picker, &filtered, 0, "");
    assert!(lines.iter().any(|l| l.contains("FOOTLINE")), "{lines:#?}");
}

#[test]
fn pick_argv_appends_pick_and_id() {
    assert_eq!(
        pick_argv(&["--verbose".to_string(), "run".to_string()], "abc-1"),
        vec!["--verbose", "run", "--pick", "abc-1"]
    );
    assert_eq!(pick_argv(&[], "x"), vec!["--pick", "x"]);
}

#[test]
fn spec_plain_lists_title_and_items() {
    let spec = PickerSpec {
        title: "Pick one".into(),
        selected_id: Some("b".into()),
        items: vec![item("a", "alpha", Some("first")), item("b", "beta", None)],
    };
    let text = spec_plain(&spec);
    assert!(text.contains("Pick one"), "{text}");
    assert!(text.contains("alpha"), "{text}");
    assert!(text.contains("first"), "{text}");
    assert!(text.contains("beta"), "{text}");
    assert!(!text.contains('\x1b'), "{text}");
}
