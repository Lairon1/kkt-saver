use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};

pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub fn file_digest(path: &Path) -> Result<[u8; 32]> {
    digest_reader(File::open(path)?)
}
fn digest_reader(mut f: impl Read) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().into())
}
pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.len() > 220
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        bail!("Недопустимое имя файла. Уберите служебные символы; длина — до 220 байт.");
    }
    if !name.to_lowercase().ends_with(".pdf") {
        bail!("Имя должно оканчиваться на .pdf");
    }
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
    {
        bail!("Это имя зарезервировано Windows.");
    }
    Ok(())
}

#[derive(Debug)]
pub struct Saved {
    pub path: PathBuf,
    pub removed: bool,
    pub existed: bool,
    pub warning: Option<String>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy()),
        _ => false,
    }
}

pub fn save(
    source: &Path,
    expected: [u8; 32],
    folder: &Path,
    name: &str,
    move_source: bool,
) -> Result<Saved> {
    save_with_options(source, expected, folder, name, move_source, None, true)
}

pub fn save_dated(
    source: &Path,
    expected: [u8; 32],
    folder: &Path,
    name: &str,
    move_source: bool,
    date: crate::dates::CardDate,
) -> Result<Saved> {
    save_with_options(
        source,
        expected,
        folder,
        name,
        move_source,
        Some(date),
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn save_with_options(
    source: &Path,
    expected: [u8; 32],
    folder: &Path,
    name: &str,
    move_source: bool,
    date: Option<crate::dates::CardDate>,
    accept_identical: bool,
) -> Result<Saved> {
    validate_name(name)?;
    if !folder.is_dir() {
        bail!(
            "Папка недоступна: {}. Проверьте подключение к диску.",
            folder.display()
        );
    }
    // Freeze and verify the exact file that the user reviewed before touching the destination.
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_INFO, FILE_GENERIC_READ, FILE_SHARE_READ, FILE_WRITE_ATTRIBUTES,
        FileDispositionInfo, SetFileInformationByHandle,
    };
    if move_source && fs::symlink_metadata(source)?.file_type().is_symlink() {
        bail!("Для переноса выберите сам PDF, а не символическую ссылку на него.");
    }
    // Deny concurrent writes/deletions for the entire transaction. Delete the opened file
    // by handle, so a replacement at the source path can never be deleted accidentally.
    let try_delete = move_source.then(|| {
        OpenOptions::new()
            .access_mode(
                FILE_GENERIC_READ
                    | DELETE
                    | if date.is_some() {
                        FILE_WRITE_ATTRIBUTES
                    } else {
                        0
                    },
            )
            .share_mode(FILE_SHARE_READ)
            .open(source)
    });
    let (mut source_file, can_delete) = match try_delete {
        Some(Ok(f)) => (f, true),
        _ => (
            OpenOptions::new()
                .access_mode(
                    FILE_GENERIC_READ
                        | if date.is_some() && same_path(source, &folder.join(name)) {
                            FILE_WRITE_ATTRIBUTES
                        } else {
                            0
                        },
                )
                .share_mode(FILE_SHARE_READ)
                .open(source)
                .context(
                    "Не удалось открыть исходник; закройте программы, которые изменяют этот PDF",
                )?,
            false,
        ),
    };
    let mut bytes = Vec::new();
    source_file
        .read_to_end(&mut bytes)
        .context("Не удалось прочитать исходный файл; он сохранён на прежнем месте")?;
    if digest(&bytes) != expected {
        bail!("Исходный файл изменился после чтения. Откройте карточку заново.");
    }
    let stem = Path::new(name).file_stem().unwrap().to_string_lossy();
    let mut dest = folder.join(name);
    let mut existed = false;
    let mut completed = false;
    // create_new staging + persist_noclobber protect existing files even across concurrent saves.
    let mut tmp = tempfile::NamedTempFile::new_in(folder)
        .context("Нет доступа на запись в выбранную папку")?;
    tmp.write_all(&bytes)?;
    tmp.flush()?;
    tmp.as_file().sync_all()?;
    if file_digest(tmp.path())? != expected {
        bail!("Ошибка проверки копии. Исходный файл сохранён.");
    }
    for n in 1..=10000 {
        if dest.exists() {
            if (accept_identical || same_path(source, &dest))
                && file_digest(&dest).ok() == Some(expected)
            {
                existed = true;
                completed = true;
                break;
            }
        } else {
            match tmp.persist_noclobber(&dest) {
                Ok(_) => {
                    completed = true;
                    break;
                }
                Err(e) => {
                    let exists = e.error.kind() == std::io::ErrorKind::AlreadyExists;
                    tmp = e.file;
                    if !exists {
                        return Err(e.error)
                            .context("Не удалось завершить сохранение; исходный файл не удалён");
                    }
                    continue;
                }
            }
        }
        dest = folder.join(format!("{stem} ({}).pdf", n + 1));
    }
    if !completed {
        bail!("Слишком много файлов с одинаковым именем. Измените имя.");
    }
    // The same path is already locked by source_file, which may own DELETE access.
    let same = same_path(source, &dest);
    let destination_guard = if same {
        None
    } else {
        Some(OpenOptions::new().access_mode(FILE_GENERIC_READ | if date.is_some() { FILE_WRITE_ATTRIBUTES } else { 0 }).share_mode(FILE_SHARE_READ).open(&dest).context("Копия записана, но заблокировать её для проверки даты не удалось; исходник не удалён")?)
    };
    if !same && digest_reader(destination_guard.as_ref().unwrap())? != expected {
        bail!("Сохранённый файл не прошёл проверку. Исходный файл не удалён.");
    }
    let mut result = Saved {
        path: dest,
        removed: false,
        existed,
        warning: None,
    };
    if let Some(date) = date {
        let file = destination_guard.as_ref().unwrap_or(&source_file);
        if let Err(e) = set_card_time(file, date) {
            result.warning = Some(format!(
                "Карточка сохранена, но дату создания/изменения установить не удалось: {e:#}. Исходник оставлен на прежнем месте."
            ));
            return Ok(result);
        }
    }
    if move_source && !same {
        if !can_delete {
            result.warning=Some("Копия сохранена, но нет доступа на удаление исходника. Он оставлен на прежнем месте.".into());
        } else {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            if unsafe {
                SetFileInformationByHandle(
                    source_file.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } != 0
            {
                result.removed = true;
            } else {
                result.warning = Some(format!(
                    "Копия сохранена, но исходник удалить не удалось: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
    }
    Ok(result)
}

/// Creation and last-write times are set together; PDF bytes are untouched.
pub fn set_card_time(file: &File, date: crate::dates::CardDate) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::SetFileTime};
    let ticks = date.windows_ticks()?;
    let time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    if unsafe { SetFileTime(file.as_raw_handle(), &time, std::ptr::null(), &time) } == 0 {
        return Err(std::io::Error::last_os_error()).context("Windows не установила даты файла");
    }
    let meta = file.metadata()?;
    if meta.creation_time() != ticks || meta.last_write_time() != ticks {
        bail!("Файловая система не сохранила даты карточки точно");
    }
    Ok(())
}

/// Organizer transaction for a fixed target. Identical cards remain separate.
/// `resume` is allowed only after the caller durably journals this exact operation.
pub fn relocate_exact(
    source: &Path,
    target: &Path,
    expected: [u8; 32],
    date: Option<crate::dates::CardDate>,
    resume: bool,
) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_SHARE_READ, FILE_WRITE_ATTRIBUTES,
    };
    crate::archive::check_path(source)?;
    crate::archive::check_path(target)?;
    let file = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE | FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .open(source)
        .context("Не удалось заблокировать исходный файл для переноса")?;
    if digest_reader(&file)? != expected {
        bail!("Исходник изменился после анализа: {}", source.display());
    }
    if same_path(source, target) {
        if let Some(date) = date {
            set_card_time(&file, date)?;
        }
        return Ok(());
    }
    let parent = target.parent().context("Не определена папка назначения")?;
    crate::archive::ensure_directory(parent)?;
    if !target.exists() {
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        let mut reader = &file;
        reader.seek(SeekFrom::Start(0))?;
        std::io::copy(&mut reader, &mut tmp)?;
        tmp.flush()?;
        tmp.as_file().sync_all()?;
        if file_digest(tmp.path())? != expected {
            bail!("Ошибка проверки копии");
        }
        tmp.persist_noclobber(target)
            .map_err(|e| e.error)
            .context("Файл назначения уже существует или недоступен; исходник сохранён")?;
    } else if !resume {
        bail!(
            "Путь назначения занят; исходник сохранён: {}",
            target.display()
        );
    }
    let guard = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | FILE_WRITE_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .open(target)?;
    if digest_reader(&guard)? != expected {
        bail!("Копия отличается; исходник сохранён");
    }
    if let Some(date) = date {
        set_card_time(&guard, date)?;
    } else {
        preserve_times(&file, &guard)?;
    }
    mark_deleted(&file).context("Копия сохранена, но исходник остался на месте")?;
    Ok(())
}

fn preserve_times(source: &File, target: &File) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::SetFileTime};
    let meta = source.metadata()?;
    let ft = |ticks: u64| FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let created = ft(meta.creation_time());
    let modified = ft(meta.last_write_time());
    if unsafe {
        SetFileTime(
            target.as_raw_handle(),
            &created,
            std::ptr::null(),
            &modified,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("Не удалось сохранить исходные даты");
    }
    Ok(())
}

fn mark_deleted(file: &File) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("Не удалось удалить открытый файл");
    }
    Ok(())
}

