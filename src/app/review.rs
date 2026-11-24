//! Debug-only render review. Captures the application's own framebuffer, then
//! closes. This never saves, moves or prints a card, or creates an archive folder,
//! and is absent from release.
use super::*;

impl SaverApp {
    /// Exercises the actual native window and the shared tray dispatcher. A
    /// worker drives the sequence so hiding cannot stall the check itself.
    pub(super) fn start_tray_check(&self) {
        let Some(dir) = std::env::var_os("KKT_SAVER_TRAY_CHECK_DIR").map(PathBuf::from) else {
            return;
        };
        let Some(window) = self.native_window.filter(|_| self.tray.is_some()) else {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(
                dir.join("tray-check.json"),
                r#"{"passed":false,"error":"Native tray unavailable"}"#,
            );
            return;
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let hidden_to_tray = self.hidden_to_tray.clone();
        let hidden_hold = std::env::var("KKT_SAVER_TRAY_CHECK_HIDDEN_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(|millis| Duration::from_millis(millis.min(120_000)))
            .unwrap_or(Duration::from_millis(300));
        std::thread::spawn(move || {
            let mut steps = Vec::new();
            let wait_visible = |expected| -> anyhow::Result<()> {
                let deadline = Instant::now() + Duration::from_secs(5);
                while window.visible() != expected {
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "Window visibility did not become {expected}"
                    );
                    std::thread::sleep(Duration::from_millis(25));
                }
                Ok(())
            };
            let result = (|| -> anyhow::Result<()> {
                std::fs::create_dir_all(&dir)?;
                wait_visible(true)?;
                anyhow::ensure!(
                    window.dark_mode_enabled(),
                    "Native title bar dark mode is disabled"
                );
                steps.push("native-dark-titlebar".to_owned());
                for cycle in 1..=3 {
                    window.request_close();
                    wait_visible(false)?;
                    std::thread::sleep(hidden_hold);
                    dispatch_tray_action(Event::Show, &tx, &ctx, window, &hidden_to_tray);
                    wait_visible(true)?;
                    std::thread::sleep(Duration::from_millis(300));
                    anyhow::ensure!(window.visible(), "Window hid again after restoration");
                    steps.push(format!("hide-and-restore-{cycle}"));
                }
                window.request_close();
                wait_visible(false)?;
                steps.push("quit-requested-while-hidden".to_owned());
                Ok(())
            })();
            let report = serde_json::json!({
                "passed": result.is_ok(), "steps": steps,
                "error": result.err().map(|error| format!("{error:#}")),
            });
            let _ = std::fs::write(
                dir.join("tray-check.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            dispatch_tray_action(Event::Quit, &tx, &ctx, window, &hidden_to_tray);
        });
    }

    pub(super) fn record_tray_check_exit(&self) {
        if let Some(dir) = std::env::var_os("KKT_SAVER_TRAY_CHECK_DIR").map(PathBuf::from) {
            let _ = std::fs::write(
                dir.join("exit-completed.txt"),
                "Orderly eframe on_exit completed\n",
            );
        }
    }

    pub(super) fn capture_for_review(&mut self, ctx: &egui::Context) {
        let Some(dir) = std::env::var_os("KKT_SAVER_CAPTURE_DIR").map(PathBuf::from) else {
            return;
        };
        let queue_id = egui::Id::new("render-review-queue-seeded");
        if !ctx.data(|data| data.get_temp::<bool>(queue_id).unwrap_or(false)) {
            if let Some(paths) = std::env::var_os("KKT_SAVER_REVIEW_QUEUE") {
                self.pending.extend(std::env::split_paths(&paths));
            }
            ctx.data_mut(|data| data.insert_temp(queue_id, true));
            ctx.request_repaint();
            return;
        }
        let id = egui::Id::new("render-review-stage");
        let mut stage = ctx.data(|data| data.get_temp::<u8>(id).unwrap_or(0));
        let stage_before_events = stage;
        for event in ctx.input(|i| i.raw.events.clone()) {
            if let egui::Event::Screenshot {
                user_data, image, ..
            } = event
                && let Some(name) = user_data
                    .data
                    .as_ref()
                    .and_then(|data| data.downcast_ref::<String>())
            {
                let result = (|| -> std::io::Result<()> {
                    use std::io::Write;
                    std::fs::create_dir_all(&dir)?;
                    let mut file = std::fs::File::create(dir.join(format!("{name}.ppm")))?;
                    write!(file, "P6\n{} {}\n255\n", image.size[0], image.size[1])?;
                    let rgb: Vec<_> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| [pixel.r(), pixel.g(), pixel.b()])
                        .collect();
                    file.write_all(&rgb)
                })();
                if let Err(e) = result {
                    let _ = std::fs::write(dir.join("capture-error.txt"), e.to_string());
                }
                if stage == 1 && name == "01-card" {
                    self.choose();
                    stage = 2;
                } else if stage == 3 && name == "02-folders" {
                    self.begin_new_folder();
                    ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::new("review-modal-opened"), Instant::now())
                    });
                    stage = 4;
                } else if stage == 5 && name == "03-create-folder" {
                    self.exiting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    stage = 6;
                }
            }
        }
        if stage == 0 && self.loaded.is_some() && !self.loading && !self.scanning {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "01-card".to_string(),
            )));
            stage = 1;
        } else if stage == 2
            && !self.searching
            && self.query_due.is_none()
            && !self.matches.is_empty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "02-folders".to_string(),
            )));
            stage = 3;
        } else if stage == 4
            && stage_before_events == 4
            && self.new_folder.is_some()
            && ctx
                .data(|data| data.get_temp::<Instant>(egui::Id::new("review-modal-opened")))
                .is_some_and(|opened| opened.elapsed() >= Duration::from_millis(250))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "03-create-folder".to_string(),
            )));
            stage = 5;
        }
        ctx.data_mut(|data| data.insert_temp(id, stage));
    }
}
