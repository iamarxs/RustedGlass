use super::*;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::BOOL;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Controls::{
    LVFINDINFOW, LVFI_PARTIAL, LVFI_WRAP, LVIF_TEXT, LVM_FINDITEMW, LVM_GETITEMCOUNT, NMLVDISPINFOW,
};
use windows::Win32::UI::Shell::DROPFILES;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, FindWindowExW, SendMessageW, GWL_EXSTYLE, WS_EX_ACCEPTFILES,
};

fn drop_paths(hwnd: HWND, paths: &[&Path]) {
    let names: Vec<u16> = paths
        .iter()
        .flat_map(|path| path.as_os_str().encode_wide().chain([0]))
        .chain([0])
        .collect();
    let header_size = std::mem::size_of::<DROPFILES>();
    unsafe {
        let memory = GlobalAlloc(
            GMEM_MOVEABLE,
            header_size + std::mem::size_of_val(names.as_slice()),
        )
        .unwrap();
        let data = GlobalLock(memory).cast::<u8>();
        assert!(!data.is_null());
        data.cast::<DROPFILES>().write(DROPFILES {
            pFiles: header_size as u32,
            fWide: BOOL(1),
            ..Default::default()
        });
        std::ptr::copy_nonoverlapping(names.as_ptr(), data.add(header_size).cast(), names.len());
        let _ = GlobalUnlock(memory);
        // The receiving window releases the drop handle with DragFinish.
        let _ = SendMessageW(hwnd, WM_DROPFILES, Some(WPARAM(memory.0 as usize)), None);
    }
}

#[test]
fn playlist_drops_append_but_viewer_drops_replace() {
    let dir = crate::test_path("playlist-drop");
    let folder = dir.join("folder");
    std::fs::create_dir_all(&folder).unwrap();
    let existing = dir.join("existing.ppm");
    let dropped = dir.join("dropped.ppm");
    let nested = folder.join("nested.ppm");
    let config = dir.join("rgconfig.json");
    for file in [&existing, &dropped, &nested] {
        std::fs::write(file, b"P6\n1 1\n255\n\xff\x00\x00").unwrap();
    }

    let mut viewer = state::State::new(config.clone(), &[]);
    viewer.open_files(std::slice::from_ref(&existing));
    state::install(viewer);
    let hwnd = unsafe { create_window() }.unwrap();
    input::apply_action(hwnd, tools::Action::ShowPlaylist);
    let list = unsafe {
        FindWindowExW(
            Some(hwnd),
            None,
            windows::core::w!("SysListView32"),
            PCWSTR::null(),
        )
    }
    .unwrap();
    assert_ne!(
        unsafe { GetWindowLongPtrW(list, GWL_EXSTYLE) } as u32 & WS_EX_ACCEPTFILES.0,
        0
    );
    drop_paths(list, &[&dropped, &folder]);
    assert_eq!(
        unsafe { SendMessageW(list, LVM_GETITEMCOUNT, None, None) }.0,
        3
    );
    let mut text = [0u16; 32];
    let mut display = NMLVDISPINFOW::default();
    display.hdr.hwndFrom = list;
    display.hdr.code = LVN_GETDISPINFOW;
    display.item.mask = LVIF_TEXT;
    display.item.iItem = 2;
    display.item.pszText = windows::core::PWSTR(text.as_mut_ptr());
    display.item.cchTextMax = text.len() as i32;
    let query = LVFINDINFOW {
        flags: LVFI_PARTIAL | LVFI_WRAP,
        psz: windows::core::w!("DROPP"),
        ..Default::default()
    };
    unsafe {
        let _ = SendMessageW(
            hwnd,
            WM_NOTIFY,
            None,
            Some(LPARAM(&mut display as *mut _ as isize)),
        );
        assert_eq!(
            SendMessageW(
                list,
                LVM_FINDITEMW,
                Some(WPARAM(usize::MAX)),
                Some(LPARAM(&query as *const _ as isize)),
            )
            .0,
            1
        );
    }
    assert_eq!(&text[..11], w32::wide("nested.ppm"));
    state::with_state(|viewer| {
        assert_eq!(
            viewer.photos.files,
            [existing.clone(), dropped.clone(), nested.clone()]
        );
        assert_eq!(viewer.photos.current(), Some(&existing));
    })
    .unwrap();

    input::apply_action(hwnd, tools::Action::ShowPlaylist);
    drop_paths(hwnd, &[&dropped]);
    assert_eq!(
        state::with_state(|viewer| viewer.photos.files.clone()),
        Some(vec![dropped.clone()])
    );
    unsafe { DestroyWindow(hwnd) }.unwrap();
    for file in [existing, dropped, nested, config] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(folder).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
