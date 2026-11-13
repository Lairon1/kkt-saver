use anyhow::{Context, Result, bail};
use std::{ffi::OsString, path::PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("Ошибка: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let value = |flag: &str| -> Result<PathBuf> {
        let at = args
            .iter()
            .position(|a| a == flag)
            .with_context(|| format!("Не указан {flag}"))?;
        args.get(at + 1)
            .map(PathBuf::from)
            .with_context(|| format!("Не указан путь после {flag}"))
    };
    match args.first().and_then(|a| a.to_str()) {
        Some("analyze") => {
            let root = value("--root")?;
            let reports = value("--report-dir")?;
            let plan = kkt_saver::organizer::analyze(&root, &reports, |done, total| {
                println!("Анализ: {done} / {total}")
            })?;
            println!(
                "{}\nОтчёт: {}",
                kkt_saver::organizer::summary(&plan),
                reports.join("report.html").display()
            );
        }
        Some("apply") => {
            let plan = value("--plan")?;
            let backup_confirmed = args.iter().any(|a| a == "--backup-confirmed");
            kkt_saver::organizer::apply(&plan, backup_confirmed, |done, total| {
                println!("Применено: {done} / {total}")
            })?;
            println!("Готово. Все сохранённые файлы проверены по SHA-256 и датам.");
        }
        Some("verify") => {
            kkt_saver::organizer::verify(&value("--plan")?)?;
            println!("Проверка содержимого и дат завершена.");
        }
        _ => bail!(
            "Команды:\n  analyze --root <папка> --report-dir <отчёты вне архива>\n  apply --plan <plan.json> --backup-confirmed\n  verify --plan <plan.json>"
        ),
    }
    Ok(())
}
