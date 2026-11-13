//! Small, audited binding to the bundled PDFium C API. All calls are serialized:
//! PDFium is not thread safe. Documents, pages, text pages and bitmaps are RAII-owned.
use crate::config;
use anyhow::{Context, Result, bail};
use libloading::Library;
use std::{
    ffi::{c_char, c_int, c_uint, c_ulong, c_void},
    io::Write,
    path::PathBuf,
    ptr,
    sync::Mutex,
};

static PDF_LOCK: Mutex<()> = Mutex::new(());
static PDFIUM: &[u8] = include_bytes!("../assets/bin/pdfium.dll");
type Handle = *mut c_void;

macro_rules! api {
    ($($field:ident: $symbol:literal => $ty:ty),* $(,)?) => {
        struct Api { $($field:$ty,)* _library:Library }
        impl Api {
            unsafe fn load(path:PathBuf)->Result<Self> {
                let library=unsafe{Library::new(path)}.context("Не удалось загрузить встроенный PDFium")?;
                Ok(Self { $($field:unsafe{*library.get::<$ty>(concat!($symbol,"\0").as_bytes())?},)* _library:library })
            }
        }
    }
}
api! {
    init:"FPDF_InitLibrary" => unsafe extern "system" fn(),
    destroy:"FPDF_DestroyLibrary" => unsafe extern "system" fn(),
    load_doc:"FPDF_LoadMemDocument64" => unsafe extern "system" fn(*const c_void,usize,*const c_char)->Handle,
    close_doc:"FPDF_CloseDocument" => unsafe extern "system" fn(Handle),
    error:"FPDF_GetLastError" => unsafe extern "system" fn()->c_ulong,
    page_count:"FPDF_GetPageCount" => unsafe extern "system" fn(Handle)->c_int,
    load_page:"FPDF_LoadPage" => unsafe extern "system" fn(Handle,c_int)->Handle,
    close_page:"FPDF_ClosePage" => unsafe extern "system" fn(Handle),
    width:"FPDF_GetPageWidthF" => unsafe extern "system" fn(Handle)->f32,
    height:"FPDF_GetPageHeightF" => unsafe extern "system" fn(Handle)->f32,
    load_text:"FPDFText_LoadPage" => unsafe extern "system" fn(Handle)->Handle,
    count_chars:"FPDFText_CountChars" => unsafe extern "system" fn(Handle)->c_int,
    get_text:"FPDFText_GetText" => unsafe extern "system" fn(Handle,c_int,c_int,*mut u16)->c_int,
    unicode:"FPDFText_GetUnicode" => unsafe extern "system" fn(Handle,c_int)->c_uint,
    origin:"FPDFText_GetCharOrigin" => unsafe extern "system" fn(Handle,c_int,*mut f64,*mut f64)->c_int,
    char_box:"FPDFText_GetCharBox" => unsafe extern "system" fn(Handle,c_int,*mut f64,*mut f64,*mut f64,*mut f64)->c_int,
    close_text:"FPDFText_ClosePage" => unsafe extern "system" fn(Handle),
    bitmap_create:"FPDFBitmap_Create" => unsafe extern "system" fn(c_int,c_int,c_int)->Handle,
    bitmap_fill:"FPDFBitmap_FillRect" => unsafe extern "system" fn(Handle,c_int,c_int,c_int,c_int,c_ulong)->c_int,
    bitmap_render:"FPDF_RenderPageBitmap" => unsafe extern "system" fn(Handle,Handle,c_int,c_int,c_int,c_int,c_int,c_int),
    bitmap_buffer:"FPDFBitmap_GetBuffer" => unsafe extern "system" fn(Handle)->*const u8,
    bitmap_stride:"FPDFBitmap_GetStride" => unsafe extern "system" fn(Handle)->c_int,
    bitmap_destroy:"FPDFBitmap_Destroy" => unsafe extern "system" fn(Handle),
    render_printer:"FPDF_RenderPage" => unsafe extern "system" fn(Handle,Handle,c_int,c_int,c_int,c_int,c_int,c_int),
}

