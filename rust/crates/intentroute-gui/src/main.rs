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
    canonical_order, constraint, AppConfig, LoadStatus, ProxyMode, ProxyRule, ProxyServer,
    ProxyType, Workspace,
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
    edit_saved_fmt: &'static str, // {count}
    edit_blocked_hint: &'static str,
    // Constraints editor (parity slice)
    edit_constraints: &'static str,
    constraints_title: &'static str,
    constraints_hosts: &'static str,
    constraints_ips: &'static str,
    constraints_ports: &'static str,
    constraints_protocol: &'static str,
    constraints_protocol_any: &'static str,
    constraints_save: &'static str,
    constraints_invalid: &'static str,
    // Rule add / delete (parity slice 2)
    delete_rule: &'static str,
    confirm_delete_fmt: &'static str, // {rule}
    add_rule: &'static str,
    add_title: &'static str,
    add_process_label: &'static str,
    add_process_hint: &'static str,
    add_mode_label: &'static str,
    add_duplicate: &'static str,
    // Proxy server editor (parity slice 3)
    edit_servers: &'static str,
    servers_title: &'static str,
    servers_count_fmt: &'static str, // {count}
    server_type_socks: &'static str,
    server_type_http: &'static str,
    server_type_https: &'static str,
    server_host_label: &'static str,
    server_port_label: &'static str,
    server_username_label: &'static str,
    server_password_label: &'static str,
    server_bad_host: &'static str,
    server_bad_port: &'static str,
    server_enabled_label: &'static str,
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
    edit_constraints: "编辑约束",
    constraints_title: "编辑约束（原子写入）",
    constraints_hosts: "域名约束",
    constraints_ips: "IP / CIDR 约束",
    constraints_ports: "端口约束",
    constraints_protocol: "协议",
    constraints_protocol_any: "不限",
    constraints_save: "保存",
    constraints_invalid: "约束格式无效，修正后才能保存",
    delete_rule: "删除规则",
    confirm_delete_fmt: "删除规则 {rule}？此操作不可撤销。",
    add_rule: "添加规则",
    add_title: "添加规则（原子写入）",
    add_process_label: "进程名（如 chrome.exe 或 *）",
    add_process_hint: "仅精确进程名；* 表示全局规则",
    add_mode_label: "模式",
    add_duplicate: "同名进程的完整身份已存在，拒绝添加。",
    edit_servers: "代理服务器",
    servers_title: "代理服务器（原子写入，密码经 DPAPI 加密）",
    servers_count_fmt: "共 {count} 个服务器",
    server_type_socks: "SOCKS5",
    server_type_http: "HTTP",
    server_type_https: "HTTPS",
    server_host_label: "回环 IP",
    server_port_label: "端口",
    server_username_label: "用户名（可选）",
    server_password_label: "密码（可选，DPAPI 保存）",
    server_bad_host: "仅支持字面量回环 IP（如 127.0.0.1 或 ::1）",
    server_bad_port: "端口必须在 1–65535 之间",
    server_enabled_label: "启用",
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
    edit_constraints: "edit constraints",
    constraints_title: "edit constraints (atomic commit)",
    constraints_hosts: "hosts",
    constraints_ips: "ip / cidr",
    constraints_ports: "ports",
    constraints_protocol: "protocol",
    constraints_protocol_any: "any",
    constraints_save: "save",
    constraints_invalid: "invalid constraint format — fix before saving",
    delete_rule: "delete rule",
    confirm_delete_fmt: "delete rule {rule}? This cannot be undone.",
    add_rule: "add rule",
    add_title: "add rule (atomic commit)",
    add_process_label: "process name (e.g. chrome.exe or *)",
    add_process_hint: "exact process names only; * is the global rule",
    add_mode_label: "mode",
    add_duplicate: "a rule with the same full identity already exists; refused.",
    edit_servers: "proxy servers",
    servers_title: "proxy servers (atomic commit, DPAPI-protected passwords)",
    servers_count_fmt: "{count} server(s)",
    server_type_socks: "SOCKS5",
    server_type_http: "HTTP",
    server_type_https: "HTTPS",
    server_host_label: "loopback IP",
    server_port_label: "port",
    server_username_label: "username (optional)",
    server_password_label: "password (optional, DPAPI at rest)",
    server_bad_host: "only a literal loopback IP such as 127.0.0.1 or ::1 is supported",
    server_bad_port: "port must be 1–65535",
    server_enabled_label: "enabled",
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
    constraints_draft: Option<ConstraintsDraft>,
    add_draft: Option<AddDraft>,
    servers_open: bool,
    server_drafts: Vec<(String, ProxyServerEdit)>,
}

