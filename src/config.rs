use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_ROOT: &str = r"Z:\!Общие документы\!!!ЦТО\!КАРТОЧКИ РЕГИСТРАЦИИ";

pub fn data_dir() -> PathBuf {
    // The override lets diagnostics and integration tests use a fully isolated profile.
    std::env::var_os("KKT_SAVER_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                std::env::var_os("LOCALAPPDATA")
                    .unwrap_or_else(|| std::env::temp_dir().into_os_string()),
            )
            .join("KktSaver")
        })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Choice {
    pub inn: String,
    pub organization: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub root: PathBuf,
    pub print: bool,
    pub move_source: bool,
    pub auto_next: bool,
    pub choices: Vec<Choice>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            root: DEFAULT_ROOT.into(),
            print: false,
            move_source: false,
            auto_next: false,
            choices: Vec::new(),
        }
    }
}
impl Config {
    pub fn load() -> Self {
        std::fs::read(data_dir().join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> anyhow::Result<()> {
        write_json("settings.json", self)
    }
    pub fn remember(&mut self, inn: &str, organization: &str, path: PathBuf) {
        if inn.is_empty() && organization.is_empty() {
            return;
        }
        self.choices
            .retain(|c| !(c.inn == inn && c.organization == organization));
        self.choices.insert(
            0,
            Choice {
                inn: inn.into(),
                organization: organization.into(),
                path,
            },
        );
        self.choices.truncate(300);
    }
}
pub fn write_json(name: &str, value: &impl Serialize) -> anyhow::Result<()> {
    write_json_in(&data_dir(), name, value)
}
pub(crate) fn write_json_in(
    dir: &std::path::Path,
    name: &str,
    value: &impl Serialize,
) -> anyhow::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut tmp, value)?;
    tmp.flush()?;
    tmp.as_file().sync_all()?;
    tmp.persist(dir.join(name)).map_err(|e| e.error)?;
    Ok(())
}