fn runtime_path() -> Result<PathBuf> {
    let dir = config::data_dir().join("runtime");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("pdfium-8152-ba884cc5.dll");
    if std::fs::read(&path).ok().as_deref() != Some(PDFIUM) {
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)?;
        tmp.write_all(PDFIUM)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path)
            .map_err(|e| e.error)
            .context("Не удалось подготовить встроенный PDF-движок")?;
    }
    Ok(path)
}
struct Document<'a> {
    api: Api,
    doc: Handle,
    _bytes: &'a [u8],
}
impl<'a> Document<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self> {
        let api = unsafe { Api::load(runtime_path()?)? };
        unsafe {
            (api.init)();
        }
        let doc = unsafe { (api.load_doc)(bytes.as_ptr().cast(), bytes.len(), ptr::null()) };
        if doc.is_null() {
            let error = unsafe { (api.error)() };
            unsafe {
                (api.destroy)();
            }
            bail!(
                "Не удалось открыть PDF (код {error}). Файл может быть повреждён или защищён паролем."
            );
        }
        Ok(Self {
            api,
            doc,
            _bytes: bytes,
        })
    }
    fn page(&self, n: usize) -> Result<Page<'_>> {
        let page = unsafe { (self.api.load_page)(self.doc, n as i32) };
        if page.is_null() {
            bail!("Не удалось прочитать страницу {}", n + 1);
        }
        Ok(Page {
            api: &self.api,
            page,
        })
    }
    fn count(&self) -> usize {
        unsafe { (self.api.page_count)(self.doc).max(0) as usize }
    }
}
impl Drop for Document<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.close_doc)(self.doc);
            (self.api.destroy)();
        }
    }
}
struct Page<'a> {
    api: &'a Api,
    page: Handle,
}
impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.close_page)(self.page);
        }
    }
}
struct Text<'a> {
    api: &'a Api,
    text: Handle,
}
impl Drop for Text<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.close_text)(self.text);
        }
    }
}
struct Bitmap<'a> {
    api: &'a Api,
    bitmap: Handle,
}
impl Drop for Bitmap<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.bitmap_destroy)(self.bitmap);
        }
    }
}