/// One confirmed edit intention; performed under the management lock.
#[derive(Clone)]
enum PendingEdit {
    ToggleEnabled { rule_id: String },
    SetMode { rule_id: String, mode: ProxyMode },
    /// Constraints editor: hosts / IPs / ports / protocol, pre-validated in
    /// the dialog and re-validated by the engine inside the transaction.
    UpdateConstraints {
        rule_id: String,
        hosts: String,
        ips: String,
        ports: String,
        protocol: String,
    },
    /// Delete the rule (after explicit confirmation).
    DeleteRule { rule_id: String },
    /// Add a rule with a fresh id, enabled, next priority, and the given
    /// process/mode — rejected inside the transaction when the full identity
    /// already exists.
    AddRule { exe_name: String, mode: ProxyMode },
    /// Replace a proxy server's editable fields (type/host/port/credentials/
    /// enabled); the engine re-validates loopback + port and re-encrypts the
    /// password on commit.
    UpdateServer { server_id: String, server: ProxyServerEdit },
}

#[derive(Clone)]
struct ProxyServerEdit {
    pub proxy_type: u8, // 0 socks, 1 http, 2 https
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub enabled: bool,
}

/// Editing buffer for the constraints dialog (phase parity slice).
#[derive(Clone)]
struct ConstraintsDraft {
    rule_id: String,
    hosts: String,
    ips: String,
    ports: String,
    protocol: usize, // index into the fixed protocol list
}

/// Editing buffer for the add-rule dialog.
#[derive(Clone)]
struct AddDraft {
    exe_name: String,
    mode: usize, // 0 proxy, 1 direct, 2 block
}

