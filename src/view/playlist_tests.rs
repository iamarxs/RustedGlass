use super::*;
use std::fs::{self, File, FileTimes};
use std::path::Path;
use std::time::{Duration, Instant, UNIX_EPOCH};
use windows::core::{w, PWSTR};
use windows::Win32::UI::Controls::{
    HDM_GETITEMCOUNT, LVCF_TEXT, LVCOLUMNW, LVIF_TEXT, LVM_GETCOLUMNW, LVM_GETCOLUMNWIDTH,
    LVM_GETEXTENDEDLISTVIEWSTYLE, LVM_GETHEADER, LVM_GETTOPINDEX, LVM_SETCOLUMNWIDTH,
    LVN_COLUMNCLICK, LVS_EX_DOUBLEBUFFER, NMLVDISPINFOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_NEXT, VK_PRIOR};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, FindWindowExW, SendMessageW, SW_HIDE,
};

mod header;
mod import_scroll;

struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

fn write_image(path: &Path, width: usize, height: usize, modified: u64) {
    let mut data = format!("P6\n{width} {height}\n255\n").into_bytes();
    data.resize(data.len() + 3 * width * height, 0);
    fs::write(path, data).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(modified)))
        .unwrap();
}

fn notify(owner: HWND, data: &mut impl Sized) {
    let _ = unsafe {
        SendMessageW(
            owner,
            WM_NOTIFY,
            None,
            Some(LPARAM(data as *mut _ as isize)),
        )
    };
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
    notify(owner, &mut request);
    String::from_utf16_lossy(&text[..text.iter().position(|&unit| unit == 0).unwrap()])
}

fn headers(list: HWND) -> Vec<String> {
    let header = HWND(unsafe { SendMessageW(list, LVM_GETHEADER, None, None) }.0 as *mut _);
    let count = unsafe { SendMessageW(header, HDM_GETITEMCOUNT, None, None) }.0;
    (0..count)
        .map(|index| {
            let mut label = [0; 32];
            let mut column = LVCOLUMNW {
                mask: LVCF_TEXT,
                pszText: PWSTR(label.as_mut_ptr()),
                cchTextMax: label.len() as i32,
                ..Default::default()
            };
            assert_ne!(
                unsafe {
                    SendMessageW(
                        list,
                        LVM_GETCOLUMNW,
                        Some(WPARAM(index as usize)),
                        Some(LPARAM(&mut column as *mut _ as isize)),
                    )
                }
                .0,
                0
            );
            String::from_utf16_lossy(&label[..label.iter().position(|&unit| unit == 0).unwrap()])
        })
        .collect()
}

fn click_header(owner: HWND, list: HWND, column: i32) {
    let mut event = NMLISTVIEW::default();
    event.hdr.hwndFrom = list;
    event.hdr.code = LVN_COLUMNCLICK;
    event.iItem = -1;
    event.iSubItem = column;
    notify(owner, &mut event);
    background::drain(owner);
}

#[test]
fn page_keys_move_the_preview_and_scroll_from_either_window() {
    let directory = crate::test_path("playlist-page-keys");
    fs::create_dir_all(&directory).unwrap();
    let first = directory.join("first.ppm");
    let second = directory.join("second.ppm");
    let config = directory.join("test.json");
    write_image(&first, 2, 3, 1_000_000_000);
    write_image(&second, 7, 3, 1_000_000_000);
    let mut viewer = state::State::new(config.clone(), &[]);
    viewer.photos.files = vec![second.clone(); 256];
    viewer.photos.files[0] = first.clone();
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    let key = |window, key: u16| unsafe {
        SendMessageW(window, WM_KEYDOWN, Some(WPARAM(key.into())), None);
    };
    key(owner.0, VK_NEXT.0);
    assert_eq!(state::read_state(|viewer| viewer.photos.index), Some(0));
    input::apply_action(owner.0, tools::Action::ShowPlaylist);
    let list =
        unsafe { FindWindowExW(Some(owner.0), None, w!("SysListView32"), PCWSTR::null()) }.unwrap();
    let page = playlist::rows_per_page();
    assert!(page > 0 && page * 2 < 256);
    let styles = unsafe { SendMessageW(list, LVM_GETEXTENDEDLISTVIEWSTYLE, None, None) }.0;
    assert_ne!(styles & LVS_EX_DOUBLEBUFFER as isize, 0);

    key(owner.0, VK_NEXT.0);
    assert_eq!(playlist::focused(), Some(page));
    assert!(unsafe { SendMessageW(list, LVM_GETTOPINDEX, None, None) }.0 > 0);
    let finish_preview = || unsafe {
        SendMessageW(owner.0, WM_TIMER, Some(WPARAM(NAV_TIMER)), None);
    };
    finish_preview();
    assert_eq!(
        state::read_state(|viewer| viewer.frame().unwrap().width),
        Some(7)
    );

    key(list, VK_NEXT.0);
    assert_eq!(playlist::focused(), Some(page * 2));
    key(list, VK_PRIOR.0);
    key(list, VK_PRIOR.0);
    key(list, VK_PRIOR.0);
    assert_eq!(playlist::focused(), Some(0));
    finish_preview();
    assert_eq!(
        state::read_state(|viewer| viewer.frame().unwrap().width),
        Some(2)
    );

    drop(owner);
    fs::remove_file(first).unwrap();
    fs::remove_file(second).unwrap();
    let _ = fs::remove_file(config);
    fs::remove_dir(directory).unwrap();
}

