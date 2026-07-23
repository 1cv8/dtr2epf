use crate::generator::{GenerationIssue, GenerationVariant, Severity, generate};
use crate::model::{Project, TreeRowKind, TreeSort};
use crate::templates::Templates;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Tab {
    Selection,
    Templates,
    Preview,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AdapterSort {
    Name,
    Handlers,
}

pub struct DtrApp {
    source_path: String,
    project: Option<Project>,
    selected: HashSet<String>,
    adapter_selected: HashSet<String>,
    expanded: HashSet<String>,
    templates: Templates,
    generation_variant: GenerationVariant,
    filter: String,
    tree_sort: TreeSort,
    adapter_sort: AdapterSort,
    tab: Tab,
    preview: String,
    issues: Vec<GenerationIssue>,
    status: String,
    show_warnings: bool,
}

impl DtrApp {
    pub fn new(ctx: egui::Context, initial_source: Option<PathBuf>) -> Self {
        configure_fonts(&ctx);
        let mut app = Self {
            source_path: initial_source
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            project: None,
            selected: HashSet::new(),
            adapter_selected: HashSet::new(),
            expanded: HashSet::new(),
            templates: Templates::default(),
            generation_variant: GenerationVariant::Debug,
            filter: String::new(),
            tree_sort: TreeSort::Name,
            adapter_sort: AdapterSort::Name,
            tab: Tab::Selection,
            preview: String::new(),
            issues: Vec::new(),
            status: "Выберите каталог исходников Datareon".to_owned(),
            show_warnings: false,
        };
        if initial_source.is_some() {
            app.load_project();
        }
        app
    }

    fn load_project(&mut self) {
        self.clear_project_state();
        if self.source_path.trim().is_empty() {
            self.status = "Выберите каталог исходников Datareon".to_owned();
            return;
        }
        match Project::load(&PathBuf::from(self.source_path.trim())) {
            Ok(project) => {
                self.expanded = project.folders.iter().map(|f| f.id.clone()).collect();
                self.status = format!(
                    "Загружено: {} адаптеров 1С, {} обработчиков и функций",
                    project.adapters.len(),
                    project.handlers.len()
                );
                self.project = Some(project);
            }
            Err(error) => self.status = format!("Ошибка: {error}"),
        }
    }

    fn clear_project_state(&mut self) {
        self.project = None;
        self.selected.clear();
        self.adapter_selected.clear();
        self.expanded.clear();
        self.filter.clear();
        self.preview.clear();
        self.issues.clear();
        self.show_warnings = false;
        self.tab = Tab::Selection;
    }

    fn choose_source_directory(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Выберите каталог исходников Datareon");
        let current_path = PathBuf::from(self.source_path.trim());
        if current_path.is_dir() {
            dialog = dialog.set_directory(current_path);
        }
        if let Some(path) = dialog.pick_folder() {
            self.source_path = path.display().to_string();
            self.load_project();
        }
    }

    fn toggle_adapter(&mut self, id: &str, checked: bool) {
        let handler_ids: Vec<String> = self
            .project
            .as_ref()
            .and_then(|project| {
                project.adapters.iter().find(|a| a.id == id).map(|adapter| {
                    adapter
                        .handler_ids
                        .iter()
                        .filter(|handler_id| project.handler(handler_id).is_some())
                        .cloned()
                        .collect()
                })
            })
            .unwrap_or_default();
        if checked {
            self.adapter_selected.insert(id.to_owned());
        } else {
            self.adapter_selected.remove(id);
        }
        for handler_id in handler_ids {
            if checked {
                self.selected.insert(handler_id);
            } else {
                self.selected.remove(&handler_id);
            }
        }
    }

    fn toggle_folder(&mut self, id: &str) {
        let descendants = self
            .project
            .as_ref()
            .map(|p| p.folder_descendants(id))
            .unwrap_or_default();
        let all =
            !descendants.is_empty() && descendants.iter().all(|item| self.selected.contains(item));
        for handler_id in descendants {
            if all {
                self.selected.remove(&handler_id);
            } else {
                self.selected.insert(handler_id);
            }
        }
    }

    fn generate_preview(&mut self) {
        let Some(project) = &self.project else {
            self.status = "Сначала загрузите исходники".to_owned();
            return;
        };
        let result = generate(
            project,
            &self.selected,
            &self.templates,
            self.generation_variant,
        );
        self.preview = result.text;
        self.issues = result.issues;
        let errors = self
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count();
        self.status = format!(
            "Сформировано элементов: {}; вариант: {}; ошибок: {errors}",
            result.selected_count,
            self.generation_variant.label()
        );
        self.tab = Tab::Preview;
    }

    fn save_module(&mut self) {
        self.generate_preview();
        if self.issues.iter().any(|i| i.severity == Severity::Error) {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("ObjectModule.bsl")
            .add_filter("Модуль 1С", &["bsl"])
            .save_file()
        else {
            return;
        };
        match fs::write(&path, self.preview.as_bytes()) {
            Ok(()) => self.status = format!("Модуль сохранён: {}", path.display()),
            Err(error) => self.status = format!("Не удалось сохранить {}: {error}", path.display()),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Исходники Datareon:");
            let path_width = (ui.available_width() - 285.0).max(160.0);
            ui.add(egui::TextEdit::singleline(&mut self.source_path).desired_width(path_width));
            if ui
                .add_sized([170.0, 25.0], egui::Button::new("Выбрать каталог…"))
                .clicked()
            {
                self.choose_source_directory();
            }
            if ui
                .add_sized([100.0, 25.0], egui::Button::new("Загрузить"))
                .clicked()
            {
                self.load_project();
            }
        });
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.tab == Tab::Selection, "Выбор обработчиков")
                .clicked()
            {
                self.tab = Tab::Selection;
            }
            if ui
                .selectable_label(self.tab == Tab::Templates, "Шаблоны и области")
                .clicked()
            {
                self.tab = Tab::Templates;
            }
            if ui
                .selectable_label(self.tab == Tab::Preview, "Предпросмотр и проверка")
                .clicked()
            {
                self.tab = Tab::Preview;
            }
            ui.separator();
            ui.label(&self.status);
        });
    }

    fn selection_tab(&mut self, ui: &mut egui::Ui) {
        if self.project.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading("Исходники Datareon не выбраны");
                ui.label("Выберите каталог, содержащий папки Adapters и Metadata.");
                ui.add_space(16.0);
                if ui
                    .add_sized(
                        [280.0, 42.0],
                        egui::Button::new("Выбрать каталог исходников Datareon…"),
                    )
                    .clicked()
                {
                    self.choose_source_directory();
                }
            });
            return;
        }
        egui::Panel::left("selection_adapters_panel")
            .resizable(true)
            .show_separator_line(true)
            .default_size(300.0)
            .size_range(250.0..=480.0)
            .show_inside(ui, |ui| self.adapters_ui(ui));

        egui::CentralPanel::default().show_inside(ui, |ui| self.tree_ui(ui));
    }

    fn adapters_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Адаптеры 1С");
        ui.horizontal(|ui| {
            ui.label("Сортировка:");
            egui::ComboBox::from_id_salt("adapter_sort")
                .selected_text(match self.adapter_sort {
                    AdapterSort::Name => "По имени",
                    AdapterSort::Handlers => "По числу обработчиков",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.adapter_sort, AdapterSort::Name, "По имени");
                    ui.selectable_value(
                        &mut self.adapter_sort,
                        AdapterSort::Handlers,
                        "По числу обработчиков",
                    );
                });
        });
        ui.separator();
        let Some(project) = &self.project else {
            ui.label("Исходники не загружены");
            return;
        };
        let mut adapters: Vec<(String, String, usize, String)> = project
            .adapters
            .iter()
            .map(|a| {
                (
                    a.id.clone(),
                    a.name.clone(),
                    a.handler_ids.len(),
                    a.source_path.display().to_string(),
                )
            })
            .collect();
        match self.adapter_sort {
            AdapterSort::Name => adapters.sort_by_key(|a| a.1.to_lowercase()),
            AdapterSort::Handlers => adapters.sort_by(|a, b| {
                b.2.cmp(&a.2)
                    .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
            }),
        }
        egui::ScrollArea::vertical()
            .id_salt("adapters")
            .show(ui, |ui| {
                egui::Grid::new("adapter_grid")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.strong("Выбор");
                        ui.strong("Адаптер");
                        ui.strong("Связей");
                        ui.end_row();
                        for (id, name, count, source_path) in adapters {
                            let mut checked = self.adapter_selected.contains(&id);
                            if ui.checkbox(&mut checked, "").changed() {
                                self.toggle_adapter(&id, checked);
                            }
                            ui.label(name).on_hover_text(source_path);
                            ui.label(count.to_string());
                            ui.end_row();
                        }
                    });
            });
    }

    fn tree_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(format!("Обработчики ({})", self.selected.len()));
            if ui.button("Установить всё").clicked() {
                if let Some(project) = &self.project {
                    self.selected = project.handlers.iter().map(|h| h.id.clone()).collect();
                }
            }
            if ui.button("Снять всё").clicked() {
                self.selected.clear();
                self.adapter_selected.clear();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Поиск:");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(180.0));
            egui::ComboBox::from_id_salt("tree_sort")
                .selected_text(match self.tree_sort {
                    TreeSort::Source => "Порядок исходников",
                    TreeSort::Name => "По имени",
                    TreeSort::Type => "По типу",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.tree_sort,
                        TreeSort::Source,
                        "Порядок исходников",
                    );
                    ui.selectable_value(&mut self.tree_sort, TreeSort::Name, "По имени");
                    ui.selectable_value(&mut self.tree_sort, TreeSort::Type, "По типу");
                });
        });
        ui.separator();
        let Some(project) = &self.project else {
            ui.label("Исходники не загружены");
            return;
        };
        let rows = project.tree_rows(&self.expanded, self.tree_sort, &self.filter);
        let folder_states: HashMap<String, (usize, usize)> = rows
            .iter()
            .filter_map(|row| match &row.kind {
                TreeRowKind::Folder(id) => {
                    let ids = project.folder_descendants(id);
                    let checked = ids
                        .iter()
                        .filter(|item| self.selected.contains(*item))
                        .count();
                    Some((id.clone(), (checked, ids.len())))
                }
                _ => None,
            })
            .collect();
        egui::ScrollArea::vertical().id_salt("tree").show(ui, |ui| {
            for row in rows {
                ui.horizontal(|ui| {
                    ui.add_space(row.depth as f32 * 18.0);
                    match &row.kind {
                        TreeRowKind::Folder(id) => {
                            let open = self.expanded.contains(id);
                            if ui.small_button(if open { "▼" } else { "▶" }).clicked() {
                                if open {
                                    self.expanded.remove(id);
                                } else {
                                    self.expanded.insert(id.clone());
                                }
                            }
                            let (checked, total) =
                                folder_states.get(id).copied().unwrap_or_default();
                            let symbol = if total > 0 && checked == total {
                                "☑"
                            } else if checked > 0 {
                                "◩"
                            } else {
                                "☐"
                            };
                            if ui
                                .small_button(symbol)
                                .on_hover_text(format!("Выбрано {checked} из {total}"))
                                .clicked()
                            {
                                self.toggle_folder(id);
                            }
                            ui.strong(&row.name);
                        }
                        TreeRowKind::Handler(id) => {
                            ui.add_space(20.0);
                            let mut checked = self.selected.contains(id);
                            if ui.checkbox(&mut checked, "").changed() {
                                if checked {
                                    self.selected.insert(id.clone());
                                } else {
                                    self.selected.remove(id);
                                }
                            }
                            ui.label(&row.name);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.weak(&row.detail);
                    });
                });
            }
        });
    }

    fn templates_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("templates_page_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Восстановить встроенные").clicked() {
                        self.templates = Templates::default();
                        self.status = "Встроенные шаблоны восстановлены".to_owned();
                    }
                    if ui.button("Экспорт JSON…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("dtr2epf-templates.json")
                            .add_filter("JSON", &["json"])
                            .save_file()
                        {
                            self.status = match self.templates.save(&path) {
                                Ok(()) => format!("Шаблоны сохранены: {}", path.display()),
                                Err(e) => format!("Ошибка сохранения: {e}"),
                            };
                        }
                    }
                    if ui.button("Импорт JSON…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .pick_file()
                        {
                            match Templates::load(&path) {
                                Ok(value) => {
                                    self.templates = value;
                                    self.status =
                                        format!("Шаблоны загружены: {}", path.display());
                                }
                                Err(e) => self.status = format!("Ошибка импорта: {e}"),
                            }
                        }
                    }
                });
                ui.add_space(4.0);
                egui::CollapsingHeader::new("Имена областей модуля")
                    .default_open(false)
                    .show(ui, |ui| {
                        region_name_row(
                            ui,
                            "FromPlatform (в 1С)",
                            &mut self.templates.region_from_platform,
                        );
                        region_name_row(
                            ui,
                            "ToPlatform (из 1С)",
                            &mut self.templates.region_to_platform,
                        );
                        region_name_row(
                            ui,
                            "Функции 1С",
                            &mut self.templates.region_functions,
                        );
                    });
                ui.add(
                    egui::Label::new("Плейсхолдеры элемента: {{NAME}}, {{ORIGINAL_NAME}}, {{PARAMETERS}}, {{CODE}}, {{TYPE}}, {{INTEGRATION}}, {{ENTITY_ID}}, {{SOURCE_PATH}}, {{SUBSCRIPTION_OBJECT}}, {{SUBSCRIPTION_OBJECT_MANAGER}}, {{SUBSCRIPTION_OBJECT_TYPE_REF}}, {{SUBSCRIPTION_OBJECT_TYPE_OBJ}}. Объект подписки, его менеджер и типы заполняются только для Subscription1C / ToPlatform.")
                        .wrap(),
                );
                ui.separator();

                template_editor_block(
                    ui,
                    "template_module",
                    "Общий шаблон модуля",
                    &mut self.templates.module,
                );
                ui.strong("Вариант «Для отладки»");
                template_editor_block(
                    ui,
                    "template_from_platform",
                    "Входящий обработчик FromPlatform → 1С",
                    &mut self.templates.subscription_from_platform,
                );
                template_editor_block(
                    ui,
                    "template_to_platform",
                    "Исходящий обработчик ToPlatform ← 1С",
                    &mut self.templates.subscription_to_platform,
                );
                ui.strong("Вариант «Для синтаксического контроля»");
                template_editor_block(
                    ui,
                    "syntax_control_template_from_platform",
                    "Входящий обработчик FromPlatform → 1С",
                    &mut self.templates.syntax_control_subscription_from_platform,
                );
                template_editor_block(
                    ui,
                    "syntax_control_template_to_platform",
                    "Исходящий обработчик ToPlatform ← 1С",
                    &mut self.templates.syntax_control_subscription_to_platform,
                );
                ui.strong("Общий шаблон функций");
                template_editor_block(
                    ui,
                    "template_function",
                    "Функция Function1C",
                    &mut self.templates.function,
                );
            });
    }

    fn preview_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Обновить предпросмотр").clicked() {
                self.generate_preview();
            }
            if ui
                .add_enabled(
                    !self.preview.is_empty(),
                    egui::Button::new("Копировать модуль"),
                )
                .on_hover_text("Скопировать весь текст модуля в системный буфер обмена")
                .clicked()
            {
                ui.ctx().copy_text(self.preview.clone());
                self.status = format!(
                    "Текст модуля скопирован в буфер обмена ({} символов)",
                    self.preview.chars().count()
                );
            }
            if ui.button("Сохранить ObjectModule.bsl…").clicked() {
                self.save_module();
            }
            let errors = self
                .issues
                .iter()
                .filter(|i| i.severity == Severity::Error)
                .count();
            let warnings = self.issues.len().saturating_sub(errors);
            ui.label(format!("Ошибок: {errors}; предупреждений: {warnings}"));
        });
        for issue in &self.issues {
            let color = if issue.severity == Severity::Error {
                egui::Color32::LIGHT_RED
            } else {
                egui::Color32::YELLOW
            };
            ui.colored_label(
                color,
                format!(
                    "{} {}",
                    if issue.severity == Severity::Error {
                        "Ошибка:"
                    } else {
                        "Предупреждение:"
                    },
                    issue.message
                ),
            );
        }
        ui.separator();
        let viewport_width = ui.available_width().max(240.0);
        let viewport_height = ui.available_height().max(180.0);
        let line_count = self.preview.lines().count().max(10);
        let longest_line = self
            .preview
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        let content_width = (longest_line as f32 * 8.0 + 48.0).max(viewport_width - 20.0);
        let content_height = (line_count as f32 * 18.0 + 24.0).max(viewport_height - 20.0);

        egui::ScrollArea::both()
            .id_salt("module_preview_scroll")
            .max_width(viewport_width)
            .max_height(viewport_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_sized(
                    [content_width, content_height],
                    egui::TextEdit::multiline(&mut self.preview)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(1)
                        .code_editor(),
                );
            });
    }
}

