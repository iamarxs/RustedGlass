use super::*;

/// A config with nothing in it: every setting keeps its default.
fn fresh_config() -> Config {
    Config::load(crate::test_path("unused-config.json"), &[])
}

#[test]
fn the_tool_bar_starts_shown() {
    // `EnableToolBar` is this app's own key, not one shared with C#, so it is kept out of the
    // list of defaults above.
    assert!(Knobs::from(&fresh_config()).show_toolbar);
}

#[test]
fn defaults_match_the_csharp_defaults() {
    let knobs = Knobs::from(&fresh_config());
    assert_eq!(knobs.zoom_mode, ZoomMode::AutoZoom);
    assert!(knobs.loop_back, "EnableLoopBackNavigation is on in C#");
    assert!(!knobs.auto_sibling_dir);
    assert!(!knobs.random_order);
    assert!(knobs.loop_slideshow);
    assert_eq!(knobs.slideshow_seconds, 5);
    assert_eq!(knobs.zoom_speed, 0.0);
    assert_eq!(knobs.pan_speed, 20);
    assert!(knobs.move_to.as_os_str().is_empty());
}

#[test]
fn a_manual_zoom_overrides_the_zoom_mode() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.zoom_mode = ZoomMode::ScaleToFit;
    state.set_zoom_factor(3.0);
    assert_eq!(state.factor((100, 100)), 3.0);
    assert_eq!(state.active_zoom_mode(), ZoomMode::LockZoom);
    state.clear_manual_zoom();
    assert_eq!(state.active_zoom_mode(), ZoomMode::ScaleToFit);
    assert_eq!(state.factor((100, 100)), 1.0);
}

#[test]
fn selecting_lock_zoom_keeps_the_factor_on_screen() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.zoom_mode = ZoomMode::ScaleToWidth;
    let kept = state.factor((400, 400));
    state.select_zoom_mode(ZoomMode::LockZoom, (400, 400));
    assert_eq!(state.scale, kept);
    state.select_zoom_mode(ZoomMode::AutoZoom, (400, 400));
    assert_eq!(state.scale, 0.0);
}

/// A state holding three images, at the first of them.
fn three_images() -> State {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.photos.files = (0..3)
        .map(|index| PathBuf::from(format!("{index}.png")))
        .collect();
    state
}

#[test]
fn starting_a_slideshow_leaves_shuffle_and_repeat_to_the_user() {
    let mut state = three_images();
    state.knobs.loop_slideshow = false;
    state.start_slideshow();
    assert!(state.slideshow_running);
    assert!(!state.knobs.random_order, "Shuffle stays off");
    assert!(!state.knobs.loop_slideshow, "Repeat stays off");
}

#[test]
fn a_slideshow_without_repeat_ends_at_the_last_image() {
    let mut state = three_images();
    state.knobs.loop_slideshow = false;
    state.knobs.loop_back = false;
    state.start_slideshow();
    assert!(state.step(1));
    assert!(state.step(1));
    assert_eq!(state.photos.index, 2);
    assert!(!state.step(1), "the last image is the end");
    assert_eq!(state.photos.index, 2);
    assert!(!state.slideshow_running);
}

