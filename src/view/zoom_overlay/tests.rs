use super::{inside, parse_percent, wheel_factor};

#[test]
fn the_wheel_zooms_only_while_the_pointer_is_over_the_picture() {
    let pane = (400, 600);
    assert!(inside(pane, (0, 0)));
    assert!(inside(pane, (399, 599)));
    assert!(!inside(pane, (400, 300)));
    assert!(!inside(pane, (200, 600)));
    assert!(!inside(pane, (-1, 300)));
}

#[test]
fn a_wheel_up_zooms_in_and_a_wheel_down_zooms_out() {
    assert!(wheel_factor(12.0, 120) > 1.0);
    assert!(wheel_factor(12.0, -120) < 1.0);
    assert_eq!(wheel_factor(12.0, 0), 1.0);
}

#[test]
fn exact_zoom_accepts_decimal_percentages_within_the_manual_range() {
    assert_eq!(parse_percent("137.5"), Some(137.5));
    assert_eq!(parse_percent("137,5 %"), Some(137.5));
    assert_eq!(parse_percent("10000"), Some(10_000.0));
    for invalid in ["", "NaN", "inf", "0", "10000.1", "-25", "1,2.3"] {
        assert_eq!(parse_percent(invalid), None);
    }
}