pub fn remove_verified(source: &Path, expected: [u8; 32]) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_READ, FILE_SHARE_READ};
    crate::archive::check_path(source)?;
    let file = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ)
        .open(source)?;
    if digest_reader(&file)? != expected {
        bail!(
            "Файл изменился после анализа; удаление отменено: {}",
            source.display()
        );
    }
    mark_deleted(&file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dated_copy_move_and_same_path_keep_pdf_bytes_and_set_both_times() {
        use std::os::windows::fs::MetadataExt;
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("old.pdf");
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        fs::write(&source, b"unaltered signed PDF bytes").unwrap();
        let hash = file_digest(&source).unwrap();
        let date = crate::dates::CardDate::parse("26.07.2019").unwrap();
        let saved = save_dated(&source, hash, &out, "card.pdf", true, date).unwrap();
        assert!(saved.warning.is_none(), "{:?}", saved.warning);
        assert!(saved.removed);
        assert!(!source.exists());
        assert_eq!(file_digest(&saved.path).unwrap(), hash);
        let meta = fs::metadata(&saved.path).unwrap();
        assert_eq!(meta.creation_time(), date.windows_ticks().unwrap());
        assert_eq!(meta.last_write_time(), date.windows_ticks().unwrap());
        let next_date = crate::dates::CardDate::parse("24.12.2025").unwrap();
        let same = save_dated(&saved.path, hash, &out, "card.pdf", true, next_date).unwrap();
        assert!(!same.removed && same.warning.is_none());
        assert_eq!(
            fs::metadata(&same.path).unwrap().creation_time(),
            next_date.windows_ticks().unwrap()
        );
    }
    #[test]
    fn date_error_keeps_source_and_fixed_target_recovery_is_verified() {
        let t = tempfile::tempdir().unwrap();
        let source = t.path().join("one.pdf");
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        fs::write(&source, b"card").unwrap();
        let hash = file_digest(&source).unwrap();
        let bad = crate::dates::CardDate {
            year: 2025,
            month: 2,
            day: 30,
        };
        let saved = save_dated(&source, hash, &out, "card.pdf", true, bad).unwrap();
        assert!(saved.warning.is_some());
        assert!(!saved.removed && source.exists());
        let date = crate::dates::CardDate::parse("06.02.2025").unwrap();
        assert!(relocate_exact(&source, &saved.path, hash, Some(date), false).is_err());
        assert!(source.exists());
        relocate_exact(&source, &saved.path, hash, Some(date), true).unwrap();
        assert!(!source.exists());
        assert_eq!(file_digest(&saved.path).unwrap(), hash);
    }
    #[test]
    fn copy_move_collision_and_same_path() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("source.pdf");
        let out = t.path().join("out");
        fs::create_dir(&out).unwrap();
        fs::write(&src, b"original").unwrap();
        let hash = digest(b"original");
        fs::write(out.join("card.pdf"), b"different").unwrap();
        let a = save(&src, hash, &out, "card.pdf", false).unwrap();
        assert_eq!(a.path.file_name().unwrap(), "card (2).pdf");
        assert!(src.exists());
        assert_eq!(fs::read(out.join("card.pdf")).unwrap(), b"different");
        let b = save(&src, hash, &out, "card.pdf", true).unwrap();
        assert!(b.existed && b.removed);
        assert!(!src.exists());
        let c = save(&a.path, hash, &out, "card (2).pdf", true).unwrap();
        assert!(!c.removed);
        assert!(a.path.exists());
    }
    #[test]
    fn changed_source_never_saved_or_deleted() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("a.pdf");
        fs::write(&src, b"changed").unwrap();
        assert!(save(&src, digest(b"old"), t.path(), "b.pdf", true).is_err());
        assert!(src.exists());
        assert!(!t.path().join("b.pdf").exists());
    }
    #[test]
    fn path_traversal_and_reserved_names_rejected() {
        for n in [
            "../a.pdf",
            "C:\\a.pdf",
            "NUL.pdf",
            "a.pdf.",
            "a.txt",
            "a/b.pdf",
        ] {
            assert!(validate_name(n).is_err(), "{n}");
        }
    }
}