/// Random GUID-ish id for newly added rules (the WPF app uses Guid.NewGuid).
fn new_rule_id() -> String {
    format!(
        "r-{:x}-{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        std::process::id()
    )
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
            constraints_draft: None,
            add_draft: None,
            servers_open: false,
            server_drafts: Vec::new(),
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
            // Add-duplicate check runs against the freshly loaded state so a
            // same-identity rule (possibly added by another editor after this
            // view was loaded) is still refused.
            if let PendingEdit::AddRule { exe_name, mode } = edit {
                use intentroute_core::rule_identity_key;
                let draft = ProxyRule {
                    exe_name: exe_name.clone(),
                    mode: *mode,
                    priority: workspace
                        .snapshot()
                        .rules
                        .iter()
                        .map(|r| r.priority)
                        .max()
                        .unwrap_or(0)
                        + 10,
                    is_enabled: true,
                    ..ProxyRule::new(new_rule_id(), exe_name)
                };
                let draft_key = rule_identity_key(&draft);
                if workspace
                    .snapshot()
                    .rules
                    .iter()
                    .any(|r| rule_identity_key(r).eq_ignore_ascii_case(&draft_key))
                {
                    return Err(s.add_duplicate.to_string());
                }
            }

            let rule_id_for_add = matches!(edit, PendingEdit::AddRule { .. })
                .then(new_rule_id)
                .unwrap_or_default();
            let priority_for_add = workspace
                .snapshot()
                .rules
                .iter()
                .map(|r| r.priority)
                .max()
                .unwrap_or(0)
                + 10;
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
                PendingEdit::UpdateConstraints {
                    rule_id,
                    hosts,
                    ips,
                    ports,
                    protocol,
                } => {
                    if let Some(rule) = candidate.rules.iter_mut().find(|r| &r.id == rule_id) {
                        rule.target_hosts = hosts.clone();
                        rule.target_ips = ips.clone();
                        rule.target_ports = ports.clone();
                        rule.protocol = protocol.clone();
                    }
                }
                PendingEdit::DeleteRule { rule_id } => {
                    candidate.rules.retain(|r| &r.id != rule_id);
                }
                PendingEdit::AddRule { exe_name, mode } => {
                    candidate.rules.push(ProxyRule {
                        exe_name: exe_name.clone(),
                        mode: *mode,
                        priority: priority_for_add,
                        is_enabled: true,
                        ..ProxyRule::new(rule_id_for_add, exe_name)
                    });
                }
                PendingEdit::UpdateServer { server_id, server } => {
                    let port: i32 = server.port.trim().parse().unwrap_or(-1);
                    if let Some(target) = candidate
                        .proxy_servers
                        .iter_mut()
                        .find(|srv| &srv.id == server_id)
                    {
                        target.proxy_type = match server.proxy_type {
                            1 => ProxyType::Http,
                            2 => ProxyType::Https,
                            _ => ProxyType::Socks5,
                        };
                        target.host = server.host.trim().to_string();
                        target.port = port;
                        target.username = server.username.clone();
                        target.password = server.password.clone();
                        target.enabled = server.enabled;
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

/// Fixed protocol list for the constraints combo; "" = unrestricted, matching
/// the WPF editor's ordering (any / TCP / UDP / Both).
fn protocol_list(s: &UiStrings) -> [String; 4] {
    [
        s.constraints_protocol_any.to_string(),
        "TCP".to_string(),
        "UDP".to_string(),
        "Both".to_string(),
    ]
}

fn protocol_index(stored: &str) -> usize {
    match stored.trim().to_ascii_uppercase().as_str() {
        "TCP" => 1,
        "UDP" => 2,
        "BOTH" => 3,
        _ => 0,
    }
}

fn protocol_stored(index: usize) -> &'static str {
    match index {
        1 => "TCP",
        2 => "UDP",
        3 => "Both",
        _ => "",
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
        let mut open_constraints: Option<String> = None;
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
                            if ui.button(s.edit_constraints).clicked() {
                                open_constraints = Some(rule.id.clone());
                            }
                            ui.separator();
                            if ui.button(RichText::new(s.delete_rule).color(RED)).clicked() {
                                requested_edit = Some(PendingEdit::DeleteRule {
                                    rule_id: rule.id.clone(),
                                });
                            }
                        });
                    });
            }
            if let Some(edit) = requested_edit {
                self.pending_edit = Some(edit);
            }
            if let Some(rule_id) = open_constraints {
                if let Some(rule) = self.rules.iter().find(|r| r.id == rule_id) {
                    self.constraints_draft = Some(ConstraintsDraft {
                        rule_id: rule.id.clone(),
                        hosts: rule.target_hosts.clone(),
                        ips: rule.target_ips.clone(),
                        ports: rule.target_ports.clone(),
                        protocol: protocol_index(&rule.protocol),
                    });
                }
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
                ui.separator();
                if ui.button(s.add_rule).clicked() {
                    self.add_draft = Some(AddDraft {
                        exe_name: String::new(),
                        mode: 0,
                    });
                }
                if ui.button(s.edit_servers).clicked() {
                    self.server_drafts = self
                        .config
                        .as_ref()
                        .map(|c| {
                            c.proxy_servers
                                .iter()
                                .map(|srv| {
                                    (
                                        srv.id.clone(),
                                        ProxyServerEdit {
                                            proxy_type: match srv.proxy_type {
                                                ProxyType::Http => 1,
                                                ProxyType::Https => 2,
                                                ProxyType::Socks5 => 0,
                                            },
                                            host: srv.host.clone(),
                                            port: srv.port.to_string(),
                                            username: srv.username.clone(),
                                            password: srv.password.clone(),
                                            enabled: srv.enabled,
                                        },
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.servers_open = true;
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
            // Constraints and add edits skip the generic dialog: their editor
            // windows (with live validation / explicit fields) were the
            // confirmation. Delete keeps the destructive-action dialog.
            let needs_dialog = !matches!(
                edit,
                PendingEdit::UpdateConstraints { .. }
                    | PendingEdit::AddRule { .. }
                    | PendingEdit::UpdateServer { .. }
            );
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
                PendingEdit::UpdateConstraints { .. } => String::new(),
                PendingEdit::DeleteRule { rule_id } => {
                    let Some(rule) = self.rules.iter().find(|r| &r.id == rule_id) else {
                        self.pending_edit = None;
                        return;
                    };
                    s.confirm_delete_fmt.replace("{rule}", &rule.exe_name)
                }
                PendingEdit::AddRule { .. } => String::new(),
                PendingEdit::UpdateServer { .. } => String::new(),
            };
            if !needs_dialog {
                let edit = self.pending_edit.take().unwrap();
                self.perform_edit(&edit);
                return;
            }
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

        // Constraints editor dialog: live validation with the shared core
        // validator; Save is disabled until every field parses (the same gate
        // as the WPF RuleEditWindow), then routes through the confirmed-edit
        // path with its own lock→commit transaction.
        if let Some(draft) = self.constraints_draft.clone() {
            let mut close = false;
            let mut save_requested = false;
            let protocols = protocol_list(s);
            let valid = constraint::explain(&draft.hosts, &draft.ips, &draft.ports).is_empty();
            egui::Window::new(s.constraints_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("constraints-grid")
                        .num_columns(2)
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            let draft = &mut self.constraints_draft.as_mut().unwrap();
                            ui.label(s.constraints_hosts);
                            ui.add(egui::TextEdit::singleline(&mut draft.hosts).desired_width(320.0));
                            ui.end_row();
                            ui.label(s.constraints_ips);
                            ui.add(egui::TextEdit::singleline(&mut draft.ips).desired_width(320.0));
                            ui.end_row();
                            ui.label(s.constraints_ports);
                            ui.add(egui::TextEdit::singleline(&mut draft.ports).desired_width(320.0));
                            ui.end_row();
                            ui.label(s.constraints_protocol);
                            let selected = draft.protocol;
                            egui::ComboBox::new("constraints-protocol", "")
                                .selected_text(&protocols[selected])
                                .show_ui(ui, |ui| {
                                    for (index, label) in protocols.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut self.constraints_draft.as_mut().unwrap().protocol,
                                            index,
                                            label,
                                        );
                                    }
                                    let _ = selected;
                                });
                            ui.end_row();
                        });
                    let draft_for_errors = self.constraints_draft.clone().unwrap();
                    let localized: Vec<&'static str> = explain_localized(
                        &ProxyRule {
                            target_hosts: draft_for_errors.hosts.clone(),
                            target_ips: draft_for_errors.ips.clone(),
                            target_ports: draft_for_errors.ports.clone(),
                            ..ProxyRule::new(&draft_for_errors.rule_id, "")
                        },
                        std::ptr::eq(self.s, &ZH),
                    );
                    if !localized.is_empty() {
                        ui.label(
                            RichText::new(format!("{}: {}", s.constraints_invalid, localized.join(", ")))
                                .color(RED)
                                .small(),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(s.confirm_cancel).clicked() {
                            close = true;
                        }
                        let save = ui.add_enabled(valid, egui::Button::new(s.constraints_save));
                        if save.clicked() {
                            save_requested = true;
                        }
                    });
                });
            if close {
                self.constraints_draft = None;
            }
            if save_requested {
                let draft = self.constraints_draft.take().unwrap();
                self.pending_edit = Some(PendingEdit::UpdateConstraints {
                    rule_id: draft.rule_id,
                    hosts: draft.hosts.trim().to_string(),
                    ips: draft.ips.trim().to_string(),
                    ports: draft.ports.trim().to_string(),
                    protocol: protocol_stored(draft.protocol).to_string(),
                });
                // Perform immediately: the dialog itself was the confirmation.
                if let Some(edit) = self.pending_edit.take() {
                    self.perform_edit(&edit);
                }
            }
        }

        // Add-rule dialog: explicit process name + mode, enabled by default;
        // the duplicate check runs inside the transaction.
        if self.add_draft.is_some() {
            let mut close = false;
            let mut add_requested = false;
            egui::Window::new(s.add_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let draft = self.add_draft.as_mut().unwrap();
                    ui.label(s.add_process_label);
                    ui.add(egui::TextEdit::singleline(&mut draft.exe_name).desired_width(320.0));
                    ui.label(RichText::new(s.add_process_hint).color(MUTED).small());
                    ui.add_space(6.0);
                    ui.label(s.add_mode_label);
                    ui.horizontal(|ui| {
                        let labels = [s.mode_proxy, s.mode_direct, s.mode_block];
                        for (index, label) in labels.iter().enumerate() {
                            if ui
                                .selectable_label(draft.mode == index, *label)
                                .clicked()
                            {
                                draft.mode = index;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(s.confirm_cancel).clicked() {
                            close = true;
                        }
                        let name = self.add_draft.as_ref().unwrap().exe_name.trim().to_string();
                        let valid = !name.is_empty()
                            && (name == "*"
                                || (!name.contains(['*', '?', '/', '\\', ':'])
                                    && name.chars().all(|c| !c.is_control())));
                        if ui
                            .add_enabled(valid, egui::Button::new(s.constraints_save))
                            .clicked()
                        {
                            add_requested = true;
                        }
                    });
                });
            if close {
                self.add_draft = None;
            }
            if add_requested {
                let draft = self.add_draft.take().unwrap();
                let mode = match draft.mode {
                    1 => ProxyMode::Direct,
                    2 => ProxyMode::Block,
                    _ => ProxyMode::Proxy,
                };
                let edit = PendingEdit::AddRule {
                    exe_name: draft.exe_name.trim().to_string(),
                    mode,
                };
                self.perform_edit(&edit);
            }
        }

        // Proxy-server editor (parity slice 3): every server's editable fields
        // with live loopback/port validation. Save performs one locked
        // transaction per changed server; the engine re-validates and the
        // serializer re-encrypts passwords with DPAPI.
        if self.servers_open {
            let mut close = false;
            let mut save_requested = false;
            egui::Window::new(s.servers_title)
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let count = self.server_drafts.len();
                    ui.label(
                        RichText::new(s.servers_count_fmt.replace("{count}", &count.to_string()))
                            .color(MUTED)
                            .small(),
                    );
                    ui.add_space(4.0);
                    let mut all_valid = true;
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        for (index, (server_id, draft)) in self.server_drafts.iter_mut().enumerate()
                        {
                            let host_ok = intentroute_core::try_normalize_local_endpoint(
                                &draft.host,
                                draft.port.trim().parse().unwrap_or(-1),
                            )
                            .is_ok();
                            let port_ok = draft
                                .port
                                .trim()
                                .parse::<i32>()
                                .map(|p| (1..=65535).contains(&p))
                                .unwrap_or(false);
                            all_valid &= host_ok && port_ok;
                            egui::collapsing_header::CollapsingHeader::new(format!(
                                "{server_id} · {}",
                                draft.host
                            ))
                            .default_open(index == 0)
                            .show(ui, |ui| {
                                egui::Grid::new(format!("server-{server_id}"))
                                    .num_columns(2)
                                    .spacing([16.0, 6.0])
                                    .show(ui, |ui| {
                                        ui.label("type");
                                        ui.horizontal(|ui| {
                                            let labels = [
                                                s.server_type_socks,
                                                s.server_type_http,
                                                s.server_type_https,
                                            ];
                                            for (type_index, label) in
                                                labels.iter().enumerate()
                                            {
                                                if ui
                                                    .selectable_label(
                                                        draft.proxy_type == type_index as u8,
                                                        *label,
                                                    )
                                                    .clicked()
                                                {
                                                    draft.proxy_type = type_index as u8;
                                                }
                                            }
                                        });
                                        ui.end_row();
                                        ui.label(s.server_host_label);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut draft.host)
                                                .desired_width(220.0),
                                        );
                                        ui.end_row();
                                        ui.label(s.server_port_label);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut draft.port)
                                                .desired_width(120.0),
                                        );
                                        ui.end_row();
                                        ui.label(s.server_username_label);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut draft.username)
                                                .desired_width(220.0),
                                        );
                                        ui.end_row();
                                        ui.label(s.server_password_label);
                                        ui.add(
                                            egui::TextEdit::singleline(&mut draft.password)
                                                .desired_width(220.0)
                                                .password(true),
                                        );
                                        ui.end_row();
                                        ui.label(s.server_enabled_label);
                                        ui.checkbox(&mut draft.enabled, "");
                                        ui.end_row();
                                    });
                                if !host_ok {
                                    ui.label(
                                        RichText::new(s.server_bad_host).color(RED).small(),
                                    );
                                }
                                if !port_ok {
                                    ui.label(
                                        RichText::new(s.server_bad_port).color(RED).small(),
                                    );
                                }
                            });
                            ui.separator();
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button(s.confirm_cancel).clicked() {
                            close = true;
                        }
                        if ui
                            .add_enabled(all_valid, egui::Button::new(s.constraints_save))
                            .clicked()
                        {
                            save_requested = true;
                        }
                    });
                });
            if close {
                self.servers_open = false;
            }
            if save_requested {
                self.servers_open = false;
                let drafts = std::mem::take(&mut self.server_drafts);
                // One transaction per changed server keeps each commit a
                // single-purpose atomic unit, exactly like the WPF per-field
                // save paths.
                for (server_id, server) in drafts {
                    let edit = PendingEdit::UpdateServer { server_id, server };
                    self.perform_edit(&edit);
                }
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
