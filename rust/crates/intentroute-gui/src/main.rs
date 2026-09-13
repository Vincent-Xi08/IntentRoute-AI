//! intentroute-gui — rules console (Rust migration, phase 3a–3c).
//!
//! A pure-Rust eframe/egui shell over the shared `intentroute-core`: loads the
//! real product configuration (strict UTF-8, schema-checked), shows rules in
//! Canonical Runtime Order with search, column sorting, constraint-validation
//! flags, and a detail panel.
//!
//! Phase 3c GUI slice — bounded editing: the detail panel can toggle a rule's
//! enabled state and set its mode (proxy/direct/block). Every edit runs one
//! load→commit transaction through the core `workspace` engine while holding
//! the `sing-box.runtime.lock` management lock, so a running WPF instance
//! blocks the edit (and vice versa) instead of racing on `config.json`. The
//! engine validates the complete candidate and never leaves memory or disk
//! half-written; passwords stay DPAPI-protected at rest. No other fields are
//! editable from this shell yet.
//!
//! Phase 3b — Chinese UI parity: a CJK system font (Microsoft YaHei → SimHei →
//! SimSun) is loaded at runtime so Chinese renders correctly; nothing is
//! bundled into the repository. When no CJK font is present the UI falls back
//! to English instead of rendering placeholder boxes. `INTENTROUTE_GUI_LANG`
//! (zh|en) overrides the automatic choice. GUI strings are self-contained in
//! this crate for now — the 553-key WPF resource system is not yet wired into
//! Rust.

use eframe::egui;
use egui::{Color32, RichText, Sense};
use intentroute_core::runtime_lock::with_management_lock;
use intentroute_core::{
    canonical_order, constraint, AppConfig, LoadStatus, ProxyMode, ProxyRule, Workspace,
};
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

// ── UI strings (self-contained; see module docs) ────────────────────────────

struct UiStrings {
    title: &'static str,
    file: &'static str,
    open: &'static str,
    reload: &'static str,
    reload_short: &'static str,
    readonly_banner: &'static str,
    filter: &'static str,
    filter_hint: &'static str,
    col_index: &'static str,
    col_process: &'static str,
    col_mode: &'static str,
    col_status: &'static str,
    col_priority: &'static str,
    col_created: &'static str,
    col_constraints: &'static str,
    all_traffic: &'static str,
    enabled: &'static str,
    disabled: &'static str,
    mode_proxy: &'static str,
    mode_direct: &'static str,
    mode_block: &'static str,
    no_config: &'static str,
    no_match: &'static str,
    detail: &'static str,
    note_prefix: &'static str,
    label_id: &'static str,
    label_path: &'static str,
    label_msp: &'static str,
    label_hosts: &'static str,
    label_ips: &'static str,
    label_ports: &'static str,
    valid: &'static str,
    invalid_prefix: &'static str,
    status_none: &'static str,
    status_failed: &'static str,
    status_loaded_fmt: &'static str, // {path} {count}
    dash: &'static str,
    // Phase 3c edit strings
    banner_edit: &'static str,
    edit_group: &'static str,
    toggle_enabled: &'static str,
    set_proxy: &'static str,
    set_direct: &'static str,
    set_block: &'static str,
    confirm_title: &'static str,
    confirm_toggle_fmt: &'static str, // {rule} {state}
    confirm_mode_fmt: &'static str,   // {rule} {mode}
    confirm_ok: &'static str,
    confirm_cancel: &'static str,
    edit_saved_fmt: &'static str,     // {count}
    edit_blocked_hint: &'static str,
}