impl egui_software_backend::App for DtrApp {
    fn ui(
        &mut self,
        root_ui: &mut egui::Ui,
        _software_backend: &mut egui_software_backend::SoftwareBackend,
    ) {
        egui::Panel::top("top").show_inside(root_ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom").show_inside(root_ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Вариант генерации:");
                egui::ComboBox::from_id_salt("generation_variant")
                    .selected_text(self.generation_variant.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.generation_variant,
                            GenerationVariant::Debug,
                            GenerationVariant::Debug.label(),
                        );
                        ui.selectable_value(
                            &mut self.generation_variant,
                            GenerationVariant::SyntaxControl,
                            GenerationVariant::SyntaxControl.label(),
                        );
                    });
                if ui.button("Сформировать обработку").clicked() {
                    self.save_module();
                }
                if let Some(project) = &self.project {
                    ui.separator();
                    ui.label(format!(
                        "{} · адаптеров: {} · элементов: {} · выбрано: {}",
                        project.root.display(),
                        project.adapters.len(),
                        project.handlers.len(),
                        self.selected.len()
                    ));
                    if !project.warnings.is_empty()
                        && ui
                            .button(format!(
                                "Предупреждения загрузки: {}",
                                project.warnings.len()
                            ))
                            .clicked()
                    {
                        self.show_warnings = true;
                    }
                }
            });
        });
        egui::CentralPanel::default().show_inside(root_ui, |ui| match self.tab {
            Tab::Selection => self.selection_tab(ui),
            Tab::Templates => self.templates_tab(ui),
            Tab::Preview => self.preview_tab(ui),
        });
        if self.show_warnings {
            let warnings = self
                .project
                .as_ref()
                .map(|p| p.warnings.clone())
                .unwrap_or_default();
            egui::Window::new("Предупреждения загрузки")
                .open(&mut self.show_warnings)
                .vscroll(true)
                .show(root_ui.ctx(), |ui| {
                    for warning in warnings {
                        ui.label(warning);
                    }
                });
        }
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ];
    for path in candidates {
        let Ok(bytes) = fs::read(path) else { continue };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system_cyrillic".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system_cyrillic".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("system_cyrillic".to_owned());
        ctx.set_fonts(fonts);
        break;
    }
}

