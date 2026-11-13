#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
fn main() {
    if let Err(e) = run() {
        let message = format!("{e:#}");
        let _ = std::fs::create_dir_all(kkt_saver::config::data_dir());
        let _ = std::fs::write(
            kkt_saver::config::data_dir().join("last-error.txt"),
            &message,
        );
        if std::env::args_os().len() == 1 {
            rfd::MessageDialog::new()
                .set_title("Карточки ККТ — ошибка")
                .set_description(&message)
                .set_level(rfd::MessageLevel::Error)
                .show();
        } else {
            eprintln!("{message}");
        }
        std::process::exit(1);
    }
}
fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--inspect") {
        let path = args
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("--inspect <PDF> [output.json]"))?;
        let info = kkt_saver::pdf::inspect(&std::fs::read(path)?)?;
        let card = kkt_saver::card::Card::parse(&info.text);
        let output = serde_json::to_vec_pretty(
            &serde_json::json!({"pages":info.pages,"card":card,"text":info.text}),
        )?;
        if let Some(path) = args.get(3) {
            std::fs::write(path, output)?;
        } else {
            use std::io::Write;
            std::io::stdout().write_all(&output)?;
        }
        return Ok(());
    }
    app::run(args.get(1).map(std::path::PathBuf::from))
}
