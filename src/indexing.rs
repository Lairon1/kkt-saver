//! Hourly scans and local index updates run on one worker, including in the tray.
use crate::search::{self, Cache, SearchIndex};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
pub const RETRY_INTERVAL: Duration = Duration::from_secs(60);

pub enum Update {
    Started,
    Snapshot(Option<Arc<SearchIndex>>, String),
    Failed(String),
    Finished,
}

enum Command {
    Refresh,
    AddFolder(PathBuf),
    AddCard(PathBuf),
}

pub struct Indexer {
    root: PathBuf,
    commands: Sender<Command>,
    cancel: Arc<AtomicBool>,
}

impl Indexer {
    pub fn start(
        root: PathBuf,
        cache_directory: PathBuf,
        publish: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::start_with_intervals(
            root,
            cache_directory,
            REFRESH_INTERVAL,
            RETRY_INTERVAL,
            publish,
        )
    }

    fn start_with_intervals(
        root: PathBuf,
        cache_directory: PathBuf,
        refresh_interval: Duration,
        retry_interval: Duration,
        mut publish: impl FnMut(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        // One worker owns scans, local mutations and cache writes. Changes queued
        // during a scan are applied to its result, so snapshots cannot erase them.
        let (commands, rx) = mpsc::channel::<Command>();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_root = root.clone();
        std::thread::Builder::new()
            .name("folder-index".into())
            .spawn(move || {
                let mut cache = Cache::load_in(&worker_root, &cache_directory)
                    .unwrap_or_else(|| Cache::empty(worker_root.clone()));
                let mut has_index = cache.scanned_at != 0;
                let mut index = Arc::new(SearchIndex::new(cache.folders.clone()));
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let age = Duration::from_secs(now.saturating_sub(cache.scanned_at));
                let mut next_scan = Instant::now()
                    + if has_index {
                        refresh_interval.saturating_sub(age)
                    } else {
                        Duration::ZERO
                    };
                if has_index || !cache.folders.is_empty() {
                    publish(Update::Snapshot(
                        Some(index.clone()),
                        format!("Локальный индекс: {} папок", cache.folders.len()),
                    ));
                    publish(Update::Finished);
                }
                loop {
                    if worker_cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    match rx.recv_timeout(next_scan.saturating_duration_since(Instant::now())) {
                        Ok(command) => {
                            let mut pending = vec![command];
                            pending.extend(rx.try_iter());
                            let mut changed = false;
                            for command in pending {
                                let folders = match command {
                                    Command::Refresh => {
                                        next_scan = Instant::now();
                                        continue;
                                    }
                                    Command::AddFolder(path) => cache.add_local(&path, false),
                                    Command::AddCard(path) => cache.add_local(&path, true),
                                };
                                if !folders.is_empty() {
                                    let updated = Arc::make_mut(&mut index);
                                    for folder in folders {
                                        updated.upsert(folder);
                                    }
                                    changed = true;
                                }
                            }
                            if changed {
                                let cache_error = cache.save_in(&cache_directory).err();
                                publish(Update::Snapshot(
                                    Some(index.clone()),
                                    format!("Индекс: {} папок (обновлён локально)", index.len()),
                                ));
                                if let Some(e) = cache_error {
                                    publish(Update::Failed(format!(
                                        "Индекс в памяти; не удалось сохранить кэш: {e}"
                                    )));
                                }
                            }
                            // A local update never resets the full-scan deadline.
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    publish(Update::Started);
                    let result = search::scan(&worker_root, worker_cancel.clone(), |folders, p| {
                        let count = folders.len();
                        let snapshot = if !has_index || p.finished {
                            index = Arc::new(SearchIndex::new(folders));
                            Some(index.clone())
                        } else {
                            None
                        };
                        let status = if p.finished {
                            format!(
                                "Индекс: {count} папок{}",
                                if p.skipped > 0 {
                                    format!("; недоступно элементов: {}", p.skipped)
                                } else {
                                    String::new()
                                }
                            )
                        } else {
                            format!("Индексация: {} папок просмотрено…", p.directories)
                        };
                        publish(Update::Snapshot(snapshot, status));
                    });
                    if worker_cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let delay = match result {
                        Ok(scanned) => {
                            cache = scanned;
                            has_index = true;
                            if let Err(e) = cache.save_in(&cache_directory) {
                                publish(Update::Failed(format!(
                                    "Индекс в памяти; не удалось сохранить кэш: {e}"
                                )));
                            }
                            publish(Update::Finished);
                            refresh_interval
                        }
                        Err(e) => {
                            publish(Update::Failed(format!(
                                "{e:#}. {} Повтор через минуту.",
                                if has_index {
                                    "Поиск использует предыдущий индекс."
                                } else {
                                    "Папку можно выбрать вручную."
                                }
                            )));
                            retry_interval
                        }
                    };
                    next_scan = Instant::now() + delay;
                }
            })?;
        Ok(Self {
            root,
            commands,
            cancel,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn refresh(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    pub fn add_folder(&self, path: PathBuf) {
        let _ = self.commands.send(Command::AddFolder(path));
    }

    pub fn add_card(&self, path: PathBuf) {
        let _ = self.commands.send(Command::AddCard(path));
    }
}

impl Drop for Indexer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Dropping the last sender wakes recv_timeout immediately.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_cards_update_fresh_cache_without_scanning_until_manual_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("archive");
        let profile = temp.path().join("profile");
        let owner = root.join("ООО Ромашка");
        std::fs::create_dir_all(&owner).unwrap();
        let mut cache = Cache::empty(root.clone());
        cache.add_local(&owner, false);
        cache.scanned_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 120;
        let scanned_at = cache.scanned_at;
        cache.save_in(&profile).unwrap();
        // This folder must remain undiscovered until an actual full scan.
        std::fs::create_dir(root.join("Внешнее изменение")).unwrap();
        let (tx, rx) = mpsc::channel();
        let indexer = Indexer::start(root.clone(), profile.clone(), move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Update::Started => panic!("Fresh cache triggered a startup scan"),
                Update::Finished => break,
                _ => {}
            }
        }
        let folder = owner.join("2026");
        std::fs::create_dir(&folder).unwrap();
        let pdf = folder.join("ИНН 9876543210 РН 1234567890123456.pdf");
        std::fs::write(&pdf, b"local fixture").unwrap();
        indexer.add_folder(folder.clone());
        indexer.add_card(pdf);
        let card = crate::card::Card {
            inn: "9876543210".into(),
            ..Default::default()
        };
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Update::Started => panic!("Saving a card triggered a full scan"),
                Update::Snapshot(Some(index), _) => {
                    let results = index.search(&card, "", &[]);
                    if results.iter().any(|m| {
                        m.folder.path == owner && m.reasons.iter().any(|r| r == "ИНН в имени PDF")
                    }) {
                        assert_eq!(index.len(), 2);
                        break;
                    }
                }
                Update::Failed(e) => panic!("{e}"),
                _ => {}
            }
        }
        let cache = Cache::load_in(&root, &profile).unwrap();
        assert_eq!(cache.scanned_at, scanned_at);
        assert_eq!(cache.folders.len(), 2);
        assert!(
            cache
                .folders
                .iter()
                .any(|f| f.path == folder && f.hints.contains(&card.inn))
        );

        indexer.refresh();
        let mut started = false;
        let mut discovered = false;
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Update::Started => started = true,
                Update::Snapshot(Some(index), _) if index.len() == 3 => discovered = true,
                Update::Finished => break,
                Update::Failed(e) => panic!("{e}"),
                _ => {}
            }
        }
        assert!(
            started && discovered,
            "Manual refresh did not scan the archive"
        );
        drop(indexer);
        while rx.recv_timeout(Duration::from_secs(2)).is_ok() {}
    }

