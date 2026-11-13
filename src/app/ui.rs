use super::theme::*;
use super::*;
use egui::{Align, Align2, FontId, Frame, Layout, Margin, Sense, Stroke, Vec2};

fn panel_frame() -> Frame {
    Frame::new().fill(PANEL).inner_margin(12)
}
fn editor_frame() -> Frame {
    Frame::new().fill(EDITOR).inner_margin(18)
}
fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE))
        .fill(BLUE)
        .stroke(Stroke::NONE)
        .min_size(egui::vec2(210.0, 34.0))
}
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(15.0);
    ui.label(RichText::new(title).size(14.0).strong().color(TEXT));
    ui.separator();
    ui.add_space(3.0);
}
fn copy_field_button(ui: &mut egui::Ui, label: &str, value: &str) {
    let id = egui::Id::new("field-copy-feedback");
    let now = ui.input(|i| i.time);
    let response = ui.add_enabled(
        !value.is_empty(),
        egui::Button::new("")
            .small()
            .min_size(egui::vec2(28.0, 26.0)),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            response.enabled(),
            format!("Скопировать значение поля «{label}»"),
        )
    });
    if ui.is_rect_visible(response.rect) {
        let center = response.rect.center();
        let stroke = Stroke::new(
            1.3_f32,
            if response.enabled() {
                ui.style().interact(&response).fg_stroke.color
            } else {
                MUTED.gamma_multiply(0.45)
            },
        );
        ui.painter().add(egui::Shape::line(
            vec![
                center + egui::vec2(-5.0, 4.0),
                center + egui::vec2(-7.0, 4.0),
                center + egui::vec2(-7.0, -7.0),
                center + egui::vec2(3.0, -7.0),
                center + egui::vec2(3.0, -5.0),
            ],
            stroke,
        ));
        ui.painter().rect_stroke(
            egui::Rect::from_min_size(center + egui::vec2(-3.0, -3.0), egui::vec2(10.0, 11.0)),
            1.5,
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() {
        ui.ctx().copy_text(value.to_owned());
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, (now + 1.5, response.rect)));
    } else if !ui.ctx().data(|data| {
        data.get_temp::<(f64, egui::Rect)>(id)
            .is_some_and(|(until, _)| until > now)
    }) {
        response.on_hover_text(format!("Скопировать значение поля «{label}»"));
    }
}

fn copy_feedback(ctx: &egui::Context) {
    let id = egui::Id::new("field-copy-feedback");
    let Some((until, rect)) = ctx.data(|data| data.get_temp::<(f64, egui::Rect)>(id)) else {
        return;
    };
    let remaining = until - ctx.input(|i| i.time);
    if remaining <= 0.0 {
        ctx.data_mut(|data| data.remove::<(f64, egui::Rect)>(id));
        return;
    }
    egui::Area::new(egui::Id::new("field-copy-popup"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(rect.right_top() - egui::vec2(0.0, 6.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(PANEL)
                .stroke(Stroke::new(1.0_f32, BORDER))
                .corner_radius(5)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(RichText::new("Скопировано").color(GREEN));
                });
        });
    ctx.request_repaint_after(Duration::from_secs_f64(remaining));
}

fn field_rows(ui: &mut egui::Ui, id: &str, fields: &[(&str, &str)], show_empty: bool) {
    let value_width = (ui.available_width() - 155.0 - 28.0 - 36.0).max(80.0);
    egui::Grid::new(id)
        .num_columns(3)
        .spacing([18.0, 11.0])
        .min_col_width(0.0)
        .show(ui, |ui| {
            for (row, (label, value)) in fields.iter().enumerate() {
                if !show_empty && value.is_empty() {
                    continue;
                }
                ui.allocate_ui_with_layout(
                    egui::vec2(155.0, 18.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(155.0);
                        ui.add(egui::Label::new(RichText::new(*label).color(MUTED)).wrap());
                    },
                );
                let text = if value.is_empty() {
                    RichText::new("Не указано").color(MUTED)
                } else {
                    RichText::new(*value).color(TEXT)
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(value_width, 18.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(value_width);
                        ui.add(egui::Label::new(text).wrap().selectable(true));
                    },
                );
                ui.push_id((id, row), |ui| copy_field_button(ui, label, value));
                ui.end_row();
            }
        });
}
fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.14))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(color));
        });
}
fn editor_tab(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let response = ui.add(
        egui::Button::new(RichText::new(label).color(if selected { TEXT } else { MUTED }))
            .frame(false)
            .min_size(egui::vec2(140.0, 36.0)),
    );
    if selected {
        ui.painter().line_segment(
            [response.rect.left_bottom(), response.rect.right_bottom()],
            Stroke::new(3.0_f32, BLUE),
        );
    }
    response.clicked()
}

