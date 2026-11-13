//! Two-phase archive organizer: read-only analysis, then an explicit journaled apply.
//! Card age is intentionally not part of any decision. Copies are not deduplicated.
use crate::{archive, card::Card, pdf, storage};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Card,
    Review,
    Delete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: usize,
    pub source: PathBuf,
    pub size: u64,
    pub sha256: [u8; 32],
    pub action: Action,
    pub target: Option<PathBuf>,
    pub reason: String,
    pub card: Option<Card>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Plan {
    pub version: u32,
    pub root: PathBuf,
    pub created_at_unix: u64,
    pub policy: String,
    pub entries: Vec<Entry>,
    pub skipped: Vec<PathBuf>,
}

const POLICY: &str = "all_card_dates_and_all_copies_preserved_v1";

fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}
fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!(
            "В плане недопустимый относительный путь: {}",
            path.display()
        );
    }
    Ok(())
}

fn walk(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    archive::check_path(root)?;
    let mut pending = vec![root.to_path_buf()];
    let (mut files, mut skipped) = (Vec::new(), Vec::new());
    while let Some(dir) = pending.pop() {
        for entry in
            fs::read_dir(&dir).with_context(|| format!("Не удалось прочитать {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if archive::is_reparse(&meta) {
                skipped.push(path.strip_prefix(root)?.to_path_buf());
            } else if meta.is_dir() {
                pending.push(path);
            } else if meta.is_file() {
                files.push(path.strip_prefix(root)?.to_path_buf());
            }
        }
    }
    files.sort_by_key(|p| key(p));
    skipped.sort();
    Ok((files, skipped))
}

