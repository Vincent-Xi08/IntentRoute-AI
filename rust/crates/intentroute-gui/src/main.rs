//! intentroute-gui — read-only rules console (Rust migration, phase 3a).
//!
//! A pure-Rust eframe/egui shell over the shared `intentroute-core`: loads the
//! real product configuration (strict UTF-8, schema-checked), shows rules in
//! Canonical Runtime Order with search, column sorting, constraint-validation
//! flags, and a detail panel. It deliberately **never writes** configuration —
//! editing (and the DPAPI password boundary it implies) stays in the WPF app
//! until the Rust shell proves parity.
//!
//! UI strings are English for this phase: egui's bundled fonts do not cover
//! CJK, and shipping a CJK font file is deferred with the edit paths.

use eframe::egui;
use egui::{Color32, RichText, Sense};
use intentroute_core::{canonical_order, constraint, AppConfig, ProxyMode, ProxyRule};
use std::path::PathBuf;

// Product palette (README "dark professional" tokens).
const BG: Color32 = Color32::from_rgb(0x0D, 0x11, 0x17);
const CARD: Color32 = Color32::from_rgb(0x1C, 0x21, 0x28);
const BORDER: Color32 = Color32::from_rgb(0x30, 0x36, 0x3D);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xED, 0xF3);
const MUTED: Color32 = Color32::from_rgb(0x8B, 0x94, 0x9E);
const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x8D, 0xFF);
const GREEN: Color32 = Color32::from_rgb(0x3F, 0xB9, 0x50);
const RED: Color32 = Color32::from_rgb(0xF8, 0x51, 0x49);
const AMBER: Color32 = Color32::from_rgb(0xD2, 0x99, 0x22);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Canonical,
    Process,
    Mode,
    Enabled,
    Priority,
    Created,
}

impl SortKey {
    fn label(self) -> &'static str {
        match self {
            SortKey::Canonical => "#",
            SortKey::Process => "process",
            SortKey::Mode => "mode",
            SortKey::Enabled => "status",
            SortKey::Priority => "priority",
            SortKey::Created => "created",
        }
    }

    fn order_of(self, rules: &[ProxyRule]) -> Vec<usize> {
        let mut indexed: Vec<usize> = (0..rules.len()).collect();
        match self {
            SortKey::Canonical => {
                // canonical_order already sorts clones; map back to source indices
                let ordered = canonical_order(rules.to_vec());
                let mut result = Vec::with_capacity(rules.len());
                for rule in &ordered {
                    if let Some(index) = rules.iter().position(|r| r.id == rule.id) {
                        result.push(index);
                    }
                }
                result
            }
            SortKey::Process => {
                indexed.sort_by(|&a, &b| rules[a].exe_name.cmp(&rules[b].exe_name));
                indexed
            }
            SortKey::Mode => {
                indexed.sort_by_key(|&i| rules[i].mode.as_u8());
                indexed
            }
            SortKey::Enabled => {
                indexed.sort_by_key(|&i| !rules[i].is_enabled);
                indexed
            }
            SortKey::Priority => {
                indexed.sort_by_key(|&i| rules[i].priority);
                indexed
            }
            SortKey::Created => {
                indexed.sort_by(|&a, &b| rules[a].created_at.cmp(&rules[b].created_at));
                indexed
            }
        }
    }
}

struct ConsoleApp {
    config_path: Option<PathBuf>,
    config: Option<AppConfig>,
    rules: Vec<ProxyRule>,
    view: Vec<usize>,
    search: String,
    sort_key: SortKey,
    sort_descending: bool,
    selected_id: Option<String>,
    error: Option<String>,
    status: String,
}