#[test]
fn metadata_is_ready_for_offscreen_rows_without_opening_the_playlist() {
    let directory = crate::test_path("playlist-background-metadata");
    fs::create_dir_all(&directory).unwrap();
    let last = directory.join("last.ppm");
    let config = directory.join("test.json");
    write_image(&last, 9, 4, 1_000_000_000);
    let mut viewer = state::State::interactive(config.clone(), &[]);
    viewer.photos.files = (0..255)
        .map(|row| directory.join(format!("{row}.ppm")))
        .collect();
    viewer.photos.files.push(last.clone());
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    background::attach(owner.0);
    playlist::show(owner.0);
    refresh(owner.0);
    let list =
        unsafe { FindWindowExW(Some(owner.0), None, w!("SysListView32"), PCWSTR::null()) }.unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while cell(owner.0, list, 255, 2) != "9" {
        assert!(
            Instant::now() < deadline,
            "metadata stalled before the last row"
        );
        std::thread::sleep(Duration::from_millis(10));
        background::drain(owner.0);
    }
    assert_eq!(
        state::read_state(|viewer| viewer.playlist_visible),
        Some(false)
    );
    assert_eq!(
        unsafe { SendMessageW(list, LVM_GETTOPINDEX, None, None) }.0,
        0
    );
    drop(owner);
    fs::remove_file(last).unwrap();
    let _ = fs::remove_file(config);
    fs::remove_dir(directory).unwrap();
}

#[test]
fn playlist_columns_sort_and_remove_duplicates_through_native_notifications() {
    let directory = crate::test_path("playlist-columns");
    fs::create_dir_all(&directory).unwrap();
    let large = directory.join("Color wide.ppm");
    let small = directory.join("Pieni kuva ä.ppm");
    let config = directory.join("rgconfig.json");
    write_image(&large, 10, 4, 1_000_000_000);
    write_image(&small, 2, 3, 1_650_000_000);

    let mut viewer = state::State::interactive(config.clone(), &[]);
    viewer.photos.files = vec![large.clone(), small.clone(), large.clone()];
    viewer.photos.index = 1;
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    background::attach(owner.0);
    input::apply_action(owner.0, tools::Action::ShowPlaylist);
    let list =
        unsafe { FindWindowExW(Some(owner.0), None, w!("SysListView32"), PCWSTR::null()) }.unwrap();
    assert_eq!(
        headers(list),
        [
            "Name",
            "Full path",
            "Width",
            "Height",
            "Type",
            "Size",
            "Date"
        ]
    );

    let deadline = Instant::now() + Duration::from_secs(8);
    while cell(owner.0, list, 1, 2) != "2" {
        assert!(
            Instant::now() < deadline,
            "playlist dimensions didn't arrive"
        );
        std::thread::sleep(Duration::from_millis(10));
        background::drain(owner.0);
    }
    assert_eq!(cell(owner.0, list, 1, 0), "Pieni kuva ä.ppm");
    assert_eq!(cell(owner.0, list, 1, 1), small.to_string_lossy());
    assert_eq!(cell(owner.0, list, 1, 3), "3");
    assert!(!cell(owner.0, list, 1, 4).is_empty());
    assert_eq!(cell(owner.0, list, 1, 5), "1 KB");
    assert!(!cell(owner.0, list, 1, 6).is_empty());

    click_header(owner.0, list, 2);
    assert_eq!(cell(owner.0, list, 0, 2), "10");
    click_header(owner.0, list, 2);
    assert_eq!(cell(owner.0, list, 0, 2), "2");
    click_header(owner.0, list, 6);
    assert_eq!(cell(owner.0, list, 0, 2), "2");
    click_header(owner.0, list, 6);
    assert_eq!(cell(owner.0, list, 0, 2), "10");

    let _ = unsafe { SendMessageW(list, LVM_SETCOLUMNWIDTH, Some(WPARAM(1)), Some(LPARAM(197))) };
    input::apply_action(
        owner.0,
        tools::Action::TogglePlaylistColumn(crate::photo::Column::FullPath),
    );
    assert_eq!(
        headers(list),
        ["Name", "Width", "Height", "Type", "Size", "Date"]
    );
    assert_eq!(cell(owner.0, list, 0, 1), "10");
    input::apply_action(
        owner.0,
        tools::Action::TogglePlaylistColumn(crate::photo::Column::FullPath),
    );
    assert_eq!(headers(list)[1], "Full path");
    assert_eq!(
        unsafe { SendMessageW(list, LVM_GETCOLUMNWIDTH, Some(WPARAM(1)), None) }.0,
        197
    );

    input::apply_action(owner.0, tools::Action::RemoveDuplicates);
    let deadline = Instant::now() + Duration::from_secs(3);
    while state::read_state(|viewer| viewer.photos.len()) != Some(2) {
        assert!(Instant::now() < deadline, "duplicate results didn't arrive");
        std::thread::sleep(Duration::from_millis(5));
        background::drain(owner.0);
    }
    state::with_state(|viewer| {
        assert_eq!(viewer.photos.len(), 2);
        assert_eq!(viewer.photos.current(), Some(&small));
    });
    assert!(large.is_file() && small.is_file());

    drop(owner);
    for path in [large, small, config] {
        let _ = fs::remove_file(path);
    }
    fs::remove_dir(directory).unwrap();
}