fn classify(path: &Path, bytes: &[u8]) -> (Action, Option<Card>, String) {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let pdf_header = bytes
        .iter()
        .take(1024)
        .copied()
        .collect::<Vec<_>>()
        .windows(5)
        .any(|s| s == b"%PDF-");
    if pdf_header {
        if bytes.len() > 100 * 1024 * 1024 {
            return (
                Action::Review,
                None,
                "PDF больше 100 МиБ; требуется ручная проверка".into(),
            );
        }
        match pdf::extract_text(bytes) {
            Ok(text) => {
                let card = Card::parse(&text);
                if card.recognized() {
                    let mut missing = Vec::new();
                    if card.organization.is_empty() {
                        missing.push("организация");
                    }
                    if !archive::valid_registration(&card.registration) {
                        missing.push("РНМ");
                    }
                    if card.date().is_none() {
                        missing.push("дата карточки");
                    }
                    if missing.is_empty() {
                        return (
                            Action::Card,
                            Some(card),
                            "Карточка сохранена независимо от года и наличия одинаковых копий"
                                .into(),
                        );
                    }
                    return (
                        Action::Review,
                        Some(card),
                        format!("Карточка; не распознаны: {}", missing.join(", ")),
                    );
                }
                // Delete only PDFs with a positively recognized unrelated purpose.
                let lower = text.to_lowercase();
                if !lower.contains("карточк")
                    && [
                        "договор",
                        "счет-фактура",
                        "счёт-фактура",
                        "товарная накладная",
                        "универсальный передаточный документ",
                    ]
                    .iter()
                    .any(|word| lower.contains(word))
                {
                    return (
                        Action::Delete,
                        None,
                        "Распознан посторонний PDF (договор / накладная / счёт-фактура)".into(),
                    );
                }
                return (
                    Action::Review,
                    None,
                    if text.trim().is_empty() {
                        "PDF без текста: возможен скан карточки"
                    } else {
                        "Тип PDF не определён уверенно"
                    }
                    .into(),
                );
            }
            Err(e) => return (Action::Review, None, format!("PDF не прочитан: {e:#}")),
        }
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let possible_card = [
        "pdf", "jpg", "jpeg", "png", "tif", "tiff", "bmp", "gif", "webp", "heic", "djvu",
    ]
    .contains(&ext.as_str())
        || (ext == "partial" && !bytes.is_empty())
        || ["карточ", "сняти", "kkt"]
            .iter()
            .any(|part| name.contains(part));
    if possible_card {
        (
            Action::Review,
            None,
            "Возможная карточка / изображение / незавершённый PDF; ручная проверка".into(),
        )
    } else {
        (
            Action::Delete,
            None,
            "Посторонний файл: не PDF и не возможная карточка".into(),
        )
    }
}

fn organization_key(card: &Card) -> String {
    if [10, 12].contains(&card.inn.len()) {
        format!("inn:{}", card.inn)
    } else {
        format!("name:{}", crate::search::normalize(&card.organization))
    }
}

fn allocate_targets(entries: &mut [Entry]) -> Result<()> {
    let mut owners: BTreeMap<String, &Card> = BTreeMap::new();
    for entry in entries.iter().filter(|e| e.action == Action::Card) {
        let card = entry.card.as_ref().context("Карточка без данных")?;
        let owner = organization_key(card);
        if owners
            .get(&owner)
            .is_none_or(|old| card.date() > old.date())
        {
            owners.insert(owner, card);
        }
    }
    let mut counts = HashMap::<String, usize>::new();
    for card in owners.values() {
        *counts
            .entry(crate::folders::suggested_name(&card.organization, &card.inn).to_lowercase())
            .or_default() += 1;
    }
    let names: HashMap<_, _> = owners
        .into_iter()
        .map(|(id, card)| {
            let mut name = crate::folders::suggested_name(&card.organization, &card.inn);
            if counts[&name.to_lowercase()] > 1 {
                name = format!(
                    "{name} ИНН {}",
                    if card.inn.is_empty() {
                        "не определён"
                    } else {
                        &card.inn
                    }
                );
            }
            (id, name)
        })
        .collect();
    let mut used: HashSet<String> = entries.iter().map(|e| key(&e.source)).collect();
    for entry in entries {
        if entry.action == Action::Delete {
            continue;
        }
        let target = if entry.action == Action::Card {
            let card = entry.card.as_ref().unwrap();
            let name = &names[&organization_key(card)];
            crate::folders::validate_name(name)?;
            PathBuf::from(name)
                .join(&card.registration)
                .join(card.suggested_filename())
        } else if entry
            .source
            .components()
            .next()
            .is_some_and(|c| c.as_os_str() == archive::REVIEW_FOLDER)
        {
            entry.source.clone()
        } else {
            PathBuf::from(archive::REVIEW_FOLDER).join(&entry.source)
        };
        relative(&target)?;
        let mut allocated = target.clone();
        let mut suffix = 2;
        while used.contains(&key(&allocated)) && key(&allocated) != key(&entry.source) {
            let stem = target.file_stem().unwrap_or_default().to_string_lossy();
            let ext = target
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            allocated = target.with_file_name(format!("{stem} ({suffix}){ext}"));
            suffix += 1;
        }
        used.insert(key(&allocated));
        entry.target = Some(allocated);
    }
    Ok(())
}

pub fn analyze(
    root: &Path,
    report_dir: &Path,
    mut progress: impl FnMut(usize, usize),
) -> Result<Plan> {
    archive::check_path(root)?;
    let root = fs::canonicalize(root).context("Папка архива недоступна")?;
    archive::check_path(&root)?;
    let report_dir = std::path::absolute(report_dir)?;
    if report_dir.starts_with(&root) {
        bail!("Папка отчётов должна находиться вне архива");
    }
    archive::check_path(&report_dir)?;
    fs::create_dir_all(&report_dir)?;
    let report_dir = fs::canonicalize(report_dir)?;
    if report_dir.starts_with(&root) {
        bail!("Папка отчётов должна находиться вне архива");
    }
    if report_dir.join("plan.json").exists() {
        bail!("В папке уже есть plan.json; укажите новую папку отчёта");
    }
    let (files, skipped) = walk(&root)?;
    let total = files.len();
    let mut entries = Vec::with_capacity(total);
    for source in files {
        let bytes = fs::read(root.join(&source)).with_context(|| {
            format!(
                "Чтение {}. Анализ остановлен без изменений архива",
                source.display()
            )
        })?;
        let (action, card, reason) = classify(&source, &bytes);
        entries.push(Entry {
            id: entries.len(),
            source,
            size: bytes.len() as u64,
            sha256: storage::digest(&bytes),
            action,
            target: None,
            reason,
            card,
        });
        if entries.len() % 100 == 0 || entries.len() == total {
            progress(entries.len(), total);
        }
    }
    allocate_targets(&mut entries)?;
    let plan = Plan {
        version: 1,
        root,
        created_at_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        policy: POLICY.into(),
        entries,
        skipped,
    };
    let data = serde_json::to_vec_pretty(&plan)?;
    write_new(&report_dir.join("plan.json"), &data)?;
    write_new(&report_dir.join("summary.txt"), summary(&plan).as_bytes())?;
    write_new(
        &report_dir.join("report.html"),
        report_html(&plan).as_bytes(),
    )?;
    Ok(plan)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

pub fn summary(plan: &Plan) -> String {
    let count = |action| plan.entries.iter().filter(|e| e.action == action).count();
    let owners: HashSet<_> = plan
        .entries
        .iter()
        .filter(|e| e.action == Action::Card)
        .filter_map(|e| {
            e.target
                .as_ref()?
                .components()
                .next()
                .map(|c| c.as_os_str().to_os_string())
        })
        .collect();
    let old = plan
        .entries
        .iter()
        .filter(|e| {
            e.action == Action::Card
                && e.card
                    .as_ref()
                    .and_then(Card::date)
                    .is_some_and(|d| d.year < 2022)
        })
        .count();
    format!(
        "Архив: {}\nВсего файлов: {}\nКарточки в структуру организация / РНМ: {}\nВ том числе карточек до 2022 года (все сохраняются): {}\nОрганизаций: {}\nНеразобранные (сохраняются): {}\nПосторонние файлы к удалению: {}\nПропущено ссылок / junction: {}\n\nКарточки за все годы и одинаковые копии сохраняются.\nСоздание и изменение распознанных карточек = дата карточки, 00:00 МСК.\nНеразобранным файлам без прочитанной даты сохраняются исходные даты.\nАнализ не изменяет архив. Для применения нужен отдельный запуск apply.\n",
        plan.root.display(),
        plan.entries.len(),
        count(Action::Card),
        old,
        owners.len(),
        count(Action::Review),
        count(Action::Delete),
        plan.skipped.len()
    )
}

fn report_html(plan: &Plan) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let mut html = format!(
        "<!doctype html><html lang=ru><meta charset=utf-8><title>Сортировка карточек ККТ</title><style>body{{font:14px system-ui;margin:24px;background:#202124;color:#eee}}input,select{{padding:8px;margin:4px;background:#303134;color:white}}table{{border-collapse:collapse;width:100%}}td,th{{text-align:left;padding:8px;border-bottom:1px solid #555;overflow-wrap:anywhere}}pre{{white-space:pre-wrap}}</style><h1>План сортировки</h1><pre>{}</pre><input id=q placeholder='Поиск по пути, ИНН, РНМ'><select id=a><option value=''>Все действия</option><option>Карточка</option><option>Неразобранные</option><option>Удаление</option></select><table><thead><tr><th>Действие<th>Исходный путь<th>Итоговый путь<th>Дата карточки<th>Причина</tr></thead><tbody>",
        escape(&summary(plan))
    );
    for entry in &plan.entries {
        let action = match entry.action {
            Action::Card => "Карточка",
            Action::Review => "Неразобранные",
            Action::Delete => "Удаление",
        };
        html.push_str(&format!(
            "<tr data-a='{action}'><td>{action}<td>{}<td>{}<td>{}<td>{}</tr>\n",
            escape(&entry.source.to_string_lossy()),
            escape(
                &entry
                    .target
                    .as_ref()
                    .map_or_else(String::new, |p| p.display().to_string())
            ),
            escape(
                &entry
                    .card
                    .as_ref()
                    .map_or_else(String::new, |c| c.formed_at.clone())
            ),
            escape(&entry.reason)
        ));
    }
    html.push_str("</tbody></table><script>const rows=[...document.querySelectorAll('tbody tr')];function filter(){let s=q.value.toLowerCase();for(let r of rows)r.hidden=(!r.textContent.toLowerCase().includes(s))||(a.value&&r.dataset.a!==a.value)}q.oninput=a.onchange=filter;</script></html>");
    html
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Journal {
    Manifest { sha256: [u8; 32] },
    Started { id: usize },
    Done { id: usize },
}

fn append(file: &mut File, event: &Journal) -> Result<()> {
    serde_json::to_writer(&mut *file, event)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn read_plan(path: &Path) -> Result<(Plan, [u8; 32])> {
    let data = fs::read(path)?;
    let hash = storage::digest(&data);
    let plan: Plan = serde_json::from_slice(&data)?;
    if plan.version != 1 || plan.policy != POLICY {
        bail!("Версия или правила плана не поддерживаются. Выполните новый анализ.");
    }
    archive::check_path(&plan.root)?;
    let mut sources = HashSet::new();
    let mut targets = HashSet::new();
    for (id, entry) in plan.entries.iter().enumerate() {
        relative(&entry.source)?;
        if entry.id != id || !sources.insert(key(&entry.source)) {
            bail!("Повтор исходника / неверный номер операции в плане");
        }
        if entry.action == Action::Delete {
            if entry.target.is_some() || entry.card.as_ref().is_some_and(Card::recognized) {
                bail!("План пытается удалить карточку");
            }
        } else {
            let target = entry.target.as_ref().context("Не указан путь сохранения")?;
            relative(target)?;
            if !targets.insert(key(target)) {
                bail!("Повтор итогового пути в плане");
            }
        }
    }
    Ok((plan, hash))
}

pub fn apply(
    plan_path: &Path,
    backup_confirmed: bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<()> {
    if !backup_confirmed {
        bail!("Сначала создайте резервную копию, затем передайте --backup-confirmed");
    }
    let (plan, hash) = read_plan(plan_path)?;
    let journal_path = plan_path.with_file_name("journal.jsonl");
    let mut started = HashSet::new();
    let mut done = HashSet::new();
    // One writer owns the journal for the whole run, including preflight.
    let mut journal = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ)
        .open(&journal_path)?;
    let mut contents = String::new();
    journal.read_to_string(&mut contents)?;
    if !contents.is_empty() {
        let mut records = contents.lines();
        match records
            .next()
            .map(serde_json::from_str::<Journal>)
            .transpose()?
        {
            Some(Journal::Manifest { sha256 }) if sha256 == hash => (),
            _ => {
                bail!("Журнал относится к другому плану. Изменять plan.json после запуска нельзя.")
            }
        }
        for line in records {
            match serde_json::from_str(line)? {
                Journal::Started { id } => {
                    started.insert(id);
                }
                Journal::Done { id } => {
                    done.insert(id);
                }
                Journal::Manifest { .. } => bail!("Неверная запись в журнале"),
            }
        }
    }
    // Freeze the inventory and all hashes before touching anything. New files stop apply.
    let (actual, _) = walk(&plan.root)?;
    let mut allowed: HashSet<_> = plan.entries.iter().map(|e| key(&e.source)).collect();
    for entry in &plan.entries {
        if started.contains(&entry.id)
            && let Some(target) = &entry.target
        {
            allowed.insert(key(target));
        }
    }
    for path in actual {
        if !allowed.contains(&key(&path)) {
            bail!(
                "После анализа появился новый файл: {}. Создайте новый отчёт.",
                path.display()
            );
        }
    }
    for entry in &plan.entries {
        let source = plan.root.join(&entry.source);
        if source.exists() {
            if storage::file_digest(&source)? != entry.sha256 {
                bail!("Файл изменился после анализа: {}", entry.source.display());
            }
        } else if !started.contains(&entry.id) {
            bail!("После анализа исчез файл: {}", entry.source.display());
        }
        if let Some(target) = &entry.target {
            let target = plan.root.join(target);
            if done.contains(&entry.id) || (!source.exists() && started.contains(&entry.id)) {
                if storage::file_digest(&target).ok() != Some(entry.sha256) {
                    bail!(
                        "Не найдена проверенная сохранённая карточка: {}",
                        target.display()
                    );
                }
            } else if target.exists()
                && key(&target) != key(&source)
                && !started.contains(&entry.id)
            {
                bail!("Путь назначения уже занят: {}", target.display());
            }
        }
    }
    if journal.metadata()?.len() == 0 {
        append(&mut journal, &Journal::Manifest { sha256: hash })?;
    }
    // Preserve all cards and uncertain files before deleting any unrelated files.
    for delete_phase in [false, true] {
        for entry in &plan.entries {
            if (entry.action == Action::Delete) != delete_phase || done.contains(&entry.id) {
                continue;
            }
            let resume = started.contains(&entry.id);
            if !resume {
                append(&mut journal, &Journal::Started { id: entry.id })?;
                started.insert(entry.id);
            }
            let source = plan.root.join(&entry.source);
            if source.exists() {
                if let Some(target) = &entry.target {
                    storage::relocate_exact(
                        &source,
                        &plan.root.join(target),
                        entry.sha256,
                        entry.card.as_ref().and_then(Card::date),
                        resume,
                    )
                    .with_context(|| {
                        format!("Операция {}: {}", entry.id, entry.source.display())
                    })?;
                } else {
                    storage::remove_verified(&source, entry.sha256)?;
                }
            }
            append(&mut journal, &Journal::Done { id: entry.id })?;
            done.insert(entry.id);
            if done.len() % 100 == 0 || done.len() == plan.entries.len() {
                progress(done.len(), plan.entries.len());
            }
        }
    }
    remove_empty_dirs(&plan.root)?;
    verify(plan_path)?;
    Ok(())
}

fn remove_empty_dirs(root: &Path) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    let mut dirs = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.is_dir() && !archive::is_reparse(&meta) {
                dirs.push(entry.path());
                pending.push(entry.path());
            }
        }
    }
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for dir in dirs {
        archive::check_path(&dir)?;
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

pub fn verify(plan_path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    let (plan, _) = read_plan(plan_path)?;
    for entry in &plan.entries {
        if let Some(target) = &entry.target {
            let target = plan.root.join(target);
            archive::check_path(&target)?;
            if storage::file_digest(&target)? != entry.sha256 {
                bail!("Не совпал SHA-256: {}", target.display());
            }
            if let Some(date) = entry.card.as_ref().and_then(Card::date) {
                let meta = fs::metadata(&target)?;
                if meta.creation_time() != date.windows_ticks()?
                    || meta.last_write_time() != date.windows_ticks()?
                {
                    bail!("Неверная дата: {}", target.display());
                }
            }
        } else if plan.root.join(&entry.source).exists() {
            bail!("Посторонний файл остался: {}", entry.source.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(source: &str, date: &str, inn: &str) -> Entry {
        Entry {
            id: 0,
            source: source.into(),
            size: 0,
            sha256: [0; 32],
            action: Action::Card,
            target: None,
            reason: String::new(),
            card: Some(Card {
                kind: "Регистрация".into(),
                organization: "ООО «ПРИМЕР»".into(),
                registration: "0001234567890123".into(),
                inn: inn.into(),
                formed_at: date.into(),
                ..Default::default()
            }),
        }
    }
    #[test]
    fn old_cards_and_identical_copies_are_all_kept() {
        let mut entries = vec![
            entry("one.pdf", "01.01.2017", "1234567890"),
            entry("two.pdf", "01.01.2017", "1234567890"),
            entry("three.pdf", "12.11.2026", "9876543210"),
        ];
        allocate_targets(&mut entries).unwrap();
        assert_ne!(entries[0].target, entries[1].target);
        assert!(
            entries[0]
                .target
                .as_ref()
                .unwrap()
                .display()
                .to_string()
                .contains("2017-01-01")
        );
        assert!(
            entries[0]
                .target
                .as_ref()
                .unwrap()
                .display()
                .to_string()
                .contains("ИНН 1234567890")
        );
        assert_eq!(
            entries.iter().filter(|e| e.action == Action::Card).count(),
            3
        );
    }
    #[test]
    fn scans_and_unknown_pdfs_go_to_review() {
        for name in ["card.jpg", "scan.pdf", "download.partial", "карточка.docx"] {
            assert_eq!(classify(Path::new(name), b"unreadable").0, Action::Review);
        }
        assert_eq!(
            classify(Path::new("contract.docx"), b"data").0,
            Action::Delete
        );
    }
    #[test]
    fn apply_rechecks_changes_and_resumes_after_completed_run() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("archive");
        let reports = t.path().join("reports");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("scan.jpg"), b"image").unwrap();
        fs::write(root.join("contract.docx"), b"contract").unwrap();
        let plan = analyze(&root, &reports, |_, _| {}).unwrap();
        assert_eq!(plan.entries.len(), 2);
        let plan_path = reports.join("plan.json");
        assert!(apply(&plan_path, false, |_, _| {}).is_err());
        fs::write(root.join("contract.docx"), b"changed").unwrap();
        assert!(apply(&plan_path, true, |_, _| {}).is_err());
        assert!(root.join("scan.jpg").exists());
        fs::write(root.join("contract.docx"), b"contract").unwrap();
        apply(&plan_path, true, |_, _| {}).unwrap();
        assert_eq!(
            fs::read(root.join("Неразобранные/scan.jpg")).unwrap(),
            b"image"
        );
        assert!(!root.join("contract.docx").exists());
        apply(&plan_path, true, |_, _| {}).unwrap();
        verify(&plan_path).unwrap();
    }

    #[test]
    fn restart_after_move_before_done_record_keeps_all_copies() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("archive");
        let reports = t.path().join("reports");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("one.jpg"), b"same card image").unwrap();
        fs::write(root.join("two.jpg"), b"same card image").unwrap();
        let plan = analyze(&root, &reports, |_, _| {}).unwrap();
        let path = reports.join("plan.json");
        let hash = storage::file_digest(&path).unwrap();
        let first = &plan.entries[0];
        let mut journal = File::create(reports.join("journal.jsonl")).unwrap();
        append(&mut journal, &Journal::Manifest { sha256: hash }).unwrap();
        append(&mut journal, &Journal::Started { id: first.id }).unwrap();
        storage::relocate_exact(
            &plan.root.join(&first.source),
            &plan.root.join(first.target.as_ref().unwrap()),
            first.sha256,
            None,
            false,
        )
        .unwrap();
        drop(journal);
        apply(&path, true, |_, _| {}).unwrap();
        assert_eq!(
            fs::read(root.join("Неразобранные/one.jpg")).unwrap(),
            b"same card image"
        );
        assert_eq!(
            fs::read(root.join("Неразобранные/two.jpg")).unwrap(),
            b"same card image"
        );
    }
}