#[test]
fn listed_rows_go_in_any_order_and_a_repeated_row_takes_one_entry() {
    let mut state = three_images();
    let dir = import_test_dir("remove-rows");
    let kept = test_image(&dir, "1.png");
    state.photos.files[1] = kept.clone();
    state.photos.index = 2;
    // The rows come from the list: unsorted, and a row can be in the selection twice.
    state.remove_many(&[0, 2, 0]);
    assert_eq!(state.photos.files, vec![kept.clone()]);
    assert_eq!(state.photos.index, 0, "the first of what is left");
    std::fs::remove_file(kept).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn a_repeating_slideshow_wraps_and_shows_each_image_once_per_pass() {
    let mut state = three_images();
    let dir = import_test_dir("repeat-slideshow");
    let image = test_image(&dir, "slide.png");
    state.photos.files = vec![image.clone(); 3];
    state.set_random_order(true);
    state.start_slideshow();
    assert!(
        !state.photos.order.is_empty(),
        "a shuffled pass has an order"
    );

    // The pass is a permutation of the list: each image is in it exactly once.
    let mut pass = state.photos.order.clone();
    pass.sort_unstable();
    assert_eq!(pass, vec![0, 1, 2], "each image once per pass");

    // Three steps: the rest of the pass and the wrap back to its start.
    for _ in 0..3 {
        assert!(state.step(1));
    }
    assert_eq!(state.photos.index, 0, "Repeat starts the pass again");
    assert!(state.slideshow_running);
    std::fs::remove_file(image).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn turning_the_random_order_off_restores_the_names_and_keeps_the_image() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let dir = import_test_dir("shuffle");
    let a = test_image(&dir, "a.png");
    let b = test_image(&dir, "b.png");
    state.photos.files = vec![b.clone(), a.clone()];
    state.toggle_random_order();
    assert!(state.config.get_bool("EnableRandomDisplayOrder", false));
    let shown = state.photos.current().cloned();
    state.toggle_random_order();
    assert!(state.photos.order.is_empty());
    assert!(shown.is_some());
    assert_eq!(state.photos.current(), shown.as_ref());
    std::fs::remove_file(a).unwrap();
    std::fs::remove_file(b).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

mod imports;

fn test_image(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    path
}

fn import_test_dir(name: &str) -> PathBuf {
    let dir = crate::test_path(&format!("image-import-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    std::path::absolute(dir).unwrap()
}

#[test]
fn a_slideshow_notice_preserves_status_and_reports_the_range() {
    let mut state = three_images();
    state.message = Some("Existing status".into());
    state.knobs.slideshow_seconds = 2;
    state.knobs.slideshow_seconds_to = 7;
    state.start_slideshow();
    assert_eq!(
        state.notice.as_deref(),
        Some("Slideshow started, 2–7 s per image")
    );
    state.pause_slideshow();
    assert!(state.slideshow_paused && !state.slideshow_running);
    state.resume_slideshow();
    assert!(state.slideshow_running && !state.slideshow_paused);
    state.stop_slideshow();
    assert_eq!(state.notice.as_deref(), Some("Slideshow stopped"));
    assert_eq!(state.message.as_deref(), Some("Existing status"));
}

#[test]
fn a_random_slideshow_interval_stays_between_the_two_settings() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    assert_eq!(state.slideshow_delay_ms(), 5000);
    state.knobs.slideshow_seconds = 2;
    state.knobs.slideshow_seconds_to = 2;
    assert_eq!(state.slideshow_delay_ms(), 2000);
    state.knobs.slideshow_seconds_to = 4;
    for _ in 0..20 {
        let delay = state.slideshow_delay_ms();
        assert!((2000..=4000).contains(&delay), "got {delay}");
    }
}

#[test]
fn zooming_takes_over_the_factor_the_mode_gave() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    state.zoom_mode = ZoomMode::ScaleToFit;
    state.zoom_by((200, 200), zoom::zoom_step(0.0, 120));
    assert!(state.scale > 1.0, "the mode factor was taken over");
    let zoomed = state.scale;
    state.zoom_by((200, 200), 0.5);
    assert_eq!(state.scale, zoomed * 0.5);
    assert_eq!(state.active_zoom_mode(), ZoomMode::LockZoom);
}

#[test]
fn removing_the_shown_entry_takes_the_next_one() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let dir = import_test_dir("remove-one");
    let b = test_image(&dir, "b.png");
    state.photos.files = vec![PathBuf::from("a.png"), b.clone()];
    assert!(state.remove_current());
    assert_eq!(state.photos.files, vec![b.clone()]);
    assert_eq!(state.photos.index, 0);
    assert!(state.remove_current(), "the last entry can be removed too");
    assert!(state.photos.is_empty());
    assert!(!state.remove_current(), "nothing is left to remove");
    std::fs::remove_file(b).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn a_file_prompt_cannot_rename_a_different_selection() {
    let mut state = State::new(crate::test_path("unused-config.json"), &[]);
    let first = PathBuf::from("a.png");
    let second = PathBuf::from("b.png");
    state.photos.files = vec![first.clone(), second];
    state.pending = Some((Pending::Rename, first));
    state.photos.index = 1;
    assert!(state
        .apply_pending("renamed")
        .unwrap_err()
        .contains("selection changed"));
}
