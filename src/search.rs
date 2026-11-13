use crate::{
    card::Card,
    config::{self, Choice},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub fn normalize(text: &str) -> String {
    let mut s = text.to_lowercase().replace('ё', "е");
    for (a, b) in [
        ("общество с ограниченной ответственностью", " ооо "),
        ("индивидуальный предприниматель", " ип "),
        ("публичное акционерное общество", " пао "),
        ("открытое акционерное общество", " оао "),
        ("закрытое акционерное общество", " зао "),
        ("акционерное общество", " ао "),
    ] {
        s = s.replace(a, b);
    }
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn meaningful(s: &str) -> Vec<String> {
    s.split_whitespace()
        .filter(|w| {
            w.chars().count() > 1
                && ![
                    "ооо",
                    "ип",
                    "ао",
                    "оао",
                    "пао",
                    "зао",
                    "с",
                    "и",
                    "фирма",
                    "расторгнут",
                    "расторгнуто",
                    "безвозмездно",
                    "бывший",
                    "бывшая",
                    "устаревшие",
                    "данные",
                    "копия",
                    "инн",
                    "кпп",
                ]
                .contains(w)
                && !(w.chars().all(|c| c.is_ascii_digit()) && w.len() < 6)
        })
        .map(str::to_string)
        .collect()
}
fn trigrams(s: &str) -> HashSet<String> {
    let c: Vec<_> = s.chars().collect();
    if c.len() < 3 {
        return HashSet::from([s.to_string()]);
    }
    c.windows(3).map(|w| w.iter().collect()).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub path: PathBuf,
    pub owner: String,
    pub depth: usize,
    pub hints: String,
}
impl Folder {
    fn organization_path(&self) -> Option<&Path> {
        let levels = self.depth.checked_sub(1)?;
        let path = self.path.ancestors().nth(levels)?;
        if path.file_name()?.to_string_lossy().to_lowercase()
            == crate::archive::REVIEW_FOLDER.to_lowercase()
        {
            return None;
        }
        // Numeric/date-only directories are not organization names.
        path.file_name()?
            .to_string_lossy()
            .chars()
            .any(char::is_alphabetic)
            .then_some(path)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    pub root: PathBuf,
    pub folders: Vec<Folder>,
    pub scanned_at: u64,
    pub skipped: usize,
}
impl Cache {
    pub(crate) fn empty(root: PathBuf) -> Self {
        Self {
            version: 2,
            root,
            folders: Vec::new(),
            scanned_at: 0,
            skipped: 0,
        }
    }

    /// Adds known paths and PDF name hints without reading the archive.
    /// `scanned_at` always describes the last full scan, never a local update.
    pub(crate) fn add_local(&mut self, path: &Path, is_pdf: bool) -> Vec<Folder> {
        let folder = if is_pdf {
            let Some(parent) = path.parent() else {
                return Vec::new();
            };
            parent
        } else {
            path
        };
        let Ok(relative) = folder.strip_prefix(&self.root) else {
            return Vec::new();
        };
        let parts: Vec<_> = relative.components().collect();
        if parts
            .iter()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return Vec::new();
        }
        let owner = parts.first().map_or_else(
            || {
                self.root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            },
            |part| part.as_os_str().to_string_lossy().into_owned(),
        );
        let mut changed = Vec::new();
        let mut current = self.root.clone();
        let mut paths = Vec::new();
        for part in &parts {
            current.push(part.as_os_str());
            paths.push(current.clone());
        }
        if paths.is_empty() {
            paths.push(current);
        }
        for (position, current) in paths.into_iter().enumerate() {
            let existing = self.folders.iter().position(|f| f.path == current);
            let id = existing.unwrap_or_else(|| {
                self.folders.push(Folder {
                    path: current.clone(),
                    owner: owner.clone(),
                    depth: if parts.is_empty() { 0 } else { position + 1 },
                    hints: String::new(),
                });
                self.folders.len() - 1
            });
            let entry = &mut self.folders[id];
            let mut modified = existing.is_none();
            if is_pdf
                && current == folder
                && let Some(name) = path.file_name()
            {
                let name = name.to_string_lossy();
                // Hints are search text, not a file inventory. An already present
                // name contributes the same terms and must not accumulate on retries.
                if !entry.hints.contains(name.as_ref()) {
                    if !entry.hints.is_empty() {
                        entry.hints.push(' ');
                    }
                    entry.hints.push_str(&name);
                    modified = true;
                }
            }
            if modified {
                changed.push(entry.clone());
            }
        }
        changed
    }

    pub fn load(root: &Path) -> Option<Self> {
        Self::load_in(root, &config::data_dir())
    }
    pub(crate) fn load_in(root: &Path, directory: &Path) -> Option<Self> {
        let c: Self =
            serde_json::from_slice(&std::fs::read(directory.join("folders-v2.json")).ok()?).ok()?;
        (c.version == 2 && c.root == root).then_some(c)
    }
    pub fn save(&self) -> Result<()> {
        self.save_in(&config::data_dir())
    }
    pub(crate) fn save_in(&self, directory: &Path) -> Result<()> {
        config::write_json_in(directory, "folders-v2.json", self)
    }
}

pub struct ScanProgress {
    pub directories: usize,
    pub finished: bool,
    pub skipped: usize,
}

/// Enumerates directory metadata only. Four workers bound SMB concurrency; no PDF is opened.
/// Root results are published immediately; cancellation is checked between network calls.
pub fn scan(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut publish: impl FnMut(Vec<Folder>, ScanProgress),
) -> Result<Cache> {
    let children =
        std::fs::read_dir(root).with_context(|| format!("Архив недоступен: {}", root.display()))?;
    let mut roots = Vec::new();
    let mut root_errors = 0;
    for child in children {
        let Ok(child) = child else {
            root_errors += 1;
            continue;
        };
        if let Ok(ft) = child.file_type()
            && ft.is_dir()
            && !ft.is_symlink()
        {
            roots.push(Folder {
                path: child.path(),
                owner: child.file_name().to_string_lossy().to_string(),
                depth: 1,
                hints: String::new(),
            });
        }
    }
    roots.sort_by(|a, b| a.path.cmp(&b.path));
    publish(
        roots.clone(),
        ScanProgress {
            directories: roots.len(),
            finished: false,
            skipped: root_errors,
        },
    );
    let next = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(root_errors);
    let (tx, rx) = std::sync::mpsc::channel();
    let mut folders = Vec::new();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let tx = tx.clone();
            let roots = &roots;
            let next = &next;
            let skipped = &skipped;
            let cancel = &cancel;
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(first) = roots.get(i) else { break };
                    let mut queue = VecDeque::from([first.clone()]);
                    let mut batch = Vec::new();
                    while let Some(mut folder) = queue.pop_front() {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        match std::fs::read_dir(&folder.path) {
                            Ok(entries) => {
                                let mut names = Vec::new();
                                for entry in entries {
                                    let Ok(entry) = entry else {
                                        skipped.fetch_add(1, Ordering::Relaxed);
                                        continue;
                                    };
                                    let Ok(ft) = entry.file_type() else { continue };
                                    if ft.is_symlink() {
                                        continue;
                                    }
                                    if ft.is_dir() {
                                        queue.push_back(Folder {
                                            path: entry.path(),
                                            owner: folder.owner.clone(),
                                            depth: folder.depth + 1,
                                            hints: String::new(),
                                        });
                                    } else if entry
                                        .path()
                                        .extension()
                                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                                        && names.len() < 250
                                    {
                                        names.push(entry.file_name().to_string_lossy().to_string());
                                    }
                                }
                                folder.hints = names.join(" ");
                            }
                            Err(_) => {
                                skipped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        batch.push(folder);
                        if batch.len() >= 128 && tx.send(std::mem::take(&mut batch)).is_err() {
                            return;
                        }
                    }
                    if !batch.is_empty() && tx.send(batch).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx);
        let mut last = Instant::now();
        for batch in rx {
            folders.extend(batch);
            if last.elapsed() > Duration::from_secs(2) {
                // Keep roots not visited yet available during progressive updates.
                let seen: HashSet<_> = folders.iter().map(|f| f.path.clone()).collect();
                let mut snapshot = folders.clone();
                snapshot.extend(roots.iter().filter(|f| !seen.contains(&f.path)).cloned());
                publish(
                    snapshot,
                    ScanProgress {
                        directories: folders.len(),
                        finished: false,
                        skipped: skipped.load(Ordering::Relaxed),
                    },
                );
                last = Instant::now();
            }
        }
    });
    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("Индексация отменена");
    }
    folders.sort_by(|a, b| a.path.cmp(&b.path));
    let count = folders.len();
    let skipped = skipped.load(Ordering::Relaxed);
    publish(
        folders.clone(),
        ScanProgress {
            directories: count,
            finished: true,
            skipped,
        },
    );
    Ok(Cache {
        version: 2,
        root: root.into(),
        folders,
        scanned_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        skipped,
    })
}

