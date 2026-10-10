use windows_sys::Win32::Foundation::FreeLibrary;
// PDFium stays on a single worker. All handles are closed before document bytes or DLL are freed.
use std::{
    ffi::{c_void, CString},
    path::{Path, PathBuf},
    ptr::null_mut,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
};
type Handle = *mut c_void;
type Init = unsafe extern "system" fn();
type Load = unsafe extern "system" fn(*const c_void, usize, *const i8) -> Handle;
type Close = unsafe extern "system" fn(Handle);
type Count = unsafe extern "system" fn(Handle) -> i32;
type LoadPage = unsafe extern "system" fn(Handle, i32) -> Handle;
type Dimension = unsafe extern "system" fn(Handle) -> f64;
type Bitmap = unsafe extern "system" fn(i32, i32, i32, *mut c_void, i32) -> Handle;
type Fill = unsafe extern "system" fn(Handle, i32, i32, i32, i32, u32) -> i32;
type Render = unsafe extern "system" fn(Handle, Handle, i32, i32, i32, i32, i32, i32);
type LoadText = unsafe extern "system" fn(Handle) -> Handle;
type Text = unsafe extern "system" fn(Handle, i32, i32, *mut u16) -> i32;
struct Engine {
    lib: Handle,
    load: Load,
    close: Close,
    count: Count,
    page: LoadPage,
    close_page: Close,
    width: Dimension,
    height: Dimension,
    bitmap: Bitmap,
    fill: Fill,
    render: Render,
    close_bitmap: Close,
    text_page: LoadText,
    close_text: Close,
    text_count: Count,
    text: Text,
    destroy: Init,
}
impl Engine {
    fn new(path: &Path) -> Result<Self, String> {
        let wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: absolute bundled DLL path and restricted dependency search; library kept for every symbol's lifetime.
        unsafe {
            let lib = LoadLibraryExW(
                wide.as_ptr(),
                null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if lib.is_null() {
                return Err("无法加载 pdfium.dll，请将程序完整解压后再启动。".into());
            }
            macro_rules! sym {
                ($name:literal,$ty:ty) => {
                    match GetProcAddress(lib, concat!($name, "\0").as_ptr()) {
                        Some(f) => {
                            std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(f)
                        }
                        None => {
                            FreeLibrary(lib);
                            return Err(format!("PDF 引擎缺少接口：{}", $name));
                        }
                    }
                };
            }
            let init = sym!("FPDF_InitLibrary", Init);
            let e = Self {
                lib,
                load: sym!("FPDF_LoadMemDocument64", Load),
                close: sym!("FPDF_CloseDocument", Close),
                count: sym!("FPDF_GetPageCount", Count),
                page: sym!("FPDF_LoadPage", LoadPage),
                close_page: sym!("FPDF_ClosePage", Close),
                width: sym!("FPDF_GetPageWidth", Dimension),
                height: sym!("FPDF_GetPageHeight", Dimension),
                bitmap: sym!("FPDFBitmap_CreateEx", Bitmap),
                fill: sym!("FPDFBitmap_FillRect", Fill),
                render: sym!("FPDF_RenderPageBitmap", Render),
                close_bitmap: sym!("FPDFBitmap_Destroy", Close),
                text_page: sym!("FPDFText_LoadPage", LoadText),
                close_text: sym!("FPDFText_ClosePage", Close),
                text_count: sym!("FPDFText_CountChars", Count),
                text: sym!("FPDFText_GetText", Text),
                destroy: sym!("FPDF_DestroyLibrary", Init),
            };
            init();
            Ok(e)
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: worker closes document first, no outstanding pages.
        unsafe {
            (self.destroy)();
            FreeLibrary(self.lib);
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Key {
    pub page: usize,
    pub width: i32,
    pub rotation: i32,
}
pub struct Pixels {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
}
pub enum Request {
    Open {
        generation: u64,
        path: PathBuf,
        password: String,
    },
    Render {
        generation: u64,
        key: Key,
    },
    Search {
        generation: u64,
        token: u64,
        query: String,
    },
    Stop,
}
pub enum Response {
    Loaded {
        generation: u64,
        path: PathBuf,
        sizes: Vec<(f64, f64)>,
        bytes: u64,
    },
    Image {
        generation: u64,
        key: Key,
        pixels: Pixels,
    },
    Matches {
        generation: u64,
        token: u64,
        pages: Vec<usize>,
    },
    Error {
        generation: u64,
        message: String,
    },
    RenderError {
        generation: u64,
        key: Key,
    },
}
pub struct Worker {
    pub tx: SyncSender<Request>,
    pub rx: Receiver<Response>,
    pub generation: Arc<AtomicU64>,
    pub search: Arc<AtomicU64>,
}
impl Worker {
    pub fn start(path: PathBuf) -> Self {
        let (tx, requests) = mpsc::sync_channel(32);
        let (responses, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let search = Arc::new(AtomicU64::new(0));
        let gen = generation.clone();
        let token = search.clone();
        std::thread::spawn(move || {
            let engine = match Engine::new(&path) {
                Ok(e) => e,
                Err(message) => {
                    let _ = responses.send(Response::Error {
                        generation: 0,
                        message,
                    });
                    return;
                }
            };
            let mut doc: Handle = null_mut();
            let mut bytes: Vec<u8> = vec![];
            let mut current = 0;
            while let Ok(req) = requests.recv() {
                match req {
                    Request::Stop => break,
                    Request::Open {
                        generation,
                        path,
                        password,
                    } => {
                        if generation != gen.load(Ordering::Relaxed) {
                            continue;
                        }
                        // SAFETY: no page/text handles escape any worker request.
                        if !doc.is_null() {
                            unsafe { (engine.close)(doc) };
                            doc = null_mut();
                        }
                        bytes.clear();
                        current = generation;
                        let result = (|| {
                            let meta = std::fs::metadata(&path)
                                .map_err(|e| format!("无法读取文件：{e}"))?;
                            if meta.len() > 512 * 1024 * 1024 {
                                return Err("文件超过首版 512 MB 上限。".to_string());
                            }
                            bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            let pass = CString::new(password)
                                .map_err(|_| "密码包含无效字符".to_string())?;
                            // SAFETY: bytes remains allocated and unchanged until the document closes; password is NUL terminated.
                            doc = unsafe {
                                (engine.load)(bytes.as_ptr().cast(), bytes.len(), pass.as_ptr())
                            };
                            if doc.is_null() {
                                return Err(
                                    "无法打开 PDF：文件可能已损坏、格式不支持或需要密码。".into()
                                );
                            }
                            let count = unsafe { (engine.count)(doc) };
                            if !(1..=20000).contains(&count) {
                                return Err("PDF 页数无效或超过 20,000 页。".into());
                            }
                            let mut sizes = Vec::with_capacity(count as usize);
                            for i in 0..count {
                                if generation != gen.load(Ordering::Relaxed) {
                                    return Err("已切换文档".into());
                                }
                                unsafe {
                                    let page = (engine.page)(doc, i);
                                    if page.is_null() {
                                        return Err(format!("无法解析第 {} 页", i + 1));
                                    }
                                    let w = (engine.width)(page);
                                    let h = (engine.height)(page);
                                    (engine.close_page)(page);
                                    if !w.is_finite() || !h.is_finite() || w <= 0. || h <= 0. {
                                        return Err("页面尺寸无效".into());
                                    }
                                    sizes.push((w, h));
                                }
                            }
                            Ok((sizes, meta.len()))
                        })();
                        match result {
                            Ok((sizes, size)) => {
                                let _ = responses.send(Response::Loaded {
                                    generation,
                                    path,
                                    sizes,
                                    bytes: size,
                                });
                            }
                            Err(message) => {
                                let _ = responses.send(Response::Error {
                                    generation,
                                    message,
                                });
                            }
                        }
                    }
                    Request::Render { generation, key } => {
                        if doc.is_null()
                            || generation != current
                            || generation != gen.load(Ordering::Relaxed)
                        {
                            continue;
                        }
                        // SAFETY: page and bitmap handles are local, dimensions and allocation sizes bounded before FFI.
                        let result = unsafe {
                            let page = (engine.page)(doc, key.page as i32);
                            if page.is_null() {
                                None
                            } else {
                                let (w, h) = ((engine.width)(page), (engine.height)(page));
                                let ratio = if key.rotation % 2 == 0 { h / w } else { w / h };
                                let width = key
                                    .width
                                    .clamp(40, 4096)
                                    .min((8_000_000.0 / ratio.max(0.01)).sqrt() as i32)
                                    .max(1);
                                let height = (width as f64 * ratio).round().clamp(1., 8192.) as i32;
                                let mut data = vec![255u8; width as usize * height as usize * 4];
                                let bitmap = (engine.bitmap)(
                                    width,
                                    height,
                                    4,
                                    data.as_mut_ptr().cast(),
                                    width * 4,
                                );
                                if bitmap.is_null() {
                                    (engine.close_page)(page);
                                    None
                                } else {
                                    (engine.fill)(bitmap, 0, 0, width, height, 0xffffffff);
                                    (engine.render)(
                                        bitmap,
                                        page,
                                        0,
                                        0,
                                        width,
                                        height,
                                        key.rotation,
                                        1 | 2,
                                    );
                                    (engine.close_bitmap)(bitmap);
                                    (engine.close_page)(page);
                                    Some(Pixels {
                                        width,
                                        height,
                                        data,
                                    })
                                }
                            }
                        };
                        if generation == gen.load(Ordering::Relaxed) {
                            let response = match result {
                                Some(pixels) => Response::Image {
                                    generation,
                                    key,
                                    pixels,
                                },
                                None => Response::RenderError { generation, key },
                            };
                            if responses.send(response).is_err() {
                                break;
                            }
                        }
                    }
                    Request::Search {
                        generation,
                        token: request_token,
                        query,
                    } => {
                        if doc.is_null() || generation != current {
                            continue;
                        }
                        let mut pages = vec![];
                        let needle = query.to_lowercase();
                        if needle.is_empty() {
                            continue;
                        }
                        // SAFETY: text buffers sized from PDFium's count and all handles released in this scope.
                        unsafe {
                            for i in 0..(engine.count)(doc) {
                                if generation != gen.load(Ordering::Relaxed)
                                    || request_token != token.load(Ordering::Relaxed)
                                {
                                    break;
                                }
                                let page = (engine.page)(doc, i);
                                if page.is_null() {
                                    continue;
                                }
                                let tp = (engine.text_page)(page);
                                if !tp.is_null() {
                                    let count = (engine.text_count)(tp);
                                    if (0..=5_000_000).contains(&count) {
                                        let mut text = vec![0u16; count as usize + 1];
                                        let n = (engine.text)(tp, 0, count, text.as_mut_ptr());
                                        if n > 0
                                            && String::from_utf16_lossy(
                                                &text[..(n as usize)
                                                    .saturating_sub(1)
                                                    .min(text.len())],
                                            )
                                            .to_lowercase()
                                            .contains(&needle)
                                        {
                                            pages.push(i as usize)
                                        }
                                    }
                                    (engine.close_text)(tp);
                                }
                                (engine.close_page)(page);
                            }
                        }
                        if generation == gen.load(Ordering::Relaxed)
                            && request_token == token.load(Ordering::Relaxed)
                        {
                            let _ = responses.send(Response::Matches {
                                generation,
                                token: request_token,
                                pages,
                            });
                        }
                    }
                }
            }
            if !doc.is_null() {
                unsafe { (engine.close)(doc) }
            }
            drop(bytes);
            drop(engine);
        });
        Self {
            tx,
            rx,
            generation,
            search,
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.try_send(Request::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn render_search_bad_file_and_reopen() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let worker = Worker::start(root.join("vendor/bin/pdfium.dll"));
        let fixture = root.join("test-output/多页阅读测试.pdf");
        worker.generation.store(1, Ordering::Relaxed);
        worker
            .tx
            .send(Request::Open {
                generation: 1,
                path: fixture.clone(),
                password: String::new(),
            })
            .expect("worker open");
        match worker
            .rx
            .recv_timeout(Duration::from_secs(20))
            .expect("load response")
        {
            Response::Loaded { sizes, .. } => assert_eq!(sizes.len(), 7),
            _ => panic!("valid PDF did not load"),
        }
        let key = Key {
            page: 0,
            width: 600,
            rotation: 0,
        };
        worker
            .tx
            .send(Request::Render { generation: 1, key })
            .expect("render");
        match worker
            .rx
            .recv_timeout(Duration::from_secs(20))
            .expect("pixels")
        {
            Response::Image { pixels, .. } => {
                assert_eq!(pixels.width, 600);
                assert!(pixels.data.iter().filter(|&&v| v < 220).count() > 10000);
            }
            _ => panic!("blank/missing image"),
        }
        worker.search.store(1, Ordering::Relaxed);
        worker
            .tx
            .send(Request::Search {
                generation: 1,
                token: 1,
                query: "leafpdf".into(),
            })
            .expect("search");
        match worker
            .rx
            .recv_timeout(Duration::from_secs(20))
            .expect("matches")
        {
            Response::Matches { pages, .. } => assert_eq!(pages, (0..7).collect::<Vec<_>>()),
            _ => panic!("search failed"),
        }
        worker.generation.store(2, Ordering::Relaxed);
        worker
            .tx
            .send(Request::Open {
                generation: 2,
                path: root.join("test-output/invalid.pdf"),
                password: String::new(),
            })
            .expect("invalid open");
        assert!(matches!(
            worker.rx.recv_timeout(Duration::from_secs(20)),
            Ok(Response::Error { .. })
        ));
        worker.generation.store(3, Ordering::Relaxed);
        worker
            .tx
            .send(Request::Open {
                generation: 3,
                path: fixture,
                password: String::new(),
            })
            .expect("reopen");
        assert!(matches!(
            worker.rx.recv_timeout(Duration::from_secs(20)),
            Ok(Response::Loaded { generation: 3, .. })
        ));
    }
}
