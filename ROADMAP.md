# Roadmap

The roadmap is evidence-driven. Items move forward when implementation, tests, and maintenance capacity agree.

## v0.1 — honest preview

- [x] sing-box TUN configuration generation and validation
- [x] exact per-process Proxy, Direct, and Block actions
- [x] host, IP/CIDR, port, and protocol constraints
- [x] managed sing-box lifecycle and redacted runtime logs
- [x] DPAPI password storage and credential-free exports
- [x] Windows CI, release archives, and checksums
- [x] dual-stack Windows TUN configuration
- [x] failed-start rollback, orphan recovery, and stale generated-config cleanup

## v0.2 — AI-assisted intent authoring

- [x] IntentRoute AI product/repository branding
- [x] OpenAI Responses API strict structured output with `store=false`
- [x] Literal `127.0.0.1`/`::1` local Ollama provider and installed-model discovery
- [x] Shared untrusted-output validator and enabled-clone dry-run
- [x] Human preview plus disabled, atomic rule acceptance
- [x] Non-destructive, interruption-safe migration from the v0.1 data directory
- [x] Mocked provider, validation, persistence, and migration tests

## v0.3 — Policy Intelligence maturity work

- [x] Canonical Runtime Order shared by generated routes and read-only views
- [x] Explicit TCP + UDP compilation for `Both`, excluding implicit ICMP broadening
- [x] Local deterministic duplicate, conflict, shadowing, broad-scope, disabled-invalid, inactive-duplicate, priority-tie, and ProxyAll findings
- [x] Dedicated WPF Policy Intelligence page with local evidence and rule navigation
- [x] User-selected, exact-preview Policy Disclosure with request-level confirmation
- [x] Strict OpenAI and literal-loopback Ollama policy explanation that cannot mutate configuration
- [x] Policy fingerprint invalidation for responses that return after configuration changes
- [x] Privacy canaries, matcher containment, provider payload, and canonical-order tests
- [x] Static Route Decision Simulator with conservative three-state evaluation, local trace, stale-result rejection, and Recovery Protection

## v0.4 — assistance and accessibility maturity (2026-08-27)

- [x] Conservative partial-overlap hints with an explicit non-proven classification
- [x] Provider health diagnostics that remain credential-free
- [x] Editable AI draft fields before acceptance, with revalidation after every edit
- [x] Accessible keyboard navigation with visible focus states, UIA names, and smoke-level Tab/arrow coverage

## v0.5 — DPI and localization maturity (2026-08-27)

- [x] Per-Monitor V2 DPI awareness declared in the manifest and asserted in the smoke gate
- [x] Localization infrastructure with static XAML text fully extracted (Chinese default, explicit English, applied at restart)

## v0.6 — localization and display-stack completion (2026-08-27)

- [x] Localization: every window-layer string (statics, display values, status messages, dialogs) follows a Chinese/English preference; 371 resource keys with parity tests

## v0.7 — domain-layer localization completion (2026-08-28)

- [x] Localized AI-provider errors, draft validation messages, provider diagnostics, and AppService runtime/status text (450 resource keys total)

## v0.8 — localization end to end (2026-08-28)

- [x] Every user-visible string localized (window layer, provider errors, validation, diagnostics, runtime status, configuration errors, startup dialogs); Policy Intelligence finding titles and the persisted default proxy name stay Chinese by design

## v0.9 — build provenance (2026-08-28)

- [x] Unsigned build-provenance inventory embedded in every package (version, commit, builder, SDK, full dependency set with content hashes; enforced by the package gate)

## v0.10 — runtime-log triage maturity (2026-09-01)

- [x] Minimum-level filter, debounced case-insensitive search, and an auto-scroll toggle for the redacted sing-box runtime-log view
- [x] Local export of the exact filtered view as a re-redacted, UTF-8 (no BOM) text file
- [x] Complete Chinese README, curated changelog-based release notes, and a Strings-accessor reflection test

## v0.11 — process-to-rule workflow (2026-09-01)

- [x] Name/PID search filter and real executable paths on the process page (local limited-information query)
- [x] One-click rule creation from a selected running process through the normal Configuration Workspace transaction, with explicit duplicate reporting and rule-list navigation
- [x] Fixed the v0.1 ANSI/Unicode Toolhelp32 mismatch that produced mojibake process names and broken candidate matching

## v0.12 — batch rule management (2026-09-02)

- [x] Extended multi-selection on the rule list with toolbar batch enable/disable/delete, one atomic Configuration Workspace transaction per action and a count-formatted delete confirmation
- [x] Batch operations provably leave rule priority and persisted order untouched (only Move Rule reorders)
- [x] Release-notes extraction consolidated into a shared script the CI gate executes (republished v0.11.0)

## v0.13 — rule-constraints editor (2026-09-02)