#[derive(Clone)]
struct Entry {
    folder: Folder,
    owner_norm: String,
    owner_tokens: Vec<String>,
    path_norm: String,
    tokens: Vec<String>,
    hint_norm: String,
}
#[derive(Clone)]
pub struct SearchIndex {
    entries: Vec<Entry>,
    paths: HashMap<PathBuf, usize>,
    words: HashMap<String, Vec<usize>>,
    grams: HashMap<String, Vec<usize>>,
}
#[derive(Clone, Debug)]
pub struct Match {
    pub folder: Folder,
    pub score: f64,
    pub reasons: Vec<String>,
}

impl Entry {
    fn new(folder: Folder) -> Self {
        let owner_norm = normalize(&folder.owner);
        let owner_tokens = meaningful(&owner_norm);
        let path_norm = normalize(&folder.path.to_string_lossy());
        let tokens = meaningful(&path_norm);
        let hint_norm = normalize(&folder.hints);
        Self {
            folder,
            owner_norm,
            owner_tokens,
            path_norm,
            tokens,
            hint_norm,
        }
    }

    fn terms(&self) -> (HashSet<String>, HashSet<String>) {
        let words: HashSet<_> = self
            .tokens
            .iter()
            .cloned()
            .chain(meaningful(&self.hint_norm))
            .collect();
        let grams = words.iter().flat_map(|word| trigrams(word)).collect();
        (words, grams)
    }
}