#[derive(Debug)]
pub struct Preview {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
pub struct Inspection {
    pub text: String,
    pub pages: usize,
    pub preview: Preview,
}

pub fn inspect(bytes: &[u8]) -> Result<Inspection> {
    let _lock = PDF_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let doc = Document::open(bytes)?;
    let (text, pages) = document_text(&doc)?;
    let preview = render(&doc, 0)?;
    Ok(Inspection {
        text,
        pages,
        preview,
    })
}

/// The archive organizer needs text only, without allocating/rendering a preview.
pub fn extract_text(bytes: &[u8]) -> Result<String> {
    let _lock = PDF_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let doc = Document::open(bytes)?;
    document_text(&doc).map(|(text, _)| text)
}

fn document_text(doc: &Document<'_>) -> Result<(String, usize)> {
    let pages = doc.count();
    if pages == 0 || pages > 200 {
        bail!("В PDF {pages} страниц. Поддерживаются карточки от 1 до 200 страниц.");
    }
    let mut text = String::new();
    for n in 0..pages {
        let page = doc.page(n)?;
        let tp = unsafe { (doc.api.load_text)(page.page) };
        if tp.is_null() {
            continue;
        }
        let tp = Text {
            api: &doc.api,
            text: tp,
        };
        let count = unsafe { (doc.api.count_chars)(tp.text) };
        if !(0..=2_000_000).contains(&count) {
            bail!("Слишком много текста в PDF.");
        }
        let mut buf = vec![0u16; count as usize + 1];
        let len =
            unsafe { (doc.api.get_text)(tp.text, 0, count, buf.as_mut_ptr()) }.max(0) as usize;
        let len = len.min(buf.len());
        let raw = String::from_utf16_lossy(&buf[..len])
            .trim_end_matches('\0')
            .replace("\r\n", "\n");
        let ordered = ordered_text(&doc.api, tp.text, count);
        text.push_str(
            if ordered.chars().count()
                >= raw.chars().filter(|c| !c.is_whitespace()).count() * 9 / 10
            {
                &ordered
            } else {
                &raw
            },
        );
        text.push('\n');
    }
    Ok((text, pages))
}
/// PDF objects may store table labels first and all values afterwards. Reconstruct
/// visual reading order from character baselines to keep dates and counters together.
fn ordered_text(api: &Api, tp: Handle, count: i32) -> String {
    struct Glyph {
        x: f64,
        y: f64,
        right: f64,
        c: char,
    }
    let mut glyphs = Vec::new();
    for i in 0..count {
        let Some(c) = char::from_u32(unsafe { (api.unicode)(tp, i) }) else {
            continue;
        };
        if c.is_control() {
            continue;
        }
        let (mut x, mut y) = (0.0, 0.0);
        if unsafe { (api.origin)(tp, i, &mut x, &mut y) } == 0 || !x.is_finite() || !y.is_finite() {
            continue;
        }
        let (mut left, mut right, mut bottom, mut top) = (x, x, y, y);
        unsafe {
            (api.char_box)(tp, i, &mut left, &mut right, &mut bottom, &mut top);
        }
        glyphs.push(Glyph { x, y, right, c });
    }
    glyphs.sort_by(|a, b| b.y.total_cmp(&a.y).then_with(|| a.x.total_cmp(&b.x)));
    let mut lines: Vec<(f64, Vec<Glyph>)> = Vec::new();
    for glyph in glyphs {
        if let Some((y, line)) = lines.last_mut()
            && (*y - glyph.y).abs() < 2.2
        {
            line.push(glyph);
            continue;
        }
        lines.push((glyph.y, vec![glyph]));
    }
    let mut text = String::new();
    for (_, mut line) in lines {
        line.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut right = None;
        for g in line {
            if right.is_some_and(|r| g.x - r > 2.5)
                && !text.ends_with(char::is_whitespace)
                && !g.c.is_whitespace()
            {
                text.push(' ');
            }
            text.push(g.c);
            right = Some(g.right);
        }
        text.push('\n');
    }
    text
}
fn render(doc: &Document<'_>, n: usize) -> Result<Preview> {
    let page = doc.page(n)?;
    let w = unsafe { (doc.api.width)(page.page) } as f64;
    let h = unsafe { (doc.api.height)(page.page) } as f64;
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        bail!("Некорректный размер страницы.");
    }
    let scale = (850.0 / w).min(1200.0 / h);
    let width = (w * scale).round().max(1.0) as usize;
    let height = (h * scale).round().max(1.0) as usize;
    let bitmap = unsafe { (doc.api.bitmap_create)(width as i32, height as i32, 1) };
    if bitmap.is_null() {
        bail!("Не удалось создать предпросмотр.");
    }
    let bitmap = Bitmap {
        api: &doc.api,
        bitmap,
    };
    unsafe {
        (doc.api.bitmap_fill)(bitmap.bitmap, 0, 0, width as i32, height as i32, 0xffffffff);
        (doc.api.bitmap_render)(
            bitmap.bitmap,
            page.page,
            0,
            0,
            width as i32,
            height as i32,
            0,
            1,
        );
    }
    let stride = unsafe { (doc.api.bitmap_stride)(bitmap.bitmap) };
    let raw = unsafe { (doc.api.bitmap_buffer)(bitmap.bitmap) };
    if raw.is_null() || stride < width as i32 * 4 {
        bail!("Не удалось получить предпросмотр.");
    }
    let pixels = unsafe { std::slice::from_raw_parts(raw, stride as usize * height) };
    let mut rgba = vec![0; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let a = y * stride as usize + x * 4;
            let b = (y * width + x) * 4;
            rgba[b..b + 4].copy_from_slice(&[pixels[a + 2], pixels[a + 1], pixels[a], 255]);
        }
    }
    Ok(Preview {
        width,
        height,
        rgba,
    })
}
pub fn preview(bytes: &[u8], page: usize) -> Result<Preview> {
    let _lock = PDF_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let doc = Document::open(bytes)?;
    if page >= doc.count() {
        bail!("Страница отсутствует.");
    }
    render(&doc, page)
}

