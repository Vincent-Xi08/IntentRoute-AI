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
    canonical_order, constraint, AppConfig, GlobalMode, LoadStatus, ProxyMode, ProxyRule,
    ProxyServer, ProxyType, Workspace,
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
    // Rule move (parity slice 4)
    move_up: &'static str,
    move_down: &'static str,
    // Policy panel (parity slice 5)
    policy_check: &'static str,
    policy_title: &'static str,
    policy_active: &'static str,
    policy_critical: &'static str,
    policy_warning: &'static str,
    policy_disabled: &'static str,
    finding_duplicate: &'static str,
    finding_no_server: &'static str,
    finding_chain: &'static str,
    finding_global_proxy: &'static str,
    finding_clean: &'static str,
    finding_count_fmt: &'static str, // {count}
    finding_shadow: &'static str,
    finding_broad: &'static str,
    finding_overlap: &'static str,
    // Route simulator (parity slice 6)
    sim_toggle: &'static str,
    sim_title: &'static str,
    sim_process: &'static str,
    sim_dest: &'static str,
    sim_dest_ip: &'static str,
    sim_dest_domain: &'static str,
    sim_port: &'static str,
    sim_transport: &'static str,
    sim_run: &'static str,
    sim_result_matched_fmt: &'static str, // {rule} {mode}
    sim_result_fallback: &'static str,
    sim_result_invalid: &'static str,
    // Global mode (parity slice 7)
    global_direct: &'static str,
    global_proxy: &'static str,
    global_confirm_fmt: &'static str, // {mode}
    // Process list (parity slice 8)
    process_toggle: &'static str,
    process_title: &'static str,
    process_search_hint: &'static str,
    process_add_rule: &'static str,
    process_count_fmt: &'static str, // {count}
    // Monitor / runtime log page (parity slice 11)
    monitor_toggle: &'static str,
    monitor_title: &'static str,
    monitor_exe: &'static str,
    monitor_browse: &'static str,
    monitor_probe: &'static str,
    monitor_start: &'static str,
    monitor_stop: &'static str,
    monitor_running: &'static str,
    monitor_stopped: &'static str,
    monitor_failed: &'static str,
    monitor_pid: &'static str, // {pid}
    monitor_version: &'static str, // {version}
    monitor_logs_title: &'static str,
    monitor_level: &'static str,
    monitor_search_hint: &'static str,
    monitor_clear: &'static str,
    monitor_export: &'static str,
    monitor_autoscroll: &'static str,
    monitor_empty: &'static str,
    monitor_need_config: &'static str,
    monitor_need_exe: &'static str,
    monitor_edit_blocked: &'static str,
    process_exists: &'static str,
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
    move_up: "上移",
    move_down: "下移",
    policy_check: "策略体检",
    policy_title: "策略体检（本地确定性分析，不发送任何数据）",
    policy_active: "活动规则",
    policy_critical: "高风险",
    policy_warning: "需复核",
    policy_disabled: "禁用草案",
    finding_duplicate: "完全重复身份",
    finding_no_server: "代理规则但无可用服务器",
    finding_chain: "引用不支持的代理链",
    finding_global_proxy: "全局代理但无可用服务器",
    finding_clean: "未发现可确定的问题。",
    finding_count_fmt: "共 {count} 项发现",
    finding_shadow: "规则不可达（被更早的规则遮蔽）",
    finding_broad: "范围过宽（无目标约束）",
    finding_overlap: "范围部分重叠（非证明提示）",
    sim_toggle: "路由推演",
    sim_title: "路由推演（严格静态 what-if，不解析 DNS、不探测、不观察流量）",
    sim_process: "进程名",
    sim_dest: "目标",
    sim_dest_ip: "IP",
    sim_dest_domain: "域名",
    sim_port: "端口",
    sim_transport: "协议",
    sim_run: "推演",
    sim_result_matched_fmt: "命中规则 {rule} → {mode}",
    sim_result_fallback: "无规则命中（全局回退）",
    sim_result_invalid: "查询无效",
    global_direct: "默认直连",
    global_proxy: "默认代理",
    global_confirm_fmt: "将全局模式切换为 {mode}？这影响所有未命中规则的流量走向。",
    process_toggle: "进程列表",
    process_title: "运行中的进程（点击「添加为规则」创建 Proxy 模式规则）",
    process_search_hint: "进程名或 PID",
    process_add_rule: "添加为规则",
    process_count_fmt: "{count} 个进程",
    monitor_toggle: "运行日志",
    monitor_title: "运行监控（启动前需显式选择本机 sing-box v1.13+ 可执行文件）",
    monitor_exe: "可执行文件：",
    monitor_browse: "浏览…",
    monitor_probe: "探测版本",
    monitor_start: "启动 sing-box",
    monitor_stop: "停止并释放",
    monitor_running: "运行中",
    monitor_stopped: "已停止",
    monitor_failed: "异常退出",
    monitor_pid: "PID {pid}",
    monitor_version: "sing-box {version}",
    monitor_logs_title: "运行日志（捕获时已脱敏，最多保留 200 行）",
    monitor_level: "级别",
    monitor_search_hint: "搜索日志",
    monitor_clear: "清空",
    monitor_export: "导出…",
    monitor_autoscroll: "自动滚动",
    monitor_empty: "暂无日志 —— 启动后 sing-box 的控制台输出会显示在这里",
    monitor_need_config: "请先加载配置再启动 sing-box",
    monitor_need_exe: "请先选择 sing-box 可执行文件（不会自动执行任何未确认的路径）",
    monitor_edit_blocked: "sing-box 托管运行中：请先在「运行日志」中停止，再编辑配置",
    process_exists: "同名进程已有完整身份相同的规则，拒绝添加。",
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
    move_up: "move up",
    move_down: "move down",
    policy_check: "policy check",
    policy_title: "policy check (local deterministic analysis, nothing sent)",
    policy_active: "active rules",
    policy_critical: "critical",
    policy_warning: "review",
    policy_disabled: "disabled drafts",
    finding_duplicate: "exact identity duplicate",
    finding_no_server: "proxy rule without an available server",
    finding_chain: "references unsupported proxy chain",
    finding_global_proxy: "global proxy mode without an available server",
    finding_clean: "No determinable issues found.",
    finding_count_fmt: "{count} finding(s)",
    finding_shadow: "rule unreachable (shadowed by an earlier rule)",
    finding_broad: "broad scope (no destination constraints)",
    finding_overlap: "partially overlapping scopes (unproven hint)",
    sim_toggle: "route simulator",
    sim_title: "route simulator (strict static what-if — no DNS, no probes, no traffic)",
    sim_process: "process",
    sim_dest: "destination",
    sim_dest_ip: "IP",
    sim_dest_domain: "domain",
    sim_port: "port",
    sim_transport: "transport",
    sim_run: "simulate",
    sim_result_matched_fmt: "matched rule {rule} → {mode}",
    sim_result_fallback: "no rule matched (global fallback)",
    sim_result_invalid: "invalid query",
    global_direct: "default direct",
    global_proxy: "default proxy",
    global_confirm_fmt: "switch global mode to {mode}? This affects traffic that no rule matches.",
    process_toggle: "processes",
    process_title: "running processes (click \"add as rule\" to create a Proxy-mode rule)",
    process_search_hint: "process name or PID",
    process_add_rule: "add as rule",
    process_count_fmt: "{count} processes",
    monitor_toggle: "runtime log",
    monitor_title: "runtime monitor (explicitly select a local sing-box v1.13+ executable before starting)",
    monitor_exe: "executable:",
    monitor_browse: "browse…",
    monitor_probe: "probe version",
    monitor_start: "start sing-box",
    monitor_stop: "stop and release",
    monitor_running: "running",
    monitor_stopped: "stopped",
    monitor_failed: "failed",
    monitor_pid: "PID {pid}",
    monitor_version: "sing-box {version}",
    monitor_logs_title: "runtime log (redacted at capture, at most 200 lines kept)",
    monitor_level: "level",
    monitor_search_hint: "search log",
    monitor_clear: "clear",
    monitor_export: "export…",
    monitor_autoscroll: "auto-scroll",
    monitor_empty: "no log lines yet — sing-box console output appears here once started",
    monitor_need_config: "load a configuration before starting sing-box",
    monitor_need_exe: "select the sing-box executable first (no unapproved path is ever executed)",
    monitor_edit_blocked: "sing-box is under management here: stop it on the runtime log page before editing configuration",
    process_exists: "a rule with the same full identity for this process already exists; refused.",
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
    policy_open: bool,
    sim_open: bool,
    sim_process: String,
    sim_dest: String,
    sim_is_ip: bool,
    sim_port: String,
    sim_is_udp: bool,
    sim_result: Option<String>,
    sim_result_ok: bool,
    process_open: bool,
    process_search: String,
    process_snapshot: Vec<intentroute_core::process::ProcessInfo>,
    // Monitor / runtime log page (parity slice 11). While `monitor` is
    // Some, the runtime lock is held and configuration edits are blocked.
    monitor_open: bool,
    monitor_exe_draft: String,
    monitor_probe_result: Option<(bool, String)>,
    monitor: Option<intentroute_core::singbox::ManagedRuntime>,
    monitor_min_level: intentroute_core::runtime_log::LogLevel,
    monitor_search: String,
    monitor_auto_scroll: bool,
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
    /// Move a rule up (-1) or down (+1) in Canonical Runtime Order, exactly
    /// like the WPF MoveRule: swap in canonical order, rewrite the persisted
    /// list in that order, and reassign priorities as (i+1)*10.
    MoveRule { rule_id: String, delta: i32 },
    /// Switch the global mode (ProxyAll ↔ DirectAll).
    SetGlobalMode { mode: GlobalMode },
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
            policy_open: false,
            sim_open: false,
            sim_process: String::new(),
            sim_dest: String::new(),
            sim_is_ip: false,
            sim_port: String::from("443"),
            sim_is_udp: false,
            sim_result: None,
            sim_result_ok: false,
            process_open: false,
            process_search: String::new(),
            process_snapshot: Vec::new(),
            monitor_open: false,
            monitor_exe_draft: String::new(),
            monitor_probe_result: None,
            monitor: None,
            monitor_min_level: intentroute_core::runtime_log::LogLevel::Info,
            monitor_search: String::new(),
            monitor_auto_scroll: true,
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

    /// Starts the managed sing-box for the monitor page: probe the
    /// explicitly selected executable, build the full configuration from a
    /// freshly workspace-loaded snapshot (DPAPI passwords are decrypted at
    /// that boundary; the built JSON is never displayed — it carries
    /// passwords), and hand both to the runtime engine, which holds the
    /// management lock for as long as the process is managed.
    fn start_monitor(&mut self) {
        if self.monitor.is_some() {
            return;
        }
        let Some(config_path) = self.config_path.clone() else {
            self.error = Some(self.s.monitor_need_config.to_string());
            return;
        };
        let exe = PathBuf::from(self.monitor_exe_draft.trim());
        if exe.as_os_str().is_empty() {
            self.error = Some(self.s.monitor_need_exe.to_string());
            return;
        }
        let directory = config_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default();
        // The raw display config still carries DPAPI envelopes; the runtime
        // file needs the decrypted values, so load through the workspace.
        let snapshot = match Workspace::load(&config_path) {
            LoadStatus::Loaded(workspace) => workspace.snapshot().clone(),
            LoadStatus::Missing => {
                self.error = Some(self.s.status_failed.to_string());
                return;
            }
            LoadStatus::Unusable(_, reason) => {
                self.error = Some(reason);
                return;
            }
        };

        match intentroute_core::singbox::probe_version(&exe) {
            Ok(version) => {
                self.monitor_probe_result =
                    Some((true, self.s.monitor_version.replace("{version}", &version)));
                match intentroute_core::build_sing_box_config(&snapshot) {
                    Ok(build) => {
                        match intentroute_core::singbox::ManagedRuntime::start(
                            &directory,
                            &exe,
                            &build.config_json,
                            Some(&version),
                            200,
                        ) {
                            Ok(runtime) => {
                                self.monitor = Some(runtime);
                            }
                            Err(error) => {
                                self.error = Some(error);
                            }
                        }
                    }
                    Err(error) => {
                        self.error = Some(error);
                    }
                }
            }
            Err(error) => {
                self.monitor_probe_result = Some((false, error));
            }
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
        if self.monitor.is_some() {
            // Our own managed runtime holds the lock; stop it first.
            self.error = Some(self.s.monitor_edit_blocked.to_string());
            return;
        }
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
                PendingEdit::MoveRule { rule_id, delta } => {
                    // WPF MoveRule parity: swap in canonical order (all rules,
                    // not just enabled), rewrite persisted order, reassign
                    // priorities as (i+1)*10. Out-of-range moves are no-ops.
                    let ordered = canonical_order(candidate.rules.clone());
                    if let Some(index) = ordered.iter().position(|r| &r.id == rule_id) {
                        let new_index = index as i32 + delta;
                        if new_index >= 0 && (new_index as usize) < ordered.len() {
                            let mut reordered = ordered;
                            reordered.swap(index, new_index as usize);
                            candidate.rules = reordered;
                            for (i, rule) in candidate.rules.iter_mut().enumerate() {
                                rule.priority = (i as i32 + 1) * 10;
                            }
                        }
                    }
                }
                PendingEdit::SetGlobalMode { mode } => {
                    candidate.global_mode = *mode;
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

// ── Policy check (parity slice 5) ──────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum PolicySeverity {
    Critical,
    Warning,
}

struct PolicyFindingGui {
    severity: PolicySeverity,
    kind: &'static str,
    detail: String,
}

/// Local deterministic analysis using existing core modules: identity
/// duplicates, proxy availability, chain references. This is the basic
/// subset — the full WPF engine also proves shadowing/containment and
/// broad-scope findings, which remain WPF-only.
fn analyze_policy(config: &AppConfig, s: &UiStrings) -> Vec<PolicyFindingGui> {
    use intentroute_core::rule_identity_key;
    let mut findings = Vec::new();

    // Identity duplicates: same full identity key (case-insensitive).
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for rule in &config.rules {
        let key = rule_identity_key(rule).to_lowercase();
        if let Some(first) = seen.get(key.as_str()) {
            findings.push(PolicyFindingGui {
                severity: PolicySeverity::Warning,
                kind: s.finding_duplicate,
                detail: format!("{} ↔ {}", first, rule.exe_name),
            });
        } else {
            seen.insert(key, &rule.exe_name);
        }
    }

    let has_enabled_server = config.proxy_servers.iter().any(|srv| srv.enabled);

    // Proxy rules without a usable server.
    for rule in &config.rules {
        if rule.is_enabled && rule.mode == ProxyMode::Proxy && !has_enabled_server {
            findings.push(PolicyFindingGui {
                severity: PolicySeverity::Critical,
                kind: s.finding_no_server,
                detail: rule.exe_name.clone(),
            });
        }
    }

    // Global proxy mode without a server.
    if config.global_mode == GlobalMode::ProxyAll && !has_enabled_server {
        findings.push(PolicyFindingGui {
            severity: PolicySeverity::Critical,
            kind: s.finding_global_proxy,
            detail: String::new(),
        });
    }

    // Chain references.
    for rule in &config.rules {
        if !rule.proxy_chain_id.trim().is_empty() {
            findings.push(PolicyFindingGui {
                severity: PolicySeverity::Critical,
                kind: s.finding_chain,
                detail: format!("{} → {}", rule.exe_name, rule.proxy_chain_id),
            });
        }
    }

    // Shadowing + broad scope (parity slice 9).
    for finding in intentroute_core::policy_findings::analyze(&config.rules) {
        let severity = match finding.severity {
            intentroute_core::policy_findings::Severity::Critical => PolicySeverity::Critical,
            intentroute_core::policy_findings::Severity::Warning => PolicySeverity::Warning,
            intentroute_core::policy_findings::Severity::Info => PolicySeverity::Warning,
        };
        findings.push(PolicyFindingGui {
            severity,
            kind: match finding.code {
                "PIR-SHADOW" => s.finding_shadow,
                "PIR-BROAD" => s.finding_broad,
                "PIR-OVERLAP" => s.finding_overlap,
                _ => finding.code,
            },
            detail: finding.detail,
        });
    }

    findings
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
                            if ui.button(s.move_up).clicked() {
                                requested_edit = Some(PendingEdit::MoveRule {
                                    rule_id: rule.id.clone(),
                                    delta: -1,
                                });
                            }
                            if ui.button(s.move_down).clicked() {
                                requested_edit = Some(PendingEdit::MoveRule {
                                    rule_id: rule.id.clone(),
                                    delta: 1,
                                });
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

        // Route simulator panel (parity slice 6)
        if self.sim_open {
            egui::TopBottomPanel::bottom("simulator")
                .frame(
                    egui::Frame::default()
                        .fill(CARD)
                        .stroke(egui::Stroke::new(1.0, BORDER))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                )
                .show(ctx, |ui| {
                    ui.label(RichText::new(s.sim_title).strong().color(ACCENT).small());
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(s.sim_process);
                        egui::TextEdit::singleline(&mut self.sim_process)
                            .hint_text("chrome.exe")
                            .desired_width(140.0)
                            .show(ui);
                        ui.separator();
                        ui.label(s.sim_dest);
                        egui::TextEdit::singleline(&mut self.sim_dest)
                            .hint_text("github.com")
                            .desired_width(180.0)
                            .show(ui);
                        if ui
                            .selectable_label(!self.sim_is_ip, s.sim_dest_domain)
                            .clicked()
                        {
                            self.sim_is_ip = false;
                        }
                        if ui.selectable_label(self.sim_is_ip, s.sim_dest_ip).clicked() {
                            self.sim_is_ip = true;
                        }
                        ui.separator();
                        ui.label(s.sim_port);
                        egui::TextEdit::singleline(&mut self.sim_port)
                            .desired_width(60.0)
                            .show(ui);
                        ui.separator();
                        ui.label(s.sim_transport);
                        if ui.selectable_label(!self.sim_is_udp, "TCP").clicked() {
                            self.sim_is_udp = false;
                        }
                        if ui.selectable_label(self.sim_is_udp, "UDP").clicked() {
                            self.sim_is_udp = true;
                        }
                        ui.separator();
                        if ui.button(s.sim_run).clicked() {
                            if let Some(config) = &self.config {
                                let port = self.sim_port.trim().parse::<u16>().unwrap_or(0);
                                let query = intentroute_core::route_sim::RouteQuery {
                                    process: self.sim_process.clone(),
                                    destination: self.sim_dest.clone(),
                                    is_ip: self.sim_is_ip,
                                    port,
                                    is_udp: self.sim_is_udp,
                                };
                                match intentroute_core::route_sim::simulate(&query, config) {
                                    intentroute_core::route_sim::RouteDecision::Matched {
                                        exe_name,
                                        mode,
                                    } => {
                                        self.sim_result = Some(
                                            s.sim_result_matched_fmt
                                                .replace("{rule}", &exe_name)
                                                .replace("{mode}", Self::mode_label(mode, s)),
                                        );
                                        self.sim_result_ok = true;
                                    }
                                    intentroute_core::route_sim::RouteDecision::NoMatch => {
                                        self.sim_result = Some(s.sim_result_fallback.to_string());
                                        self.sim_result_ok = true;
                                    }
                                    intentroute_core::route_sim::RouteDecision::InvalidQuery(r) => {
                                        self.sim_result =
                                            Some(format!("{}: {}", s.sim_result_invalid, r));
                                        self.sim_result_ok = false;
                                    }
                                }
                            }
                        }
                    });
                    if let Some(result) = &self.sim_result {
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(result)
                                .color(if self.sim_result_ok { GREEN } else { RED })
                                .strong(),
                        );
                    }
                });
        }

        // Process list panel (parity slice 8)
        if self.process_open {
            let mut refresh_requested = false;
            egui::TopBottomPanel::bottom("processes")
                .frame(
                    egui::Frame::default()
                        .fill(CARD)
                        .stroke(egui::Stroke::new(1.0, BORDER))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                )
                .default_height(240.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.process_title).strong().color(ACCENT).small());
                        ui.separator();
                        ui.label(
                            RichText::new(
                                s.process_count_fmt
                                    .replace("{count}", &self.process_snapshot.len().to_string()),
                            )
                            .color(MUTED)
                            .small(),
                        );
                        ui.separator();
                        egui::TextEdit::singleline(&mut self.process_search)
                            .hint_text(s.process_search_hint)
                            .desired_width(180.0)
                            .show(ui);
                        if ui.button(s.reload_short).clicked() {
                            refresh_requested = true;
                        }
                    });
                    ui.separator();
                    let needle = self.process_search.trim().to_lowercase();
                    let filtered: Vec<intentroute_core::process::ProcessInfo> = self
                        .process_snapshot
                        .iter()
                        .filter(|p| {
                            needle.is_empty()
                                || p.name.to_lowercase().contains(&needle)
                                || p.pid.to_string().contains(&needle)
                        })
                        .take(200)
                        .cloned()
                        .collect();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for process in &filtered {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:>6}", process.pid))
                                        .color(MUTED)
                                        .small(),
                                );
                                ui.label(RichText::new(&process.name).strong());
                                if !process.path.is_empty() {
                                    ui.label(
                                        RichText::new(process.path.trim())
                                            .color(MUTED)
                                            .small(),
                                    );
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button(s.process_add_rule).clicked() {
                                            let edit = PendingEdit::AddRule {
                                                exe_name: process.name.clone(),
                                                mode: ProxyMode::Proxy,
                                            };
                                            self.perform_edit(&edit);
                                        }
                                    },
                                );
                            });
                        }
                        if filtered.len() > 200 {
                            ui.label(
                                RichText::new(format!("… {} more", filtered.len() - 200))
                                    .color(MUTED)
                                    .small(),
                            );
                        }
                    });
                });
            if refresh_requested {
                self.process_snapshot = intentroute_core::process::snapshot_processes();
            }
        }

        // Monitor / runtime log panel (parity slice 11).
        if self.monitor_open {
            let mut start_requested = false;
            let mut stop_requested = false;
            let mut probe_requested = false;
            let mut browse_requested = false;
            let mut clear_requested = false;
            let mut export_requested = false;

            egui::TopBottomPanel::bottom("monitor")
                .frame(
                    egui::Frame::default()
                        .fill(CARD)
                        .stroke(egui::Stroke::new(1.0, BORDER))
                        .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                )
                .default_height(220.0)
                .show(ctx, |ui| {
                    ui.label(RichText::new(s.monitor_title).strong().color(ACCENT).small());
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label(s.monitor_exe);
                        egui::TextEdit::singleline(&mut self.monitor_exe_draft)
                            .hint_text("sing-box.exe")
                            .desired_width(340.0)
                            .show(ui);
                        if ui.button(s.monitor_browse).clicked() {
                            browse_requested = true;
                        }
                        if ui.button(s.monitor_probe).clicked() {
                            probe_requested = true;
                        }
                        if let Some((ok, text)) = &self.monitor_probe_result {
                            ui.label(RichText::new(text).color(if *ok { GREEN } else { RED }).small());
                        }
                    });

                    ui.horizontal(|ui| {
                        let running = self
                            .monitor
                            .as_ref()
                            .is_some_and(|runtime| runtime.status().is_running);
                        if running {
                            if ui.button(RichText::new(s.monitor_stop).strong()).clicked() {
                                stop_requested = true;
                            }
                        } else if ui.button(s.monitor_start).clicked() {
                            start_requested = true;
                        }
                        ui.separator();
                        if let Some(runtime) = &self.monitor {
                            let status = runtime.status();
                            let state_label = match status.state {
                                intentroute_core::singbox::RuntimeState::Running => {
                                    RichText::new(s.monitor_running).color(GREEN)
                                }
                                intentroute_core::singbox::RuntimeState::Stopped => {
                                    RichText::new(s.monitor_stopped).color(MUTED)
                                }
                                intentroute_core::singbox::RuntimeState::Failed => {
                                    RichText::new(s.monitor_failed).color(RED)
                                }
                            };
                            ui.label(state_label.strong());
                            if let Some(pid) = status.process_id {
                                ui.label(
                                    RichText::new(s.monitor_pid.replace("{pid}", &pid.to_string()))
                                        .color(MUTED)
                                        .small(),
                                );
                            }
                            if let Some(version) = &status.version {
                                ui.label(
                                    RichText::new(s.monitor_version.replace("{version}", version))
                                        .color(MUTED)
                                        .small(),
                                );
                            }
                            if let Some(error) = &status.last_error {
                                ui.label(RichText::new(error).color(RED).small());
                            }
                        }
                    });

                    ui.add_space(2.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.monitor_logs_title).color(MUTED).small());
                        ui.separator();
                        ui.label(RichText::new(s.monitor_level).color(MUTED).small());
                        egui::ComboBox::new("monitor-level", "")
                            .selected_text(self.monitor_min_level.name())
                            .show_ui(ui, |ui| {
                                for level in intentroute_core::runtime_log::LogLevel::all() {
                                    ui.selectable_value(
                                        &mut self.monitor_min_level,
                                        level,
                                        level.name(),
                                    );
                                }
                            });
                        egui::TextEdit::singleline(&mut self.monitor_search)
                            .hint_text(s.monitor_search_hint)
                            .desired_width(150.0)
                            .show(ui);
                        if ui.checkbox(&mut self.monitor_auto_scroll, s.monitor_autoscroll)
                            .clicked()
                        {
                            // State is stored directly.
                        }
                        if ui.button(s.monitor_clear).clicked() {
                            clear_requested = true;
                        }
                        if ui.button(s.monitor_export).clicked() {
                            export_requested = true;
                        }
                    });

                    let logs = self
                        .monitor
                        .as_ref()
                        .map(|runtime| runtime.recent_logs())
                        .unwrap_or_default();
                    let filtered: Vec<&intentroute_core::runtime_log::LogLine> = logs
                        .iter()
                        .filter(|entry| {
                            intentroute_core::runtime_log::matches(
                                &entry.message,
                                self.monitor_min_level,
                                &self.monitor_search,
                            )
                        })
                        .collect();
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .stick_to_bottom(self.monitor_auto_scroll)
                        .show(ui, |ui| {
                            if filtered.is_empty() {
                                ui.add_space(10.0);
                                ui.label(RichText::new(s.monitor_empty).color(MUTED));
                            }
                            for entry in filtered {
                                let level =
                                    intentroute_core::runtime_log::parse_level(&entry.message)
                                        .unwrap_or(intentroute_core::runtime_log::LogLevel::Info);
                                let message_color = if level
                                    >= intentroute_core::runtime_log::LogLevel::Error
                                {
                                    RED
                                } else {
                                    TEXT
                                };
                                ui.label(
                                    RichText::new(format!("[{}] {}", entry.time, entry.message))
                                        .family(egui::FontFamily::Monospace)
                                        .color(message_color)
                                        .small(),
                                );
                            }
                        });
                });

            if browse_requested {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("sing-box executable", &["exe"])
                    .pick_file()
                {
                    self.monitor_exe_draft = path.display().to_string();
                }
            }
            if probe_requested {
                let exe = PathBuf::from(self.monitor_exe_draft.trim());
                match intentroute_core::singbox::probe_version(&exe) {
                    Ok(version) => {
                        self.monitor_probe_result = Some((true, s.monitor_version.replace("{version}", &version)));
                    }
                    Err(error) => {
                        self.monitor_probe_result = Some((false, error));
                    }
                }
            }
            if start_requested {
                self.start_monitor();
            }
            if stop_requested {
                if let Some(runtime) = self.monitor.take() {
                    runtime.stop();
                }
                ctx.request_repaint();
            }
            if clear_requested {
                if let Some(runtime) = &self.monitor {
                    runtime.clear_logs();
                }
            }
            if export_requested {
                let logs = self
                    .monitor
                    .as_ref()
                    .map(|runtime| runtime.recent_logs())
                    .unwrap_or_default();
                let filtered: Vec<intentroute_core::runtime_log::LogLine> = logs
                    .into_iter()
                    .filter(|entry| {
                        intentroute_core::runtime_log::matches(
                            &entry.message,
                            self.monitor_min_level,
                            &self.monitor_search,
                        )
                    })
                    .collect();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("text", &["txt", "log"])
                    .set_file_name("intentroute-runtime-log.txt")
                    .save_file()
                {
                    // Export text passes through redaction a second time.
                    let text = intentroute_core::runtime_log::build_export_text(&filtered);
                    if let Err(error) = std::fs::write(&path, text.as_bytes()) {
                        self.error = Some(format!("{}: {error}", path.display()));
                    }
                }
            }
            if self.monitor.is_some() {
                // Keep the log view live while the managed process runs.
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }

        // Policy check panel (parity slice 5): KPI stats + basic findings,
        // all local using existing core modules.
        if self.policy_open {
            if let Some(config) = &self.config {
                let findings = analyze_policy(config, s);
                egui::TopBottomPanel::bottom("policy")
                    .frame(
                        egui::Frame::default()
                            .fill(CARD)
                            .stroke(egui::Stroke::new(1.0, BORDER))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                    )
                    .default_height(200.0)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(s.policy_title).strong().color(ACCENT).small());
                            ui.separator();
                            ui.label(RichText::new(
                                s.finding_count_fmt.replace("{count}", &findings.len().to_string()),
                            )
                            .color(MUTED)
                            .small());
                        });
                        ui.add_space(4.0);
                        // KPI row
                        ui.horizontal(|ui| {
                            let active = config.rules.iter().filter(|r| r.is_enabled).count();
                            let disabled = config.rules.len() - active;
                            let critical = findings
                                .iter()
                                .filter(|f| f.severity == PolicySeverity::Critical)
                                .count();
                            let warning = findings
                                .iter()
                                .filter(|f| f.severity == PolicySeverity::Warning)
                                .count();
                            for (label, count, color) in [
                                (s.policy_active, active, TEXT),
                                (s.policy_critical, critical, RED),
                                (s.policy_warning, warning, AMBER),
                                (s.policy_disabled, disabled, MUTED),
                            ] {
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(label).color(MUTED).small());
                                    ui.label(RichText::new(format!("{count}")).size(20.0).strong().color(color));
                                });
                                ui.add_space(24.0);
                            }
                        });
                        ui.separator();
                        // Findings
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if findings.is_empty() {
                                ui.label(RichText::new(s.finding_clean).color(GREEN).small());
                            }
                            for finding in &findings {
                                let color = match finding.severity {
                                    PolicySeverity::Critical => RED,
                                    PolicySeverity::Warning => AMBER,
                                };
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("●").color(color).small());
                                    ui.label(
                                        RichText::new(format!("{}: {}", finding.kind, finding.detail))
                                            .color(MUTED)
                                            .small(),
                                    );
                                });
                            }
                        });
                    });
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
                if ui.button(s.policy_check).clicked() {
                    self.policy_open = !self.policy_open;
                }
                if ui.button(s.sim_toggle).clicked() {
                    self.sim_open = !self.sim_open;
                }
                if ui.button(s.process_toggle).clicked() {
                    self.process_open = !self.process_open;
                    if self.process_open && self.process_snapshot.is_empty() {
                        self.process_snapshot = intentroute_core::process::snapshot_processes();
                    }
                }
                if ui.button(s.monitor_toggle).clicked() {
                    self.monitor_open = !self.monitor_open;
                }
                // Global mode display + toggle (parity slice 7): showing the
                // current persisted mode; clicking offers the other one behind
                // a confirmation dialog that runs the locked transaction.
                if let Some(config) = &self.config {
                    let current_label = match config.global_mode {
                        GlobalMode::ProxyAll => s.global_proxy,
                        GlobalMode::DirectAll => s.global_direct,
                    };
                    let next_mode = match config.global_mode {
                        GlobalMode::ProxyAll => GlobalMode::DirectAll,
                        GlobalMode::DirectAll => GlobalMode::ProxyAll,
                    };
                    ui.separator();
                    ui.label(RichText::new(current_label).color(MUTED).small());
                    if ui.button(RichText::new("⇄").strong()).clicked() {
                        self.pending_edit = Some(PendingEdit::SetGlobalMode { mode: next_mode });
                    }
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
                    | PendingEdit::MoveRule { .. }
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
                PendingEdit::MoveRule { .. } => String::new(),
                PendingEdit::SetGlobalMode { mode } => {
                    let mode_label = match mode {
                        GlobalMode::ProxyAll => s.global_proxy,
                        GlobalMode::DirectAll => s.global_direct,
                    };
                    s.global_confirm_fmt.replace("{mode}", mode_label)
                }
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
