use eframe::egui::{self, Color32, RichText};
use kkt_saver::{
    archive,
    card::{self, Card},
    config::{Config, data_dir},
    folders,
    indexing::{self, Indexer},
    pdf,
    search::{Match, SearchIndex},
    storage,
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
mod native_window;
#[cfg(debug_assertions)]
mod review;
mod theme;
mod ui;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorTab {
    Details,
    Text,
}

struct Loaded {
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    hash: [u8; 32],
    card: Card,
    text: String,
    pages: usize,
    preview: Option<pdf::Preview>,
}
struct Report {
    saved: storage::Saved,
    printing: Option<Result<String, String>>,
}
struct NewFolder {
    id: u64,
    parent: String,
    name: String,
    error: String,
    picking_parent: bool,
    creating: bool,
    focus_name: bool,
}
#[derive(Default)]
struct PendingIndex {
    index: Option<Arc<SearchIndex>>,
    status: String,
    scanning: bool,
    dirty: bool,
}
impl PendingIndex {
    fn apply(&mut self, update: indexing::Update) {
        self.dirty = true;
        match update {
            indexing::Update::Started => {
                self.scanning = true;
                self.status = "Обновляю индекс папок в фоне…".into();
            }
            indexing::Update::Snapshot(index, status) => {
                self.status = status;
                if index.is_some() {
                    self.index = index;
                }
            }
            indexing::Update::Failed(error) => {
                self.scanning = false;
                self.status = error;
            }
            indexing::Update::Finished => self.scanning = false,
        }
    }
    fn take(&mut self) -> Option<Self> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some(Self {
            index: self.index.take(),
            status: self.status.clone(),
            scanning: self.scanning,
            dirty: false,
        })
    }
}
enum Event {
    Loaded(u64, Result<Box<Loaded>, String>),
    Preview(u64, usize, Result<pdf::Preview, String>),
    Matches(u64, Vec<Match>, u128),
    Source(Option<PathBuf>),
    Destination(Option<PathBuf>),
    Root(Option<PathBuf>),
    FolderParent(u64, Option<PathBuf>),
    FolderCreated(u64, Result<PathBuf, String>),
    Saved(Result<Report, String>),
    Printed(Result<String, String>),
    Show,
    Quit,
}
struct SaverApp {
    config: Config,
    root_edit: String,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    ctx: egui::Context,
    loaded: Option<Loaded>,
    pending: VecDeque<PathBuf>,
    loading: bool,
    load_id: u64,
    texture: Option<egui::TextureHandle>,
    page: usize,
    preview_loading: bool,
    index: Arc<SearchIndex>,
    scanning: bool,
    indexer: Option<Indexer>,
    index_updates: Arc<Mutex<PendingIndex>>,
    index_status: String,
    choosing: bool,
    query: String,
    query_inn: String,
    matches: Vec<Match>,
    selected: Option<PathBuf>,
    manual_destination: bool,
    new_folder: Option<NewFolder>,
    folder_sequence: u64,
    query_id: u64,
    query_due: Option<Instant>,
    searching: bool,
    search_ms: u128,
    filename: String,
    save_registration: String,
    save_date: String,
    busy: bool,
    saved: bool,
    last_saved: Option<PathBuf>,
    print_retry: bool,
    status: String,
    error: bool,
    settings_open: bool,
    tray: Option<TrayIcon>,
    hidden_to_tray: Arc<AtomicBool>,
    native_window: Option<native_window::NativeWindow>,
    exiting: bool,
    printer: String,
    editor_tab: EditorTab,
    documents_open: bool,
    preview_open: bool,
}
pub fn run(path: Option<PathBuf>) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([1040.0, 720.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(egui::IconData {
                rgba: icon_rgba(),
                width: 32,
                height: 32,
            })),
        ..Default::default()
    };
    eframe::run_native(
        "Карточки ККТ",
        options,
        Box::new(move |cc| Ok(Box::new(SaverApp::new(cc, path)))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}
fn icon_rgba() -> Vec<u8> {
    let mut pixels = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let paper = (7..25).contains(&x) && (4..28).contains(&y);
            let line = (10..22).contains(&x) && [10, 11, 16, 17, 22, 23].contains(&y);
            let c = if line {
                [37, 67, 94, 255]
            } else if paper {
                [245, 248, 251, 255]
            } else {
                [37, 101, 146, 255]
            };
            pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4].copy_from_slice(&c);
        }
    }
    pixels
}
fn dispatch_tray_action(
    event: Event,
    tx: &Sender<Event>,
    ctx: &egui::Context,
    window: native_window::NativeWindow,
    hidden_to_tray: &AtomicBool,
) {
    // Queue first: ShowWindow can deliver native messages synchronously. Restore
    // with Win32 before waiting for an egui frame: hidden HWNDs do not repaint.
    if tx.send(event).is_ok() {
        hidden_to_tray.store(false, Ordering::Release);
        window.restore();
        ctx.request_repaint();
    }
}
fn build_tray(
    tx: Sender<Event>,
    ctx: egui::Context,
    window: native_window::NativeWindow,
    hidden_to_tray: Arc<AtomicBool>,
) -> anyhow::Result<TrayIcon> {
    let menu = Menu::new();
    let open = MenuItem::new("Открыть карточки ККТ", true, None);
    let quit = MenuItem::new("Выйти из программы", true, None);
    menu.append_items(&[&open, &PredefinedMenuItem::separator(), &quit])?;
    let open_id = open.id().clone();
    let quit_id = quit.id().clone();
    let ctx1 = ctx.clone();
    let tx1 = tx.clone();
    let hidden1 = hidden_to_tray.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == open_id {
            dispatch_tray_action(Event::Show, &tx1, &ctx1, window, &hidden1);
        } else if e.id == quit_id {
            dispatch_tray_action(Event::Quit, &tx1, &ctx1, window, &hidden1);
        }
    }));
    let hidden2 = hidden_to_tray;
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if matches!(
            e,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        ) {
            dispatch_tray_action(Event::Show, &tx, &ctx, window, &hidden2);
        }
    }));
    Ok(TrayIconBuilder::new()
        .with_tooltip("Карточки ККТ — нажмите, чтобы открыть")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(tray_icon::Icon::from_rgba(icon_rgba(), 32, 32)?)
        .build()?)
}
impl SaverApp {
    fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let config = Config::load();
        let root_edit = config.root.display().to_string();
        let ctx = cc.egui_ctx.clone();
        let hidden_to_tray = Arc::new(AtomicBool::new(false));
        let native = native_window::NativeWindow::from_context(cc);
        if let Ok(window) = native.as_ref() {
            window.dark_titlebar();
        }
        let native_window = native.as_ref().ok().copied();
        let (tray, status, error) = match native
            .and_then(|window| build_tray(tx.clone(), ctx.clone(), window, hidden_to_tray.clone()))
        {
            Ok(t) => (
                Some(t),
                "Перетащите PDF-карточку в окно или нажмите «Открыть PDF».".into(),
                false,
            ),
            Err(e) => (
                None,
                format!("Трей недоступен: {e}. Закрытие окна завершит программу."),
                true,
            ),
        };
        let mut app = Self {
            config,
            root_edit,
            tx,
            rx,
            ctx,
            loaded: None,
            pending: VecDeque::new(),
            loading: false,
            load_id: 0,
            texture: None,
            page: 0,
            preview_loading: false,
            index: Arc::new(SearchIndex::new(Vec::new())),
            scanning: false,
            indexer: None,
            index_updates: Arc::new(Mutex::new(PendingIndex::default())),
            index_status: String::new(),
            choosing: false,
            query: String::new(),
            query_inn: String::new(),
            matches: Vec::new(),
            selected: None,
            manual_destination: false,
            new_folder: None,
            folder_sequence: 0,
            query_id: 0,
            query_due: None,
            searching: false,
            search_ms: 0,
            filename: String::new(),
            save_registration: String::new(),
            save_date: String::new(),
            busy: false,
            saved: false,
            last_saved: None,
            print_retry: false,
            status,
            error,
            settings_open: false,
            tray,
            hidden_to_tray,
            native_window,
            exiting: false,
            printer: pdf::default_printer()
                .unwrap_or_else(|_| "Принтер по умолчанию не выбран".into()),
            editor_tab: EditorTab::Details,
            documents_open: true,
            preview_open: true,
        };
        app.start_scan();
        #[cfg(debug_assertions)]
        app.start_tray_check();
        if let Some(path) = path {
            app.open(path);
        }
        app
    }
    fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }
    fn persist(&mut self) {
        if let Err(e) = self.config.save() {
            self.notify(format!("Не удалось сохранить настройки: {e}"), true);
        }
    }
    fn spawn(&self, name: &str, work: impl FnOnce(Sender<Event>) + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let hidden_to_tray = self.hidden_to_tray.clone();
        let _ = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                work(tx);
                if !hidden_to_tray.load(Ordering::Acquire) {
                    ctx.request_repaint();
                }
            });
    }
    fn open(&mut self, path: PathBuf) {
        if !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("pdf"))
        {
            self.notify(
                "Откройте файл PDF с карточкой регистрации или снятия ККТ.",
                true,
            );
            return;
        }
        if self.busy || self.loading {
            self.pending.push_back(path);
            return;
        }
        self.load_id += 1;
        self.new_folder = None;
        self.loaded = None;
        self.texture = None;
        self.last_saved = None;
        let id = self.load_id;
        self.loading = true;
        self.choosing = false;
        self.saved = false;
        self.editor_tab = EditorTab::Details;
        self.print_retry = false;
        self.notify(
            format!(
                "Читаю {}…",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            false,
        );
        self.spawn("read-pdf", move |tx| {
            let result = (|| -> anyhow::Result<Loaded> {
                let metadata = std::fs::metadata(&path)?;
                if metadata.len() > 100 * 1024 * 1024 {
                    anyhow::bail!("Файл больше 100 МБ. Выберите карточку ККТ.");
                }
                let bytes = Arc::new(std::fs::read(&path)?);
                let hash = storage::digest(&bytes);
                let info = pdf::inspect(&bytes)?;
                Ok(Loaded {
                    path,
                    hash,
                    card: Card::parse(&info.text),
                    bytes,
                    text: info.text,
                    pages: info.pages,
                    preview: Some(info.preview),
                })
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Loaded(id, result.map(Box::new)));
        });
    }
    fn open_queued(&mut self, index: usize) {
        if self.busy || self.loading || self.new_folder.is_some() {
            return;
        }
        let Some(path) = self.pending.remove(index) else {
            return;
        };
        // Switching to another pending card keeps the current unsaved card available.
        if !self.saved
            && let Some(loaded) = &self.loaded
            && loaded.path != path
        {
            self.pending.push_front(loaded.path.clone());
        }
        self.open(path);
    }

    fn advance_queue(&mut self) {
        if self.config.auto_next
            && self.saved
            && !self.busy
            && !self.loading
            && let Some(path) = self.pending.pop_front()
        {
            self.open(path);
        }
    }

    fn start_scan(&mut self) {
        if let Some(indexer) = &self.indexer
            && indexer.root() == self.config.root
        {
            self.scanning = true;
            indexer.refresh();
            return;
        }
        // Dropping the previous worker cancels its scan and wakes its timer.
        self.indexer = None;
        self.index_updates = Arc::new(Mutex::new(PendingIndex::default()));
        let updates = self.index_updates.clone();
        let root = self.config.root.clone();
        let ctx = self.ctx.clone();
        let hidden_to_tray = self.hidden_to_tray.clone();
        self.scanning = true;
        self.index_status = "Читаю индекс и проверяю архив…".into();
        match Indexer::start(root, data_dir(), move |update| {
            // Keep only the latest index while the window is hidden. Repeated
            // scans must not accumulate full indexes in the UI event queue.
            if let Ok(mut pending) = updates.lock() {
                pending.apply(update);
            }
            if !hidden_to_tray.load(Ordering::Acquire) {
                ctx.request_repaint();
            }
        }) {
            Ok(indexer) => self.indexer = Some(indexer),
            Err(e) => {
                self.scanning = false;
                self.index_status = format!("Не удалось запустить индексацию: {e}");
            }
        }
    }
    fn schedule_search(&mut self) {
        self.query_id += 1;
        self.query_due = Some(Instant::now() + Duration::from_millis(160));
    }
    fn search_now(&mut self) {
        self.query_due = None;
        let id = self.query_id;
        let index = self.index.clone();
        let query = self.query.clone();
        let mut card = self
            .loaded
            .as_ref()
            .map(|l| l.card.clone())
            .unwrap_or_default();
        card.inn = self.query_inn.trim().to_string();
        let choices = self.config.choices.clone();
        self.searching = true;
        self.spawn("folder-search", move |tx| {
            let start = Instant::now();
            let matches = index.search(&card, &query, &choices);
            let _ = tx.send(Event::Matches(id, matches, start.elapsed().as_millis()));
        });
    }
    fn choose(&mut self) {
        if self.saved || self.new_folder.is_some() {
            return;
        }
        if let Some(loaded) = &self.loaded {
            self.query = loaded.card.organization.clone();
            self.query_inn = loaded.card.inn.clone();
            self.choosing = true;
            self.selected = None;
            self.matches.clear();
            self.manual_destination = false;
            self.schedule_search();
        }
    }
    fn perform_save(&mut self) {
        if self.busy || self.new_folder.is_some() {
            return;
        }
        let Some(loaded) = &self.loaded else { return };
        let Some(selected) = self.selected.clone() else {
            return;
        };
        let mut card = loaded.card.clone();
        card.registration = card::compact_registration(&self.save_registration);
        let Some(date) = kkt_saver::dates::CardDate::parse(&self.save_date) else {
            self.notify("Укажите дату карточки из PDF в формате ДД.ММ.ГГГГ. Она нужна для даты создания и изменения файла.", true);
            return;
        };
        let folder = match archive::destination(&self.config.root, &selected, &card) {
            Ok(folder) => folder,
            Err(e) => {
                self.notify(e.to_string(), true);
                return;
            }
        };
        if let Err(e) = storage::validate_name(&self.filename) {
            self.notify(e.to_string(), true);
            return;
        }
        let source = loaded.path.clone();
        let hash = loaded.hash;
        let name = self.filename.clone();
        let bytes = loaded.bytes.clone();
        let print = self.config.print;
        let move_source = self.config.move_source;
        self.busy = true;
        self.choosing = false;
        self.notify("Сохраняю и проверяю карточку…", false);
        self.persist();
        self.spawn("save-card", move |tx| {
            let result = (|| -> anyhow::Result<_> {
                let parent = folder
                    .parent()
                    .and_then(|p| p.parent())
                    .ok_or_else(|| anyhow::anyhow!("Не определена папка архива"))?;
                if !parent.is_dir() {
                    anyhow::bail!("Папка архива недоступна: {}", parent.display());
                }
                archive::ensure_directory(&folder)?;
                storage::save_dated(&source, hash, &folder, &name, move_source, date)
            })()
            .map(|saved| {
                let printing =
                    print.then(|| pdf::print(&bytes, &name).map_err(|e| format!("{e:#}")));
                Report { saved, printing }
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::Saved(result));
        });
    }
    fn retry_print(&mut self) {
        if self.busy {
            return;
        }
        if let Some(loaded) = &self.loaded {
            let bytes = loaded.bytes.clone();
            let name = self.filename.clone();
            self.busy = true;
            self.notify("Отправляю карточку на печать…", false);
            self.spawn("print-card", move |tx| {
                let _ = tx.send(Event::Printed(
                    pdf::print(&bytes, &name).map_err(|e| format!("{e:#}")),
                ));
            });
        }
    }
    fn request_preview(&mut self, page: usize) {
        if let Some(loaded) = &self.loaded {
            if page >= loaded.pages || self.preview_loading {
                return;
            }
            self.page = page;
            self.preview_loading = true;
            let bytes = loaded.bytes.clone();
            let id = self.load_id;
            self.spawn("pdf-preview", move |tx| {
                let _ = tx.send(Event::Preview(
                    id,
                    page,
                    pdf::preview(&bytes, page).map_err(|e| format!("{e:#}")),
                ));
            });
        }
    }
    fn set_preview(&mut self, p: pdf::Preview) {
        self.texture = Some(self.ctx.load_texture(
            "card-preview",
            egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba),
            egui::TextureOptions::LINEAR,
        ));
    }
    fn pick_file(&self) {
        self.spawn("pick-file", |tx| {
            let p = rfd::FileDialog::new()
                .add_filter("Карточка ККТ (PDF)", &["pdf"])
                .pick_file();
            let _ = tx.send(Event::Source(p));
        });
    }
    fn pick_destination(&self) {
        self.spawn("pick-destination", |tx| {
            let _ = tx.send(Event::Destination(
                rfd::FileDialog::new()
                    .set_title("Выберите папку для карточки")
                    .pick_folder(),
            ));
        });
    }
    fn begin_new_folder(&mut self) {
        if self.busy || self.new_folder.is_some() {
            return;
        }
        self.folder_sequence += 1;
        let (organization, inn) = self
            .loaded
            .as_ref()
            .map(|loaded| (loaded.card.organization.as_str(), loaded.card.inn.as_str()))
            .unwrap_or((&self.query, &self.query_inn));
        self.new_folder = Some(NewFolder {
            id: self.folder_sequence,
            parent: self.config.root.display().to_string(),
            name: folders::suggested_name(organization, inn),
            error: String::new(),
            picking_parent: false,
            creating: false,
            focus_name: true,
        });
    }
    fn pick_new_folder_parent(&mut self) {
        let Some(dialog) = &mut self.new_folder else {
            return;
        };
        if dialog.picking_parent || dialog.creating {
            return;
        }
        dialog.picking_parent = true;
        let id = dialog.id;
        let parent = PathBuf::from(dialog.parent.trim());
        self.spawn("pick-folder-parent", move |tx| {
            let path = rfd::FileDialog::new()
                .set_title("Где создать новую папку")
                .set_directory(parent)
                .pick_folder();
            let _ = tx.send(Event::FolderParent(id, path));
        });
    }
    fn create_new_folder(&mut self) {
        let Some(dialog) = &mut self.new_folder else {
            return;
        };
        if self.busy || dialog.creating || dialog.picking_parent {
            return;
        }
        if let Err(error) = folders::validate_name(&dialog.name) {
            dialog.error = error.to_string();
            return;
        }
        let id = dialog.id;
        let parent = PathBuf::from(dialog.parent.trim());
        let name = dialog.name.clone();
        dialog.creating = true;
        dialog.error.clear();
        self.busy = true;
        self.notify("Создаю папку…", false);
        self.spawn("create-folder", move |tx| {
            let result = folders::create(&parent, &name).map_err(|error| format!("{error:#}"));
            let _ = tx.send(Event::FolderCreated(id, result));
        });
    }
    fn process_events(&mut self) {
        let index_update = self.index_updates.lock().ok().and_then(|mut p| p.take());
        if let Some(update) = index_update {
            self.scanning = update.scanning;
            self.index_status = update.status;
            if let Some(index) = update.index {
                self.index = index;
                if self.choosing {
                    self.schedule_search();
                }
            }
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Loaded(id, result) if id == self.load_id => {
                    self.loading = false;
                    match result {
                        Ok(mut loaded) => {
                            self.save_registration =
                                card::format_registration(&loaded.card.registration);
                            self.save_date = loaded.card.formed_at.clone();
                            self.filename = if loaded.card.registration.is_empty()
                                && loaded.card.serial.is_empty()
                            {
                                loaded
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string()
                            } else {
                                loaded.card.suggested_filename()
                            };
                            if let Some(p) = loaded.preview.take() {
                                self.set_preview(p);
                            }
                            self.page = 0;
                            self.preview_loading = false;
                            self.last_saved = None;
                            if loaded.text.trim().is_empty() {
                                self.notify("В PDF нет текстового слоя. Проверьте предпросмотр и выберите папку вручную.",true);
                            } else if loaded.card.organization.is_empty() {
                                self.notify("Название организации не распознано. Весь текст доступен ниже; папку можно найти вручную.",true);
                            } else {
                                self.notify("Карточка прочитана. Проверьте данные и нажмите «Сохранить карточку».",false);
                            }
                            self.loaded = Some(*loaded);
                        }
                        Err(e) => self.notify(format!("Не удалось открыть карточку: {e}"), true),
                    }
                }
                Event::Preview(id, page, result) if id == self.load_id && page == self.page => {
                    self.preview_loading = false;
                    match result {
                        Ok(p) => self.set_preview(p),
                        Err(e) => self.notify(e, true),
                    }
                }
                Event::Matches(id, matches, ms) if id == self.query_id => {
                    self.matches = matches;
                    self.search_ms = ms;
                    self.searching = false;
                }
                Event::Source(Some(p)) => self.open(p),
                Event::Destination(Some(p)) => {
                    self.selected = Some(p);
                    self.manual_destination = true;
                }
                Event::Root(Some(p)) => self.root_edit = p.display().to_string(),
                Event::FolderParent(id, parent) => {
                    if let Some(dialog) = &mut self.new_folder
                        && dialog.id == id
                    {
                        dialog.picking_parent = false;
                        if let Some(path) = parent {
                            dialog.parent = path.display().to_string();
                            dialog.error.clear();
                        }
                    }
                }
                Event::FolderCreated(id, result) => {
                    self.busy = false;
                    if self
                        .new_folder
                        .as_ref()
                        .is_none_or(|dialog| dialog.id != id)
                    {
                        continue;
                    }
                    match result {
                        Ok(path) => {
                            if let Some(indexer) = &self.indexer {
                                indexer.add_folder(path.clone());
                            }
                            self.selected = Some(path);
                            self.manual_destination = true;
                            self.new_folder = None;
                            self.choosing = true;
                            self.notify("Папка создана и выбрана. Нажмите «Сохранить», чтобы записать карточку.", false);
                        }
                        Err(error) => {
                            if let Some(dialog) = &mut self.new_folder {
                                dialog.creating = false;
                                dialog.error = error.clone();
                            }
                            self.notify(error, true);
                        }
                    }
                }
                Event::Saved(result) => {
                    self.busy = false;
                    match result {
                        Ok(report) => {
                            self.saved = true;
                            self.last_saved = Some(report.saved.path.clone());
                            if let Some(loaded) = &self.loaded
                                && let Some(folder) =
                                    report.saved.path.parent().and_then(|p| p.parent())
                            {
                                self.config.remember(
                                    &loaded.card.inn,
                                    &loaded.card.organization,
                                    folder.into(),
                                );
                            }
                            self.persist();
                            if let Some(indexer) = &self.indexer {
                                indexer.add_card(report.saved.path.clone());
                            }
                            let mut text = format!(
                                "{}: {}",
                                if report.saved.existed {
                                    "Такая карточка уже есть"
                                } else {
                                    "Карточка сохранена"
                                },
                                report.saved.path.display()
                            );
                            if report.saved.removed {
                                text.push_str("\nИсходник удалён после проверки копии.");
                            }
                            if let Some(w) = &report.saved.warning {
                                text.push_str(&format!("\n{w}"));
                            }
                            let mut error = report.saved.warning.is_some();
                            if let Some(printing) = report.printing {
                                match printing {
                                    Ok(p) => text.push_str(&format!("\n{p}")),
                                    Err(e) => {
                                        error = true;
                                        self.print_retry = true;
                                        text.push_str(&format!(
                                            "\nФайл сохранён. Печать не удалась: {e}"
                                        ));
                                    }
                                }
                            }
                            self.notify(text, error);
                            if !error {
                                self.advance_queue();
                            }
                        }
                        Err(e) => self.notify(format!("Не удалось сохранить: {e}"), true),
                    }
                }
                Event::Printed(result) => {
                    self.busy = false;
                    match result {
                        Ok(p) => {
                            self.print_retry = false;
                            self.notify(p, false);
                            self.advance_queue();
                        }
                        Err(e) => {
                            self.print_retry = true;
                            self.notify(format!("Печать не удалась: {e}"), true)
                        }
                    }
                }
                Event::Show => {
                    self.hidden_to_tray.store(false, Ordering::Release);
                    if let Some(window) = self.native_window {
                        window.restore();
                    }
                    self.ctx
                        .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    self.ctx
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Event::Quit => {
                    if self.busy {
                        self.ctx
                            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        self.notify(
                            "Дождитесь окончания операции, затем выберите «Выйти» ещё раз.",
                            true,
                        );
                    } else {
                        self.exiting = true;
                        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                _ => {}
            }
        }
    }
}