impl ConsoleApp {
    fn new() -> Self {
        let mut app = Self {
            config_path: None,
            config: None,
            rules: Vec::new(),
            view: Vec::new(),
            search: String::new(),
            sort_key: SortKey::Canonical,
            sort_descending: false,
            selected_id: None,
            error: None,
            status: String::from("no configuration loaded — Ctrl+O to open"),
        };
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let default = PathBuf::from(appdata).join("IntentRouteAI").join("config.json");
            if default.is_file() {
                app.load(&default);
            }
        }
        app
    }

    fn load(&mut self, path: &PathBuf) {
        match std::fs::read(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))
            .and_then(|bytes| {
                let text = std::str::from_utf8(&bytes).map_err(|_| {
                    format!("{} is not valid UTF-8", path.display())
                })?;
                serde_json::from_str::<AppConfig>(text)
                    .map_err(|e| format!("{} does not match the config schema: {e}", path.display()))
            }) {
            Ok(config) => {
                self.rules = config.rules.clone();
                self.config = Some(config);
                self.config_path = Some(path.clone());
                self.error = None;
                self.status = format!(
                    "loaded {} — {} rule(s)",
                    path.display(),
                    self.rules.len()
                );
                self.selected_id = None;
                self.rebuild_view();
            }
            Err(error) => {
                self.error = Some(error);
                self.status = String::from("load failed");
            }
        }
    }

    fn reload(&mut self) {
        if let Some(path) = self.config_path.clone() {
            self.load(&path);
        }
    }

    fn rebuild_view(&mut self) {
        let needle = self.search.trim().to_lowercase();
        let mut indexes: Vec<usize> = (0..self.rules.len())
            .filter(|&i| {
                needle.is_empty()
                    || self.rules[i].exe_name.to_lowercase().contains(&needle)
                    || self.rules[i].target_hosts.to_lowercase().contains(&needle)
                    || self.rules[i].note.to_lowercase().contains(&needle)
            })
            .collect();

        let sorted = self.sort_key.order_of(&self.rules);
        let mut view: Vec<usize> = sorted
            .into_iter()
            .filter(|index| indexes.contains(index))
            .collect();
        if self.sort_descending {
            view.reverse();
        }
        indexes.clear();
        self.view = view;
    }

    fn mode_color(mode: ProxyMode) -> Color32 {
        match mode {
            ProxyMode::Proxy => ACCENT,
            ProxyMode::Direct => GREEN,
            ProxyMode::Block => RED,
        }
    }

    fn condition_summary(rule: &ProxyRule) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !rule.target_hosts.trim().is_empty() {
            parts.push(format!("hosts {}", rule.target_hosts));
        }
        if !rule.target_ips.trim().is_empty() {
            parts.push(format!("ip {}", rule.target_ips));
        }
        if !rule.target_ports.trim().is_empty() {
            parts.push(format!("port {}", rule.target_ports));
        }
        if !rule.protocol.trim().is_empty() {
            parts.push(rule.protocol.to_uppercase());
        }
        if parts.is_empty() {
            String::from("all traffic")
        } else {
            parts.join(" | ")
        }
    }

    fn validation_errors(rule: &ProxyRule) -> Vec<&'static str> {
        constraint::explain(&rule.target_hosts, &rule.target_ips, &rule.target_ports)
    }
}

