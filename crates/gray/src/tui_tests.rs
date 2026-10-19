use super::*;

#[test]
fn ascii_symbol_leaves_plain_text_alone() {
    assert_eq!(ascii_symbol("plain ascii").as_deref(), None);
    // Accents, CJK, and emoji are user text, not decoration — a console
    // font that covers them keeps them.
    assert_eq!(ascii_symbol("héllo 世界 🎉").as_deref(), None);
    // Control sequences (hyperlinks, kitty placeholders) pass through.
    assert_eq!(ascii_symbol("\x1b]8;;http://x\x07→").as_deref(), None);
}

#[test]
fn ascii_symbol_transliterates_decorative_glyphs() {
    assert_eq!(ascii_symbol("⬢").as_deref(), Some("#"));
    assert_eq!(ascii_symbol("⬡").as_deref(), Some("o"));
    assert_eq!(ascii_symbol("❯").as_deref(), Some(">"));
    assert_eq!(ascii_symbol("✓").as_deref(), Some("v"));
    assert_eq!(ascii_symbol("✗").as_deref(), Some("x"));
    assert_eq!(ascii_symbol("⏎").as_deref(), Some("<"));
    assert_eq!(ascii_symbol("●").as_deref(), Some("*"));
    assert_eq!(ascii_symbol("○").as_deref(), Some("o"));
    assert_eq!(ascii_symbol("─").as_deref(), Some("-"));
    assert_eq!(ascii_symbol("└").as_deref(), Some("+"));
    assert_eq!(ascii_symbol("█").as_deref(), Some("#"));
    assert_eq!(ascii_symbol("⚠").as_deref(), Some("!"));
}

#[test]
fn ascii_symbol_transliterates_mixed_symbols() {
    assert_eq!(ascii_symbol("⬢⬡").as_deref(), Some("#o"));
    // Text survives next to decoration.
    assert_eq!(ascii_symbol("→ done 界").as_deref(), Some("> done 界"));
}

#[test]
fn ascii_symbol_maps_braille_by_dot_density() {
    assert_eq!(ascii_symbol("⠀").as_deref(), Some(" "));
    assert_eq!(ascii_symbol("⠁").as_deref(), Some("."));
    assert_eq!(ascii_symbol("⣿").as_deref(), Some("#"));
}