fn remove_terms(postings: &mut HashMap<String, Vec<usize>>, terms: HashSet<String>, id: usize) {
    for term in terms {
        if let Some(ids) = postings.get_mut(&term) {
            ids.retain(|&old| old != id);
            if ids.is_empty() {
                postings.remove(&term);
            }
        }
    }
}

impl SearchIndex {
    pub fn new(folders: Vec<Folder>) -> Self {
        let mut result = Self {
            entries: Vec::with_capacity(folders.len()),
            paths: HashMap::with_capacity(folders.len()),
            words: HashMap::new(),
            grams: HashMap::new(),
        };
        for folder in folders {
            let entry = Entry::new(folder);
            result
                .paths
                .insert(entry.folder.path.clone(), result.entries.len());
            result.add_terms(result.entries.len(), &entry);
            result.entries.push(entry);
        }
        result
    }

    /// Reindexes only the changed folder; existing search snapshots stay valid.
    pub(crate) fn upsert(&mut self, folder: Folder) {
        let entry = Entry::new(folder);
        if let Some(&id) = self.paths.get(&entry.folder.path) {
            let (words, grams) = self.entries[id].terms();
            remove_terms(&mut self.words, words, id);
            remove_terms(&mut self.grams, grams, id);
            self.add_terms(id, &entry);
            self.entries[id] = entry;
        } else {
            self.paths
                .insert(entry.folder.path.clone(), self.entries.len());
            self.add_terms(self.entries.len(), &entry);
            self.entries.push(entry);
        }
    }