impl SaverApp {
    fn index_button(&mut self, ui: &mut egui::Ui) {
        let label = if self.scanning {
            "Индексация…"
        } else {
            "Обновить индекс"
        };
        if ui
            .add_enabled(!self.scanning, egui::Button::new(label))
            .on_hover_text("Запустить индексацию папок сейчас")
            .clicked()
        {
            self.start_scan();
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("main-toolbar")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(12, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    Frame::new()
                        .fill(BLUE)
                        .corner_radius(5)
                        .inner_margin(Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.label(RichText::new("ККТ").strong().color(Color32::WHITE));
                        });
                    ui.add_space(5.0);
                    ui.label(RichText::new("KKT Saver").strong());
                    ui.separator();
                    ui.menu_button("Файл", |ui| {
                        if ui
                            .add_enabled(
                                !self.busy && !self.loading,
                                egui::Button::new("Открыть PDF…             Ctrl+O"),
                            )
                            .clicked()
                        {
                            self.pick_file();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.loaded.is_some() && !self.saved && !self.busy,
                                egui::Button::new("Сохранить карточку…    Ctrl+S"),
                            )
                            .clicked()
                        {
                            self.choose();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Выйти из программы").clicked() {
                            let _ = self.tx.send(Event::Quit);
                            ui.close();
                        }
                    });
                    ui.menu_button("Вид", |ui| {
                        ui.checkbox(&mut self.documents_open, "Панель документов");
                        ui.checkbox(&mut self.preview_open, "Предпросмотр PDF");
                        if ui.button("Настройки…").clicked() {
                            self.settings_open = true;
                            ui.close();
                        }
                    });
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(
                            !self.busy && !self.loading,
                            egui::Button::new("Открыть PDF…"),
                        )
                        .on_hover_text("Ctrl+O")
                        .clicked()
                    {
                        self.pick_file();
                    }
                    self.index_button(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Настройки").clicked() {
                            self.settings_open = !self.settings_open;
                        }
                        ui.label(
                            RichText::new("Регистрация и снятие с учёта")
                                .small()
                                .color(MUTED),
                        );
                    });
                });
            });
    }

    fn status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status-bar")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let color = if self.error {
                        AMBER
                    } else if self.busy || self.loading {
                        BLUE
                    } else if self.saved {
                        GREEN
                    } else {
                        MUTED
                    };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, color);
                    ui.label(
                        RichText::new(if self.busy {
                            "Выполняется операция"
                        } else if self.loading {
                            "Чтение PDF"
                        } else if self.saved {
                            "Карточка сохранена"
                        } else {
                            "Готово"
                        })
                        .small()
                        .color(MUTED),
                    );
                    ui.separator();
                    if self.scanning {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                    ui.add(
                        egui::Label::new(RichText::new(&self.index_status).small().color(MUTED))
                            .truncate(),
                    )
                    .on_hover_text(&self.index_status);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("PDF  •  Windows x64").small().color(MUTED));
                    });
                });
            });
    }

    fn operations(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("save-tool-window")
            .frame(panel_frame())
            .resizable(true)
            .default_height(154.0)
            .min_height(136.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Сохранение").strong());
                    ui.label(RichText::new("Ctrl+S").small().color(MUTED));
                    if self.saved {
                        badge(ui, "СОХРАНЕНО", GREEN);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(path) = &self.last_saved
                            && ui.button("Открыть папку").clicked()
                            && let Some(parent) = path.parent()
                        {
                            open_folder(parent);
                        }
                        if self.print_retry
                            && ui
                                .add_enabled(!self.busy, egui::Button::new("Повторить печать"))
                                .clicked()
                        {
                            self.retry_print();
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.busy, |ui| {
                        let mut changed = switch(ui, &mut self.config.print, "Печатать карточку");
                        ui.add_space(7.0);
                        changed |= switch(
                            ui,
                            &mut self.config.move_source,
                            "Удалить исходник (перенести)",
                        );
                        if changed {
                            self.persist();
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if self.busy {
                            "Обработка…"
                        } else if self.saved {
                            "Карточка сохранена"
                        } else {
                            "Сохранить карточку"
                        };
                        if ui
                            .add_enabled(
                                self.loaded.is_some() && !self.busy && !self.loading && !self.saved,
                                primary(label),
                            )
                            .on_hover_text("Найти папку и подтвердить сохранение • Ctrl+S")
                            .clicked()
                        {
                            self.choose();
                        }
                        if self.busy || self.loading {
                            ui.spinner();
                        }
                    });
                });
                if self.config.print {
                    ui.label(
                        RichText::new(format!("Принтер: {}", self.printer))
                            .small()
                            .color(MUTED),
                    );
                }
                egui::ScrollArea::vertical()
                    .id_salt("operation-message")
                    .max_height(58.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&self.status).size(13.0).color(
                                if self.error {
                                    AMBER
                                } else if self.saved {
                                    GREEN
                                } else {
                                    MUTED
                                },
                            ))
                            .wrap(),
                        );
                    });
            });
    }

    fn tool_rail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tool-rail")
            .exact_width(42.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(5, 10)),
            )
            .show(ctx, |ui| {
                if rail_button(ui, 0, self.documents_open, "Документы").clicked() {
                    self.documents_open = !self.documents_open;
                }
                ui.add_space(5.0);
                if rail_button(ui, 1, self.preview_open, "Предпросмотр PDF").clicked() {
                    self.preview_open = !self.preview_open;
                }
                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    if rail_button(ui, 2, self.settings_open, "Настройки").clicked() {
                        self.settings_open = !self.settings_open;
                    }
                });
            });
    }

    fn documents(&mut self, ctx: &egui::Context) {
        if !self.documents_open {
            return;
        }
        egui::SidePanel::left("documents")
            .default_width(218.0)
            .width_range(180.0..=300.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Документы");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text("Скрыть панель")
                            .clicked()
                        {
                            self.documents_open = false;
                        }
                    });
                });
                ui.separator();
                ui.add_space(6.0);
                ui.label(RichText::new("ОТКРЫТАЯ КАРТОЧКА").size(10.0).color(MUTED));
                if let Some(loaded) = &self.loaded {
                    let color = if loaded.card.is_deregistration() {
                        AMBER
                    } else {
                        GREEN
                    };
                    Frame::new()
                        .fill(SELECTED)
                        .corner_radius(4)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("PDF").monospace().size(11.0).color(color));
                                ui.add(
                                    egui::Label::new(
                                        loaded
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(loaded.path.display().to_string());
                            });
                        });
                    ui.label(RichText::new(&loaded.card.kind).small().color(color));
                    if !loaded.card.inn.is_empty() {
                        ui.label(
                            RichText::new(format!("ИНН {}", loaded.card.inn))
                                .monospace()
                                .size(11.0)
                                .color(MUTED),
                        );
                    }
                } else {
                    ui.label(RichText::new("Файл не открыт").color(MUTED));
                }
                ui.add_space(12.0);
                ui.separator();
                ui.label(
                    RichText::new(format!("ОЧЕРЕДЬ ({})", self.pending.len()))
                        .size(10.0)
                        .color(MUTED),
                );
                ui.add_enabled_ui(!self.busy, |ui| {
                    if switch(
                        ui,
                        &mut self.config.auto_next,
                        "Следующая\nпосле сохранения",
                    ) {
                        self.persist();
                    }
                });
                ui.add_space(5.0);
                if self.pending.is_empty() {
                    ui.label(RichText::new("В очереди нет карточек").small().color(MUTED));
                } else {
                    let mut chosen = None;
                    let mut removed = None;
                    let can_open = !self.busy && !self.loading && self.new_folder.is_none();
                    let queue_height = (ui.available_height() - 130.0).clamp(64.0, 260.0);
                    let queue_width = ui.available_width();
                    egui::ScrollArea::vertical()
                        .id_salt("file-queue")
                        .max_height(queue_height)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (i, path) in self.pending.iter().enumerate() {
                                ui.push_id(("queued-card", path, i), |ui| {
                                    Frame::new()
                                        .fill(EDITOR)
                                        .stroke(Stroke::new(1.0_f32, BORDER))
                                        .corner_radius(4)
                                        .inner_margin(6)
                                        .show(ui, |ui| {
                                            ui.set_width((queue_width - 12.0).max(80.0));
                                            ui.horizontal_top(|ui| {
                                                let name_width = (queue_width - 42.0).max(50.0);
                                                let name = format!(
                                                    "{}.  {}",
                                                    i + 1,
                                                    path.file_name()
                                                        .unwrap_or_default()
                                                        .to_string_lossy()
                                                );
                                                let mut name = egui::text::LayoutJob::simple(
                                                    name,
                                                    FontId::proportional(13.0),
                                                    if can_open { TEXT } else { MUTED },
                                                    (name_width - 22.0).max(28.0),
                                                );
                                                name.wrap.break_anywhere = true;
                                                name.wrap.max_rows = 3;
                                                if ui
                                                    .allocate_ui_with_layout(
                                                        egui::vec2(name_width, 32.0),
                                                        Layout::top_down(Align::Min),
                                                        |ui| {
                                                            ui.set_width(name_width);
                                                            ui.add_enabled(
                                                                can_open,
                                                                egui::Button::new(name)
                                                                    .wrap()
                                                                    .frame(false)
                                                                    .min_size(egui::vec2(
                                                                        name_width, 32.0,
                                                                    )),
                                                            )
                                                        },
                                                    )
                                                    .inner
                                                    .on_hover_text(path.display().to_string())
                                                    .clicked()
                                                {
                                                    chosen = Some(i);
                                                }
                                                if ui
                                                    .add_sized(
                                                        [22.0, 24.0],
                                                        egui::Button::new("×").small(),
                                                    )
                                                    .on_hover_text("Убрать из очереди")
                                                    .clicked()
                                                {
                                                    removed = Some(i);
                                                }
                                            });
                                        });
                                });
                                ui.add_space(5.0);
                            }
                        });
                    if let Some(i) = removed {
                        self.pending.remove(i);
                    } else if let Some(i) = chosen {
                        self.open_queued(i);
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    if ui.button("Параметры архива…").clicked() {
                        self.settings_open = true;
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(self.config.root.display().to_string())
                                .small()
                                .color(MUTED),
                        )
                        .wrap(),
                    );
                    ui.label(RichText::new("АРХИВ КАРТОЧЕК").size(10.0).color(MUTED));
                    ui.separator();
                });
            });
    }

    fn preview_panel(&mut self, ctx: &egui::Context) {
        if !self.preview_open {
            return;
        }
        egui::SidePanel::right("pdf-tool-window")
            .default_width(345.0)
            .width_range(255.0..=600.0)
            .resizable(true)
            .frame(panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Предпросмотр PDF");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text("Скрыть предпросмотр")
                            .clicked()
                        {
                            self.preview_open = false;
                        }
                    });
                });
                ui.separator();
                if let Some(loaded) = &self.loaded {
                    let pages = loaded.pages;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.page > 0 && !self.preview_loading,
                                egui::Button::new("‹").min_size(egui::vec2(28.0, 26.0)),
                            )
                            .clicked()
                        {
                            self.request_preview(self.page - 1);
                        }
                        ui.label(
                            RichText::new(format!("{} / {}", self.page + 1, pages))
                                .monospace()
                                .color(MUTED),
                        );
                        if ui
                            .add_enabled(
                                self.page + 1 < pages && !self.preview_loading,
                                egui::Button::new("›").min_size(egui::vec2(28.0, 26.0)),
                            )
                            .clicked()
                        {
                            self.request_preview(self.page + 1);
                        }
                        if self.preview_loading {
                            ui.spinner();
                        }
                    });
                    egui::ScrollArea::vertical()
                        .id_salt("pdf-preview")
                        .show(ui, |ui| {
                            if let Some(texture) = &self.texture {
                                let size = texture.size_vec2();
                                let width = ui.available_width();
                                ui.add(
                                    egui::Image::new(texture)
                                        .fit_to_exact_size(size * (width / size.x)),
                                );
                            }
                        });
                } else {
                    ui.add_space(65.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("PDF").monospace().size(30.0).color(MUTED));
                        ui.label(
                            RichText::new("Здесь появится исходная карточка")
                                .small()
                                .color(MUTED),
                        );
                    });
                }
            });
    }

    fn editor(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(EDITOR).inner_margin(0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(8, 0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if editor_tab(
                                ui,
                                "Данные карточки",
                                self.editor_tab == EditorTab::Details,
                            ) {
                                self.editor_tab = EditorTab::Details;
                            }
                            if editor_tab(ui, "Текст PDF", self.editor_tab == EditorTab::Text)
                            {
                                self.editor_tab = EditorTab::Text;
                            }
                        });
                    });
                if let Some(loaded) = &self.loaded {
                    Frame::new()
                        .inner_margin(Margin::symmetric(18, 8))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "PDF  /  {}  /  {}",
                                        loaded
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy(),
                                        loaded.card.kind
                                    ))
                                    .small()
                                    .color(MUTED),
                                )
                                .truncate(),
                            )
                            .on_hover_text(loaded.path.display().to_string());
                        });
                    ui.separator();
                    let tab = self.editor_tab;
                    egui::ScrollArea::vertical()
                        .id_salt(("editor", tab == EditorTab::Text))
                        .show(ui, |ui| {
                            editor_frame().show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if tab == EditorTab::Text {
                                    self.text_editor(ui);
                                } else {
                                    self.card_editor(ui);
                                }
                            });
                        });
                } else {
                    self.empty_editor(ui);
                }
            });
    }

    fn empty_editor(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(105.0);
            ui.label(RichText::new("Карточки ККТ").size(28.0).strong());
            ui.add_space(8.0);
            ui.label(
                RichText::new("Регистрация и снятие с учёта")
                    .size(16.0)
                    .color(MUTED),
            );
            ui.add_space(30.0);
            Frame::new()
                .fill(PANEL)
                .stroke(Stroke::new(1.0_f32, BORDER))
                .corner_radius(6)
                .inner_margin(24)
                .show(ui, |ui| {
                    ui.set_max_width(355.0);
                    ui.label("Перетащите сюда PDF-карточку");
                    ui.add_space(12.0);
                    if ui
                        .add_enabled(!self.loading, primary("Открыть PDF…"))
                        .clicked()
                    {
                        self.pick_file();
                    }
                    ui.label(RichText::new("Ctrl+O").small().color(MUTED));
                });
            ui.add_space(26.0);
            ui.label(
                RichText::new("Проверьте реквизиты → выберите папку → сохраните")
                    .small()
                    .color(MUTED),
            );
        });
    }

    fn card_editor(&self, ui: &mut egui::Ui) {
        let Some(loaded) = &self.loaded else { return };
        let c = &loaded.card;
        let registration = card::format_registration(&c.registration);
        let removal = c.is_deregistration();
        ui.heading(if removal {
            "Снятие ККТ с учёта"
        } else if c.kind == "Регистрация" {
            "Регистрация ККТ"
        } else {
            "Карточка ККТ"
        });
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            badge(
                ui,
                if removal {
                    "СНЯТИЕ С УЧЁТА"
                } else if c.kind == "Регистрация" {
                    "РЕГИСТРАЦИЯ"
                } else {
                    "ПРОВЕРЬТЕ ТИП"
                },
                if removal {
                    AMBER
                } else if c.kind == "Регистрация" {
                    GREEN
                } else {
                    MUTED
                },
            );
            if removal && !c.deregistered_at.is_empty() {
                ui.label(RichText::new(format!("от {}", c.deregistered_at)).color(AMBER));
            }
        });
        ui.add_space(8.0);
        if ui.small_button("Скопировать реквизиты").clicked() {
            ui.ctx().copy_text(
                c.display_fields()
                    .iter()
                    .map(|(k, v)| {
                        let value = if *k == "Регистрационный номер ККТ" {
                            registration.as_str()
                        } else {
                            v
                        };
                        format!("{k}: {value}")
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        if removal && c.deregistered_at.is_empty() {
            Frame::new().fill(AMBER.gamma_multiply(0.10)).inner_margin(10).corner_radius(4).show(ui,|ui|{ui.label(RichText::new("Дата снятия не распознана. Проверьте исходный PDF; папку можно выбрать вручную.").color(AMBER));});
        }
        section(ui, "Владелец ККТ");
        let owner_fields: Vec<_> = [
            ("Организация / ИП", &c.organization),
            ("ИНН", &c.inn),
            ("КПП", &c.kpp),
            ("ОГРН / ОГРНИП", &c.ogrn),
        ]
        .into_iter()
        .filter(|(label, value)| {
            !removal || !value.is_empty() || matches!(*label, "Организация / ИП" | "ИНН")
        })
        .map(|(label, value)| (label, value.as_str()))
        .collect();
        field_rows(ui, "owner-fields", &owner_fields, true);
        section(ui, "Контрольно-кассовая техника");
        field_rows(
            ui,
            "kkt-fields",
            &[
                ("Модель ККТ", &c.model),
                ("Заводской номер", &c.serial),
                ("Рег. номер ККТ", &registration),
            ],
            true,
        );
        section(
            ui,
            if removal {
                "Сведения о снятии"
            } else {
                "Регистрационные сведения"
            },
        );
        if removal {
            field_rows(
                ui,
                "removal-fields",
                &[
                    ("Дата снятия с учёта", &c.deregistered_at),
                    ("Дата карточки", &c.formed_at),
                ],
                true,
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    "Для карточки снятия ФН, адрес установки и ОФД могут не указываться.",
                )
                .small()
                .color(MUTED),
            );
        } else {
            field_rows(
                ui,
                "registration-fields",
                &[
                    ("Дата регистрации", &c.registered_at),
                    ("Последнее изменение", &c.changed_at),
                    ("Дата карточки", &c.formed_at),
                    ("Перерегистраций", &c.changes),
                ],
                true,
            );
        }
        if !removal || !c.fn_model.is_empty() || !c.fn_serial.is_empty() {
            section(ui, "Фискальный накопитель");
            field_rows(
                ui,
                "fn-fields",
                &[
                    ("Модель ФН", &c.fn_model),
                    ("Заводской номер ФН", &c.fn_serial),
                ],
                !removal,
            );
        }
        if !removal || !c.address.is_empty() || !c.place.is_empty() || !c.ofd.is_empty() {
            section(ui, "Установка и фискальные данные");
            field_rows(
                ui,
                "additional-fields",
                &[
                    ("Адрес установки", &c.address),
                    ("Место установки", &c.place),
                    ("ОФД", &c.ofd),
                    ("ИНН ОФД", &c.ofd_inn),
                ],
                !removal,
            );
        }
        ui.add_space(18.0);
        if !c.flags.is_empty() {
            egui::CollapsingHeader::new(format!("Признаки применения · {}", c.flags.len())).show(
                ui,
                |ui| {
                    let fields: Vec<_> = c
                        .flags
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.as_str()))
                        .collect();
                    field_rows(ui, "flag-fields", &fields, true);
                },
            );
        }
        egui::CollapsingHeader::new("Электронная подпись").show(ui, |ui| {
            let fields: Vec<_> = c
                .signature
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            field_rows(ui, "signature-fields", &fields, true);
            ui.label(
                RichText::new("Сведения из текста PDF. Криптографическая проверка не выполняется.")
                    .small()
                    .color(MUTED),
            );
        });
    }

    fn text_editor(&self, ui: &mut egui::Ui) {
        let Some(loaded) = &self.loaded else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Исходный текст PDF").strong());
            if ui.small_button("Скопировать всё").clicked() {
                ui.ctx().copy_text(loaded.text.clone());
            }
        });
        ui.add_space(12.0);
        egui::Grid::new("text-lines")
            .num_columns(2)
            .spacing([14.0, 4.0])
            .show(ui, |ui| {
                for (i, line) in loaded.text.lines().enumerate() {
                    ui.add_sized(
                        [28.0, 18.0],
                        egui::Label::new(
                            RichText::new((i + 1).to_string())
                                .monospace()
                                .size(12.0)
                                .color(MUTED),
                        ),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(if line.is_empty() { " " } else { line })
                                .monospace()
                                .size(13.0),
                        )
                        .wrap()
                        .selectable(true),
                    );
                    ui.end_row();
                }
            });
    }

    fn destination_ui(&mut self) {
        if !self.choosing {
            return;
        }
        let ctx = self.ctx.clone();
        let mut open = true;
        let mut save_on_double_click = false;
        let down = self.new_folder.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let up = self.new_folder.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let keyboard_selection = (down || up) && !self.matches.is_empty();
        if keyboard_selection {
            let selected = self
                .matches
                .iter()
                .position(|m| Some(&m.folder.path) == self.selected.as_ref());
            let index = match selected {
                Some(i) if down => (i + 1).min(self.matches.len() - 1),
                Some(i) => i.saturating_sub(1),
                None => 0,
            };
            self.selected = Some(self.matches[index].folder.path.clone());
            self.manual_destination = false;
        }
        // Keep a manual/new choice visible even when it is absent from search results.
        let extra_selection = self
            .selected
            .as_ref()
            .filter(|path| !self.matches.iter().any(|m| &m.folder.path == *path))
            .cloned();
        let folder_count = self.matches.len() + usize::from(extra_selection.is_some());
        egui::Window::new("Папка сохранения")
            .id(egui::Id::new("compact-destination-picker"))
            .open(&mut open)
            .collapsible(false)
            .resizable([true, false])
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(740.0)
            .min_width(600.0)
            .show(&ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Поиск");
                    let width = (ui.available_width() - 175.0).max(200.0);
                    let query_changed = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.query)
                                .id(egui::Id::new("folder-query"))
                                .desired_width(width)
                                .hint_text("Организация или название папки"),
                        )
                        .changed();
                    ui.label("ИНН");
                    let inn_changed = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.query_inn)
                                .desired_width(125.0)
                                .font(egui::TextStyle::Monospace),
                        )
                        .changed();
                    if query_changed || inn_changed {
                        self.selected = None;
                        self.manual_destination = false;
                        self.schedule_search();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Папки организаций · {folder_count}"))
                            .color(MUTED),
                    );
                    if self.searching {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Выбрать вручную…").clicked() {
                            self.pick_destination();
                        }
                        if ui.button("Создать папку…").clicked() {
                            self.begin_new_folder();
                        }
                    });
                });
                Frame::new()
                    .fill(EDITOR)
                    .stroke(Stroke::new(1.0_f32, BORDER))
                    .corner_radius(4)
                    .inner_margin(4)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.spacing_mut().interact_size.y = 26.0;
                        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                        egui::ScrollArea::vertical()
                            .id_salt("compact-folder-matches")
                            .max_height(280.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                if folder_count == 0 {
                                    ui.add_space(12.0);
                                    ui.add(egui::Label::new(RichText::new(
                                        if self.scanning || self.searching {
                                            "Ищу папки… Можно выбрать папку вручную."
                                        } else {
                                            "Папки не найдены. Измените поиск, выберите папку вручную или создайте новую."
                                        }
                                    ).color(MUTED)).wrap());
                                    ui.add_space(12.0);
                                }
                                let folders = extra_selection
                                    .iter()
                                    .map(|path| (path, None))
                                    .chain(self.matches.iter().map(|m| (&m.folder.path, Some(m.score))));
                                for (path, score) in folders {
                                    let selected = self.selected.as_ref() == Some(path);
                                    let name = path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy();
                                    // The growing spacer keeps names left-aligned in full-width rows.
                                    let row = ui.push_id(path, |ui| ui
                                        .add_sized(
                                            [ui.available_width(), 26.0],
                                            egui::Button::selectable(
                                                selected,
                                                (RichText::new(name), egui::Atom::grow()),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(format!(
                                            "{}{}\nДвойной щелчок — сохранить карточку.",
                                            path.display(),
                                            if score.is_some_and(|score| score < 45.0) {
                                                "\nСлабое совпадение — проверьте папку."
                                            } else {
                                                ""
                                            }
                                        ))).inner;
                                    if selected && keyboard_selection {
                                        row.scroll_to_me(Some(Align::Center));
                                    }
                                    if row.clicked() || row.double_clicked() {
                                        self.selected = Some(path.clone());
                                        self.manual_destination = false;
                                    }
                                    save_on_double_click |= row.double_clicked();
                                }
                            });
                    });
                ui.horizontal(|ui| {
                    self.index_button(ui);
                    if self.scanning {
                        ui.add(egui::Spinner::new().size(12.0));
                    }
                    ui.add(
                        egui::Label::new(RichText::new(&self.index_status).small().color(MUTED))
                            .truncate(),
                    )
                    .on_hover_text(&self.index_status);
                });
                ui.horizontal(|ui| {
                    ui.label("РНМ");
                    let rnm = ui.add(egui::TextEdit::singleline(&mut self.save_registration).desired_width(190.0).font(egui::TextStyle::Monospace).hint_text("0000 0000 0000 0000"));
                    let rnm_changed = rnm.changed();
                    if rnm.lost_focus() {
                        self.save_registration = card::format_registration(&self.save_registration);
                    }
                    ui.label("Дата карточки");
                    let date_changed = ui.add(egui::TextEdit::singleline(&mut self.save_date).desired_width(105.0).font(egui::TextStyle::Monospace).hint_text("ДД.ММ.ГГГГ")).changed();
                    ui.label(RichText::new("Даты файла: 00:00 МСК").small().color(MUTED));
                    if (rnm_changed || date_changed) && let Some(loaded) = &self.loaded {
                        let mut card = loaded.card.clone();
                        card.registration = card::compact_registration(&self.save_registration);
                        card.formed_at = self.save_date.trim().to_string();
                        self.filename = card.suggested_filename();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Имя PDF");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filename)
                            .desired_width(ui.available_width())
                            .font(egui::TextStyle::Monospace),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.config.move_source {
                            "Перенос после проверки копии"
                        } else {
                            "Копирование с сохранением исходника"
                        })
                        .small()
                        .color(MUTED),
                    );
                    if self.config.print {
                        ui.label(RichText::new("· Печать включена").small().color(BLUE));
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Двойной щелчок / Ctrl+Enter — сохранить · ↑ ↓ — выбор")
                            .small()
                            .color(MUTED),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Отмена").clicked() {
                            self.choosing = false;
                        }
                        if ui
                            .add_enabled(
                                self.selected.is_some() && !self.busy,
                                egui::Button::new(RichText::new("Сохранить").color(Color32::WHITE))
                                    .fill(BLUE)
                                    .min_size(egui::vec2(130.0, 32.0)),
                            )
                            .clicked()
                        {
                            self.perform_save();
                        }
                    });
                });
            });
        if !open {
            self.choosing = false;
        }
        if self.choosing
            && self.new_folder.is_none()
            && self.selected.is_some()
            && (save_on_double_click
                || ctx.input_mut(|i| {
                    i.consume_shortcut(&egui::KeyboardShortcut::new(
                        egui::Modifiers::CTRL,
                        egui::Key::Enter,
                    ))
                }))
        {
            self.perform_save();
        }
    }

    fn new_folder_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.new_folder.take() else {
            return;
        };
        let locked = dialog.creating || dialog.picking_parent || self.busy;
        let mut browse = false;
        let mut create = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("create-destination-folder")).show(ctx, |ui| {
            ui.set_width(600.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Создать папку").size(20.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel |= ui
                        .add_enabled(!locked, egui::Button::new("×").frame(false))
                        .clicked();
                });
            });
            ui.separator();
            ui.add_space(3.0);
            ui.label(RichText::new("Где создать папку").color(MUTED));
            ui.horizontal(|ui| {
                ui.add_enabled(
                    !locked,
                    egui::TextEdit::singleline(&mut dialog.parent)
                        .desired_width(ui.available_width() - 96.0)
                        .hint_text("Полный путь к родительской папке"),
                );
                browse = ui
                    .add_enabled(!locked, egui::Button::new("Обзор…"))
                    .clicked();
            });
            ui.label(RichText::new("Название новой папки").color(MUTED));
            let name_input = ui.add_enabled(
                !locked,
                egui::TextEdit::singleline(&mut dialog.name)
                    .id(egui::Id::new("new-folder-name"))
                    .desired_width(ui.available_width())
                    .hint_text("Название организации или ИП"),
            );
            if dialog.focus_name {
                name_input.request_focus();
                dialog.focus_name = false;
            }
            ui.add_space(5.0);
            ui.label(
                RichText::new("После создания папка будет выбрана для сохранения карточки.")
                    .small()
                    .color(MUTED),
            );
            if !dialog.error.is_empty() {
                ui.add(egui::Label::new(RichText::new(&dialog.error).color(AMBER)).wrap());
            }
            if dialog.creating || dialog.picking_parent {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(if dialog.creating {
                        "Создаю папку…"
                    } else {
                        "Выберите родительскую папку в диалоге Windows…"
                    });
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    cancel |= ui
                        .add_enabled(!locked, egui::Button::new("Отмена"))
                        .clicked();
                    create = ui
                        .add_enabled(
                            !locked && !dialog.name.trim().is_empty(),
                            primary("Создать и выбрать"),
                        )
                        .clicked();
                });
            });
        });
        cancel |= modal.should_close() && !locked;
        if !locked
            && ctx.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::Enter,
                ))
            })
        {
            create = true;
        }
        if !cancel {
            self.new_folder = Some(dialog);
            if browse {
                self.pick_new_folder_parent();
            }
            if create {
                self.create_new_folder();
            }
        }
    }

    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Настройки")
            .open(&mut open)
            .collapsible(false)
            .default_width(700.0)
            .show(ctx, |ui| {
                section(ui, "Архив карточек");
                ui.label(RichText::new("Корневая папка архива").color(MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.root_edit)
                        .desired_width(ui.available_width()),
                );
                ui.horizontal(|ui| {
                    if ui.button("Обзор…").clicked() {
                        self.spawn("pick-root", |tx| {
                            let _ = tx.send(Event::Root(
                                rfd::FileDialog::new()
                                    .set_title("Корневая папка архива")
                                    .pick_folder(),
                            ));
                        });
                    }
                    if ui.button("Применить").clicked() {
                        let root = PathBuf::from(self.root_edit.trim());
                        if root.is_absolute() {
                            self.config.root = root;
                            self.index = Arc::new(SearchIndex::new(Vec::new()));
                            self.matches.clear();
                            self.selected = None;
                            self.persist();
                            self.start_scan();
                        } else {
                            self.notify("Укажите полный путь к папке архива.", true);
                        }
                    }
                    self.index_button(ui);
                });
                ui.label("Автоматическое обновление каждые 60 минут, в том числе в трее.");
                ui.label("Если архив недоступен, повторная попытка выполняется через минуту.");
                ui.label(RichText::new(&self.index_status).small().color(MUTED));
                section(ui, "Очередь карточек");
                ui.add_enabled_ui(!self.busy, |ui| {
                    if switch(
                        ui,
                        &mut self.config.auto_next,
                        "После сохранения открывать следующую карточку",
                    ) {
                        self.persist();
                    }
                });
                ui.label("Автопереход выполняется после успешного сохранения и печати.");
                section(ui, "Печать");
                ui.label(&self.printer);
                if ui.button("Обновить принтер по умолчанию").clicked() {
                    self.printer = pdf::default_printer().unwrap_or_else(|e| e.to_string());
                }
                section(ui, "Окно и трей");
                ui.label("Крестик скрывает окно в трей. Щелчок по значку возвращает окно.");
                ui.label("Полный выход: меню значка → «Выйти из программы».");
                ui.add_space(14.0);
            });
        if !open {
            self.settings_open = false;
        }
    }
}