const ZH: UiStrings = UiStrings {
    title: "IntentRoute AI — 规则控制台（只读）",
    file: "文件",
    open: "打开… (Ctrl+O)",
    reload: "重新加载 (F5)",
    reload_short: "重新加载",
    readonly_banner: "只读控制台 —— 请在 WPF 应用中编辑",
    filter: "过滤",
    filter_hint: "进程 / 域名 / 备注",
    col_index: "#",
    col_process: "进程",
    col_mode: "模式",
    col_status: "状态",
    col_priority: "优先级",
    col_created: "创建时间",
    col_constraints: "约束",
    all_traffic: "全部流量",
    enabled: "已启用",
    disabled: "已禁用",
    mode_proxy: "代理",
    mode_direct: "直连",
    mode_block: "阻止",
    no_config: "尚未加载配置 —— Ctrl+O 打开 config.json，或从 %APPDATA%\\IntentRouteAI 自动加载",
    no_match: "没有符合过滤条件的规则",
    detail: "详情",
    note_prefix: "备注：",
    label_id: "ID",
    label_path: "路径",
    label_msp: "模式 / 状态 / 优先级",
    label_hosts: "域名",
    label_ips: "IP / CIDR",
    label_ports: "端口",
    valid: "约束校验：通过",
    invalid_prefix: "约束无效：",
    status_none: "未加载配置 —— Ctrl+O 打开",
    status_failed: "加载失败",
    status_loaded_fmt: "已加载 {path} —— {count} 条规则",
    dash: "—",
    banner_edit: "编辑通过原子事务写入 —— WPF 实例运行时将被锁定阻止",
    edit_group: "编辑（原子写入）",
    toggle_enabled: "切换启用状态",
    set_proxy: "设为代理",
    set_direct: "设为直连",
    set_block: "设为阻止",
    confirm_title: "确认编辑",
    confirm_toggle_fmt: "将规则 {rule} 的启用状态切换为 {state}？",
    confirm_mode_fmt: "将规则 {rule} 的模式设为 {mode}？",
    confirm_ok: "确认",
    confirm_cancel: "取消",
    edit_saved_fmt: "编辑已提交 —— {count} 条规则",
    edit_blocked_hint: "另一个 IntentRoute AI 实例正在管理此目录，编辑被阻止。",
};

const EN: UiStrings = UiStrings {
    title: "IntentRoute AI — rules console (read-only)",
    file: "file",
    open: "open… (Ctrl+O)",
    reload: "reload (F5)",
    reload_short: "reload",
    readonly_banner: "read-only console — edit in the WPF app",
    filter: "filter",
    filter_hint: "process / hosts / note",
    col_index: "#",
    col_process: "process",
    col_mode: "mode",
    col_status: "status",
    col_priority: "priority",
    col_created: "created",
    col_constraints: "constraints",
    all_traffic: "all traffic",
    enabled: "enabled",
    disabled: "disabled",
    mode_proxy: "Proxy",
    mode_direct: "Direct",
    mode_block: "Block",
    no_config: "no configuration loaded — Ctrl+O opens a config.json, or launch from %APPDATA%\\IntentRouteAI",
    no_match: "no rules match the filter",
    detail: "detail",
    note_prefix: "note: ",
    label_id: "id",
    label_path: "path",
    label_msp: "mode / status / priority",
    label_hosts: "hosts",
    label_ips: "ip / cidr",
    label_ports: "ports",
    valid: "constraints: valid",
    invalid_prefix: "constraints invalid: ",
    status_none: "no configuration loaded — Ctrl+O to open",
    status_failed: "load failed",
    status_loaded_fmt: "loaded {path} — {count} rule(s)",
    dash: "—",
    banner_edit: "edits commit atomically — blocked while a WPF instance runs",
    edit_group: "edit (atomic commit)",
    toggle_enabled: "toggle enabled",
    set_proxy: "set proxy",
    set_direct: "set direct",
    set_block: "set block",
    confirm_title: "confirm edit",
    confirm_toggle_fmt: "toggle rule {rule} enabled state to {state}?",
    confirm_mode_fmt: "set rule {rule} mode to {mode}?",
    confirm_ok: "confirm",
    confirm_cancel: "cancel",
    edit_saved_fmt: "edit committed — {count} rule(s)",
    edit_blocked_hint: "Another IntentRoute AI instance is managing this directory; the edit was blocked.",
};