fn region_name_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add_sized([190.0, 24.0], egui::Label::new(label));
        let field_width = ui.available_width().max(100.0);
        ui.add_sized(
            [field_width, 24.0],
            egui::TextEdit::singleline(value).desired_width(f32::INFINITY),
        );
    });
}

fn template_editor_block(ui: &mut egui::Ui, id: &str, title: &str, value: &mut String) {
    egui::CollapsingHeader::new(title)
        .id_salt(id)
        .default_open(false)
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt(format!("{id}_scroll"))
                .max_height(190.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let line_count = value.lines().count().max(10);
                    let longest_line = value
                        .lines()
                        .map(|line| line.chars().count())
                        .max()
                        .unwrap_or(0);
                    let editor_width = (longest_line as f32 * 8.0 + 48.0)
                        .max((ui.available_width() - 4.0).max(200.0));
                    let editor_height = (line_count as f32 * 18.0 + 24.0).max(180.0);
                    ui.add_sized(
                        [editor_width, editor_height],
                        egui::TextEdit::multiline(value)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(10)
                            .code_editor(),
                    );
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::load_fixture;

    #[test]
    fn startup_without_source_is_empty() {
        let app = DtrApp::new(egui::Context::default(), None);

        assert!(app.source_path.is_empty());
        assert!(app.project.is_none());
        assert!(app.selected.is_empty());
        assert!(app.generation_variant == GenerationVariant::Debug);
    }

    #[test]
    fn changing_project_clears_project_dependent_state() {
        let mut app = DtrApp::new(egui::Context::default(), None);
        app.project = Some(load_fixture());
        app.selected.insert("handler".into());
        app.adapter_selected.insert("adapter".into());
        app.expanded.insert("folder".into());
        app.filter = "старый фильтр".into();
        app.preview = "старый модуль".into();
        app.issues.push(GenerationIssue {
            severity: Severity::Warning,
            message: "старая ошибка".into(),
        });
        app.tab = Tab::Preview;

        app.clear_project_state();

        assert!(app.project.is_none());
        assert!(app.selected.is_empty());
        assert!(app.adapter_selected.is_empty());
        assert!(app.expanded.is_empty());
        assert!(app.filter.is_empty());
        assert!(app.preview.is_empty());
        assert!(app.issues.is_empty());
        assert!(app.tab == Tab::Selection);
    }
}
