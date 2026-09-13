# IntentRoute AI

English | **[中文](README.zh-CN.md)**

[![CI](https://github.com/Lucas-Xi/IntentRoute-AI/actions/workflows/ci.yml/badge.svg)](https://github.com/Lucas-Xi/IntentRoute-AI/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Lucas-Xi/IntentRoute-AI?include_prereleases)](https://github.com/Lucas-Xi/IntentRoute-AI/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

IntentRoute AI is an open-source Windows control plane that turns plain-language network intent into locally validated routing-rule drafts. It can use the OpenAI Responses API or an already-running local Ollama model, then hands accepted rules to the same deterministic sing-box TUN configuration pipeline used by the manual editor.

> **Project status: v0.27.0 preview.** IntentRoute AI is useful for testing and early adoption, but it has not yet demonstrated broad production usage. AI output can be incomplete or wrong. Every generated rule is locally validated, added disabled, and must be explicitly enabled by the user.

v0.26.0 gives every bordered dialog a dark system title bar via the DWM immersive-dark-mode attribute and exercises the rule-constraints editor end-to-end for the first time since v0.13. v0.25.0 replaced all twenty in-app message boxes with themed dark dialogs and fixed a latent v0.17 entrance-animation crash; v0.24.0 claimed mixed-DPI visual validation with a recorded 100% + 125% dual-display run; v0.15–v0.23 delivered the import preview, shared dark design system, component polish, theme-colored pages, batch operations, keyboard interaction, and review hardening. Every release archive still embeds the unsigned `provenance.json` build inventory introduced in v0.9.0; code signing itself still requires a certificate.

## Why this project exists

Many Windows applications do not expose useful proxy controls, while hand-authoring process/domain/IP routing rules is error-prone. IntentRoute AI provides two complementary paths:

- A conventional, inspectable rule editor for deterministic manual configuration.
- An optional AI authoring assistant that translates natural language into a bounded, reviewable rule draft.
- A local Policy Intelligence workflow that proves ordering, duplicate, conflict, shadowing, broad-scope, and disabled-draft findings before an optional AI explanation.
- A local Route Decision Simulator that explains what the saved policy can prove for one concrete hypothetical process/destination/port/protocol input without pretending to observe traffic.

The application does not capture packets itself. It generates a validated [sing-box](https://github.com/SagerNet/sing-box) v1.13+ TUN configuration, starts and supervises the external sing-box process, and keeps the default route direct unless a rule says otherwise.

## AI workflow

1. Select **OpenAI** or **Ollama (local)**.
2. Enter an intent such as: “Route Chrome and Cursor traffic for GitHub and OpenAI through the proxy; keep everything else direct.”
3. The provider returns a strict structured draft containing process, host/IP, port, protocol, action, rationale, confidence, and warnings.
4. IntentRoute AI treats the result as untrusted input and validates field limits, executable names, domains, CIDRs, ports, protocols, actions, duplicates, and proxy availability.
5. A temporary enabled candidate is passed through `SingBoxConfigBuilder` so disabled-rule filtering cannot make validation a no-op. This preview dry-run is deterministic in-process config construction; it intentionally does not execute an external program.
6. The user reviews the preview and may edit any draft field (process, action, domains, IP/CIDR, ports, protocol, rationale). Every edit re-runs the same deterministic validation — including the enabled-clone dry-run — before the draft can be accepted as **disabled rules**.
7. Enabling remains a separate user action. That state-changing path writes a candidate file and executes `sing-box check -c` before the managed runtime is replaced.

AI never directly enables rules, invokes commands, selects files, installs models, downloads sing-box, or applies an unreviewed configuration.

## AI Policy Intelligence workflow

1. Open **AI Policy Intelligence**. A cancellation-aware background worker analyzes a detached configuration snapshot without blocking the WPF dispatcher; this performs no provider request, filesystem write, runtime apply, executable probe, proxy connection, DNS lookup, or traffic observation.
2. Findings use the same Canonical Runtime Order as the generated sing-box route: priority ascending, creation timestamp ascending, then persisted source order. The rules page uses that order too.
3. The local report distinguishes exact duplicates, same-scope different outcomes, proven earlier-superset shadowing, broad process/global rules, invalid disabled rules, inactive duplicates, same-priority overlaps, the ProxyAll default posture, and non-proven partial-overlap hints. Equivalent suffix, integer-port-union, and mergeable CIDR-union spellings are canonicalized before comparison. Uncertain partial overlaps are not promoted into facts.
4. Local rows may show real rule labels and can navigate to an affected rule. They are never serialized to a provider.
5. To request an explanation, select 1–20 findings and click **Explain selected summary with AI**. A confirmation dialog shows the exact logical JSON, provider, and exclusion list for that single request.
6. The closed Policy Disclosure contains only aggregate counts plus finding code, category, severity, relationship, and affected-rule count. The AI response must use a strict schema and reference only those finding codes.
7. AI explanation is plain, untrusted, read-only text. It cannot change local findings, write a note, create/enable/reorder a rule, save configuration, or apply sing-box. The local fingerprint is rechecked before preview, after confirmation but before sending, and after the response; stale summaries are not sent and stale responses are discarded.

Policy Intelligence describes static configuration semantics, not real connection behavior. A clean report is not proof that TUN creation, a proxy listener, authentication, upstream reachability, DNS behavior, or a particular connection succeeded.

## AI Route Decision Simulator

1. Open **AI Route Simulator** and enter one exact process name, one concrete domain or literal IPv4/IPv6 address, one port, and TCP or UDP.
2. A bounded background worker validates the detached Configuration Snapshot through the production `SingBoxConfigBuilder`, then evaluates enabled rules in Canonical Runtime Order.
3. The result is deliberately three-valued: a proven first-rule match, a proven global fallback after all rules miss, or **Indeterminate** when a missing resolved IP/domain context could allow an earlier rule to win. Invalid input and invalid policy are separate fail-closed states and never return an action.
4. The page shows a local evaluation trace and can navigate to a proven matched rule. A fingerprint binds the result to both the snapshot and normalized query; query or configuration changes hide the old result.
5. Recovery Protection disables simulation rather than evaluating the empty placeholder state.

This is a static what-if tool, not telemetry. It does not resolve DNS, reverse-resolve an IP, probe a proxy, inspect connections, read runtime logs, enumerate packets, invoke sing-box, or change/apply configuration. The hypothetical query, local rule labels/IDs, proxy identity, and evaluation trace never cross an AI-provider seam.

## Provider setup

### OpenAI

IntentRoute AI reads the user's key at request time from `OPENAI_API_KEY`. The key is not accepted in the app UI and is never written to the application configuration, profiles, logs, exports, or diagnostics.

PowerShell example for the current Windows user:

```powershell
[Environment]::SetEnvironmentVariable('OPENAI_API_KEY', 'your-api-key', 'User')
```

Restart IntentRoute AI after changing the environment variable. The OpenAI request uses the Responses API, strict JSON Schema output, no tools, a bounded timeout/output size, and `store=false`.

### Local Ollama

Install [Ollama](https://ollama.com/), start its local service, and install a model separately. For example:

```powershell
ollama pull qwen3:8b
```

IntentRoute AI queries only literal HTTP `127.0.0.1` (the default) or `::1`. It rejects hostnames, other loopback addresses, credentialed endpoints, HTTPS, LAN, and public Ollama endpoints; disables proxy use and redirects for these requests; and never pulls a model or launches Ollama automatically. The UI lists models already installed through `GET /api/tags`.

### Health diagnostics

The Settings page offers a credential-free provider health check for the currently selected provider and model. For OpenAI it reports only whether `OPENAI_API_KEY` is present — the key is never displayed and no network request is sent. For Ollama it reports loopback service reachability, the installed-model count, and whether the selected model is installed. Diagnostics never contain credentials or endpoints beyond the literal loopback constant.

## AI data boundary

| Data | OpenAI | Local Ollama |
|---|---:|---:|
| User-entered intent | Sent | Sent to loopback only |
| Static rule schema/instructions | Sent | Sent to loopback only |
| User-selected Policy Disclosure after exact preview/confirmation | Sent | Sent to loopback only |
| Route Simulator hypothetical query or local evaluation trace | Never | Never |
| Proxy username/password | Never | Never |
| Proxy server address | Never | Never |
| Existing rule values, IDs, labels, notes, or complete configuration | Never | Never |
| Process names, domains, IPs, ports, or paths from existing rules | Never | Never |
| Runtime logs | Never | Never |
| Full process list or paths | Never | Never |
| API key | Authorization header only | Not applicable locally |

OpenAI API data handling is governed by the user's OpenAI account and current API policies. `store=false` is an application-level request setting, not a promise that no provider-side security or abuse-monitoring processing exists. Ollama mode keeps the application request on loopback, but the privacy and behavior of the installed model/runtime remain the user's responsibility.

## Current routing capabilities

- Process-aware Proxy / Direct / Block rules.
- Optional exact-domain and `*.suffix` filters.
- IPv4/IPv6 address and CIDR filters.
- Single ports and ascending port ranges.
- TCP, UDP, or Both.
- `Both` is emitted explicitly as TCP + UDP; it does not silently include sing-box v1.13 ICMP matching.
- Explicit priority ordering.
- Canonical runtime ordering shared by the builder, rule view, process-candidate view, and Policy Intelligence.
- Local Policy Intelligence plus request-scoped, user-selected, structurally de-identified AI explanation.
- Conservative static Route Decision Simulator with proven-match, proven-fallback, and indeterminate states bound to a Configuration Snapshot and query.
- IPv4 and IPv6 TUN addresses with strict routing.
- Atomic candidate configuration, `sing-box check`, cancellation-aware startup-settle verification, and rollback.
- Exclusive runtime ownership plus PID/start-time orphan recovery.
- Runtime status whose path, version, and PID describe the same actually managed sing-box process, including after candidate rejection or rollback.
- Passwords protected at rest with Windows DPAPI `CurrentUser`.
- Password-free profile exports and bounded/redacted runtime logs.
- Runtime-log triage: minimum-level filter, debounced case-insensitive search, auto-scroll toggle, and a local export of the exact filtered (re-redacted) log view.
- Process-to-rule workflow: name/PID search, real executable paths, and one-click rule creation from a selected running process through the normal configuration transaction.
- Batch rule management: extended multi-selection with one-transaction batch enable/disable/delete and a count-formatted confirmation.
- Rule-constraints editor: context-menu editing of host / IP-CIDR / port constraints, protocol, and note, live-validated with builder-matching semantics before save.
- Literal-loopback-only upstream proxy endpoints with an optional bounded TCP-listener check.
- A recognized sing-box v1.13+ version gate before configuration check or launch.
- Save-blocked recovery when `config.json` or a DPAPI-protected password cannot be read safely.
- Transactional configuration edits that validate and atomically persist a complete candidate before publishing it to application state or queueing a runtime apply.
- Detached configuration snapshots, so UI or validation code cannot mutate active routing state outside the supported commit path.

IntentRoute AI does **not** provide a proxy node, VPN account, packet driver, bundled AI model, OpenAI API key, or sing-box binary.

## Install a preview build

1. Download `IntentRoute-AI-v0.27.0-win-x64.zip` and its `.sha256` file from [Releases](https://github.com/Vincent-Xi08/IntentRoute-AI/releases).
2. Verify the checksum.
3. Download the official Windows x64 sing-box v1.13+ archive separately.
4. Install `sing-box.exe` separately, then explicitly approve its exact file from **Settings → Browse on every elevated app launch**. The saved path, imported profiles/configurations, `INTENTROUTE_SING_BOX`, the legacy `PROXYMANAGER_SING_BOX`, the application directory, and `PATH` are candidate-discovery hints only: they may be displayed, but neither `version`, `check`, nor `run` executes until that file is reselected in the current session.
5. Ensure an existing proxy service is listening on a literal loopback IP such as `127.0.0.1` or `::1`. The Settings page can save SOCKS5/HTTP/HTTPS username and password values and can check whether the local TCP port accepts a connection.
6. Run `IntentRouteAI.exe` as administrator. TUN creation requires elevation.

The self-contained release targets Windows x64 and does not require a separate .NET runtime.

## Configuration and upgrade migration

Current data is stored under `%APPDATA%\IntentRouteAI`. On first launch, if the new directory has no current configuration, the application copies only `config.json` and `*.profile.json` from `%APPDATA%\ProxyManager`. Copying holds a per-directory exclusive migration lock and uses an in-progress marker plus atomic per-file moves, so an interrupted migration retries only missing known files on the next launch and never overwrites a completed copy. It deliberately does not copy generated sing-box configs, runtime leases, locks, or candidates, and it never deletes the legacy directory automatically.

Proxy passwords are protected at rest with DPAPI `CurrentUser`. Passwords entered in the UI are always treated as plaintext before storage, including legitimate values that begin with the reserved on-disk `dpapi:` marker. The generated `%APPDATA%\IntentRouteAI\sing-box.generated.json` necessarily contains any configured credential in plaintext while sing-box is running. The application removes it on stop, clean exit, and unexpected child exit; the next launch performs bounded orphan recovery and stale-artifact cleanup. Cleanup remains best effort under disk, ACL, administrator, or abrupt-crash interference.

Since v0.3.0, malformed JSON, invalid UTF-8, a null document or collection entry, a rule without a non-empty process name, duplicate rule/server IDs, an explicitly null/empty ID **or an omitted `Id` JSON property**, any non-empty proxy-chain definition, or a `dpapi:` password that cannot be decrypted for the current Windows user makes the configuration **unusable**, not empty. Persisted object IDs are required JSON members; model initializers may create IDs for new in-memory objects but cannot repair imported data silently. Proxy chains remain parseable only so legacy or imported data can be rejected explicitly; IntentRoute AI does not persist or silently ignore them until an actual sing-box runtime mapping exists. A global rule must use the explicit `*` process name; a missing name is never interpreted as global routing. IntentRoute AI leaves the original file untouched, attempts to create a timestamped `config.json.corrupt-*.bak` copy, blocks all save and runtime-apply paths, and shows explicit import/reset recovery controls. Import validates the supported endpoint and routing semantics before replacement. Both import replacement and reset are disabled unless the recovery copy still exists. If the copy could not be created, the user must first make a manual copy and restart the application so preservation can be verified before replacement.

Normal edits, rule imports, AI-draft acceptance, Profile replacement, recovery, and reset use one Configuration Workspace transaction. The application clones the active configuration, applies and validates the complete candidate, atomically saves it, and only then publishes a new detached snapshot. A validation or save failure leaves memory and disk unchanged. Local edits preserve a current-session sing-box approval only while the executable path is unchanged; Profile replacement, recovery, and reset always clear it. Clearing approval cancels any queued replacement apply. If cancellation arrives after a candidate process has started, IntentRoute AI restores the prior generated configuration and restarts its prior process before publishing `RunningStale`; cancellation and green-state publication are serialized so a late Apply cannot overwrite the warning. The executable must be explicitly approved again before the current configuration can be applied.

Candidate path/version probe results remain local to the attempted Apply. Runtime status changes its executable path and version only when that executable is actually started, and clears them when no managed process remains. A rejected candidate therefore leaves the old PID/path/version aligned; failed-start and cancellation rollback restart the prior configuration with the prior executable rather than the rejected candidate.

Cancellation before candidate promotion (including version probe, candidate write, and external `check`) also converges to a terminal runtime status: `RunningStale` when the prior process remains active, or `Failed` when no managed process exists. Direct runtime callers therefore cannot leave status indefinitely in `Starting`, `Probing`, or `Checking` after cancellation.

The local proxy **Test port** action only performs a TCP connection to the entered literal loopback IP with a five-second bound. It sends no username or password and does not prove SOCKS/HTTP negotiation, authentication, upstream reachability, DNS behavior, or routed application traffic.

## Build and test

Requirements for source builds:

- Windows 10/11 x64
- .NET 8.0.424 SDK (pinned by `global.json`)
- PowerShell 7 recommended

```powershell
./scripts/test.ps1
./scripts/check-vulnerabilities.ps1
./scripts/build.ps1
./scripts/smoke-test-wpf.ps1 -OutputDirectory ./artifacts/win-x64
./scripts/test-pinned-sing-box.ps1
```

Provider tests use mocked HTTP handlers. They do not require an OpenAI key, a paid API call, a running Ollama service, or a downloaded local model. The Windows CI also launches the published single-file executable, verifies that the expected WPF main window is created, requests a normal close, and requires a clean zero exit.

The explicit `test-pinned-sing-box.ps1` developer/CI gate temporarily downloads the official sing-box v1.13.19 Windows archive, verifies its pinned SHA-256, passes representative `SingBoxConfigBuilder` output through the real `sing-box check`, and removes the temporary executable and generated configurations. This test-only dependency is never copied into application artifacts; IntentRoute AI itself still never downloads or bundles sing-box. Pass `-SingBoxPath C:\path\to\sing-box.exe` to test an already-installed exact v1.13.19 executable without downloading it.

## Architecture and security

- [Architecture](docs/ARCHITECTURE.md)
- [Threat model](docs/THREAT_MODEL.md)
- [Security policy](SECURITY.md)
- [AI v0.2.0 approved design](docs/plans/2026-08-25-intentroute-ai-design.md)
- [Route Decision Simulator design](docs/plans/2026-08-27-route-decision-simulator-design.md)
- [Codex for Open Source readiness](docs/CODEX_FOR_OSS_READINESS.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)

Please report vulnerabilities privately through [GitHub Security Advisories](https://github.com/Lucas-Xi/IntentRoute-AI/security/advisories/new). Do not include real API keys, proxy credentials, generated configurations, or unredacted logs in an issue.

## Known limitations

- Preview quality; compatibility varies by Windows, firewall, endpoint-security, and sing-box versions.
- Version readiness recognizes the standard `sing-box version X.Y.Z` output and fails closed on unrecognized vendor output; it does not verify a third-party binary signature or checksum.
- AI suggestions are not authoritative and may omit service domains or misunderstand intent.
- Policy Intelligence proves only supported static containment/equality relations; provable partial overlaps are reported as explicitly non-proven hints, unprovable overlap claims are omitted, and live traffic is never observed.
- Route simulation accepts only one exact process, one concrete domain or literal IP, one port, and TCP/UDP. It deliberately returns Indeterminate instead of resolving DNS, inferring a domain from an IP, or claiming a later rule wins when an earlier mixed destination rule cannot be excluded.
- The UI is Chinese-first: every user-visible string follows the language preference with English (533 resource keys), except Policy Intelligence finding titles and the persisted default proxy-server name, which stay Chinese as stable deterministic-analysis identifiers and configuration data.
- Keyboard navigation ships with visible focus states, assistive-technology names on navigation and primary actions, and smoke-level Tab/arrow-key coverage plus Per-Monitor V2 DPI-awareness assertions; mixed-DPI visual layout validation was executed and recorded 2026-09-07 (100% + 125% dual-display, see docs/mixed-dpi-verification.md). Comprehensive screen-reader behavior is not yet claimed.
- Very large policies are bounded to keep the WPF UI responsive; a visible incomplete-analysis finding is emitted instead of silently presenting a partial report as complete.
- No autonomous activation, traffic self-healing, live connection attribution, arbitrary executable wildcards, or remote Ollama endpoints.
- No proxy node distribution or connectivity guarantee.
- `sing-box check` validates configuration syntax/schema, not adapter creation or upstream reachability.

## 中文文档

完整的中文说明（项目介绍、AI 工作流、策略体检、路由推演、提供方配置、数据边界表、路由能力、安装、配置迁移、构建与已知限制）见 **[README.zh-CN.md](README.zh-CN.md)**。

要点：用中文描述”哪个程序的哪些域名应该代理、直连或阻止”，由 OpenAI 或本机 Ollama 生成结构化草案——每个字段可编辑、每次编辑重新本地校验、接纳后默认禁用需手动启用。OpenAI 模式只从 `OPENAI_API_KEY` 环境变量读取密钥；Ollama 模式只允许字面量 `127.0.0.1` 或 `::1`。代理凭据、现有规则值、运行日志与完整进程列表永远不会发送给任何提供方；路由推演的输入与轨迹完全留在本地。没有配置 AI 时，手工分流、本地策略体检和本地路由推演仍可正常使用。

## License

IntentRoute AI is licensed under the [MIT License](LICENSE). sing-box is a separate GPL-licensed program and is not included in this repository or its release archives; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