/// Constraint error names from the core are English; map to Chinese for the
/// detail panel when the Chinese UI is active.
fn explain_localized(rule: &ProxyRule, zh: bool) -> Vec<&'static str> {
    constraint::explain(&rule.target_hosts, &rule.target_ips, &rule.target_ports)
        .into_iter()
        .map(|error| {
            if !zh {
                return error;
            }
            match error {
                "invalid host list" => "域名列表无效",
                "invalid IP/CIDR list" => "IP/CIDR 列表无效",
                "invalid port list" => "端口列表无效",
                _ => error,
            }
        })
        .collect()
}

// ── CJK font loading (runtime system fonts, nothing bundled) ────────────────

const CJK_FONT_CANDIDATES: [(&str, &str); 3] = [
    (r"C:\Windows\Fonts\msyh.ttc", "Microsoft YaHei"),
    (r"C:\Windows\Fonts\simhei.ttf", "SimHei"),
    (r"C:\Windows\Fonts\simsun.ttc", "SimSun"),
];

/// First CJK system font that exists on this machine (checked, not loaded).
fn probe_cjk_font() -> Option<&'static str> {
    CJK_FONT_CANDIDATES
        .iter()
        .find(|(path, _)| PathBuf::from(path).is_file())
        .map(|(_, name)| *name)
}

/// Registers the first loadable CJK system font as a fallback for every egui
/// font family. Returns the font name on success.
fn install_cjk_font(ctx: &egui::Context) -> Option<&'static str> {
    let (path, name) = CJK_FONT_CANDIDATES
        .iter()
        .find(|(path, _)| PathBuf::from(path).is_file())?;
    let bytes = std::fs::read(path).ok()?;

    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".into(), egui::FontData::from_owned(bytes).into());
    // Appended last: Latin glyphs keep the bundled faces, CJK falls through.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".into());
    }
    ctx.set_fonts(fonts);
    let _ = name;
    Some(name)
}

fn resolve_language() -> bool {
    match std::env::var("INTENTROUTE_GUI_LANG").as_deref() {
        Ok("en") => false,
        Ok("zh") => true,
        _ => probe_cjk_font().is_some(),
    }
}