impl eframe::App for ConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = CARD;
        visuals.widgets.inactive.bg_fill = CARD;
        visuals.widgets.inactive.fg_stroke.color = TEXT;
        visuals.widgets.hovered.fg_stroke.color = TEXT;
        visuals.selection.bg_fill = Color32::from_rgb(0x26, 0x4C, 0x8D);
        ctx.set_visuals(visuals);

        let mut action_open = false;
        let mut action_reload = false;

        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("file", |ui| {
                    action_open |= ui.button("open… (Ctrl+O)").clicked();
                    action_reload |= ui.button("reload (F5)").clicked();
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("read-only console — edit in the WPF app")
                            .color(AMBER)
                            .small(),
                    );
                });
            });
        });

        egui::TopBottomPanel::bottom("statusbar")
            .frame(egui::Frame::default().fill(CARD).stroke(egui::Stroke::new(1.0, BORDER)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).color(MUTED).small());
                    if let Some(error) = &self.error {
                        ui.separator();
                        ui.label(RichText::new(error).color(RED).small());
                    }
                });
            });

        if let Some(selected_id) = self.selected_id.clone() {
            if let Some(rule) = self.rules.iter().find(|r| r.id == selected_id).cloned() {
                egui::TopBottomPanel::bottom("detail")
                    .frame(
                        egui::Frame::default()
                            .fill(CARD)
                            .stroke(egui::Stroke::new(1.0, BORDER))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                    )
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("detail").strong().color(ACCENT));
                            ui.separator();
                            ui.label(RichText::new(&rule.exe_name).strong());
                            if let Some(note) = (!rule.note.trim().is_empty())
                                .then(|| format!("note: {}", rule.note.trim()))
                            {
                                ui.label(RichText::new(note).color(MUTED).small());
                            }
                        });
                        ui.add_space(4.0);
                        egui::Grid::new("detail-grid").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                            ui.label(RichText::new("id").color(MUTED).small());
                            ui.label(RichText::new(&rule.id).small());
                            ui.end_row();
                            ui.label(RichText::new("path").color(MUTED).small());
                            ui.label(RichText::new(if rule.exe_path.is_empty() { "—" } else { &rule.exe_path }).small());
                            ui.end_row();
                            ui.label(RichText::new("mode / status / priority").color(MUTED).small());
                            ui.label(
                                RichText::new(format!(
                                    "{} / {} / {}",
                                    rule.mode.name(),
                                    if rule.is_enabled { "enabled" } else { "disabled" },
                                    rule.priority
                                ))
                                .color(Self::mode_color(rule.mode))
                                .small(),
                            );
                            ui.end_row();
                            ui.label(RichText::new("hosts").color(MUTED).small());
                            ui.label(RichText::new(if rule.target_hosts.is_empty() { "—" } else { &rule.target_hosts }).small());
                            ui.end_row();
                            ui.label(RichText::new("ip / cidr").color(MUTED).small());
                            ui.label(RichText::new(if rule.target_ips.is_empty() { "—" } else { &rule.target_ips }).small());
                            ui.end_row();
                            ui.label(RichText::new("ports").color(MUTED).small());
                            ui.label(RichText::new(if rule.target_ports.is_empty() { "—" } else { &rule.target_ports }).small());
                            ui.end_row();
                        });
                        let errors = Self::validation_errors(&rule);
                        if errors.is_empty() {
                            ui.label(RichText::new("constraints: valid").color(GREEN).small());
                        } else {
                            ui.label(
                                RichText::new(format!("constraints invalid: {}", errors.join(", ")))
                                    .color(RED)
                                    .small(),
                            );
                        }
                    });
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let search_before = self.search.clone();
                ui.label("filter");
                egui::TextEdit::singleline(&mut self.search)
                    .desired_width(260.0)
                    .hint_text("process / hosts / note")
                    .show(ui);
                if self.search != search_before {
                    self.rebuild_view();
                }
                ui.separator();
                if ui
                    .button(format!("reload"))
                    .clicked()
                {
                    action_reload = true;
                }
            });
            ui.add_space(6.0);

            // Header row + click-to-sort, matching the WPF column set.
            ui.horizontal(|ui| {
                for (key, width) in [
                    (SortKey::Canonical, 34.0),
                    (SortKey::Process, 190.0),
                    (SortKey::Mode, 64.0),
                    (SortKey::Enabled, 72.0),
                    (SortKey::Priority, 58.0),
                    (SortKey::Created, 110.0),
                ] {
                    let arrow = if self.sort_key == key {
                        if self.sort_descending { " ▼" } else { " ▲" }
                    } else {
                        ""
                    };
                    let label = RichText::new(format!("{}{}", key.label(), arrow)).color(MUTED);
                    if ui
                        .add_sized([width, 18.0], egui::Button::new(label).small())
                        .clicked()
                    {
                        if self.sort_key == key {
                            self.sort_descending = !self.sort_descending;
                        } else {
                            self.sort_key = key;
                            self.sort_descending = false;
                        }
                        self.rebuild_view();
                    }
                    ui.add_space(12.0);
                }
                ui.label(RichText::new("constraints").color(MUTED).small());
            });
            ui.separator();

            if self.config.is_none() {
                ui.add_space(24.0);
                ui.label(
                    RichText::new(
                        "no configuration loaded — Ctrl+O opens a config.json, or launch from %APPDATA%\\IntentRouteAI",
                    )
                    .color(MUTED),
                );
            } else if self.view.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new("no rules match the filter").color(MUTED));
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (row, &index) in self.view.iter().enumerate() {
                        let rule = &self.rules[index];
                        let selected = self.selected_id.as_deref() == Some(rule.id.as_str());
                        let errors = Self::validation_errors(rule);
                        let row_background = if selected {
                            Color32::from_rgb(0x26, 0x4C, 0x8D)
                        } else {
                            CARD
                        };
                        let row_frame = egui::Frame::default()
                            .fill(row_background)
                            .stroke(egui::Stroke::new(1.0, BORDER))
                            .inner_margin(egui::Margin::symmetric(6.0, 5.0));
                        let response = row_frame
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add_sized(
                                        [28.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(format!("{}", row + 1)).color(MUTED).small(),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_sized(
                                        [190.0, 16.0],
                                        egui::Label::new(RichText::new(&rule.exe_name).color(TEXT))
                                            .selectable(false)
                                            .truncate(),
                                    );
                                    ui.add_sized(
                                        [64.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(rule.mode.name())
                                                .color(Self::mode_color(rule.mode))
                                                .strong(),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_sized(
                                        [72.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(if rule.is_enabled { "enabled" } else { "disabled" })
                                                .color(if rule.is_enabled { GREEN } else { MUTED }),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_sized(
                                        [58.0, 16.0],
                                        egui::Label::new(RichText::new(format!("{}", rule.priority)).color(MUTED))
                                            .selectable(false),
                                    );
                                    ui.add_sized(
                                        [110.0, 16.0],
                                        egui::Label::new(RichText::new(&rule.created_at).color(MUTED).small())
                                            .selectable(false),
                                    );
                                    ui.add_sized(
                                        [16.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(if errors.is_empty() { "" } else { "⚠" })
                                                .color(RED)
                                                .strong(),
                                        )
                                        .selectable(false),
                                    );
                                    ui.label(
                                        RichText::new(Self::condition_summary(rule))
                                            .color(MUTED)
                                            .small(),
                                    );
                                });
                            })
                            .response;
                        // The frame contents are labels; sense the whole frame for clicks.
                        let clicked = response.interact(Sense::click()).clicked();
                        if clicked {
                            self.selected_id = Some(rule.id.clone());
                        }
                        ui.add_space(2.0);
                    }
                });
            }
        });

        if action_open {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("IntentRoute configuration", &["json"])
                .pick_file()
            {
                self.load(&path);
            }
        }
        if action_reload {
            self.reload();
        }

        ctx.input_mut(|input| {
            if input.consume_key(egui::Modifiers::CTRL, egui::Key::O) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("IntentRoute configuration", &["json"])
                    .pick_file()
                {
                    self.load(&path);
                }
            }
            if input.consume_key(egui::Modifiers::NONE, egui::Key::F5) {
                self.reload();
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([860.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "IntentRoute AI — rules console (read-only)",
        options,
        Box::new(|_cc| Ok(Box::new(ConsoleApp::new()))),
    )
}
