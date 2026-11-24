//! Repeatable, isolated acceptance checks for the budgeted background drain: real worker
//! threads and real files, deadline-based loops — no fixed sleep decides an outcome.

use super::super::*;
use super::{attach, drain, RESULTS_PER_TURN};
use crate::{playlist, state};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Controls::{LVIF_TEXT, LVN_GETDISPINFOW, NMLVDISPINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, FindWindowExW, PeekMessageW, SendMessageW, ShowWindow,
    HWND_MESSAGE, MSG, PM_REMOVE, SW_HIDE, WM_NOTIFY,
};

struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

fn message_window() -> TestWindow {
    TestWindow(
        unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!(""),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .unwrap(),
    )
}

/// The wake-up messages queued for the window right now.
fn wake_ups_queued(hwnd: HWND) -> usize {
    let mut count = 0;
    let mut message = MSG::default();
    while unsafe {
        PeekMessageW(
            &mut message,
            Some(hwnd),
            state::BACKGROUND_READY,
            state::BACKGROUND_READY,
            PM_REMOVE,
        )
    }
    .as_bool()
    {
        count += 1;
    }
    count
}

fn tiny_png(path: &Path) {
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .save(path)
        .unwrap();
}

fn write_image(path: &Path, width: usize, height: usize) {
    let mut data = format!("P6\n{width} {height}\n255\n").into_bytes();
    data.resize(data.len() + 3 * width * height, 0);
    fs::write(path, data).unwrap();
}

fn event_arrived() -> bool {
    state::with_state(|state| state.worker.as_ref().and_then(|worker| worker.next_event()))
        .flatten()
        .is_some()
}

fn has_events() -> bool {
    state::with_state(|state| {
        state
            .worker
            .as_ref()
            .is_some_and(|worker| worker.has_events())
    })
    .unwrap_or(false)
}

fn queue_metadata(revision: u64, path: &Path) -> bool {
    state::with_state(|state| {
        state
            .worker
            .as_ref()
            .map(|worker| worker.metadata(revision, vec![(path.to_path_buf(), None)], true, false))
            .is_some_and(|result| result.is_ok())
    }) == Some(true)
}

#[test]
fn a_window_navigates_before_a_pending_cache_validation_is_applied() {
    let dir = crate::test_path("pending-cache-navigation");
    fs::create_dir_all(&dir).unwrap();
    let first = dir.join("warm.ppm");
    let second = dir.join("next.ppm");
    write_image(&first, 4, 3);
    write_image(&second, 7, 3);
    let crate::decode::Opened::Still(pixels) = crate::decode::open(&first).unwrap() else {
        panic!("fixture must be a still");
    };
    crate::cache::store(&first, pixels);

    let mut viewer = state::State::interactive(dir.join("config.json"), &[]);
    viewer.photos.files = vec![first, second.clone()];
    viewer.load_current();
    assert!(viewer.load_pending && viewer.frame().is_none());
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    attach(owner.0);

    // Leave the cached reply unconsumed while delivering an ordinary viewer key.
    let _ = unsafe { SendMessageW(owner.0, WM_KEYDOWN, Some(WPARAM(0x27)), None) };
    assert_eq!(
        state::read_state(|viewer| viewer.navigation_pending),
        Some(true)
    );
    let _ = unsafe { SendMessageW(owner.0, WM_TIMER, Some(WPARAM(NAV_TIMER)), None) };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        drain(owner.0);
        let sized = state::read_state(|viewer| viewer.frame().map(|frame| frame.width)).flatten();
        if let Some(width) = sized {
            assert_eq!(
                width, 7,
                "old cached pixels were applied to the new selection"
            );
            break;
        }
        assert!(Instant::now() < deadline, "selected decode did not arrive");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        state::read_state(|viewer| viewer.photos.current().cloned()).flatten(),
        Some(second)
    );
    drop(owner);
    fs::remove_dir_all(dir).unwrap();
}

fn cell(owner: HWND, list: HWND, row: i32, column: i32) -> String {
    let mut text = [0; 1024];
    let mut request = NMLVDISPINFOW::default();
    request.hdr.hwndFrom = list;
    request.hdr.code = LVN_GETDISPINFOW;
    request.item.mask = LVIF_TEXT;
    request.item.iItem = row;
    request.item.iSubItem = column;
    request.item.pszText = PWSTR(text.as_mut_ptr());
    request.item.cchTextMax = text.len() as i32;
    let _ = unsafe {
        SendMessageW(
            owner,
            WM_NOTIFY,
            None,
            Some(LPARAM(&mut request as *mut _ as isize)),
        )
    };
    String::from_utf16_lossy(&text[..text.iter().position(|&unit| unit == 0).unwrap()])
}

#[test]
fn a_burst_of_finished_results_is_one_coalesced_wake_up() {
    let directory = crate::test_path("view-bg-coalesced-wake");
    fs::create_dir_all(&directory).unwrap();
    let config = directory.join("test.json");
    let files: Vec<PathBuf> = (0..12)
        .map(|index| {
            let path = directory.join(format!("f{index}.png"));
            tiny_png(&path);
            path
        })
        .collect();

    let viewer = state::State::interactive(config.clone(), &[]);
    state::install(viewer);
    let owner = message_window();
    attach(owner.0);
    for (index, file) in files.iter().enumerate() {
        assert!(queue_metadata(index as u64 + 1, file));
    }

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = 0;
    loop {
        if event_arrived() {
            seen += 1;
            if seen == files.len() {
                break;
            }
        }
        assert!(Instant::now() < deadline, "only {seen}/12 results arrived");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        wake_ups_queued(owner.0),
        1,
        "a burst of finished results queues a single wake-up"
    );

    drop(owner);
    for file in &files {
        fs::remove_file(file).unwrap();
    }
    let _ = fs::remove_file(config);
    fs::remove_dir(directory).unwrap();
}

#[test]
fn budgeted_drain_turns_finish_the_offscreen_prefetch_within_a_deadline() {
    let directory = crate::test_path("view-bg-budgeted-drain");
    fs::create_dir_all(&directory).unwrap();
    let config = directory.join("test.json");
    let count = 3 * RESULTS_PER_TURN + 12;
    let files: Vec<PathBuf> = (0..count)
        .map(|index| {
            let path = directory.join(format!("{index:03}.ppm"));
            write_image(&path, 7, 3);
            path
        })
        .collect();

    let mut viewer = state::State::interactive(config.clone(), &[]);
    viewer.photos.files = files.clone();
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    attach(owner.0);
    playlist::show(owner.0);
    refresh(owner.0);
    let list =
        unsafe { FindWindowExW(Some(owner.0), None, w!("SysListView32"), PCWSTR::null()) }.unwrap();

    let last = (count - 1) as i32;
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut saw_pending = false;
    loop {
        let pending = has_events();
        saw_pending |= pending;
        if !pending && cell(owner.0, list, last, 2) == "7" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "budgeted drain stalled before the last row (pending={pending})"
        );
        playlist::request_metadata();
        drain(owner.0);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(saw_pending, "the test observed pending background results");
    assert!(
        !has_events(),
        "every queued result was applied across turns"
    );

    drop(owner);
    for file in &files {
        fs::remove_file(file).unwrap();
    }
    let _ = fs::remove_file(config);
    fs::remove_dir(directory).unwrap();
}