    #[test]
    fn card_saved_during_full_scan_is_merged_into_its_result() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("archive");
        let profile = temp.path().join("profile");
        std::fs::create_dir_all(root.join("ООО Первая")).unwrap();
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut gate = Some(release_rx);
        let indexer = Indexer::start(root.clone(), profile.clone(), move |event| {
            // Pause after directory enumeration but before the scan cache is saved.
            let completed_snapshot =
                matches!(&event, Update::Snapshot(_, s) if s.starts_with("Индекс:"));
            let _ = tx.send(event);
            if completed_snapshot && let Some(gate) = gate.take() {
                gate.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        })
        .unwrap();
        loop {
            if matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Update::Snapshot(_, s) if s.starts_with("Индекс:"))
            {
                break;
            }
        }
        let folder = root.join("ООО Новая");
        std::fs::create_dir(&folder).unwrap();
        let pdf = folder.join("РН 1234567890123456.pdf");
        std::fs::write(&pdf, b"local fixture").unwrap();
        indexer.add_card(pdf);
        release_tx.send(()).unwrap();
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
                Update::Started => panic!("Local change caused a second scan"),
                Update::Snapshot(Some(index), _) if index.len() == 2 => break,
                Update::Failed(e) => panic!("{e}"),
                _ => {}
            }
        }
        let cache = Cache::load_in(&root, &profile).unwrap();
        assert!(
            cache
                .folders
                .iter()
                .any(|f| f.path == folder && f.hints.contains("1234567890123456"))
        );
        drop(indexer);
        while rx.recv_timeout(Duration::from_secs(2)).is_ok() {}
    }

    #[test]
    fn retries_offline_root_and_discovers_new_folders_automatically() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("archive");
        let profile = temp.path().join("profile");
        let (tx, rx) = mpsc::channel();
        let indexer = Indexer::start_with_intervals(
            root.clone(),
            profile.clone(),
            Duration::from_millis(100),
            Duration::from_millis(80),
            move |event| {
                let _ = tx.send(event);
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        // The root starts offline; it then appears without any refresh command.
        loop {
            assert!(Instant::now() < deadline, "No offline error received");
            if matches!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                Update::Failed(_)
            ) {
                break;
            }
        }
        std::fs::create_dir_all(root.join("ООО Первая")).unwrap();
        let mut found_first = false;
        let mut found_second = false;
        loop {
            assert!(
                Instant::now() < deadline,
                "Automatic retry/refresh did not discover folders"
            );
            match rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                Update::Snapshot(Some(index), _) => {
                    if index.len() == 1 && !found_first {
                        found_first = true;
                        std::fs::create_dir(root.join("ООО Вторая")).unwrap();
                    } else if index.len() == 2 {
                        found_second = true;
                    }
                }
                Update::Finished if found_second => break,
                _ => {}
            }
        }
        assert!(found_first);
        drop(indexer);
        // The worker stops even without any window or UI event processing.
        while rx.recv_timeout(Duration::from_secs(2)).is_ok() {}
        let cache = Cache::load_in(&root, &profile).unwrap();
        assert_eq!(cache.folders.len(), 2);
    }
}