// ── Sorting / filtering ─────────────────────────────────────────────────────

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
    fn label(self, s: &UiStrings) -> &'static str {
        match self {
            SortKey::Canonical => s.col_index,
            SortKey::Process => s.col_process,
            SortKey::Mode => s.col_mode,
            SortKey::Enabled => s.col_status,
            SortKey::Priority => s.col_priority,
            SortKey::Created => s.col_created,
        }
    }

    fn order_of(self, rules: &[ProxyRule]) -> Vec<usize> {
        let mut indexed: Vec<usize> = (0..rules.len()).collect();
        match self {
            SortKey::Canonical => {
                let ordered = canonical_order(rules.to_vec());
                ordered
                    .iter()
                    .filter_map(|rule| rules.iter().position(|r| r.id == rule.id))
                    .collect()
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

// ── App ─────────────────────────────────────────────────────────────────────

struct ConsoleApp {
    s: &'static UiStrings,
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
    pending_edit: Option<PendingEdit>,
}

/// One confirmed edit intention; performed under the management lock.
#[derive(Clone)]
enum PendingEdit {
    ToggleEnabled { rule_id: String },
    SetMode { rule_id: String, mode: ProxyMode },
}

impl ConsoleApp {
    fn new(zh: bool) -> Self {
        let s = if zh { &ZH } else { &EN };
        let mut app = Self {
            s,
            config_path: None,
            config: None,
            rules: Vec::new(),
            view: Vec::new(),
            search: String::new(),
            sort_key: SortKey::Canonical,
            sort_descending: false,
            selected_id: None,
            error: None,
            status: s.status_none.to_string(),
            pending_edit: None,
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
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))?;
                serde_json::from_str::<AppConfig>(text).map_err(|e| {
                    format!("{} does not match the config schema: {e}", path.display())
                })
            }) {
            Ok(config) => {
                self.rules = config.rules.clone();
                self.config = Some(config);
                self.config_path = Some(path.clone());
                self.error = None;
                self.status = self
                    .s
                    .status_loaded_fmt
                    .replace("{path}", &path.display().to_string())
                    .replace("{count}", &self.rules.len().to_string());
                self.selected_id = None;
                self.rebuild_view();
            }
            Err(error) => {
                self.error = Some(error);
                self.status = self.s.status_failed.to_string();
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
        let matching: Vec<usize> = (0..self.rules.len())
            .filter(|&i| {
                needle.is_empty()
                    || self.rules[i].exe_name.to_lowercase().contains(&needle)
                    || self.rules[i].target_hosts.to_lowercase().contains(&needle)
                    || self.rules[i].note.to_lowercase().contains(&needle)
            })
            .collect();

        let mut view: Vec<usize> = self
            .sort_key
            .order_of(&self.rules)
            .into_iter()
            .filter(|index| matching.contains(index))
            .collect();
        if self.sort_descending {
            view.reverse();
        }
        self.view = view;
    }

    fn mode_color(mode: ProxyMode) -> Color32 {
        match mode {
            ProxyMode::Proxy => ACCENT,
            ProxyMode::Direct => GREEN,
            ProxyMode::Block => RED,
        }
    }

    fn mode_label(mode: ProxyMode, s: &UiStrings) -> &'static str {
        match mode {
            ProxyMode::Proxy => s.mode_proxy,
            ProxyMode::Direct => s.mode_direct,
            ProxyMode::Block => s.mode_block,
        }
    }

    fn condition_summary(rule: &ProxyRule, s: &UiStrings) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !rule.target_hosts.trim().is_empty() {
            parts.push(format!("{} {}", s.label_hosts, rule.target_hosts));
        }
        if !rule.target_ips.trim().is_empty() {
            parts.push(format!("{} {}", s.label_ips, rule.target_ips));
        }
        if !rule.target_ports.trim().is_empty() {
            parts.push(format!("{} {}", s.label_ports, rule.target_ports));
        }
        if !rule.protocol.trim().is_empty() {
            parts.push(rule.protocol.to_uppercase());
        }
        if parts.is_empty() {
            s.all_traffic.to_string()
        } else {
            parts.join(" | ")
        }
    }

    /// Performs the confirmed edit: hold the management lock, load the fresh
    /// on-disk configuration, apply the mutation through the transactional
    /// workspace engine, and adopt the published snapshot. A held lock (WPF
    /// running) surfaces the localized block message.
    fn perform_edit(&mut self, edit: &PendingEdit) {
        let Some(path) = self.config_path.clone() else {
            self.error = Some(self.s.status_none.to_string());
            return;
        };
        let directory = path.parent().map(PathBuf::from).unwrap_or_default();
        let s = self.s;

        let outcome = with_management_lock(&directory, || {
            let mut workspace = match Workspace::load(&path) {
                LoadStatus::Loaded(workspace) => *workspace,
                LoadStatus::Missing => return Err(s.status_failed.to_string()),
                LoadStatus::Unusable(_, reason) => return Err(reason),
            };
            workspace.commit(|candidate| match edit {
                PendingEdit::ToggleEnabled { rule_id } => {
                    if let Some(rule) = candidate.rules.iter_mut().find(|r| &r.id == rule_id) {
                        rule.is_enabled = !rule.is_enabled;
                    }
                }
                PendingEdit::SetMode { rule_id, mode } => {
                    if let Some(rule) = candidate.rules.iter_mut().find(|r| &r.id == rule_id) {
                        rule.mode = *mode;
                    }
                }
            })
        });

        match outcome {
            Ok(published) => {
                self.error = None;
                self.status = s
                    .edit_saved_fmt
                    .replace("{count}", &published.rules.len().to_string());
                self.rules = published.rules.clone();
                self.config = Some(published);
                self.rebuild_view();
            }
            Err(reason) => {
                let blocked = reason.contains("already managing");
                self.error = Some(if blocked {
                    s.edit_blocked_hint.to_string()
                } else {
                    reason
                });
                self.status = s.status_failed.to_string();
            }
        }
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
        let s = self.s;

        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button(s.file, |ui| {
                    action_open |= ui.button(s.open).clicked();
                    action_reload |= ui.button(s.reload).clicked();
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(s.banner_edit).color(AMBER).small());
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

        let mut requested_edit: Option<PendingEdit> = None;
        if let Some(selected_id) = self.selected_id.clone() {
            if let Some(rule) = self.rules.iter().find(|r| r.id == selected_id).cloned() {
                let errors = explain_localized(&rule, std::ptr::eq(self.s, &ZH));
                egui::TopBottomPanel::bottom("detail")
                    .frame(
                        egui::Frame::default()
                            .fill(CARD)
                            .stroke(egui::Stroke::new(1.0, BORDER))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                    )
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(s.detail).strong().color(ACCENT));
                            ui.separator();
                            ui.label(RichText::new(&rule.exe_name).strong());
                            if !rule.note.trim().is_empty() {
                                ui.label(
                                    RichText::new(format!("{}{}", s.note_prefix, rule.note.trim()))
                                        .color(MUTED)
                                        .small(),
                                );
                            }
                        });
                        ui.add_space(4.0);
                        let dash = s.dash;
                        egui::Grid::new("detail-grid").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                            ui.label(RichText::new(s.label_id).color(MUTED).small());
                            ui.label(RichText::new(&rule.id).small());
                            ui.end_row();
                            ui.label(RichText::new(s.label_path).color(MUTED).small());
                            ui.label(
                                RichText::new(if rule.exe_path.is_empty() { dash } else { &rule.exe_path }).small(),
                            );
                            ui.end_row();
                            ui.label(RichText::new(s.label_msp).color(MUTED).small());
                            ui.label(
                                RichText::new(format!(
                                    "{} / {} / {}",
                                    Self::mode_label(rule.mode, s),
                                    if rule.is_enabled { s.enabled } else { s.disabled },
                                    rule.priority
                                ))
                                .color(Self::mode_color(rule.mode))
                                .small(),
                            );
                            ui.end_row();
                            ui.label(RichText::new(s.label_hosts).color(MUTED).small());
                            ui.label(
                                RichText::new(if rule.target_hosts.is_empty() { dash } else { &rule.target_hosts })
                                    .small(),
                            );
                            ui.end_row();
                            ui.label(RichText::new(s.label_ips).color(MUTED).small());
                            ui.label(
                                RichText::new(if rule.target_ips.is_empty() { dash } else { &rule.target_ips }).small(),
                            );
                            ui.end_row();
                            ui.label(RichText::new(s.label_ports).color(MUTED).small());
                            ui.label(
                                RichText::new(if rule.target_ports.is_empty() { dash } else { &rule.target_ports })
                                    .small(),
                            );
                            ui.end_row();
                        });
                        if errors.is_empty() {
                            ui.label(RichText::new(s.valid).color(GREEN).small());
                        } else {
                            ui.label(
                                RichText::new(format!("{}{}", s.invalid_prefix, errors.join(", ")))
                                    .color(RED)
                                    .small(),
                            );
                        }
                        ui.add_space(6.0);
                        ui.separator();
                        ui.label(RichText::new(s.edit_group).strong().color(ACCENT).small());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button(s.toggle_enabled).clicked() {
                                requested_edit = Some(PendingEdit::ToggleEnabled {
                                    rule_id: rule.id.clone(),
                                });
                            }
                            for (label, mode) in [
                                (s.set_proxy, ProxyMode::Proxy),
                                (s.set_direct, ProxyMode::Direct),
                                (s.set_block, ProxyMode::Block),
                            ] {
                                if ui.button(label).clicked() {
                                    requested_edit = Some(PendingEdit::SetMode {
                                        rule_id: rule.id.clone(),
                                        mode,
                                    });
                                }
                            }
                        });
                    });
            }
            if let Some(edit) = requested_edit {
                self.pending_edit = Some(edit);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let search_before = self.search.clone();
                ui.label(s.filter);
                egui::TextEdit::singleline(&mut self.search)
                    .desired_width(260.0)
                    .hint_text(s.filter_hint)
                    .show(ui);
                if self.search != search_before {
                    self.rebuild_view();
                }
                ui.separator();
                if ui.button(s.reload_short).clicked() {
                    action_reload = true;
                }
            });
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                for (key, width) in [
                    (SortKey::Canonical, 44.0),
                    (SortKey::Process, 130.0),
                    (SortKey::Mode, 56.0),
                    (SortKey::Enabled, 64.0),
                    (SortKey::Priority, 58.0),
                    (SortKey::Created, 96.0),
                ] {
                    let arrow = if self.sort_key == key {
                        if self.sort_descending { " ▼" } else { " ▲" }
                    } else {
                        ""
                    };
                    let label = RichText::new(format!("{}{}", key.label(s), arrow)).color(MUTED);
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
                ui.label(RichText::new(s.col_constraints).color(MUTED).small());
            });
            ui.separator();

            if self.config.is_none() {
                ui.add_space(24.0);
                ui.label(RichText::new(s.no_config).color(MUTED));
            } else if self.view.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new(s.no_match).color(MUTED));
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (row, &index) in self.view.iter().enumerate() {
                        let rule = &self.rules[index];
                        let selected = self.selected_id.as_deref() == Some(rule.id.as_str());
                        let errors = explain_localized(rule, std::ptr::eq(self.s, &ZH));
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
                                        [56.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(Self::mode_label(rule.mode, s))
                                                .color(Self::mode_color(rule.mode))
                                                .strong(),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_sized(
                                        [64.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(if rule.is_enabled { s.enabled } else { s.disabled })
                                                .color(if rule.is_enabled { GREEN } else { MUTED }),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_sized(
                                        [58.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(format!("{}", rule.priority)).color(MUTED),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_sized(
                                        [96.0, 16.0],
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
                                        RichText::new(Self::condition_summary(rule, s))
                                            .color(MUTED)
                                            .small(),
                                    );
                                });
                            })
                            .response;
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

        // Edit confirmation dialog (phase 3c): plain-text intention shown
        // before any write path runs.
        let mut confirmed = false;
        let mut cancelled = false;
        if let Some(edit) = self.pending_edit.clone() {
            let message = match &edit {
                PendingEdit::ToggleEnabled { rule_id } => {
                    let Some(rule) = self.rules.iter().find(|r| &r.id == rule_id) else {
                        self.pending_edit = None;
                        return;
                    };
                    let next_state = if rule.is_enabled { s.disabled } else { s.enabled };
                    s.confirm_toggle_fmt
                        .replace("{rule}", &rule.exe_name)
                        .replace("{state}", next_state)
                }
                PendingEdit::SetMode { rule_id, mode } => {
                    let Some(rule) = self.rules.iter().find(|r| &r.id == rule_id) else {
                        self.pending_edit = None;
                        return;
                    };
                    s.confirm_mode_fmt
                        .replace("{rule}", &rule.exe_name)
                        .replace("{mode}", Self::mode_label(*mode, s))
                }
            };
            egui::Window::new(s.confirm_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(message);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(s.confirm_ok).clicked() {
                            confirmed = true;
                        }
                        if ui.button(s.confirm_cancel).clicked() {
                            cancelled = true;
                        }
                    });
                });
            if confirmed {
                self.pending_edit = None;
                self.perform_edit(&edit);
            } else if cancelled {
                self.pending_edit = None;
            }
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
    let zh = resolve_language();
    let title = if zh { ZH.title } else { EN.title };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([860.0, 560.0])
            .with_title(title),
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            if zh {
                install_cjk_font(&cc.egui_ctx);
            }
            Ok(Box::new(ConsoleApp::new(zh)))
        }),
    )
}
