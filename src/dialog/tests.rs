use super::controls::parse_slideshow_intervals;
use super::*;

#[test]
fn slideshow_settings_require_an_ordered_pair_of_whole_seconds() {
    assert_eq!(parse_slideshow_intervals(" 5 ", "5"), Ok((5, 5)));
    assert_eq!(parse_slideshow_intervals("5", "12"), Ok((5, 12)));
    for (minimum, maximum) in [("5", "4"), ("5", "5.5"), ("5", ""), ("0", "5")] {
        assert!(parse_slideshow_intervals(minimum, maximum).is_err());
    }
}

#[test]
fn saved_numeric_settings_are_shown_when_the_panel_reopens() {
    let mut config = Config::load(crate::test_path("unused-config.json"), &[]);
    let row = ROWS
        .iter()
        .find(|row| row.key == "SlideshowIntervalTo")
        .unwrap();
    config.set(row.key, Value::from(12.0));
    assert_eq!(value_text(&config, row), "12");
}

#[test]
fn pick_ids_end_before_the_tool_bar_starts() {
    assert!(is_pick(PICK_BASE));
    assert!(!is_pick(PICK_BASE + ROWS.len()));
    assert!(
        !is_pick(crate::bar::TOOL_BASE),
        "a tool bar click must not open the folder picker"
    );
}

#[test]
fn shortcut_button_keeps_its_bottom_right_margin() {
    for (width, height) in [(800, 600), (520, 350), (120, 120)] {
        let (x, y, w, h) = shortcut_button_rect((width, height));
        assert_eq!(x + w + MARGIN, width as i32);
        assert_eq!(y + h + MARGIN, height as i32);
    }
}
