use anyhow::{Context, Result, bail};
use regex::Regex;
use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

pub fn suggested_name(organization: &str, inn: &str) -> String {
    static LEGAL_FORMS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?i)общество\s+с\s+ограниченной\s+ответственностью", "ООО"),
            (r"(?i)индивидуальный\s+предприниматель", "ИП"),
            (r"(?i)непубличное\s+акционерное\s+общество", "АО"),
            (r"(?i)публичное\s+акционерное\s+общество", "ПАО"),
            (r"(?i)открытое\s+акционерное\s+общество", "ОАО"),
            (r"(?i)закрытое\s+акционерное\s+общество", "ЗАО"),
            (r"(?i)акционерное\s+общество", "АО"),
            (r"(?i)муниципальное\s+унитарное\s+предприятие", "МУП"),
            (
                r"(?i)федеральное\s+государственное\s+унитарное\s+предприятие",
                "ФГУП",
            ),
            (r"(?i)государственное\s+унитарное\s+предприятие", "ГУП"),
            (r"(?i)потребительское\s+общество", "ПО"),
        ]
        .into_iter()
        .map(|(pattern, replacement)| (Regex::new(pattern).unwrap(), replacement))
        .collect()
    });
    let mut name = organization.to_string();
    for (pattern, replacement) in LEGAL_FORMS.iter() {
        name = pattern.replace_all(&name, *replacement).into_owned();
    }
    name = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*«»„“”".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    name = name.trim_matches([' ', '.']).to_string();
    // Existing archive folders often begin with service numbers and punctuation.
    static PREFIX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*[0-9]+[ _!.)-]+\s*!?\s*").unwrap());
    name = PREFIX
        .replace(&name, "")
        .trim_start_matches(['!', ' '])
        .to_string();
    static NOTES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\((?:безвозмездно|расторгнут[ао]?|бывш(?:ий|ая)|архив)\)").unwrap()
    });
    name = NOTES
        .replace_all(&name, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    static FORM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^(ип|ооо|пао|оао|зао|ао|муп|фгуп|гуп|по)\b").unwrap());
    if let Some(form) = FORM.find(&name) {
        name.replace_range(form.range(), &form.as_str().to_uppercase());
    }
    if name.is_empty() {
        return "Новая организация".into();
    }
    if inn.len() == 12 && !name.to_uppercase().starts_with("ИП ") {
        name = format!("ИП {name}");
    }
    // Windows limits a single path component to 255 UTF-16 code units.
    let mut units = 0;
    name = name
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 150
        })
        .collect();
    name.trim_end_matches([' ', '.']).to_string()
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || matches!(name, "." | "..") {
        bail!("Введите название новой папки.");
    }
    if name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        bail!("В имени папки нельзя использовать <>:\"/\\|?* и точку или пробел в конце.");
    }
    if name.encode_utf16().count() > 255 {
        bail!("Название папки слишком длинное. Сократите его.");
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    let device = ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            suffix.chars().count() == 1
                && suffix.contains(['1', '2', '3', '4', '5', '6', '7', '8', '9', '¹', '²', '³'])
        })
    });
    if device || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str()) {
        bail!("Это название зарезервировано Windows. Укажите другое.");
    }
    Ok(())
}

/// Creates exactly one child directory. Existing files and directories are
/// never replaced, and a mistyped parent path is never created implicitly.
pub fn create(parent: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    if !parent.is_absolute() {
        bail!("Укажите полный путь к родительской папке.");
    }
    if !parent.is_dir() {
        bail!(
            "Родительская папка недоступна: {}. Проверьте путь и подключение к диску.",
            parent.display()
        );
    }
    let path = parent.join(name);
    match std::fs::create_dir(&path) {
        Ok(()) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!(
                "Папка или файл с таким названием уже существует. Укажите другое имя или выберите существующую папку вручную."
            )
        }
        Err(e) => Err(e).with_context(|| format!("Не удалось создать папку {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_one_child_and_preserves_existing_content() {
        let temp = tempfile::tempdir().unwrap();
        let name = suggested_name(
            "ОБЩЕСТВО С ОГРАНИЧЕННОЙ ОТВЕТСТВЕННОСТЬЮ \"НОВАЯ\"",
            "1234567890",
        );
        assert_eq!(name, "ООО НОВАЯ");
        assert_eq!(
            suggested_name("ИВАНОВ ИВАН ИВАНОВИЧ", "123456789012"),
            "ИП ИВАНОВ ИВАН ИВАНОВИЧ"
        );
        let path = create(temp.path(), &name).unwrap();
        assert!(path.is_dir());
        std::fs::write(path.join("existing.txt"), b"keep").unwrap();
        assert!(create(temp.path(), &name).is_err());
        assert_eq!(std::fs::read(path.join("existing.txt")).unwrap(), b"keep");
        for bad in [
            "",
            "..",
            "../escape",
            r"parent\child",
            "NUL",
            "CON.txt",
            "COM¹",
            "bad.",
        ] {
            assert!(create(temp.path(), bad).is_err(), "Accepted {bad}");
        }
        assert!(create(&temp.path().join("missing-parent"), "child").is_err());
        assert!(!temp.path().join("missing-parent").exists());
    }
}