impl eframe::App for SaverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events();
        if ctx.input(|i| i.viewport().close_requested()) && !self.exiting && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        if !self.busy && !self.loading && self.new_folder.is_none() {
            if ctx.input_mut(|i| {
                i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::CTRL,
                    egui::Key::O,
                ))
            }) {
                self.pick_file();
            }
            if self.loaded.is_some()
                && !self.saved
                && ctx.input_mut(|i| {
                    i.consume_shortcut(&egui::KeyboardShortcut::new(
                        egui::Modifiers::CTRL,
                        egui::Key::S,
                    ))
                })
            {
                self.choose();
            }
            if self.loaded.is_some()
                && ctx.input_mut(|i| {
                    i.consume_shortcut(&egui::KeyboardShortcut::new(
                        egui::Modifiers::CTRL,
                        egui::Key::F,
                    ))
                })
            {
                self.choose();
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("folder-query")));
            }
        }
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            if let Some(path) = file.path {
                self.open(path);
            }
        }
        if self.query_due.is_some_and(|due| Instant::now() >= due) {
            self.search_now();
        }
        ctx.request_repaint_after(
            if self.busy || self.loading || self.scanning || self.searching {
                Duration::from_millis(120)
            } else {
                Duration::from_millis(500)
            },
        );
        self.toolbar(ctx);
        self.status_bar(ctx);
        self.operations(ctx);
        self.tool_rail(ctx);
        self.documents(ctx);
        self.preview_panel(ctx);
        self.editor(ctx);
        self.destination_ui();
        self.new_folder_dialog(ctx);
        self.settings_dialog(ctx);
        copy_feedback(ctx);
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            egui::Area::new("drop-overlay".into())
                .order(egui::Order::Foreground)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    Frame::new()
                        .fill(SELECTED)
                        .stroke(Stroke::new(2.0_f32, BLUE))
                        .corner_radius(6)
                        .inner_margin(22)
                        .show(ui, |ui| {
                            ui.heading("Отпустите PDF-карточку здесь");
                            ui.label("Регистрация или снятие с учёта");
                        });
                });
        }
        #[cfg(debug_assertions)]
        self.capture_for_review(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.indexer = None;
        self.tray = None;
        let _ = self.config.save();
        #[cfg(debug_assertions)]
        self.record_tray_check_exit();
    }
}

fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let (rect, mut response) = ui.allocate_exact_size(egui::vec2(34.0, 20.0), Sense::click());
        response = response.union(ui.add(egui::Label::new(label).wrap().sense(Sense::click())));
        if response.clicked()
            || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Space)))
        {
            *value = !*value;
            changed = true;
            response.mark_changed();
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
        });
        ui.painter().rect_filled(
            rect,
            10.0,
            if *value {
                BLUE
            } else {
                Color32::from_rgb(85, 88, 96)
            },
        );
        ui.painter().circle_filled(
            egui::pos2(
                if *value {
                    rect.right() - 10.0
                } else {
                    rect.left() + 10.0
                },
                rect.center().y,
            ),
            7.0,
            TEXT,
        );
    });
    changed
}
fn rail_button(ui: &mut egui::Ui, glyph: u8, selected: bool, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), Sense::click());
    let painter = ui.painter();
    if selected || response.hovered() {
        painter.rect_filled(rect, 4.0, if selected { SELECTED } else { HOVER });
    }
    let stroke = Stroke::new(
        1.4_f32,
        if selected {
            Color32::from_rgb(125, 171, 255)
        } else {
            MUTED
        },
    );
    let r = rect.shrink(8.0);
    match glyph {
        0 => {
            painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    egui::pos2(r.left() + 4.0, r.top() + 5.0),
                    egui::pos2(r.right() - 3.0, r.top() + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.left() + 4.0, r.top() + 10.0),
                    egui::pos2(r.right() - 3.0, r.top() + 10.0),
                ],
                stroke,
            );
        }
        1 => {
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                "P",
                FontId::monospace(11.0),
                stroke.color,
            );
        }
        _ => {
            painter.circle_stroke(r.center(), 6.0, stroke);
            painter.circle_stroke(r.center(), 2.0, stroke);
            for n in 0..4 {
                let angle = n as f32 * std::f32::consts::FRAC_PI_2;
                let dir = egui::vec2(angle.cos(), angle.sin());
                painter.line_segment([r.center() + dir * 6.0, r.center() + dir * 9.0], stroke);
            }
        }
    }
    response.on_hover_text(label)
}
fn open_folder(path: &std::path::Path) {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let path: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}