    fn add_terms(&mut self, id: usize, entry: &Entry) {
        let (words, grams) = entry.terms();
        for word in words {
            self.words.entry(word).or_default().push(id);
        }
        for gram in grams {
            self.grams.entry(gram).or_default().push(id);
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn search(&self, card: &Card, query: &str, choices: &[Choice]) -> Vec<Match> {
        let q_norm = normalize(query);
        let terms = meaningful(&q_norm);
        let org_norm = normalize(&card.organization);
        let org_terms = meaningful(&org_norm);
        let mut preliminary: HashMap<usize, f64> = HashMap::new();
        let n = self.entries.len().max(1) as f64;
        // Inverted words and trigrams shortlist candidates before costly edit-distance scoring.
        for term in terms.iter().chain(
            [&card.inn, &card.serial, &card.registration]
                .into_iter()
                .filter(|s| !s.is_empty()),
        ) {
            if let Some(ids) = self.words.get(term) {
                let idf = (1.0 + n / (1.0 + ids.len() as f64)).ln();
                for &id in ids {
                    *preliminary.entry(id).or_default() += idf * 8.0;
                }
            }
            for gram in trigrams(term) {
                if let Some(ids) = self.grams.get(&gram) {
                    let idf = (1.0 + n / (1.0 + ids.len() as f64)).ln();
                    for &id in ids {
                        *preliminary.entry(id).or_default() += idf;
                    }
                }
            }
        }
        for (id, e) in self.entries.iter().enumerate() {
            if choices.iter().any(|c| {
                c.path == e.folder.path
                    && ((!card.inn.is_empty() && c.inn == card.inn)
                        || (!org_norm.is_empty() && normalize(&c.organization) == org_norm))
            }) {
                *preliminary.entry(id).or_default() += 100.0;
            }
        }
        if preliminary.len() < 10 {
            for (i, e) in self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.folder.depth == 1)
                .take(40)
            {
                preliminary.entry(i).or_insert(
                    if e.owner_norm.contains(&q_norm) && !q_norm.is_empty() {
                        5.0
                    } else {
                        0.0
                    },
                );
            }
        }
        let mut candidates: Vec<_> = preliminary.into_iter().collect();
        candidates.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
        // Limit organizations, rather than letting hundreds of date subfolders
        // from one owner crowd other organizations out of the shortlist.
        let mut roots: HashMap<PathBuf, usize> = HashMap::new();
        candidates.retain(|(id, _)| {
            let Some(root) = self.entries[*id].folder.organization_path() else {
                return false;
            };
            if !roots.contains_key(root) && roots.len() >= 800 {
                return false;
            }
            let count = roots.entry(root.to_path_buf()).or_default();
            *count += 1;
            *count <= 4
        });
        // Always score the root itself as well as its strongest descendants.
        let mut selected_ids: HashSet<_> = candidates.iter().map(|(id, _)| *id).collect();
        for root in roots.keys() {
            if let Some(&id) = self.paths.get(root)
                && selected_ids.insert(id)
            {
                candidates.push((id, 0.0));
            }
        }
        let mut grouped: HashMap<PathBuf, Match> = HashMap::with_capacity(roots.len());
        for (id, _) in candidates {
            let e = &self.entries[id];
            let mut score = 0.0;
            let mut reasons = Vec::new();
            let mut exact = 0;
            let mut fuzzy = 0;
            let mut total_weight = 0.0;
            let mut matched_weight = 0.0;
            for term in &terms {
                let df = self.words.get(term).map_or(0, Vec::len) as f64;
                let weight = (1.0 + n / (1.0 + df)).ln().max(1.0);
                total_weight += weight;
                let best = e
                    .tokens
                    .iter()
                    .map(|t| {
                        if t == term {
                            1.0
                        } else if t.starts_with(term) && term.chars().count() >= 3 {
                            0.88
                        } else if t.chars().count() >= 4 && term.chars().count() >= 4 {
                            let v = strsim::jaro_winkler(t, term);
                            if v >= 0.83 { v * 0.83 } else { 0.0 }
                        } else {
                            0.0
                        }
                    })
                    .fold(0.0_f64, f64::max);
                matched_weight += weight * best;
                if best == 1.0 {
                    exact += 1;
                } else if best > 0.0 {
                    fuzzy += 1;
                }
            }
            if total_weight > 0.0 {
                score += 70.0 * matched_weight / total_weight;
            }
            if !terms.is_empty() && exact == terms.len() {
                score += 20.0;
                reasons.push("Все слова запроса".into());
            } else if exact > 0 {
                reasons.push(format!("Совпало слов: {exact}"));
            }
            if fuzzy > 0 {
                reasons.push("Сходное написание".into());
            }
            let core = terms.join(" ");
            let owner_core = e.owner_tokens.join(" ");
            if !core.is_empty() && owner_core == core {
                score += 30.0;
                reasons.push("Точное название".into());
            } else if !core.is_empty() && owner_core.contains(&core) {
                score += 12.0;
            }
            if !card.inn.is_empty() {
                if e.tokens.contains(&card.inn) {
                    score += 100.0;
                    reasons.push("ИНН в названии папки".into());
                } else if e.hint_norm.split_whitespace().any(|w| w == card.inn) {
                    score += 65.0;
                    reasons.push("ИНН в имени PDF".into());
                }
                if e.owner_tokens.iter().any(|t| {
                    [10, 12].contains(&t.len())
                        && t.chars().all(|c| c.is_ascii_digit())
                        && *t != card.inn
                }) {
                    score -= 100.0;
                    reasons.push("Другой ИНН — проверьте".into());
                }
            }
            for (value, reason) in [
                (&card.serial, "Заводской номер ККТ"),
                (&card.registration, "Регистрационный номер ККТ"),
            ] {
                if !value.is_empty() && (e.path_norm.contains(value) || e.hint_norm.contains(value))
                {
                    score += 55.0;
                    reasons.push(reason.into());
                }
            }
            let address = meaningful(&normalize(&card.address));
            let hits = address
                .iter()
                .filter(|w| {
                    w.chars().count() > 3 && e.path_norm.split_whitespace().any(|t| t == *w)
                })
                .count();
            if hits > 0 {
                score += (hits as f64 * 3.0).min(12.0);
                reasons.push("Часть адреса".into());
            }
            let learned = choices.iter().position(|c| {
                c.path == e.folder.path
                    && ((!card.inn.is_empty() && c.inn == card.inn)
                        || (!org_terms.is_empty()
                            && meaningful(&normalize(&c.organization)) == org_terms))
            });
            if learned.is_some() {
                score += 35.0;
                reasons.push("Ранее выбрана для этой организации".into());
            }
            if e.owner_norm.contains("расторгнут") {
                score -= 4.0;
            }
            let Some(root) = e.folder.organization_path() else {
                continue;
            };
            let folder = self.paths.get(root).map_or_else(
                || Folder {
                    path: root.to_path_buf(),
                    owner: root
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    depth: 1,
                    hints: String::new(),
                },
                |&id| self.entries[id].folder.clone(),
            );
            let entry = grouped.entry(root.to_path_buf()).or_insert_with(|| Match {
                folder,
                score: f64::NEG_INFINITY,
                reasons: Vec::new(),
            });
            if score > entry.score {
                entry.score = score;
                entry.reasons = reasons;
            }
        }
        let mut matches: Vec<_> = grouped.into_values().collect();
        matches.sort_unstable_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.folder.depth.cmp(&b.folder.depth))
                .then_with(|| a.folder.path.cmp(&b.folder.path))
        });
        matches.truncate(10);
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_card_matches_return_the_organization_root_once() {
        let root = PathBuf::from("C:\\archive");
        let owner = root.join("ООО Ромашка");
        let folders = vec![
            Folder {
                path: owner.clone(),
                owner: "ООО Ромашка".into(),
                depth: 1,
                hints: String::new(),
            },
            Folder {
                path: owner.join("2026-11-12"),
                owner: "ООО Ромашка".into(),
                depth: 2,
                hints: "ИНН 1234567890 РН 9876543210123456.pdf".into(),
            },
            Folder {
                path: owner.join("2025").join("Снятие"),
                owner: "ООО Ромашка".into(),
                depth: 3,
                hints: "РН 9876543210123456.pdf".into(),
            },
            Folder {
                path: root.join("ООО Другая"),
                owner: "ООО Другая".into(),
                depth: 1,
                hints: String::new(),
            },
        ];
        let card = Card {
            organization: "ООО Ромашка".into(),
            inn: "1234567890".into(),
            registration: "9876543210123456".into(),
            ..Default::default()
        };
        let results = SearchIndex::new(folders).search(&card, &card.organization, &[]);
        assert_eq!(results[0].folder.path, owner);
        assert_eq!(results[0].folder.owner, "ООО Ромашка");
        assert!(results[0].reasons.iter().any(|r| r == "ИНН в имени PDF"));
        assert!(
            results[0]
                .reasons
                .iter()
                .any(|r| r == "Регистрационный номер ККТ")
        );
        assert!(
            results
                .iter()
                .all(|m| m.folder.depth == 1 && m.folder.path.parent() == Some(root.as_path()))
        );
        assert_eq!(results.iter().filter(|m| m.folder.path == owner).count(), 1);
    }

    #[test]
    fn date_only_roots_and_the_archive_itself_are_not_destinations() {
        let root = PathBuf::from("C:\\archive");
        let organization = root.join("Ромашка");
        let folders = vec![
            Folder {
                path: root.clone(),
                owner: "archive".into(),
                depth: 0,
                hints: "ИНН 1234567890.pdf".into(),
            },
            Folder {
                path: root.join("2026-11-12"),
                owner: "2026-11-12".into(),
                depth: 1,
                hints: "ИНН 1234567890.pdf".into(),
            },
            Folder {
                path: root.join("2026-11-12").join("Вложенная папка"),
                owner: "2026-11-12".into(),
                depth: 2,
                hints: "ИНН 1234567890.pdf".into(),
            },
            Folder {
                path: root.join(crate::archive::REVIEW_FOLDER).join("Ромашка"),
                owner: crate::archive::REVIEW_FOLDER.into(),
                depth: 2,
                hints: "ИНН 1234567890.pdf".into(),
            },
            Folder {
                path: organization.clone(),
                owner: "Ромашка".into(),
                depth: 1,
                hints: String::new(),
            },
        ];
        let card = Card {
            inn: "1234567890".into(),
            ..Default::default()
        };
        let results = SearchIndex::new(folders).search(&card, "", &[]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].folder.path, organization);
    }

    #[test]
    fn many_subfolders_do_not_crowd_other_organizations_out_of_top_ten() {
        let root = PathBuf::from("C:\\archive");
        let owner = root.join("ООО Плотный архив");
        let mut folders = vec![Folder {
            path: owner.clone(),
            owner: "ООО Плотный архив".into(),
            depth: 1,
            hints: String::new(),
        }];
        for i in 0..900 {
            folders.push(Folder {
                path: owner.join(format!("2026-11-{i:04}")),
                owner: "ООО Плотный архив".into(),
                depth: 2,
                hints: "касса ИНН 1234567890 РН 9876543210123456 ЗН 4567890123456789.pdf".into(),
            });
        }
        for i in 0..12 {
            let name = format!("ООО Клиент {i}");
            folders.push(Folder {
                path: root.join(&name),
                owner: name,
                depth: 1,
                hints: "касса.pdf".into(),
            });
        }
        let card = Card {
            inn: "1234567890".into(),
            registration: "9876543210123456".into(),
            serial: "4567890123456789".into(),
            ..Default::default()
        };
        let results = SearchIndex::new(folders).search(&card, "касса", &[]);
        assert_eq!(results.len(), 10);
        assert_eq!(results[0].folder.path, owner);
        let distinct: HashSet<_> = results.iter().map(|m| &m.folder.path).collect();
        assert_eq!(distinct.len(), 10);
        assert!(results.iter().all(|m| m.folder.depth == 1));
    }

    fn index() -> SearchIndex {
        SearchIndex::new(
            [
                "001_!ООО Викторина",
                "005 (безвозмездно) ООО Сангрия",
                "002_!ООО Аск",
                "003_!ООО Алекс",
                "999 ООО АСК 1234567890",
                "ИП Сидоров Александр Петрович",
                "ООО Ёлочка",
            ]
            .into_iter()
            .map(|s| Folder {
                path: PathBuf::from("C:\\archive").join(s),
                owner: s.into(),
                depth: 1,
                hints: String::new(),
            })
            .collect(),
        )
    }

    #[test]
    fn local_additions_preserve_scan_time_and_do_not_duplicate_hints() {
        let root = PathBuf::from("C:\\archive");
        let mut cache = Cache::empty(root.clone());
        cache.scanned_at = 123;
        let pdf = root
            .join("ООО Ромашка")
            .join("2026")
            .join("ИНН 1234567890.pdf");
        assert_eq!(cache.add_local(&pdf, true).len(), 2);
        assert!(cache.add_local(&pdf, true).is_empty());
        assert_eq!(cache.folders.len(), 2);
        assert_eq!(cache.folders[1].owner, "ООО Ромашка");
        assert_eq!(cache.folders[1].depth, 2);
        assert_eq!(cache.folders[1].hints, "ИНН 1234567890.pdf");
        assert_eq!(cache.scanned_at, 123);
        assert!(
            cache
                .add_local(&PathBuf::from("C:\\outside\\company"), false)
                .is_empty()
        );
        assert!(
            cache
                .add_local(&root.join("..\\outside\\card.pdf"), true)
                .is_empty()
        );
    }

    #[test]
    fn updating_one_folder_changes_postings_and_preserves_old_snapshot() {
        let mut original = Arc::new(index());
        let snapshot = original.clone();
        let mut folder = original.entries[0].folder.clone();
        folder.hints = "ИНН 9876543210 РН 4567891234567890.pdf".into();
        Arc::make_mut(&mut original).upsert(folder.clone());
        assert!(!snapshot.words.contains_key("9876543210"));
        assert!(original.words.contains_key("9876543210"));
        let card = Card {
            inn: "9876543210".into(),
            ..Default::default()
        };
        let found = original.search(&card, "", &[]);
        assert_eq!(found[0].folder.path, folder.path);
        assert!(found[0].reasons.iter().any(|r| r == "ИНН в имени PDF"));
        folder.hints = "ИНН 1122334455.pdf".into();
        Arc::make_mut(&mut original).upsert(folder.clone());
        Arc::make_mut(&mut original).upsert(folder);
        assert!(!original.words.contains_key("9876543210"));
        assert_eq!(original.words["1122334455"].len(), 1);
        assert_eq!(original.len(), snapshot.len());
        assert!(original.grams.values().all(|ids| {
            let unique: HashSet<_> = ids.iter().collect();
            unique.len() == ids.len()
        }));
    }
    #[test]
    fn legal_form_noise_and_typo() {
        let c = Card {
            organization: "ОБЩЕСТВО С ОГРАНИЧЕННОЙ ОТВЕТСТВЕННОСТЬЮ «АСК»".into(),
            inn: "1203003898".into(),
            ..Default::default()
        };
        let r = index().search(&c, &c.organization, &[]);
        assert_eq!(r[0].folder.owner, "002_!ООО Аск");
        let c = Card {
            organization: "ООО Виктроина".into(),
            ..Default::default()
        };
        assert_eq!(
            index().search(&c, &c.organization, &[])[0].folder.owner,
            "001_!ООО Викторина"
        );
    }
    #[test]
    fn surname_initials_and_yo() {
        assert!(
            index().search(&Card::default(), "ИП Сидоров А.П.", &[])[0]
                .folder
                .owner
                .contains("Сидоров")
        );
        assert_eq!(
            index().search(&Card::default(), "елочка", &[])[0]
                .folder
                .owner,
            "ООО Ёлочка"
        );
    }
    #[test]
    fn exact_inn_wins_and_top_ten_bound() {
        let c = Card {
            inn: "1234567890".into(),
            ..Default::default()
        };
        assert!(
            index().search(&c, "", &[])[0]
                .folder
                .owner
                .ends_with("1234567890")
        );
        assert!(index().search(&c, "", &[]).len() <= 10);
    }
}
