use anyhow::Result;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        args.len() == 3,
        "print_check <input.pdf> <local-output.pdf>"
    );
    let output = std::path::PathBuf::from(&args[2]);
    anyhow::ensure!(!output.exists(), "Output already exists");
    let bytes = std::fs::read(&args[1])?;
    println!(
        "{}",
        kkt_saver::pdf::print_document(
            &bytes,
            "KKT Saver virtual print verification",
            "Microsoft Print to PDF",
            Some(&output)
        )?
    );
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(20) {
        if let Ok(b) = std::fs::read(&output)
            && b.starts_with(b"%PDF-")
            && let Ok(pdf) = kkt_saver::pdf::inspect(&b)
        {
            println!("Printed PDF: {} pages, {} bytes", pdf.pages, b.len());
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    anyhow::bail!("Virtual printer did not produce a readable PDF")
}
