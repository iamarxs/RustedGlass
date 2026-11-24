use super::*;

#[test]
fn a_repeating_slideshow_wraps_and_shows_each_image_once_per_pass() {
    let mut state = three_images();
    let dir = import_test_dir("repeat-slideshow");
    let image = test_image(&dir, "slide.png");
    state.photos.files = vec![image.clone(); 3];
    state.set_random_order(true);
    state.start_slideshow();
    let mut pass = vec![state.photos.index];
    for _ in 0..2 {
        assert!(state.step(1));
        pass.push(state.photos.index);
    }
    let last = state.photos.index;
    assert!(state.step(1));
    assert_ne!(
        state.photos.index, last,
        "a fresh pass avoids an immediate repeat"
    );
    pass.sort_unstable();
    assert_eq!(pass, vec![0, 1, 2], "each row appears once per pass");
    assert!(state.photos.order.is_empty(), "playlist rows stay in place");
    assert!(state.slideshow_running);
    std::fs::remove_file(image).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn switching_random_navigation_keeps_the_image_and_playlist_order() {
    let mut state = three_images();
    state.photos.shuffle(7);
    let order = state.photos.order.clone();
    let selected = state.photos.index;
    state.toggle_random_order();
    assert!(state.config.get_bool("EnableRandomDisplayOrder", false));
    assert_eq!(state.photos.index, selected);
    assert_eq!(state.photos.order, order);
    assert!(state.step(1));
    let selected = state.photos.index;
    state.toggle_random_order();
    assert_eq!(state.photos.index, selected);
    assert_eq!(state.photos.order, order);
    assert!(state.step(1));
    assert_eq!(state.photos.index, (selected + 1) % order.len());
}

#[test]
fn a_shuffled_slideshow_without_looping_ends_after_one_pass() {
    let mut state = three_images();
    state
        .config
        .set("EnableRandomDisplayOrder", Value::Bool(true));
    state.config.set("EnableLoopSlideshow", Value::Bool(false));
    state.reload_knobs();
    state.photos.sort_by_column(
        crate::photo::Column::Name,
        true,
        &std::collections::HashMap::new(),
    );
    assert!(
        state.knobs.random_order,
        "sorting is independent of playback"
    );
    state.start_slideshow();
    let mut pass = vec![state.photos.index];
    while state.step(1) {
        pass.push(state.photos.index);
        assert!(pass.len() <= state.photos.len());
    }
    pass.sort_unstable();
    assert_eq!(pass, [0, 1, 2]);
    assert!(!state.slideshow_running);
}
