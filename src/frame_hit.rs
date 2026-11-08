use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT,
};

pub fn hit(point: POINT, (width, height): (u32, u32)) -> u32 {
    let edge = 6;
    let x = if point.x < edge {
        -1
    } else if point.x >= width as i32 - edge {
        1
    } else {
        0
    };
    let y = if point.y < edge {
        -1
    } else if point.y >= height as i32 - edge {
        1
    } else {
        0
    };
    match (x, y) {
        (-1, -1) => HTTOPLEFT,
        (1, -1) => HTTOPRIGHT,
        (-1, 1) => HTBOTTOMLEFT,
        (1, 1) => HTBOTTOMRIGHT,
        (-1, _) => HTLEFT,
        (1, _) => HTRIGHT,
        (_, -1) => HTTOP,
        (_, 1) => HTBOTTOM,
        _ if point.y < 14 => HTCAPTION,
        _ => HTCLIENT,
    }
}