- [x] Context-menu **Edit constraints** dialog for host / IP-CIDR / port / protocol / note with live shared validation (Save gated until every field parses)
- [x] `UpdateRuleConstraints` Configuration Workspace transaction that leaves identity, mode, and priority untouched; host validation deduplicated into the shared validator
- [x] IP/CIDR and port format validation now exist at edit time (previously length-only before build-time rejection)

## v0.14 — stability and smoke-coverage hardening (2026-09-02)

- [x] Migration-lock acquisition bound raised 10s → 60s after contention-jitter flakiness; both known timing-sensitive tests hardened (three consecutive full-suite runs green)
- [x] Packaged-WPF smoke gate now covers the v0.10–v0.13 UI surfaces (batch buttons, log toolbar, process toolbar) via verified spacebar page navigation

## v0.15 — import preview and identity-consistent dedupe (2026-09-03)

- [x] Import preview dialog classifying every incoming rule (add / already present / in-file duplicate) with confirm gated on having additions and an added+skipped completion summary
- [x] Import dedupe switched from process-name-only to the shared full rule identity, so same-process rules with different constraints import instead of silently disappearing
- [x] Shared `RuleIdentity` key reused by AI draft validation and rule import; a file without a `Rules` array now fails explicitly instead of doing nothing

## v0.16 — unified dark design system and shell redesign (2026-09-03)

- [x] Single design system in `App.xaml` (main window's drifted private palette removed; legacy keys aliased), with real dark ComboBox and CheckBox templates replacing six hard-coded light-dropdown patches
- [x] Shell redesign: grouped sidebar with active-item indicator and brand tile, stacked title/subtitle header, segmented global-mode control, standard window controls
- [x] Rules-page redesign: two-row toolbar with count pills and a dedicated batch row, dot-style mode column, drawn empty-state and drag-over icons; emoji removed from nav/toolbar strings
- [x] All automation ids, UIA names, keyboard navigation, and the smoke gate's contracts preserved (smoke passes unchanged; visually verified on rules/settings/AI pages)

## v0.17 — component-library polish pass (2026-09-03)

- [x] Containerized table treatment shared by all GridView lists (SurfaceAlt header, full-row hover, row separators)
- [x] Three-tier KPI stat cards on Policy Intelligence (dot+label / 24px value / localized footnote)
- [x] Empty-state action row on the rules page (Add rule + Import)
- [x] 200ms scale+fade dialog entrance for the constraints editor and import preview

## v0.18 — theme-colored page identity (2026-09-03)

- [x] Theme-colored header banner on all eight pages (icon chip + title/subtitle in the page's accent: blue for rules/AI/settings, green for policy, amber for simulator/process, neutral for log/about)
- [x] Runtime-log and process pages restructured to the standard header + toolbar card + content card grammar
- [x] Runtime log rendering in Consolas with muted timestamps (terminal-style reading)
- [x] Palette lightened one step for stronger layer separation; simulator banner restyled neutral

## v0.19 — batch mode and log severity coloring (2026-09-03)

- [x] Batch proxy/direct/block buttons on the rules toolbar, driving the existing tested `SetRulesMode` transaction that had no UI entry point; smoke gate extended to all six batch buttons
- [x] Runtime-log severity coloring (warn amber / error red / fatal red bold) parsed on arrival via the shared level parser

## v0.20 — operation feedback and page transitions (2026-09-03)

- [x] Localized batch-operation success feedback with affected-rule counts, emitted only after the existing atomic transaction returns successfully
- [x] 160ms ease-out page fade-in, preserving Visibility routing, UIA, and keyboard navigation contracts

## v0.21 — batch feedback boundary fix (2026-09-07)

- [x] Dedicated batch-result label on the rules toolbar with 4s hold + fade-out; no longer shares the runtime-status footer that async apply messages overwrite
- [x] Feedback counts use service-returned matched counts instead of selection counts

## Candidate next work

- [x] Guided sing-box discovery with version reporting
- [x] Authenticated local proxy editing in the UI
- [x] Automated CI integration tests against a pinned real sing-box release
- [ ] Visual layout validation across mixed-DPI displays (manual procedure documented in docs/mixed-dpi-verification.md; requires multi-monitor hardware — not claimed until a maintainer records a run)
- [x] Localize AppService runtime-status and readiness text (the persisted default proxy name stays Chinese as configuration data)
- [x] Decision: Policy Intelligence finding titles stay Chinese as stable deterministic-analysis identifiers (referenced by privacy-canary tests and cross-language user reports); they are analysis output, not UI chrome- [x] Corrupt-configuration recovery that preserves the source file, blocks accidental overwrite, and guides the user through restore
- [ ] Signed release artifacts when sustainable signing infrastructure exists (the unsigned provenance inventory shipped in v0.9.0)

## Explicitly not promised

Autonomous AI rule activation, proxy chains, provider subscriptions, remote node management, remote Ollama endpoints, per-connection traffic attribution, and cross-platform support are not scheduled. They require separate design and security review before implementation. Until then, non-empty proxy-chain definitions are rejected rather than persisted or silently ignored.