pub fn default_printer() -> Result<String> {
    use windows_sys::Win32::Graphics::Printing::GetDefaultPrinterW;
    let mut len = 0u32;
    unsafe {
        GetDefaultPrinterW(ptr::null_mut(), &mut len);
    }
    if len == 0 {
        bail!("В Windows не выбран принтер по умолчанию.");
    }
    let mut buf = vec![0u16; len as usize];
    if unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut len) } == 0 {
        return Err(std::io::Error::last_os_error()).context("Не удалось получить принтер");
    }
    Ok(String::from_utf16_lossy(
        &buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())],
    ))
}
pub fn print(bytes: &[u8], title: &str) -> Result<String> {
    print_document(bytes, title, &default_printer()?, None)
}
/// Diagnostics can use an explicit virtual printer and output file without
/// changing the user's default printer or sending a job to a physical device.
pub fn print_document(
    bytes: &[u8],
    title: &str,
    printer: &str,
    output: Option<&std::path::Path>,
) -> Result<String> {
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::Storage::Xps::{
        AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage,
    };
    let _lock = PDF_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let doc = Document::open(bytes)?;
    let driver: Vec<u16> = "WINSPOOL\0".encode_utf16().collect();
    let printer_w: Vec<u16> = printer.encode_utf16().chain(Some(0)).collect();
    let title_w: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let output_w = output.map(|p| {
        p.as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>()
    });
    let dc = unsafe {
        CreateDCW(
            driver.as_ptr(),
            printer_w.as_ptr(),
            ptr::null(),
            ptr::null(),
        )
    };
    if dc.is_null() {
        return Err(std::io::Error::last_os_error()).context("Не удалось открыть принтер");
    }
    struct PrinterDc {
        dc: HDC,
        active: bool,
    }
    impl Drop for PrinterDc {
        fn drop(&mut self) {
            unsafe {
                if self.active {
                    AbortDoc(self.dc);
                }
                DeleteDC(self.dc);
            }
        }
    }
    let mut guard = PrinterDc { dc, active: false };
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: title_w.as_ptr(),
        lpszOutput: output_w.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
        lpszDatatype: ptr::null(),
        fwType: 0,
    };
    let job = unsafe { StartDocW(dc, &info) };
    if job <= 0 {
        return Err(std::io::Error::last_os_error()).context("Принтер не принял задание");
    }
    guard.active = true;
    let pw = unsafe { GetDeviceCaps(dc, HORZRES as i32) };
    let ph = unsafe { GetDeviceCaps(dc, VERTRES as i32) };
    if pw <= 0 || ph <= 0 {
        bail!("Принтер сообщил некорректный размер страницы.");
    }
    for i in 0..doc.count() {
        let page = doc.page(i)?;
        let w = unsafe { (doc.api.width)(page.page) } as f64;
        let h = unsafe { (doc.api.height)(page.page) } as f64;
        if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() {
            bail!("Некорректный размер страницы PDF.");
        }
        let upright = (pw as f64 / w).min(ph as f64 / h);
        let rotated = (pw as f64 / h).min(ph as f64 / w);
        let (rw, rh, rotation) = if rotated > upright * 1.05 {
            ((h * rotated) as i32, (w * rotated) as i32, 1)
        } else {
            ((w * upright) as i32, (h * upright) as i32, 0)
        };
        if unsafe { StartPage(dc) } <= 0 {
            return Err(std::io::Error::last_os_error())
                .context("Не удалось начать печать страницы");
        }
        unsafe {
            (doc.api.render_printer)(
                dc,
                page.page,
                (pw - rw) / 2,
                (ph - rh) / 2,
                rw,
                rh,
                rotation,
                0x801,
            );
        }
        if unsafe { EndPage(dc) } <= 0 {
            return Err(std::io::Error::last_os_error())
                .context("Ошибка передачи страницы принтеру");
        }
    }
    if unsafe { EndDoc(dc) } <= 0 {
        return Err(std::io::Error::last_os_error()).context("Не удалось завершить задание печати");
    }
    guard.active = false;
    Ok(format!("Передано в очередь «{printer}», задание №{job}"))
}
