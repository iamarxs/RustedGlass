use super::*;
use windows::Win32::UI::Controls::LVM_ENSUREVISIBLE;

#[test]
fn finishing_an_import_keeps_the_browsed_row_and_the_selected_image() {
    let directory = crate::test_path("playlist-import-scroll");
    let extra = directory.join("extra");
    fs::create_dir_all(&extra).unwrap();
    write_image(&extra.join("new.ppm"), 2, 3, 1_000_000_000);
    let mut viewer = state::State::interactive(directory.join("test.json"), &[]);
    viewer.photos.files = (0..256)
        .map(|index| directory.join(format!("{index}.ppm")))
        .collect();
    state::install(viewer);
    let owner = TestWindow(unsafe { create_window() }.unwrap());
    let _ = unsafe { ShowWindow(owner.0, SW_HIDE) };
    background::attach(owner.0);
    input::apply_action(owner.0, tools::Action::ShowPlaylist);
    let list =
        unsafe { FindWindowExW(Some(owner.0), None, w!("SysListView32"), PCWSTR::null()) }.unwrap();
    let top = || unsafe { SendMessageW(list, LVM_GETTOPINDEX, None, None).0 as usize };
    let _ = unsafe { SendMessageW(list, LVM_ENSUREVISIBLE, Some(WPARAM(200)), None) };
    let first_visible = top();
    assert!(first_visible > 0);
    assert_eq!(playlist::focused(), Some(0));

    state::with_state(|viewer| viewer.append_drop(vec![extra]));
    let deadline = Instant::now() + Duration::from_secs(8);
    while state::read_state(|viewer| viewer.importing()) == Some(true) {
        assert!(Instant::now() < deadline, "folder import did not finish");
        background::drain(owner.0);
        assert_eq!(top(), first_visible);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state::read_state(|viewer| viewer.photos.len()), Some(257));
    assert_eq!(playlist::focused(), Some(0));

    drop(owner);
    fs::remove_dir_all(&directory).unwrap();
}
