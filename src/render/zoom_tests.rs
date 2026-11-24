use super::{clamped_offset, dest_rect, zoom_offset};

/// The image pixel a client point covers, the inverse of what `dest_rect` draws.
fn under(
    image: (u32, u32),
    screen: (u32, u32),
    scale: f64,
    offset: (i32, i32),
    point: (i32, i32),
) -> (f64, f64) {
    let (x, y, ..) = dest_rect(image, screen, scale, offset);
    ((point.0 - x) as f64 / scale, (point.1 - y) as f64 / scale)
}

#[test]
fn the_image_point_under_the_cursor_stays_put() {
    let (image, screen) = ((100, 100), (400, 400));
    let anchor = (180, 180);
    // A 100x100 picture at 100% sits at (150, 150); the cursor is 30 pixels into it.
    assert_eq!(under(image, screen, 1.0, (0, 0), anchor), (30.0, 30.0));

    let zoomed = zoom_offset(image, screen, 1.0, 2.0, (0, 0), anchor);
    assert_eq!(zoomed, (20, 20));
    assert_eq!(under(image, screen, 2.0, zoomed, anchor), (30.0, 30.0));
}

#[test]
fn zooming_back_out_returns_the_picture_to_the_middle() {
    let (image, screen) = ((100, 100), (400, 400));
    let anchor = (180, 180);
    let zoomed = zoom_offset(image, screen, 1.0, 2.0, (0, 0), anchor);
    assert_eq!(zoom_offset(image, screen, 2.0, 1.0, zoomed, anchor), (0, 0));
}

#[test]
fn zooming_in_moves_the_picture_away_from_the_anchor_side() {
    let (image, screen) = ((100, 100), (400, 400));
    // A cursor right of centre pulls the growing picture to the left, and below centre up.
    let left = zoom_offset(image, screen, 1.0, 2.0, (0, 0), (250, 250));
    assert_eq!(left, (-50, -50));
    let right = zoom_offset(image, screen, 1.0, 2.0, (0, 0), (150, 150));
    assert_eq!(right, (50, 50));
}

#[test]
fn the_anchor_is_given_up_past_the_pan_limits() {
    let (image, screen) = ((100, 100), (400, 400));
    let anchor = (390, 200);
    // Keeping an off-image cursor point fixed would push the picture out of the canvas, so the
    // x anchor is clamped to the centred edge; y is still exact.
    let offset = zoom_offset(image, screen, 1.0, 2.0, (0, 0), anchor);
    assert_eq!(offset, (-100, 0));
    assert_eq!(dest_rect(image, screen, 2.0, offset).0, 0);
    assert_eq!(under(image, screen, 2.0, offset, anchor).1, 50.0);
}

#[test]
fn an_unknown_image_or_canvas_keeps_the_offset() {
    assert_eq!(
        zoom_offset((0, 0), (400, 400), 0.0, 2.0, (7, -3), (10, 10)),
        (7, -3)
    );
    assert_eq!(
        zoom_offset((100, 100), (0, 0), 0.0, 2.0, (7, -3), (10, 10)),
        (7, -3)
    );
}

#[test]
fn a_zoomed_out_picture_stops_at_the_canvas_edge() {
    // 50x50 fitted into 400x400 fills it at factor 8; at 100% it could follow the cursor to
    // the bottom-right corner, and stops there because the picture may not leave the canvas.
    let (image, screen) = ((50, 50), (400, 400));
    let offset = zoom_offset(image, screen, 0.0, 1.0, (0, 0), (400, 400));
    assert_eq!(offset, (175, 175));
    let (x, y, w, h) = dest_rect(image, screen, 1.0, offset);
    assert_eq!((x + w, y + h), (400, 400));
}

#[test]
fn an_in_bounds_pan_is_never_reset_by_the_clamp() {
    // A 100x100 picture at 200% spans (-100..300) and may pan ±100 around the centre.
    let (image, screen) = ((100, 100), (400, 400));
    assert_eq!(clamped_offset(image, screen, 2.0, (-30, 45)), (-30, 45));
    assert_eq!(clamped_offset(image, screen, 2.0, (0, 0)), (0, 0));
}

#[test]
fn a_stale_pan_is_clamped_when_the_pane_shrinks() {
    // The same picture at 200% in a half-width pane cannot pan at all any more.
    let image = (100, 100);
    let clamped = clamped_offset(image, (200, 400), 2.0, (100, -100));
    assert_eq!(clamped, (0, clamped.1));
    let (x, .., w) = dest_rect(image, (200, 400), 2.0, clamped);
    assert_eq!(x, 0, "the picture still meets the pane");
    assert!(w >= 0);
}

#[test]
fn a_clamped_pan_keeps_the_picture_meeting_the_canvas() {
    let (image, screen) = ((600, 300), (400, 200));
    for scale in [0.0, 0.5, 1.5, 4.0] {
        let clamped = clamped_offset(image, screen, scale, (999_999, -999_999));
        let (x, y, w, h) = dest_rect(image, screen, scale, clamped);
        assert!(x < 400 && x + w > 0, "x axis at scale {scale}");
        assert!(y < 200 && y + h > 0, "y axis at scale {scale}");
    }
}

#[test]
fn an_unknown_size_or_canvas_keeps_the_offset_when_clamping() {
    assert_eq!(clamped_offset((0, 100), (400, 400), 2.0, (7, -3)), (7, -3));
    assert_eq!(clamped_offset((100, 100), (0, 400), 2.0, (7, -3)), (7, -3));
}
