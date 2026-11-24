//! Deterministic tests for the background lanes: real files, real decodes, tiny fixtures.

use super::*;
use crate::photo;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// The next finished event, waiting for it; on a timeout the assert fires, so a lost
/// result is never mistaken for a passing test.
pub(super) fn next_event(worker: &Worker) -> Event {
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
fn large_playlist_yields_bounded_batches_in_file_order() {
    let directory = crate::test_path("worker-batched-playlist");
    fs::create_dir_all(&directory).unwrap();
    let (a, z) = (directory.join("a.png"), directory.join("z.png"));
    let list = directory.join("many.rgpl");
    still_png(&a);
    still_png(&z);
    fs::write(&list, "z.png\na.png\n".repeat(275)).unwrap();

    let worker = Worker::new(&photo::image_formats());
    worker
        .replace(1, Source::PlaylistFile { path: list.clone() })
        .unwrap();
    let mut received = Vec::new();
    let mut batches = 0;
    loop {
        match next_event(&worker) {
            Event::ImportRows { tag: 1, files } => {
                assert!(files.len() <= 256);
                received.extend(files);
                batches += 1;
            }
            Event::Replaced { tag: 1, rows } => {
                assert!(rows.files.len() <= 256);
                received.extend(rows.files);
                break;
            }
            _ => panic!("unexpected import event"),
        }
    }
    assert_eq!(batches, 2);
    let expected: Vec<_> = [z.clone(), a.clone()]
        .into_iter()
        .cycle()
        .take(550)
        .collect();
    assert_eq!(received, expected);
    for file in [a, z, list] {
        fs::remove_file(file).unwrap();
    }
    fs::remove_dir(directory).unwrap();
}

#[test]
fn a_folder_streams_every_row_then_its_natural_order() {
    let directory = crate::test_path("worker-streamed-folder");
    fs::create_dir_all(&directory).unwrap();
    let expected: Vec<_> = (1..=267)
        .map(|i| directory.join(format!("photo{i}.png")))
        .collect();
    for path in expected.iter().rev() {
        fs::write(path, b"test").unwrap();
    }
    let worker = Worker::new(&photo::image_formats());
    worker
        .replace(
            1,
            Source::Folder {
                path: directory.clone(),
                include_hidden: false,
                recurse: false,
                seed: None,
            },
        )
        .unwrap();
    let Event::FolderRows { tag: 1, rows, .. } = next_event(&worker) else {
        panic!("folder did not report its first discovery");
    };
    assert_eq!(rows.len(), 1);
    let mut discovered = rows;
    loop {
        match next_event(&worker) {
            Event::FolderRows { tag: 1, rows, .. } => {
                assert!(rows.len() <= 256);
                discovered.extend(rows);
            }
            Event::FolderFinished { tag: 1, order, .. } => {
                let sorted: Vec<_> = order
                    .into_iter()
                    .map(|index| &discovered[index].0)
                    .collect();
                assert_eq!(sorted, expected.iter().collect::<Vec<_>>());
                assert!(discovered.iter().all(|(_, facts)| facts.size == 4));
                break;
            }
            _ => panic!("unexpected event"),
        }
    }
    for path in expected {
        fs::remove_file(path).unwrap();
    }
    fs::remove_dir(directory).unwrap();
}

#[test]
fn declining_network_rows_keeps_local_rows_and_a_new_open_cancels_the_prompt() {
    let directory = crate::test_path("worker-remote-consent");
    fs::create_dir_all(&directory).unwrap();
    let local = directory.join("local.png");
    let list = directory.join("remote.rgpl");
    still_png(&local);
    fs::write(&list, "local.png\n\\\\network.invalid\\photos\\one.png\n").unwrap();

    let worker = Worker::new(&photo::image_formats());
    let source = || Source::PlaylistFile { path: list.clone() };
    worker.replace(1, source()).unwrap();
    let Event::RemoteImport { tag: 1, respond } = next_event(&worker) else {
        panic!("remote entry was not gated");
    };
    respond.send(false).unwrap();
    let Event::Replaced { tag: 1, rows } = next_event(&worker) else {
        panic!("local rows did not finish");
    };
    assert_eq!(rows.files, [local.clone()]);
    assert_eq!(rows.remote_skipped, 1);

    worker.replace(2, source()).unwrap();
    let Event::RemoteImport { tag: 2, respond } = next_event(&worker) else {
        panic!("second import did not request consent");
    };
    worker
        .replace(
            3,
            Source::Folder {
                path: directory.clone(),
                include_hidden: false,
                recurse: false,
                seed: None,
            },
        )
        .unwrap();
    assert!(matches!(
        next_event(&worker),
        Event::FolderRows { tag: 3, replacing: true, rows } if rows.len() == 1 && rows[0].0 == local
    ));
    assert!(matches!(
        next_event(&worker),
        Event::FolderFinished { tag: 3, order, .. } if order == [0]
    ));
    drop(respond);
    fs::remove_file(local).unwrap();
    fs::remove_file(list).unwrap();
    fs::remove_dir(directory).unwrap();
}

#[test]
fn a_still_arrives_opened_with_its_generation_and_file_size() {
    let path = crate::test_path("worker-still.png");
    still_png(&path);
    let worker = Worker::new(&photo::image_formats());
    let generation = worker.open(path.clone(), None).unwrap();

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

    let animation = worker.open(gif.clone(), None).unwrap();
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
    let still = worker.open(png.clone(), None).unwrap();
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
    let again = worker.open(gif.clone(), None).unwrap();
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
                seed: None,
            },
        )
        .unwrap();
    let mut paths = Vec::new();
    loop {
        match next_event(&worker) {
            Event::FolderRows {
                tag: 7,
                replacing: false,
                rows,
            } => {
                assert_eq!(rows.len(), 1);
                for (path, facts) in rows {
                    assert!(facts.size > 0 && facts.modified > 0);
                    paths.push(path);
                }
            }
            Event::FolderFinished { tag: 7, order, .. } => {
                let ordered: Vec<_> = order.iter().map(|&row| paths[row].clone()).collect();
                assert_eq!(ordered, [dir.join("a.png"), dir.join("b.png")]);
                break;
            }
            _ => panic!("unexpected folder event"),
        }
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
fn a_mixed_drop_keeps_valid_rows_and_the_first_playlist_read_error() {
    let directory = crate::test_path("worker-mixed-drop");
    fs::create_dir_all(&directory).unwrap();
    let (a, z) = (directory.join("a.png"), directory.join("z.png"));
    still_png(&a);
    still_png(&z);
    let valid = directory.join("valid.rgpl");
    fs::write(&valid, "z.png\na.png\nz.png\n").unwrap();
    let broken = directory.join("broken.rgpl");
    fs::write(&broken, b"\xff\xfe\n").unwrap();

    let rows = super::prep::batch(
        &[broken, valid.clone()],
        false,
        false,
        &photo::image_formats(),
    );
    assert_eq!(rows.files, [z.clone(), a.clone(), z.clone()]);
    let error = rows.read_error.unwrap();
    assert!(error.contains("not valid UTF-8"), "{error}");

    for file in [a, z, valid, directory.join("broken.rgpl")] {
        fs::remove_file(file).unwrap();
    }
    fs::remove_dir(directory).unwrap();
}

#[test]
fn a_superseded_large_playlist_stops_without_a_stale_replaced_event() {
    let directory = crate::test_path("worker-superseded-playlist");
    fs::create_dir_all(&directory).unwrap();
    let a = directory.join("a.png");
    still_png(&a);
    let large = directory.join("large.rgpl");
    let text = "a.png\n".repeat(100_000);
    fs::write(&large, text).unwrap();
    let small = directory.join("small.rgpl");
    fs::write(&small, "a.png\n").unwrap();

    let worker = Worker::new(&photo::image_formats());
    worker
        .replace(
            1,
            Source::PlaylistFile {
                path: large.clone(),
            },
        )
        .unwrap();
    loop {
        match next_event(&worker) {
            Event::ImportRows { tag: 1, .. } => break,
            _ => panic!("the large playlist did not stream a batch"),
        }
    }
    worker
        .replace(
            2,
            Source::PlaylistFile {
                path: small.clone(),
            },
        )
        .unwrap();
    loop {
        match next_event(&worker) {
            Event::Replaced { tag: 2, rows } => {
                assert_eq!(rows.files, [a.clone()], "the new open's rows land");
                break;
            }
            Event::Replaced { tag: 1, .. } => panic!("the superseded import finished"),
            _ => {}
        }
    }
    std::thread::sleep(Duration::from_millis(50));
    while let Some(event) = worker.next_event() {
        assert!(
            !matches!(event, Event::Replaced { tag: 1, .. }),
            "a stale replacement arrived after the newer open"
        );
    }

    drop(worker);
    for file in [a, large, small] {
        fs::remove_file(file).unwrap();
    }
    fs::remove_dir(directory).unwrap();
}

#[test]
fn a_dropped_worker_shuts_its_lanes_down() {
    let png = crate::test_path("worker-shutdown.png");
    still_png(&png);
    let worker = Worker::new(&photo::image_formats());
    worker.open(png.clone(), None).unwrap();
    // The drop must return without waiting for the decode to finish: the join happens
    // on the throw-away thread, not on this one.
    drop(worker);
    fs::remove_file(&png).ok();
}
