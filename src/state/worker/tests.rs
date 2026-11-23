//! Deterministic tests for the background lanes: real files, real decodes, tiny fixtures.

use super::*;
use crate::photo;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// The next finished event, waiting for it; on a timeout the assert fires, so a lost
/// result is never mistaken for a passing test.
fn next_event(worker: &Worker) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(event) = worker.next_event() {
            return event;
        }
        assert!(Instant::now() < deadline, "no event arrived within 10s");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A tiny still PNG.
fn still_png(path: &Path) {
    image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 8, 7, 255]))
        .save(path)
        .unwrap();
}

/// A two-frame animated GIF: the smallest file the animation path streams.
fn animated_gif(path: &Path) {
    let mut encoder = image::codecs::gif::GifEncoder::new(fs::File::create(path).unwrap());
    for colour in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        let buffer = image::RgbaImage::from_pixel(2, 2, image::Rgba(colour));
        let frame =
            image::Frame::from_parts(buffer, 0, 0, image::Delay::from_numer_denom_ms(20, 1));
        encoder.encode_frame(frame).unwrap();
    }
}

#[test]
fn a_still_arrives_opened_with_its_generation_and_file_size() {
    let path = crate::test_path("worker-still.png");
    still_png(&path);
    let worker = Worker::new(&photo::image_formats());
    let generation = worker.open(path.clone()).unwrap();

    match next_event(&worker) {
        Event::Opened {
            generation: live,
            identity,
            outcome: OpenedOutcome::Still { .. },
        } => {
            assert_eq!(
                live, generation,
                "the event carries its request's generation"
            );
            assert_eq!(
                identity.map(|(len, _)| len),
                fs::metadata(&path).ok().map(|meta| meta.len()),
                "the identity is the file's size sampled before the decode"
            );
        }
        _ => panic!("a still did not come back as its Opened event"),
    }
    drop(worker);
    fs::remove_file(&path).ok();
}

#[test]
fn an_animation_streams_frames_of_only_the_live_generation() {
    let gif = crate::test_path("worker-anim.gif");
    let png = crate::test_path("worker-anim-still.png");
    animated_gif(&gif);
    still_png(&png);
    let worker = Worker::new(&photo::image_formats());

    let animation = worker.open(gif.clone()).unwrap();
    match next_event(&worker) {
        Event::Opened {
            generation,
            outcome: OpenedOutcome::AnimatedFirst { .. },
            ..
        } => assert_eq!(generation, animation),
        _ => panic!("the animation did not come back as its Opened event"),
    }

    // A second selection supersedes the first: a frame request of the old generation
    // must stream nothing, and nothing of it can overwrite the newer selection.
    let still = worker.open(png.clone()).unwrap();
    worker.next_frame(animation).unwrap();
    match next_event(&worker) {
        Event::Opened {
            generation,
            outcome: OpenedOutcome::Still { .. },
            ..
        } => assert_eq!(
            generation, still,
            "the newer selection's event arrives last"
        ),
        _ => panic!("the still did not come back as its Opened event"),
    }
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        worker.next_event().is_none(),
        "a stale generation's frame request produced an event"
    );

    // Frames, the end of the stream and a rewind all stay with `animation`, once it is
    // the live selection again.
    let again = worker.open(gif.clone()).unwrap();
    match next_event(&worker) {
        Event::Opened {
            generation,
            outcome: OpenedOutcome::AnimatedFirst { .. },
            ..
        } => assert_eq!(generation, again),
        _ => panic!("the reopened animation did not come back opened"),
    }
    worker.next_frame(again).unwrap();
    match next_event(&worker) {
        Event::Frame { generation, frame } => {
            assert_eq!(generation, again);
            assert_eq!(frame.delay_ms, 20, "the frame keeps its delay");
        }
        _ => panic!("the second frame did not arrive as a Frame event"),
    }
    worker.next_frame(again).unwrap();
    match next_event(&worker) {
        Event::FrameEnded { generation } => assert_eq!(generation, again),
        _ => panic!("the animation end did not arrive as FrameEnded"),
    }
    worker.rewind(again).unwrap();
    match next_event(&worker) {
        Event::Frame { generation, .. } => assert_eq!(generation, again),
        _ => panic!("the rewind did not arrive as the first Frame again"),
    }
    drop(worker);
    fs::remove_file(&gif).ok();
    fs::remove_file(&png).ok();
}

#[test]
fn preparation_results_carry_their_tags() {
    let dir = crate::test_path("worker-folder");
    fs::create_dir_all(&dir).unwrap();
    still_png(&dir.join("a.png"));
    still_png(&dir.join("b.png"));

    let worker = Worker::new(&photo::image_formats());
    worker
        .append(
            7,
            Source::Folder {
                path: dir.clone(),
                include_hidden: false,
                recurse: false,
            },
        )
        .unwrap();
    match next_event(&worker) {
        Event::Appended { tag, rows } => {
            assert_eq!(tag, 7);
            assert_eq!(rows.files, vec![dir.join("a.png"), dir.join("b.png")]);
        }
        _ => panic!("a folder did not come back as an Appended event"),
    }

    let list = crate::test_path("worker-list.rgpl");
    fs::write(&list, format!("{}\n", dir.join("a.png").display())).unwrap();
    worker
        .replace(8, Source::PlaylistFile { path: list.clone() })
        .unwrap();
    match next_event(&worker) {
        Event::Replaced { tag, rows } => {
            assert_eq!(tag, 8);
            assert_eq!(rows.files, vec![dir.join("a.png")]);
        }
        _ => panic!("a playlist did not come back as a Replaced event"),
    }

    worker
        .append(
            9,
            Source::PlaylistFile {
                path: crate::test_path("worker-missing.rgpl"),
            },
        )
        .unwrap();
    match next_event(&worker) {
        Event::PrepFailed { tag, error } => {
            assert_eq!(tag, 9);
            assert!(!error.is_empty(), "the failure says what went wrong");
        }
        _ => panic!("an unreadable playlist did not come back as a failure"),
    }
    drop(worker);
    fs::remove_dir_all(&dir).ok();
    fs::remove_file(&list).ok();
}

#[test]
fn a_dropped_worker_shuts_its_lanes_down() {
    let png = crate::test_path("worker-shutdown.png");
    still_png(&png);
    let worker = Worker::new(&photo::image_formats());
    worker.open(png.clone()).unwrap();
    // The drop must return without waiting for the decode to finish: the join happens
    // on the throw-away thread, not on this one.
    drop(worker);
    fs::remove_file(&png).ok();
}
