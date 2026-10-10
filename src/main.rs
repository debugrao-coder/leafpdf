#![windows_subsystem = "windows"]
// SAFETY: Win32 handles remain on the UI thread. Callback state uses try_borrow_mut
// to avoid reentrant mutation. GDI objects are scoped and native pointers come only
// from the OS for the corresponding message lifetime.
mod model;
mod paint;
mod pdf;
use model::*;
use paint::*;
use pdf::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{null, null_mut},
    sync::atomic::Ordering,
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::*, Gdi::*},
    Storage::FileSystem::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{Dialogs::*, *},
        HiDpi::*,
        Input::KeyboardAndMouse::*,
        Shell::*,
        WindowsAndMessaging::*,
    },
};
const TOP: f64 = 46.;
const CONTENT_TOP: f64 = 0.;
const THEME: usize = 116;
const OPEN: usize = 101;
const SIDEBAR: usize = 102;
const PREV: usize = 103;
const NEXT: usize = 104;
const PAGE: usize = 105;
const MINUS: usize = 106;
const PLUS: usize = 107;
const FIT: usize = 108;
const ROTATE: usize = 109;
const SINGLE: usize = 110;
const CONTINUOUS: usize = 111;
const SPREAD: usize = 112;
const SEARCH: usize = 113;
const FIND: usize = 114;
const MATCH_NEXT: usize = 115;
thread_local! {static APP:RefCell<Option<App>>=const{RefCell::new(None)};}
struct App {
    hwnd: HWND,
    worker: Worker,
    controls: HashMap<usize, HWND>,
    ui_font: HFONT,
    dpi: f64,
    generation: u64,
    path: Option<PathBuf>,
    sizes: Vec<(f64, f64)>,
    bytes: u64,
    page: usize,
    mode: Mode,
    zoom: Zoom,
    rotation: i32,
    sidebar: bool,
    scroll: f64,
    pan: f64,
    thumb_scroll: f64,
    cache: HashMap<Key, Pixels>,
    order: VecDeque<Key>,
    pending: HashSet<Key>,
    settings: Settings,
    store: PathBuf,
    status: String,
    failed: bool,
    matches: Vec<usize>,
    match_index: usize,
    search_token: u64,
    searching: bool,
    query: String,
    mouse: (i32, i32),
    drag: Option<(i32, i32, f64, f64)>,
    last_save: Instant,
    toolbar_visible: bool,
    toolbar_last_hover: Instant,
    qa: Option<PathBuf>,
    qa_step: usize,
    qa_wait: Instant,
}
impl App {
    unsafe fn new(hwnd: HWND, qa: Option<PathBuf>) -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dll = if exe.with_file_name("pdfium.dll").is_file() {
            exe.with_file_name("pdfium.dll")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/bin/pdfium.dll")
        };
        let store = std::env::var_os("LEAFPDF_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("LeafPDF")
            });
        let settings = std::fs::read(store.join("settings.json"))
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default();
        let dpi = GetDpiForWindow(hwnd) as f64 / 96.;
        let f = font((14. * dpi) as i32, false);
        let mut controls = HashMap::new();
        for (id, label) in [
            (OPEN, "打开 PDF"),
            (THEME, "暗黑"),
            (SIDEBAR, "侧栏"),
            (PREV, "‹"),
            (NEXT, "›"),
            (MINUS, "−"),
            (PLUS, "＋"),
            (FIT, "适合页"),
            (ROTATE, "旋转"),
            (SINGLE, "单页"),
            (CONTINUOUS, "连续"),
            (SPREAD, "双页"),
            (FIND, "查找"),
            (MATCH_NEXT, "下个"),
        ] {
            let h = CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide(label).as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                0,
                0,
                0,
                0,
                hwnd,
                id as _,
                GetModuleHandleW(null()),
                null(),
            );
            SendMessageW(h, WM_SETFONT, f as usize, 0);
            controls.insert(id, h);
        }
        for (id, label) in [(PAGE, "1"), (SEARCH, "")] {
            let h = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide(label).as_ptr(),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | ES_AUTOHSCROLL as u32
                    | if id == PAGE {
                        ES_NUMBER as u32 | ES_CENTER as u32
                    } else {
                        0
                    },
                0,
                0,
                0,
                0,
                hwnd,
                id as _,
                GetModuleHandleW(null()),
                null(),
            );
            SendMessageW(h, WM_SETFONT, f as usize, 0);
            SendMessageW(h, EM_SETLIMITTEXT, if id == PAGE { 6 } else { 180 }, 0);
            controls.insert(id, h);
        }
        if let Some(h) = controls.get(&SEARCH) {
            SendMessageW(
                *h,
                EM_SETCUEBANNER,
                0,
                wide("搜索文档  Ctrl+F").as_ptr() as isize,
            );
        }
        Ok(Self {
            hwnd,
            worker: Worker::start(dll),
            controls,
            ui_font: f,
            dpi,
            generation: 0,
            path: None,
            sizes: vec![],
            bytes: 0,
            page: 0,
            mode: Mode::Continuous,
            zoom: Zoom::FitPage,
            rotation: 0,
            sidebar: true,
            scroll: 0.,
            pan: 0.,
            thumb_scroll: 0.,
            cache: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            settings,
            store,
            status: "准备就绪".into(),
            failed: false,
            matches: vec![],
            match_index: 0,
            search_token: 0,
            searching: false,
            query: String::new(),
            mouse: (0, 0),
            drag: None,
            last_save: Instant::now(),
            toolbar_visible: false,
            toolbar_last_hover: Instant::now(),
            qa,
            qa_step: 0,
            qa_wait: Instant::now(),
        })
    }
    fn tone(&self, light: u32, dark: u32) -> u32 {
        if self.settings.dark {
            dark
        } else {
            light
        }
    }
    unsafe fn apply_theme(&self) {
        let dark: i32 = self.settings.dark.into();
        DwmSetWindowAttribute(
            self.hwnd,
            20,
            &dark as *const _ as _,
            size_of::<i32>() as u32,
        );
        for (attribute, rgb) in [
            (DWMWA_CAPTION_COLOR, self.tone(0xEDF0F4, 0x15181E)),
            (DWMWA_BORDER_COLOR, self.tone(0xEDF0F4, 0x15181E)),
            (DWMWA_TEXT_COLOR, self.tone(0x273243, 0xE4EAF3)),
        ] {
            let value = color(rgb);
            DwmSetWindowAttribute(
                self.hwnd,
                attribute as u32,
                &value as *const _ as _,
                size_of::<u32>() as u32,
            );
        }
        if let Some(h) = self.controls.get(&THEME) {
            SetWindowTextW(
                *h,
                wide(if self.settings.dark {
                    "浅色"
                } else {
                    "暗黑"
                })
                .as_ptr(),
            );
        }
        self.invalidate();
    }
    fn px(&self, v: f64) -> i32 {
        (v * self.dpi).round() as i32
    }
    unsafe fn size(&self) -> (f64, f64) {
        let mut r: RECT = zeroed();
        GetClientRect(self.hwnd, &mut r);
        (r.right as f64 / self.dpi, r.bottom as f64 / self.dpi)
    }
    fn side(&self) -> f64 {
        if self.sidebar {
            216.
        } else {
            0.
        }
    }
    unsafe fn view(&self) -> (f64, f64) {
        let (w, h) = self.size();
        ((w - self.side()).max(1.), (h - CONTENT_TOP - 32.).max(1.))
    }
    unsafe fn layout(&self) -> (Vec<PageRect>, f64) {
        let (w, h) = self.view();
        model::layout(
            &self.sizes,
            self.page,
            self.mode,
            self.zoom,
            self.rotation,
            w,
            h,
        )
    }
    unsafe fn invalidate(&self) {
        InvalidateRect(self.hwnd, null(), 0);
    }
    unsafe fn resize(&mut self) {
        let (w, _) = self.size();
        let mut x = 10.;
        for (id, width) in [
            (OPEN, 88.),
            (SIDEBAR, 42.),
            (PREV, 32.),
            (PAGE, 44.),
            (NEXT, 32.),
            (MINUS, 30.),
            (PLUS, 30.),
            (FIT, 60.),
            (ROTATE, 44.),
            (SINGLE, 44.),
            (CONTINUOUS, 44.),
            (SPREAD, 44.),
            (THEME, 44.),
        ] {
            if let Some(h) = self.controls.get(&id) {
                MoveWindow(
                    *h,
                    self.px(x),
                    self.px(if id == PAGE { 11. } else { 7. }),
                    self.px(width),
                    self.px(if id == PAGE { 24. } else { 32. }),
                    1,
                );
            }
            x += width + 7.;
        }
        let sx = (w - 326.).max(x + 10.);
        let sw = (w - sx - 140.).max(100.);
        for (id, xx, ww) in [
            (SEARCH, sx, sw),
            (FIND, sx + sw + 8., 52.),
            (MATCH_NEXT, sx + sw + 66., 52.),
        ] {
            if let Some(h) = self.controls.get(&id) {
                MoveWindow(
                    *h,
                    self.px(xx),
                    self.px(if id == SEARCH { 11. } else { 7. }),
                    self.px(ww),
                    self.px(if id == SEARCH { 24. } else { 32. }),
                    1,
                );
            }
        }
        for h in self.controls.values() {
            ShowWindow(
                *h,
                if self.toolbar_visible {
                    SW_SHOWNA
                } else {
                    SW_HIDE
                },
            );
        }
        self.pan = 0.;
        self.scroll = clamp_scroll(self.scroll, self.layout().1, self.view().1);
        self.invalidate();
    }
    unsafe fn set_toolbar(&mut self, visible: bool) {
        if visible {
            self.toolbar_last_hover = Instant::now();
        }
        if self.toolbar_visible == visible {
            return;
        }
        self.toolbar_visible = visible;
        if !visible && self.controls.values().any(|h| *h == GetFocus()) {
            SetFocus(self.hwnd);
        }
        for h in self.controls.values() {
            ShowWindow(*h, if visible { SW_SHOWNA } else { SW_HIDE });
        }
        self.invalidate();
    }
    unsafe fn update_toolbar(&mut self) {
        if self.qa.is_some() {
            return;
        }
        let mut pt: POINT = zeroed();
        GetCursorPos(&mut pt);
        ScreenToClient(self.hwnd, &mut pt);
        let (w, _) = self.size();
        let inside =
            GetForegroundWindow() == self.hwnd && pt.x >= 0 && pt.x < self.px(w) && pt.y >= 0;
        let y = pt.y as f64 / self.dpi;
        if inside && y <= if self.toolbar_visible { TOP + 8. } else { 8. } && self.drag.is_none() {
            self.set_toolbar(true);
        } else {
            let focused = [PAGE, SEARCH]
                .iter()
                .any(|id| self.controls.get(id).copied() == Some(GetFocus()));
            if self.toolbar_visible
                && !focused
                && self.toolbar_last_hover.elapsed().as_millis() > 550
            {
                self.set_toolbar(false);
            }
        }
    }
    unsafe fn read_control(&self, id: usize) -> String {
        let Some(h) = self.controls.get(&id) else {
            return String::new();
        };
        let n = GetWindowTextLengthW(*h).max(0) as usize;
        let mut v = vec![0; n + 1];
        GetWindowTextW(*h, v.as_mut_ptr(), v.len() as i32);
        String::from_utf16_lossy(&v[..n])
    }
    unsafe fn open(&mut self, path: PathBuf, password: String) {
        self.save();
        self.generation += 1;
        self.worker
            .generation
            .store(self.generation, Ordering::Relaxed);
        self.search_token += 1;
        self.worker
            .search
            .store(self.search_token, Ordering::Relaxed);
        self.path = Some(path.clone());
        self.sizes.clear();
        self.cache.clear();
        self.order.clear();
        self.pending.clear();
        self.matches.clear();
        self.searching = false;
        self.failed = false;
        self.page = 0;
        self.scroll = 0.;
        self.pan = 0.;
        self.thumb_scroll = 0.;
        self.status = "正在打开文档…".into();
        let req = Request::Open {
            generation: self.generation,
            path,
            password,
        };
        if self.worker.tx.send(req).is_err() {
            self.status = "PDF 工作线程已停止，请重新启动应用。".into();
            self.failed = true;
        }
        self.invalidate();
    }
    unsafe fn dialog(&mut self) {
        let mut buf = vec![0u16; 32768];
        let filter = wide("PDF 文档 (*.pdf)\0*.pdf\0所有文件\0*.*\0");
        let mut o: OPENFILENAMEW = zeroed();
        o.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        o.hwndOwner = self.hwnd;
        o.lpstrFilter = filter.as_ptr();
        o.lpstrFile = buf.as_mut_ptr();
        o.nMaxFile = buf.len() as u32;
        o.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut o) != 0 {
            let n = buf.iter().position(|x| *x == 0).unwrap_or(0);
            self.open(
                PathBuf::from(String::from_utf16_lossy(&buf[..n])),
                String::new(),
            );
        }
    }
    unsafe fn goto(&mut self, page: usize) {
        if self.sizes.is_empty() {
            return;
        }
        self.page = page.min(self.sizes.len() - 1);
        self.scroll = 0.;
        self.pan = 0.;
        if self.mode == Mode::Continuous {
            if let Some(p) = self.layout().0.iter().find(|p| p.page == self.page) {
                self.scroll = p.y - 24.;
            }
        }
        self.thumb_scroll = (self.page as f64 * 190. - 100.).max(0.);
        if let Some(h) = self.controls.get(&PAGE) {
            SetWindowTextW(*h, wide(&(self.page + 1).to_string()).as_ptr());
        }
        self.invalidate();
    }
    unsafe fn command(&mut self, id: usize) {
        match id {
            OPEN => self.dialog(),
            THEME => {
                self.settings.dark = !self.settings.dark;
                self.apply_theme();
                self.save();
            }
            SIDEBAR => {
                self.sidebar = !self.sidebar;
                self.resize();
                self.goto(self.page)
            }
            PREV => {
                self.goto(
                    self.page
                        .saturating_sub(if self.mode == Mode::Spread { 2 } else { 1 }),
                )
            }
            NEXT => self.goto(self.page + if self.mode == Mode::Spread { 2 } else { 1 }),
            SINGLE | CONTINUOUS | SPREAD => {
                self.mode = match id {
                    SINGLE => Mode::Single,
                    SPREAD => Mode::Spread,
                    _ => Mode::Continuous,
                };
                self.goto(self.page)
            }
            FIT => {
                self.zoom = if self.zoom == Zoom::FitWidth {
                    Zoom::FitPage
                } else {
                    Zoom::FitWidth
                };
                if let Some(h) = self.controls.get(&FIT) {
                    SetWindowTextW(
                        *h,
                        wide(if self.zoom == Zoom::FitWidth {
                            "适合宽"
                        } else {
                            "适合页"
                        })
                        .as_ptr(),
                    );
                }
                self.goto(self.page)
            }
            MINUS | PLUS => {
                let factor = if id == PLUS { 1.2 } else { 1. / 1.2 };
                let old = self
                    .layout()
                    .0
                    .iter()
                    .find(|p| p.page == self.page)
                    .map(|r| {
                        r.w / if self.rotation % 2 == 0 {
                            self.sizes[self.page].0
                        } else {
                            self.sizes[self.page].1
                        }
                    })
                    .unwrap_or(1.);
                self.zoom = Zoom::Custom((old * factor).clamp(0.2, 4.));
                self.goto(self.page)
            }
            ROTATE => {
                self.rotation = (self.rotation + 1) % 4;
                self.cache.clear();
                self.order.clear();
                self.goto(self.page)
            }
            FIND => {
                let q = self.read_control(SEARCH).trim().to_string();
                if q.is_empty() {
                    self.status = "输入要查找的文字。扫描件需先经过 OCR。".into();
                    self.invalidate();
                    return;
                }
                self.query = q.clone();
                self.search_token += 1;
                self.worker
                    .search
                    .store(self.search_token, Ordering::Relaxed);
                self.searching = true;
                self.status = "正在搜索文档…".into();
                if self
                    .worker
                    .tx
                    .try_send(Request::Search {
                        generation: self.generation,
                        token: self.search_token,
                        query: q,
                    })
                    .is_err()
                {
                    self.status = "渲染队列繁忙，请稍后再搜索。".into();
                    self.searching = false;
                }
                self.invalidate()
            }
            MATCH_NEXT if !self.matches.is_empty() => {
                self.match_index = (self.match_index + 1) % self.matches.len();
                self.goto(self.matches[self.match_index]);
                self.status = format!(
                    "“{}” · 匹配页 {} / {}",
                    self.query,
                    self.match_index + 1,
                    self.matches.len()
                );
            }
            _ => {}
        }
        self.invalidate();
    }
    unsafe fn save(&mut self) {
        if let Some(path) = self.path.as_ref().filter(|_| !self.sizes.is_empty()) {
            let path = path.to_string_lossy().to_string();
            self.settings.recent.retain(|r| r.path != path);
            self.settings.recent.insert(
                0,
                Recent {
                    path,
                    page: self.page,
                    mode: self.mode,
                    zoom: self.zoom,
                    rotation: self.rotation,
                },
            );
            self.settings.recent.truncate(12);
        }
        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(&self.store).map_err(|e| e.to_string())?;
            let text = serde_json::to_vec_pretty(&self.settings).map_err(|e| e.to_string())?;
            let tmp = self.store.join("settings.tmp");
            std::fs::write(&tmp, text).map_err(|e| e.to_string())?; // MoveFileEx replaces atomically on Windows.
            if MoveFileExW(
                wide(&tmp.to_string_lossy()).as_ptr(),
                wide(&self.store.join("settings.json").to_string_lossy()).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            ) == 0
            {
                return Err("无法写入阅读进度".into());
            }
            Ok(())
        })();
        if result.is_err() {
            self.status = "阅读正常，但本地进度保存失败。".into();
        }
        self.last_save = Instant::now();
    }
    unsafe fn poll(&mut self) {
        let mut changed = false;
        while let Ok(event) = self.worker.rx.try_recv() {
            match event {
                Response::Loaded {
                    generation,
                    path,
                    sizes,
                    bytes,
                } if generation == self.generation => {
                    self.sizes = sizes;
                    self.bytes = bytes;
                    let key = path.to_string_lossy();
                    if let Some(saved) =
                        self.settings.recent.iter().find(|r| r.path == key).cloned()
                    {
                        self.mode = saved.mode;
                        self.zoom = saved.zoom;
                        self.rotation = saved.rotation;
                        self.goto(saved.page);
                    } else {
                        self.goto(0)
                    }
                    self.status = "文档已就绪 · Ctrl+F 搜索 · Ctrl+滚轮缩放 · 拖动页面平移".into();
                    SetWindowTextW(
                        self.hwnd,
                        wide(&format!(
                            "{} — LeafPDF",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ))
                        .as_ptr(),
                    );
                    changed = true;
                }
                Response::Image {
                    generation,
                    key,
                    pixels,
                } if generation == self.generation => {
                    self.pending.remove(&key);
                    self.cache.insert(key, pixels);
                    self.order.retain(|k| *k != key);
                    self.order.push_back(key);
                    while self.cache.len() > 12
                        || self.cache.values().map(|p| p.data.len()).sum::<usize>()
                            > 96 * 1024 * 1024
                    {
                        if let Some(old) = self.order.pop_front() {
                            self.cache.remove(&old);
                        }
                    }
                    changed = true;
                }
                Response::Matches {
                    generation,
                    token,
                    pages,
                } if generation == self.generation && token == self.search_token => {
                    self.searching = false;
                    self.matches = pages;
                    self.match_index = 0;
                    if let Some(&page) = self.matches.first() {
                        self.goto(page);
                        self.status = format!(
                            "“{}” · 匹配页 1 / {} · 点“下个”继续",
                            self.query,
                            self.matches.len()
                        );
                    } else {
                        self.status = "没有找到匹配文字。图像型 PDF 需要 OCR 才能搜索。".into();
                    }
                    changed = true;
                }
                Response::Error {
                    generation,
                    message,
                } if generation == self.generation || generation == 0 => {
                    self.status = message;
                    self.failed = true;
                    changed = true;
                }
                Response::RenderError { generation, key } if generation == self.generation => {
                    self.pending.remove(&key);
                    self.status = format!("第 {} 页渲染失败。", key.page + 1);
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            self.invalidate()
        }
        if self.last_save.elapsed().as_secs() > 5 {
            self.save()
        }
    }
    fn request(&mut self, key: Key) {
        if !self.cache.contains_key(&key)
            && !self.pending.contains(&key)
            && self.pending.len() < 8
            && self
                .worker
                .tx
                .try_send(Request::Render {
                    generation: self.generation,
                    key,
                })
                .is_ok()
        {
            self.pending.insert(key);
        }
    }
    unsafe fn paint(&mut self, dc: HDC) {
        let (w, h) = self.size();
        let s = self.dpi;
        let rr = |x: f64, y: f64, w: f64, h: f64| {
            rect(
                (x * s) as i32,
                (y * s) as i32,
                (w * s) as i32,
                (h * s) as i32,
            )
        };
        fill(dc, rr(0., 0., w, h), self.tone(0xEDF0F4, 0x15181E));

        let side = self.side();
        let (vw, vh) = self.view();
        let (pages, total) = self.layout();
        self.scroll = clamp_scroll(self.scroll, total, vh);
        if self.sidebar {
            fill(
                dc,
                rr(0., CONTENT_TOP, 216., h - CONTENT_TOP - 32.),
                self.tone(0xF8FAFC, 0x1D222B),
            );
            fill(
                dc,
                rr(215., CONTENT_TOP, 1., h - CONTENT_TOP - 32.),
                self.tone(0xDEE4ED, 0x303846),
            );
            text(
                dc,
                "页面缩略图",
                rr(22., CONTENT_TOP + 12., 178., 26.),
                self.px(12.),
                self.tone(0x708095, 0xA8B5C8),
                true,
                DT_SINGLELINE | DT_VCENTER,
            );
            let save = SaveDC(dc);
            IntersectClipRect(
                dc,
                0,
                self.px(CONTENT_TOP + 48.),
                self.px(215.),
                self.px(h - 32.),
            );
            let count = self.sizes.len();
            self.thumb_scroll = clamp_scroll(
                self.thumb_scroll,
                count as f64 * 190.,
                (h - (CONTENT_TOP + 80.)).max(1.),
            );
            let first = (self.thumb_scroll / 190.) as usize;
            for i in first..(first + 6).min(count) {
                let y = (CONTENT_TOP + 56.) + i as f64 * 190. - self.thumb_scroll;
                let pw = 126.;
                let (wp, hp) = self.sizes[i];
                let ratio = if self.rotation % 2 == 0 {
                    hp / wp
                } else {
                    wp / hp
                };
                let ph = (pw * ratio).min(142.);
                let tw = ph / ratio;
                let x = (216. - tw) / 2.;
                let key = Key {
                    page: i,
                    width: 180,
                    rotation: self.rotation,
                };
                if i == self.page {
                    round(
                        dc,
                        rr(17., y - 7., 182., 179.),
                        self.tone(0xEAF1FF, 0x293C5B),
                        self.tone(0xC6D8FA, 0x526F9D),
                    );
                }
                round(dc, rr(x - 1., y - 1., tw + 2., ph + 2.), 0xFFFFFF, 0xDAE1EB);
                if let Some(p) = self.cache.get(&key) {
                    bitmap(dc, p, rr(x, y, tw, ph));
                } else {
                    self.request(key);
                }
                text(
                    dc,
                    &format!("{:02}", i + 1),
                    rr(20., y + 146., 176., 23.),
                    self.px(12.),
                    if i == self.page {
                        self.tone(0x365EDB, 0xA9C3FF)
                    } else {
                        self.tone(0x7C8899, 0xAAB8CA)
                    },
                    i == self.page,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
            RestoreDC(dc, save);
        }
        if self.sizes.is_empty() {
            let x = side + (vw - 500.) / 2.;
            let y = CONTENT_TOP + (vh - 260.) / 2.;
            text(
                dc,
                if self.failed {
                    "暂时无法打开"
                } else if self.path.is_some() {
                    "正在打开文档"
                } else {
                    "留一点空间，专注阅读。"
                },
                rr(x, y, 500., 55.),
                self.px(30.),
                self.tone(0x253348, 0xE4EAF3),
                true,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            );
            text(
                dc,
                if self.path.is_some() {
                    &self.status
                } else {
                    "拖入 PDF 文件，或按 Ctrl+O 打开\n本地文件 · 宽屏阅读 · 自动记住阅读位置"
                },
                rr(x + 20., y + 72., 460., 100.),
                self.px(15.),
                self.tone(0x748196, 0xB0BCD0),
                false,
                DT_CENTER | DT_WORDBREAK,
            );
            if self.path.is_none() {
                for (i, recent) in self.settings.recent.iter().take(3).enumerate() {
                    let label = PathBuf::from(&recent.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    round(
                        dc,
                        rr(x + 20., y + 190. + i as f64 * 50., 460., 42.),
                        self.tone(0xFFFFFF, 0x252D39),
                        self.tone(0xDDE4EE, 0x3C485C),
                    );
                    text(
                        dc,
                        &format!("{}   ·   第 {} 页", label, recent.page + 1),
                        rr(x + 36., y + 193. + i as f64 * 50., 428., 34.),
                        self.px(13.),
                        self.tone(0x516078, 0xC4D0E2),
                        false,
                        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                    );
                }
            }
        } else {
            let save = SaveDC(dc);
            IntersectClipRect(
                dc,
                self.px(side),
                self.px(CONTENT_TOP),
                self.px(w),
                self.px(h - 32.),
            );
            let max_width = pages.iter().map(|p| p.x + p.w + 24.).fold(vw, f64::max);
            self.pan = clamp_scroll(self.pan, max_width, vw);
            let mut visible = vec![];
            for p in &pages {
                let y = p.y - self.scroll;
                if y + p.h < 0. || y > vh {
                    continue;
                }
                visible.push(*p);
            }
            // Large page renders are enqueued ahead of thumbnails on subsequent paint cycles.
            for p in visible {
                let x = side + p.x - self.pan;
                let y = CONTENT_TOP + p.y - self.scroll;
                fill(
                    dc,
                    rr(x + 3., y + 4., p.w, p.h),
                    self.tone(0xD8DEE7, 0x090C11),
                );
                fill(dc, rr(x, y, p.w, p.h), 0xFFFFFF);
                let key = Key {
                    page: p.page,
                    width: (p.w * s).round().clamp(40., 4096.) as i32,
                    rotation: self.rotation,
                };
                if let Some(pixels) = self.cache.get(&key) {
                    bitmap(dc, pixels, rr(x, y, p.w, p.h));
                } else {
                    self.request(key);
                    text(
                        dc,
                        "正在渲染…",
                        rr(x, y + p.h / 2. - 20., p.w, 40.),
                        self.px(14.),
                        0x98A3B3,
                        false,
                        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                    );
                }
            }
            if total > vh + 1. {
                let track = vh - 16.;
                let handle = (track * vh / total).max(24.);
                let yy = (CONTENT_TOP + 8.) + (track - handle) * self.scroll / (total - vh);
                round(dc, rr(w - 8., yy, 4., handle), 0xA9B4C3, 0xA9B4C3);
            }
            RestoreDC(dc, save);
        }
        fill(dc, rr(0., h - 32., w, 32.), self.tone(0xFFFFFF, 0x1D222B));
        fill(dc, rr(0., h - 32., w, 1.), self.tone(0xDDE4EC, 0x303846));
        text(
            dc,
            &self.status,
            rr(18., h - 29., w - 280., 26.),
            self.px(12.),
            if self.failed {
                0xB14E40
            } else {
                self.tone(0x748094, 0xA8B5CA)
            },
            false,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
        );
        let scale = pages
            .iter()
            .find(|p| p.page == self.page)
            .map(|p| {
                p.w / if self.rotation % 2 == 0 {
                    self.sizes[self.page].0
                } else {
                    self.sizes[self.page].1
                } * 100.
            })
            .unwrap_or(100.);
        text(
            dc,
            &if self.sizes.is_empty() {
                "LeafPDF  0.1".into()
            } else {
                format!(
                    "{} / {} 页     ·     {:.0}%",
                    self.page + 1,
                    self.sizes.len(),
                    scale
                )
            },
            rr(w - 250., h - 29., 230., 26.),
            self.px(12.),
            self.tone(0x53637A, 0xB5C2D5),
            false,
            DT_SINGLELINE | DT_VCENTER | DT_RIGHT,
        );
        if self.toolbar_visible {
            fill(dc, rr(0., 0., w, TOP), 0x222833);
        }
    }
    unsafe fn wheel(&mut self, delta: i32, x: i32) {
        if GetKeyState(VK_CONTROL as i32) < 0 {
            self.command(if delta > 0 { PLUS } else { MINUS });
            return;
        }
        if self.sidebar && (x as f64 / self.dpi) < 216. {
            self.thumb_scroll -= delta as f64 / 120. * 90.;
        } else if GetKeyState(VK_SHIFT as i32) < 0 {
            self.pan -= delta as f64 / 120. * 100.;
        } else {
            self.scroll -= delta as f64 / 120. * 110.;
            self.sync_page();
        }
        self.invalidate();
    }
    unsafe fn sync_page(&mut self) {
        let (pages, total) = self.layout();
        self.scroll = clamp_scroll(self.scroll, total, self.view().1);
        if self.mode == Mode::Continuous {
            if let Some(p) = pages.iter().find(|p| p.y + p.h > self.scroll + 40.) {
                self.page = p.page;
                if let Some(h) = self.controls.get(&PAGE) {
                    SetWindowTextW(*h, wide(&(self.page + 1).to_string()).as_ptr());
                }
            }
        }
    }
    unsafe fn click(&mut self, x: i32, y: i32) {
        let (x, y) = (x as f64 / self.dpi, y as f64 / self.dpi);
        if self.toolbar_visible && y < TOP {
            return;
        }
        if self.sidebar && x < 216. && y > (CONTENT_TOP + 48.) {
            let i = ((y - (CONTENT_TOP + 56.) + self.thumb_scroll) / 190.)
                .floor()
                .max(0.) as usize;
            self.goto(i);
            return;
        }
        if self.path.is_none() {
            let (w, h) = self.view();
            let xx = self.side() + (w - 500.) / 2. + 20.;
            let yy = CONTENT_TOP + (h - 260.) / 2. + 190.;
            if x >= xx && x < xx + 460. && y >= yy {
                let i = ((y - yy) / 50.) as usize;
                if let Some(r) = self.settings.recent.get(i).cloned() {
                    self.open(PathBuf::from(r.path), String::new());
                }
            }
        }
    }
    unsafe fn qa_tick(&mut self) {
        let Some(dir) = self.qa.clone() else { return };
        if self.qa_wait.elapsed().as_secs() > 35 {
            let _ = std::fs::write(dir.join("failure.txt"), &self.status);
            PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
            return;
        }
        if self.sizes.is_empty()
            || self.searching
            || !self.pending.is_empty()
            || self.cache.is_empty()
            || self.qa_wait.elapsed().as_millis() < 1300
        {
            return;
        }
        let output = dir.join(format!("view-{}.png", self.qa_step)); // Capture after releasing APP's mutable borrow, via posted message.
        let _ = std::fs::write(
            dir.join("capture-next.txt"),
            output.to_string_lossy().as_bytes(),
        );
        PostMessageW(self.hwnd, WM_APP + 2, 0, 0);
    }
}
unsafe fn draw_button(dis: &DRAWITEMSTRUCT) {
    let mut txt = vec![0u16; 80];
    let n = GetWindowTextW(dis.hwndItem, txt.as_mut_ptr(), 80);
    let label = String::from_utf16_lossy(&txt[..n.max(0) as usize]);
    let selected = APP
        .with(|a| {
            a.try_borrow().ok().and_then(|a| {
                a.as_ref().map(|a| {
                    matches!(
                        (dis.CtlID as usize, a.mode),
                        (SINGLE, Mode::Single)
                            | (CONTINUOUS, Mode::Continuous)
                            | (SPREAD, Mode::Spread)
                    )
                })
            })
        })
        .unwrap_or(false);
    let s = GetDpiForWindow(dis.hwndItem) as f64 / 96.;
    fill(dis.hDC, dis.rcItem, 0x222833);
    let mut r = dis.rcItem;
    r.left += 2;
    r.right -= 2;
    r.top += 2;
    r.bottom -= 2;
    let primary = dis.CtlID as usize == OPEN;
    let bg = if primary {
        0x5775E5
    } else if selected {
        0x424E65
    } else if dis.itemState & ODS_SELECTED != 0 {
        0x394457
    } else {
        0x2C3442
    };
    round(dis.hDC, r, bg, bg);
    text(
        dis.hDC,
        &label,
        r,
        (14. * s) as i32,
        0xF4F6FB,
        primary || selected,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    if dis.itemState & ODS_FOCUS != 0 {
        r.left += 3;
        r.right -= 3;
        r.top += 3;
        r.bottom -= 3;
        DrawFocusRect(dis.hDC, &r);
    }
}
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CTLCOLOREDIT {
        let dc = wp as HDC;
        SetTextColor(dc, color(0xF0F4FA));
        SetBkColor(dc, color(0x30394A));
        SetDCBrushColor(dc, color(0x30394A));
        return GetStockObject(DC_BRUSH) as isize;
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return 0;
    }
    if msg == WM_DRAWITEM {
        draw_button(&*(lp as *const DRAWITEMSTRUCT));
        return 1;
    }
    if msg == WM_ERASEBKGND {
        return 1;
    }
    if msg == WM_GETMINMAXINFO {
        let m = &mut *(lp as *mut MINMAXINFO);
        let d = GetDpiForWindow(hwnd) as f64 / 96.;
        m.ptMinTrackSize = POINT {
            x: (1000. * d) as i32,
            y: (640. * d) as i32,
        };
        return 0;
    }
    if msg == WM_APP + 2 {
        return qa_capture(hwnd);
    }
    let handled = APP.with(|a| {
        let Ok(mut borrow) = a.try_borrow_mut() else {
            return None;
        };
        let app = borrow.as_mut()?;
        match msg {
            WM_SIZE => {
                app.resize();
                Some(0)
            }
            WM_DPICHANGED => {
                app.dpi = (wp & 0xffff) as f64 / 96.;
                let old = app.ui_font;
                app.ui_font = font(app.px(14.), false);
                for h in app.controls.values() {
                    SendMessageW(*h, WM_SETFONT, app.ui_font as usize, 1);
                }
                DeleteObject(old);
                let r = &*(lp as *const RECT);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                app.resize();
                Some(0)
            }
            WM_COMMAND => {
                let id = wp & 0xffff;
                if (wp >> 16) == BN_CLICKED as usize {
                    app.command(id)
                }
                Some(0)
            }
            WM_TIMER => {
                app.poll();
                app.update_toolbar();
                if app.qa.is_some() {
                    let dc = GetDC(hwnd);
                    app.paint(dc);
                    ReleaseDC(hwnd, dc);
                }
                app.qa_tick();
                Some(0)
            }
            WM_DROPFILES => {
                let drop = wp as HDROP;
                let n = DragQueryFileW(drop, 0, null_mut(), 0);
                let mut buf = vec![0u16; n as usize + 1];
                DragQueryFileW(drop, 0, buf.as_mut_ptr(), n + 1);
                DragFinish(drop);
                app.open(
                    PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])),
                    String::new(),
                );
                Some(0)
            }
            WM_PAINT => {
                let mut ps = zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r = zeroed();
                GetClientRect(hwnd, &mut r);
                let mem = CreateCompatibleDC(dc);
                let bmp = CreateCompatibleBitmap(dc, r.right.max(1), r.bottom.max(1));
                let old = SelectObject(mem, bmp);
                app.paint(mem);
                BitBlt(dc, 0, 0, r.right, r.bottom, mem, 0, 0, SRCCOPY);
                SelectObject(mem, old);
                DeleteObject(bmp);
                DeleteDC(mem);
                EndPaint(hwnd, &ps);
                Some(0)
            }
            WM_PRINTCLIENT => {
                app.paint(wp as HDC);
                Some(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp >> 16) as u16) as i16 as i32;
                let mut pt = POINT {
                    x: (lp as u16) as i16 as i32,
                    y: ((lp >> 16) as u16) as i16 as i32,
                };
                ScreenToClient(hwnd, &mut pt);
                app.wheel(delta, pt.x);
                Some(0)
            }
            WM_LBUTTONDOWN => {
                let x = (lp as u16) as i16 as i32;
                let y = ((lp >> 16) as u16) as i16 as i32;
                app.click(x, y);
                if x as f64 / app.dpi > app.side()
                    && y as f64 / app.dpi
                        > if app.toolbar_visible {
                            TOP
                        } else {
                            CONTENT_TOP
                        }
                {
                    app.drag = Some((x, y, app.pan, app.scroll));
                    SetCapture(hwnd);
                }
                SetFocus(hwnd);
                Some(0)
            }
            WM_MOUSEMOVE => {
                let x = (lp as u16) as i16 as i32;
                let y = ((lp >> 16) as u16) as i16 as i32;
                app.mouse = (x, y);
                if let Some((sx, sy, pan, scroll)) = app.drag {
                    app.pan = pan - (x - sx) as f64 / app.dpi;
                    app.scroll = scroll - (y - sy) as f64 / app.dpi;
                    app.sync_page();
                    app.invalidate()
                }
                Some(0)
            }
            WM_LBUTTONUP => {
                app.drag = None;
                ReleaseCapture();
                Some(0)
            }
            WM_CLOSE => {
                app.save();
                DestroyWindow(hwnd);
                Some(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                Some(0)
            }
            _ => None,
        }
    });
    handled.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wp, lp))
}
unsafe fn qa_capture(hwnd: HWND) -> LRESULT {
    let dir = APP.with(|a| a.borrow().as_ref().and_then(|a| a.qa.clone()));
    let Some(dir) = dir else { return 0 };
    let path = std::fs::read_to_string(dir.join("capture-next.txt")).unwrap_or_default();
    if !path.is_empty() {
        let _ = capture(hwnd, &PathBuf::from(path));
    }
    APP.with(|a|{if let Some(a)=a.borrow_mut().as_mut(){a.qa_step+=1;a.qa_wait=Instant::now();match a.qa_step{1=>{a.set_toolbar(true);a.command(THEME);a.command(SPREAD);a.goto(1)},2=>{a.set_toolbar(false);a.command(ROTATE);a.command(PLUS)},3=>{a.rotation=0;a.command(CONTINUOUS);a.zoom=Zoom::FitWidth;a.goto(2)},4=>{if let Some(h)=a.controls.get(&SEARCH){SetWindowTextW(*h,wide("LeafPDF").as_ptr());}a.command(FIND)},_=>{let data=serde_json::json!({"pages":a.sizes.len(),"search_matches":a.matches,"cached_images":a.cache.len(),"status":a.status,"dark":a.settings.dark});let _=std::fs::write(dir.join("result.json"),data.to_string());PostMessageW(hwnd,WM_CLOSE,0,0);}}}});
    0
}
unsafe fn key(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN {
        return false;
    }
    let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
    let key = msg.wParam as u16;
    APP.with(|a| {
        let Ok(mut b) = a.try_borrow_mut() else {
            return false;
        };
        let Some(a) = b.as_mut() else { return false };
        let focus = GetFocus();
        let in_search = a.controls.get(&SEARCH).copied() == Some(focus);
        let in_page = a.controls.get(&PAGE).copied() == Some(focus);
        if ctrl && key == b'O' as u16 {
            a.dialog();
            return true;
        }
        if ctrl && key == b'F' as u16 {
            a.set_toolbar(true);
            if let Some(h) = a.controls.get(&SEARCH) {
                SetFocus(*h);
                SendMessageW(*h, EM_SETSEL, 0, -1);
            }
            return true;
        }
        if key == VK_ESCAPE {
            a.set_toolbar(false);
            SetFocus(a.hwnd);
            return true;
        }
        if key == VK_F10 {
            a.set_toolbar(!a.toolbar_visible);
            return true;
        }
        if key == VK_RETURN {
            if in_search {
                a.command(FIND);
                return true;
            }
            if in_page {
                if let Ok(p) = a.read_control(PAGE).parse::<usize>() {
                    a.goto(p.saturating_sub(1));
                }
                SetFocus(a.hwnd);
                return true;
            }
        }
        if in_search || in_page {
            return false;
        }
        match key {
            VK_NEXT | VK_RIGHT => a.command(NEXT),
            VK_PRIOR | VK_LEFT => a.command(PREV),
            VK_DOWN => {
                a.scroll += 60.;
                a.sync_page();
                a.invalidate()
            }
            VK_UP => {
                a.scroll -= 60.;
                a.sync_page();
                a.invalidate()
            }
            VK_HOME => a.goto(0),
            VK_END => a.goto(a.sizes.len().saturating_sub(1)),
            VK_F3 => a.command(MATCH_NEXT),
            VK_ESCAPE => {
                a.drag = None;
                ReleaseCapture();
            }
            _ => return false,
        }
        true
    })
}
fn main() {
    if let Err(e) = run() {
        unsafe {
            MessageBoxW(
                null_mut(),
                wide(&e).as_ptr(),
                wide("LeafPDF").as_ptr(),
                MB_ICONERROR | MB_OK,
            );
        }
    }
}
fn run() -> Result<(), String> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(null());
        let class = wide("LeafPDFWindow");
        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hIcon = LoadIconW(instance, std::ptr::without_provenance::<u16>(1));
        RegisterClassExW(&wc);
        let args: Vec<String> = std::env::args().collect();
        let qa = args
            .iter()
            .position(|s| s == "--qa")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        if let Some(d) = &qa {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let mut work: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as _, 0);
        let ds = GetDpiForSystem() as f64 / 96.;
        let ww = (1440. * ds) as i32;
        let wh = (900. * ds) as i32;
        let ww = ww.min(work.right - work.left - 32);
        let wh = wh.min(work.bottom - work.top - 48);
        let hwnd = CreateWindowExW(
            WS_EX_ACCEPTFILES,
            class.as_ptr(),
            wide("LeafPDF · 轻阅").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            work.left + (work.right - work.left - ww) / 2,
            work.top + (work.bottom - work.top - wh) / 2,
            ww,
            wh,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err("无法创建阅读窗口".into());
        }
        let corner = 2u32;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const _ as _,
            4,
        );
        let mut a = App::new(hwnd, qa.clone())?;
        a.apply_theme();
        a.resize();
        if let Some(path) = args
            .iter()
            .skip(1)
            .find(|p| p.to_lowercase().ends_with(".pdf"))
        {
            a.open(PathBuf::from(path), String::new());
        }
        APP.with(|s| *s.borrow_mut() = Some(a));
        SetTimer(hwnd, 1, 60, None);
        if qa.is_none() {
            ShowWindow(hwnd, SW_SHOWMAXIMIZED);
            UpdateWindow(hwnd);
        } else {
            InvalidateRect(hwnd, null(), 0); // WM_PRINT paints the hidden QA window without disturbing the user's desktop.
            APP.with(|a| {
                if let Some(a) = a.borrow_mut().as_mut() {
                    let dc = GetDC(hwnd);
                    a.paint(dc);
                    ReleaseDC(hwnd, dc);
                }
            });
        }
        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if key(&msg) {
                continue;
            }
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        APP.with(|a| {
            if let Some(a) = a.borrow_mut().take() {
                DeleteObject(a.ui_font);
            }
        });
        Ok(())
    }
}
