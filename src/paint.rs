//! Small RAII GDI painting helpers. Every temporary GDI object is restored and deleted.
use crate::pdf::Pixels;
use std::{mem::size_of, ptr::null_mut};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn color(c: u32) -> u32 {
    ((c >> 16) & 255) | (c & 0xff00) | ((c & 255) << 16)
}
pub fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}
pub unsafe fn fill(dc: HDC, r: RECT, c: u32) {
    let b = CreateSolidBrush(color(c));
    FillRect(dc, &r, b);
    DeleteObject(b);
}
pub unsafe fn round(dc: HDC, r: RECT, c: u32, line: u32) {
    let brush = CreateSolidBrush(color(c));
    let pen = CreatePen(PS_SOLID, 1, color(line));
    let oldb = SelectObject(dc, brush);
    let oldp = SelectObject(dc, pen);
    RoundRect(dc, r.left, r.top, r.right, r.bottom, 14, 14);
    SelectObject(dc, oldb);
    SelectObject(dc, oldp);
    DeleteObject(brush);
    DeleteObject(pen);
}
pub unsafe fn font(size: i32, bold: bool) -> HFONT {
    CreateFontW(
        -size,
        0,
        0,
        0,
        if bold { 600 } else { 400 },
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        DEFAULT_PITCH as u32,
        wide("Microsoft YaHei UI").as_ptr(),
    )
}
pub unsafe fn text(dc: HDC, s: &str, r: RECT, size: i32, c: u32, bold: bool, flags: u32) {
    let f = font(size, bold);
    let old = SelectObject(dc, f);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color(c));
    let mut rr = r;
    let t = wide(s);
    DrawTextW(
        dc,
        t.as_ptr(),
        (t.len() - 1) as i32,
        &mut rr,
        flags | DT_NOPREFIX,
    );
    SelectObject(dc, old);
    DeleteObject(f);
}
pub unsafe fn bitmap(dc: HDC, p: &Pixels, r: RECT) {
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = p.width;
    info.bmiHeader.biHeight = -p.height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    SetStretchBltMode(dc, HALFTONE);
    StretchDIBits(
        dc,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        0,
        0,
        p.width,
        p.height,
        p.data.as_ptr().cast(),
        &info,
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}
pub unsafe fn capture(hwnd: HWND, path: &std::path::Path) -> Result<(), String> {
    let mut r = rect(0, 0, 0, 0);
    GetClientRect(hwnd, &mut r);
    let w = r.right;
    let h = r.bottom;
    let dc = GetDC(hwnd);
    let mem = CreateCompatibleDC(dc);
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = w;
    info.bmiHeader.biHeight = -h;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    let mut bits = null_mut();
    let bmp = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bmp.is_null() {
        DeleteDC(mem);
        ReleaseDC(hwnd, dc);
        return Err("Capture allocation failed".into());
    }
    let old = SelectObject(mem, bmp);
    SendMessageW(hwnd, WM_PRINTCLIENT, mem as usize, PRF_CLIENT as isize);
    let mut child = GetWindow(hwnd, GW_CHILD);
    while !child.is_null() {
        if GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_VISIBLE == 0 {
            child = GetWindow(child, GW_HWNDNEXT);
            continue;
        }
        let mut origin = POINT { x: 0, y: 0 };
        MapWindowPoints(child, hwnd, &mut origin, 1);
        let saved = SaveDC(mem);
        SetViewportOrgEx(mem, origin.x, origin.y, null_mut());
        SendMessageW(
            child,
            WM_PRINTCLIENT,
            mem as usize,
            (PRF_CLIENT | PRF_ERASEBKGND) as isize,
        );
        RestoreDC(mem, saved);
        child = GetWindow(child, GW_HWNDNEXT);
    }
    let bytes = std::slice::from_raw_parts(bits.cast::<u8>(), w as usize * h as usize * 4);
    let mut rgba = bytes.to_vec();
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
        p[3] = 255;
    }
    let result = image::save_buffer(path, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| e.to_string());
    SelectObject(mem, old);
    DeleteObject(bmp);
    DeleteDC(mem);
    ReleaseDC(hwnd, dc);
    result
}
