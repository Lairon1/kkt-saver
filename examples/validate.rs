//! Read-only archive benchmark. PDF samples and reports stay outside the archive.
use anyhow::Result;
use kkt_saver::{card::Card, config, pdf, search};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        args.len() == 4,
        "validate <samples-directory> <archive-root> <report.json>"
    );
    let samples = PathBuf::from(&args[1]);
    let root = PathBuf::from(&args[2]);
    let mut cards = Vec::new();
    for entry in std::fs::read_dir(samples)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "pdf") {
            let bytes = std::fs::read(&path)?;
            let info = pdf::inspect(&bytes)?;
            let card = Card::parse(&info.text);
            anyhow::ensure!(
                !card.organization.is_empty(),
                "Organization missing: {}",
                path.display()
            );
            anyhow::ensure!(
                [10, 12].contains(&card.inn.len()),
                "INN missing: {}",
                path.display()
            );
            anyhow::ensure!(
                card.registration.len() == 16,
                "Registration missing: {}",
                path.display()
            );
            cards.push((path, card, info.pages));
        }
    }
    let start = Instant::now();
    let cache = search::scan(&root, Arc::new(AtomicBool::new(false)), |_, p| {
        if p.finished {
            eprintln!(
                "Scanned {} directories; skipped {}",
                p.directories, p.skipped
            );
        }
    })?;
    let scan_ms = start.elapsed().as_millis();
    cache.save()?;
    let folder_count = cache.folders.len();
    let start = Instant::now();
    let index = search::SearchIndex::new(cache.folders);
    let index_ms = start.elapsed().as_millis();
    let mut reports = Vec::new();
    let mut timings = Vec::new();
    for (path, card, pages) in cards {
        let mut matches = Vec::new();
        for i in 0..25 {
            let t = Instant::now();
            let m = index.search(&card, &card.organization, &[]);
            timings.push(t.elapsed().as_micros());
            if i == 0 {
                matches = m;
            }
        }
        reports.push(serde_json::json!({"sample":path,"card":card,"pages":pages,"matches":matches.iter().map(|m|serde_json::json!({"path":m.folder.path,"score":m.score,"reasons":m.reasons})).collect::<Vec<_>>()}));
    }
    timings.sort_unstable();
    let report = serde_json::json!({"folder_count":folder_count,"scan_ms":scan_ms,"index_build_ms":index_ms,"median_search_us":timings[timings.len()/2],"p95_search_us":timings[(timings.len()*95/100).min(timings.len()-1)],"samples":reports,"profile":config::data_dir()});
    std::fs::write(&args[3], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        serde_json::to_string(
            &serde_json::json!({"folder_count":folder_count,"scan_ms":scan_ms,"index_build_ms":index_ms,"median_us":report["median_search_us"],"p95_us":report["p95_search_us"]})
        )?
    );
    Ok(())
}
