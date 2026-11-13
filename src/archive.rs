use crate::{card::Card, folders};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    os::windows::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

pub const REVIEW_FOLDER: &str = "Неразобранные";

pub fn is_reparse(meta: &fs::Metadata) -> bool {
    meta.file_attributes() & 0x400 != 0
}

/// Check every existing component: junctions must not redirect a cleanup or save.
pub fn check_path(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("Требуется абсолютный путь: {}", path.display());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir | Component::CurDir) {
            bail!("Путь содержит . или ..");
        }
        current.push(part);
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if is_reparse(&meta) => {
                bail!("Ссылки и junction не поддерживаются: {}", current.display())
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => {
                return Err(e).with_context(|| format!("Проверка пути {}", current.display()));
            }
        }
    }
    Ok(())
}

pub fn valid_registration(value: &str) -> bool {
    value.len() == 16 && value.chars().all(|c| c.is_ascii_digit())
}

/// Resolve a chosen organization (or its old child) to organization / RNM.
/// No directory is created by this function; the UI can show the exact result.
pub fn destination(root: &Path, selected: &Path, card: &Card) -> Result<PathBuf> {
    if !valid_registration(&card.registration) {
        bail!("Укажите РНМ ККТ: ровно 16 цифр, включая ведущие нули.");
    }
    let organization = if selected == root {
        if card.organization.trim().is_empty() {
            bail!("Укажите организацию или выберите её папку.");
        }
        root.join(folders::suggested_name(&card.organization, &card.inn))
    } else {
        let selected = if let Ok(relative) = selected.strip_prefix(root) {
            let Some(Component::Normal(first)) = relative.components().next() else {
                bail!("Выберите папку организации");
            };
            root.join(first)
        } else if selected
            .file_name()
            .is_some_and(|n| valid_registration(&n.to_string_lossy()))
        {
            selected
                .parent()
                .context("Не определена папка организации")?
                .to_path_buf()
        } else {
            selected.to_path_buf()
        };
        let name = selected
            .file_name()
            .context("Выберите папку организации")?
            .to_string_lossy();
        if name == REVIEW_FOLDER {
            bail!("Выберите папку организации, а не «Неразобранные».");
        }
        let clean = folders::suggested_name(&name, &card.inn);
        folders::validate_name(&clean)?;
        selected.with_file_name(clean)
    };
    Ok(organization.join(&card.registration))
}

pub fn ensure_directory(path: &Path) -> Result<()> {
    check_path(path)?;
    fs::create_dir_all(path)
        .with_context(|| format!("Не удалось создать папку {}", path.display()))?;
    check_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_nested_folder_resolves_to_clean_sibling_and_rnm() {
        let root = Path::new(r"C:\archive");
        let card = Card {
            registration: "0001234567890123".into(),
            inn: "1234567890".into(),
            ..Default::default()
        };
        assert_eq!(
            destination(root, &root.join(r"045_ООО Гонг\2020"), &card).unwrap(),
            root.join(r"ООО Гонг\0001234567890123")
        );
        assert_eq!(
            destination(root, &root.join(r"ООО Гонг\0001234567890123"), &card).unwrap(),
            root.join(r"ООО Гонг\0001234567890123")
        );
        assert!(!valid_registration("1234567890123"));
    }
}
