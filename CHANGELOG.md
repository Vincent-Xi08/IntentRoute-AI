# Changelog

All notable changes are documented here. The project follows semantic versioning while the public API and behavior are still in preview.

## [Unreleased]

## [0.30.0] - 2026-09-13

### Added

- The process list is now sortable: clicking a column header (PID, name, path, rule status) sorts by that column, clicking again toggles ascending/descending, and the active column is marked with a ▲/▼ suffix. The sort state survives search filtering and list refreshes — because the filter rebuilds the `ItemsSource`, sorting is re-applied on rebuild from a stored field rather than a collection-view description. Name, path, and status compare case-insensitively. The rules list intentionally stays in canonical runtime order and is not sortable.

## [0.29.0] - 2026-09-13

### Added

- Hovering a rule row now shows the full information its truncated cells hide: process name, the complete condition summary, and the executable path, replacing the path-only tooltip on the name.
- The runtime-log page gained an empty state: when no log line survives the current filter (fresh start before sing-box is approved, everything cleared, or a filter matching nothing), a centered hint explains where logs will come from — matching the rules-page empty-state pattern instead of a blank card.

## [0.28.0] - 2026-09-13

### Added

- Keyboard parity for the process and log pages, extending the v0.22 rules-page treatment: pressing Enter on a selected process creates the rule through the exact same duplicate-guarded flow as the toolbar button (including navigation and selection), Ctrl+F now focuses whichever search box belongs to the visible page (rules, process, or log), and Escape in the process/log search boxes clears them first and returns focus to the list on a second press — all three search boxes share one handler. No service-layer changes.

## [0.27.0] - 2026-09-13

### Changed

- The rules-page and process-page search boxes now debounce filtering by 300ms, the same treatment the runtime-log search already had. The process search previously LINQ-filtered the ~900-row snapshot and reassigned the whole `ItemsSource` (resetting virtualization, selection, and scroll) on every keystroke, which made typing visibly stutter; the rules search rebuilt its view per keystroke the same way. Both now apply the filter once, 300ms after the last keystroke, and the debounce timers are stopped on shutdown alongside the log-search timer.

## [0.26.0] - 2026-09-13

### Changed

- The three bordered dialogs (rule-constraints editor, import preview, themed message box) now request a dark system title bar through the DWM immersive-dark-mode attribute, removing the last white chrome around dark dialog content. Purely visual; falls back silently to the light title bar on Windows versions without the attribute.
- The rule-constraints editor was exercised end-to-end for the first time since its v0.13 introduction: opened by double-clicking a rule row, dark title bar and full form verified (process header, host/IP/port fields with hints, protocol combo, save/cancel), closed cleanly with no crash — a live confirmation of the v0.25 entrance-animation fix on the exact path that used to be untested. The import preview shares the same loaded path and remains covered by that verification plus its unit tests; driving the system file-open dialog was skipped during inspection to avoid disrupting active use of the machine.

## [0.25.0] - 2026-09-08

### Added

- All twenty in-app message boxes (confirmations, completion notices, error and warning prompts, the policy-disclosure confirmation) now render as a themed dark dialog instead of the white system `MessageBox` that clashed with the dark UI. The new `DarkDialogWindow` reuses the app's design tokens (soft icon chips in accent/warning/error colors, Primary/Secondary buttons, 200ms entrance animation) and keeps `MessageBoxResult` semantics, so every call site is a drop-in replacement. The two startup-failure prompts in `App.xaml.cs` intentionally stay on the system message box: they fire before any window exists.

### Fixed

- Fixed a latent crash from v0.17: the shared dialog entrance animation set `Window.RenderTransform` directly, which WPF forbids (`CoerceRenderTransform` throws "Transform is not valid for Window"), so opening the rule-constraints editor or the import preview would have terminated the process. The animation now targets the window's content root instead. The defect had never been exercised end-to-end until the new dark dialog was first opened during this release's inspection, where it crashed the app and was caught from the event log.

## [0.24.0] - 2026-09-07

### Changed

- Mixed-DPI visual layout validation is now **claimed**: the full manual procedure was executed from the physical console on a 100% + 125% dual-display setup using the official v0.23.0 release archive. Launch, display-move re-render, maximized layout, and all eight pages verified crisp and unclipped; the complete record (including the root cause of earlier false "both displays at 100%" readings — DPI-virtualized queries from system-aware processes) lives in `docs/mixed-dpi-verification.md`. The README known-limitations line is updated accordingly. No code changes in this release.

## [0.23.0] - 2026-09-07

### Fixed

- Review-hardening pass over the v0.15–v0.22 UI accumulations: the batch-result label's auto-hide timer was re-subscribed on every show, so rapid consecutive batch operations stacked duplicate Tick handlers; the timer now subscribes once at construction, and re-showing the label clears any in-flight fade animation before restoring opacity. A structural audit of all eight page grids confirmed every direct child row index fits its row definitions (the class of v0.18 overlap regression has no remaining instances), and the zh/en resource sets remain in parity at 549 keys.

## [0.22.0] - 2026-09-07

### Added

- Keyboard and pointer polish on the rules page: double-clicking a rule row opens the constraints editor (same entry as the context menu's first item), the Delete key deletes the selected rules through the same count-formatted confirmation flow as batch delete, Ctrl+F focuses the search box while the rules page is visible, and Escape inside the search box clears it — a second Escape returns focus to the rule list. No service-layer changes; every path reuses the existing batch transaction and feedback label.

## [0.21.0] - 2026-09-07

### Fixed

- Batch-operation feedback moved off the shared runtime-status footer: the v0.20 completion message was written to the same text the sing-box runtime asynchronously overwrites, so it usually vanished within a second. Each batch result now appears in a dedicated toolbar label showing the service-returned count of actually updated rules (not the selection count, which can differ if rules changed concurrently), stays for four seconds, and fades out. Runtime status keeps the footer to itself.

## [0.20.0] - 2026-09-03

### Changed

- Batch rule operations now report a localized completion message in the runtime-status footer after the atomic transaction succeeds (enable / disable / delete / proxy / direct / block), including the affected rule count. A failed transaction still follows the existing exception path and never reports success.
- Page switching now fades the selected page in over 160ms with an ease-out curve. The existing Visibility routing, UI Automation tree, and keyboard activation path remain unchanged; the animation is purely presentational.

## [0.19.0] - 2026-09-03

### Added

- Batch mode buttons on the rules page: **Batch proxy / Batch direct / Batch block** next to the existing batch enable/disable/delete buttons. They apply the v0.12 `SetRulesMode` atomic transaction (previously reachable only through the per-rule context menu) to every selected rule in one Configuration Workspace commit, never touching priority or order, and start disabled until a row is selected like the other batch actions. The packaged smoke gate now asserts all six batch buttons exist and start disabled.
- Runtime-log lines are now color-coded by severity: parsed once on arrival through the same `RuntimeLogFilter` level parser the filter uses, warning lines render amber, error lines red, and fatal lines red semi-bold — completing the v0.18 terminal-style log view.

### Fixed

- Fixed a v0.18 layout regression that hid the entire rules-page toolbar: the theme header was added as a new row without extending the page's row definitions, so the toolbar and the rule list shared the same grid row and the list card rendered on top of it. The page now has three rows (header / toolbar / list). The smoke gate could not catch this because overlapped elements remain in the UIA tree with correct enabled states; the toolbar's visibility is now covered by visual inspection.

## [0.18.0] - 2026-09-03

### Changed

- Extended the theme-colored page banners to every page: each of the eight pages now opens with a compact themed header card (icon chip + page title/subtitle) in its own accent — rules and AI assistant in accent blue, policy health in success green, route simulator and process list in warning amber, runtime log and about in neutral surface, settings in accent blue — so the current area is identifiable at a glance.
- Restructured the runtime-log and process pages onto the same visual grammar as the rest of the app: a themed header card, a separate toolbar card (filters and actions), and a dedicated content card for the list, replacing the previous single monolithic card.
- Runtime logs now render in Consolas with muted timestamps, giving the log view a terminal-like reading experience.
- The route-simulator what-if banner was restyled from accent blue to the neutral surface treatment so it no longer clashes with the page's warning theme.
- Lightened the palette one step (background `#0D1117`, surface `#161B22`, card `#1C2128`, border `#30363D`) to increase layer separation on typical monitors, and muted the secondary/muted text tiers for clearer hierarchy.

All changes are visual; automation ids, event handlers, localization keys, and the smoke gate pass unchanged.

## [0.17.0] - 2026-09-03

### Changed

- UI component-library polish pass (borrowed, not bundled, from the Shadcnblocks/ReUI/Motion component archives after a read-only selection review): every GridView-backed list (rules aside, policy findings, policy advice, route trace, runtime log, process list) now shares one containerized table treatment — a darker `SurfaceAlt` header band, full-row hover background instead of the default system highlight, row separator lines, and stretched rows with consistent padding.
- Policy Intelligence KPI cards follow the three-tier stat-card pattern: colored severity dot plus label, a 24px value, and a one-line localized footnote explaining what the count means (canonical-order matching, conflict priority, broad-scope review, disabled drafts excluded from runtime).
- The rules-page empty state gained its action row: **Add rule** and **Import** buttons under the title/description, so a fresh install can start from the empty state itself instead of hunting for the toolbar.
- Rule-constraints and import-preview dialogs enter with a 200ms scale-and-fade transition (0.97→1, cubic ease-out), mapped from the component library's dialog entrance parameters to WPF storyboards.

All changes are visual; automation ids, event handlers, the smoke gate, and the 318-test suite pass unchanged.

## [0.16.0] - 2026-09-03

### Changed

- Unified the UI on a single design system. `App.xaml` is now the only source for colors, typography, and control styles; the main window's private copy of the palette (which had drifted to a different dark gray) was removed, and legacy resource keys (`PrimaryBtn`, `SecondaryBtn`, `BgBrush`, `DangerBrush`, `TextMutedBrush`, `TextSecondaryBrush`) resolve as aliases so existing dialogs and code-behind `FindResource` calls keep working.
- Every ComboBox and CheckBox now renders with a real dark template. Previously the app-level ComboBox style had no template, so provider/model/transport/language dropdowns rendered as light-gray system controls inside the dark UI — six of them were patched with hard-coded light `#F3F4F6` backgrounds to match, which looked wrong against the theme. The overrides are gone and all dropdowns, including their popups and item hover states, follow the palette.
- Redesigned the shell: the sidebar groups navigation under labeled sections (rules & policy / runtime & monitoring / application) with an accent indicator bar on the active item and a brand tile, the title bar stacks the page subtitle under the title and switches the global mode to a segmented control, and window controls use standard 42×32 hover targets with a red close hover.
- Redesigned the rules page: the toolbar is two rows (search + count pills + primary actions; batch enable/disable/delete + clear as a separate selection row with a hint), rule rows show the mode as a colored dot plus text, and the empty state and drag-over overlay use drawn icons instead of emoji.
- Removed emoji prefixes from navigation and toolbar button strings for a consistent professional look, and refreshed the sidebar version and about tagline from the stale "v0.9.0" to the current version. Navigation order is unchanged (the smoke gate's keyboard path still walks `NavRules` → down → `NavPolicy`), and every automation id, control name, and event handler is preserved — the smoke gate passes unchanged.

## [0.15.0] - 2026-09-03

### Added

- Rule import now shows a preview dialog before anything is written. Every incoming rule is classified against the current configuration and displayed as add / already-present skip / in-file duplicate skip; the confirm button stays disabled while nothing would be added, and the completion message reports both added and skipped counts. A file without a `Rules` array now fails with an explicit message instead of silently doing nothing.
- Import duplicate detection moved from process-name-only to the shared full rule identity (process + normalized hosts/IPs/ports/protocol + mode), so rules for the same process with different constraints are imported as new rules instead of silently disappearing — matching the v0.13 constraints editor and the AI acceptance path. The identity key itself was extracted into `RuleIdentity` and is now reused by AI draft validation and rule import, so duplicate semantics cannot drift between the two paths.

## [0.14.0] - 2026-09-02

### Changed

- Raised the configuration-migration lock acquisition bound from 10 to 60 seconds. The lock retry is the only path that can surface a raw `IOException` to a concurrent startup, and 10 seconds proved too tight under antivirus or CI file-contention jitter; real startup migrations never approach even 10 seconds, so only the failure bound moves.

### Fixed

- Hardened the two timing-sensitive tests that produced the only observed flaky failures: the concurrent-migration test now tolerates slow file systems through the 60-second lock bound above, and the orphan-recovery test waits 15 seconds for the recorded orphan to exit, covering the runtime's own 3-second internal kill bound plus slow-CI margin (the previous 5-second window failed once on a GitHub runner).
- Extended the packaged-WPF smoke gate to cover the four UI surfaces added since v0.10: it asserts the rules-page batch buttons exist and start disabled, switches to the monitor and process pages, and asserts their new toolbar controls (log search/level/auto-scroll, process search, and the add-as-rule button with its disabled start state). Page switching is driven by focusing the nav radio and sending space — the empirically verified keyboard-activation path — with each switch verified by polling for a target-page control.

## [0.13.0] - 2026-09-02

### Added

- Added a rule-constraints editor: an **Edit constraints** item at the top of the rule-list context menu opens a dialog for the selected rule's host / IP-CIDR / port constraints, protocol (TCP, UDP, Both, or Any), and note. Editing is live-validated by a shared `RuleConstraintValidator` (same semantics as the sing-box builder: exact or `*.suffix` hosts, IPv4/IPv6 literals or CIDRs, single ports or ascending ranges; empty means unrestricted) and the Save button stays disabled until every field parses. The new `AppService.UpdateRuleConstraints` transaction persists the five editable fields through the Configuration Workspace (protocol allow-listed to the AI-acceptance spelling), leaves ExeName/Mode/Priority untouched, and silently no-ops on unknown rule ids like the other update paths; host/IP/port format remains covered by editor validation plus build-time rejection, matching `ImportRules`. The AI draft validator's private host validation was deduplicated into the shared validator with identical behavior.

## [0.12.0] - 2026-09-02

### Added

- Added multi-select batch operations to the rule list: the list now supports extended selection (Ctrl/Shift), and toolbar buttons enable, disable, or delete every selected rule through one atomic Configuration Workspace transaction per action. Batch delete asks for an explicit count-formatted confirmation. Batch operations never touch rule priority or persisted order — only Move Rule reorders.
- Fixed the Release workflow's curated-notes step: the inline PowerShell extraction had a syntax error (`if [string]::` missing parentheses) that failed the v0.10.0 and v0.11.0 release runs, so no packages were published for those tags. The extraction now lives in `scripts/make-release-notes.ps1`, which the Release workflow runs and `test-release-notes.ps1` executes for every released version, so an extraction-script regression now fails CI instead of a release. v0.11.0 was republished from the fixed workflow; v0.10.0 was superseded within a day and intentionally not re-cut.

## [0.11.0] - 2026-09-01

### Added

- Added a process-to-rule workflow on the process page: a name/PID search filter, real executable paths in the path column (local `QueryFullProcessImageNameW` with limited-information access), and an **Add as rule** button that creates a Proxy-mode enabled rule from the selected running process through the same Configuration Workspace transaction as manual creation, then navigates to the rule list with the new rule selected. Duplicate process names are reported explicitly instead of silently ignored; an empty `ExePath` (path query unavailable) remains legal configuration data.

### Fixed

- Fixed a v0.1-era process-enumeration defect: the Toolhelp32 declarations resolved to the ANSI exports (`Process32First`/`Process32Next`) while the entry structure was marshalled as Unicode, so the process page showed byte-swapped mojibake names and configured-rule candidate matching could never hit. The declarations now bind explicitly to the W variants, and a failed snapshot returns early on `INVALID_HANDLE_VALUE` as well as a zero handle. The defect was exposed by the first test that asserts real snapshot content.

## [0.10.0] - 2026-09-01

### Added

- Added runtime-log triage on the sing-box monitor page: a minimum-level filter (parsed from the redacted sing-box console lines, trace through error), a debounced case-insensitive text filter, an auto-scroll toggle, and an export button that writes exactly the current filtered view to a user-chosen UTF-8 (no BOM) text file. Exported lines pass through the secret redactor a second time, so exports stay credential-free; filtering and export are entirely local and nothing is transmitted anywhere.
- Added a complete Chinese README (`README.zh-CN.md`) mirroring the English one section by section — project rationale, AI/policy/simulator workflows, provider setup, the data-boundary table, routing capabilities, install, configuration migration, build, and known limitations — with language switcher links at the top of both files. Deep technical docs (architecture, threat model) remain English with links from the Chinese README.
- Release notes are now curated: the release workflow extracts the tag's section from this changelog instead of auto-generating a bare commit list, with a CI check validating the extraction for every released version.
- Added a reflection test asserting every hand-written `Strings` accessor property resolves to a non-empty value, catching property/key typos the parity tests cannot see.

## [0.9.0] - 2026-08-28

### Added

- Added an unsigned build-provenance inventory to every package: `provenance.json` records the version, exact commit, builder (GitHub Actions run URL or local build), pinned SDK, target framework, and the complete resolved NuGet dependency set (22 packages with content hashes) from the lock files. `verify-package.ps1` now fails when the inventory is missing or incomplete. This is an inventory manifest, not a signature — code signing still requires a certificate.

## [0.8.0] - 2026-08-28

### Added

- Localized the last user-visible service-layer strings: policy-explanation provider errors, configuration-workspace validation and recovery exceptions, config-store load errors, and the startup dialogs (second-instance lock and safe-start failure). With the documented exceptions of Policy Intelligence finding titles and the persisted default proxy name, every user-visible string in the application now follows the language preference (481 resource keys).

## [0.7.0] - 2026-08-28

### Added

- Localized the AI-provider domain layer: every provider exception message (OpenAI and Ollama request lifecycle errors) and the AI-draft validator messages now follow the language preference, as do the provider-health diagnostic details. Test assertions on these strings were updated to resource-derived or culture-stable forms.
- Localized the AppService runtime layer: readiness results (not-detected / not-approved / invalid saved path), footer status messages (rule stats, runtime states, apply outcomes, config-protection notice), profile and chain summaries, and import/clone errors now follow the language preference. The persisted default proxy-server name remains Chinese configuration data. Test assertions were updated to resource-derived forms.

## [0.6.0] - 2026-08-27

### Added

- Localized dynamic display values: rule-row mode/status/condition text, proxy-server status, count badges, policy severities, the route simulator's verdicts and decision badges, and version strings now follow the language preference.
- Localized AI-page status messages: the rule assistant's generation/validation lifecycle, every policy-check status and empty state, the disclosure confirmation dialog, all simulator decision sources, reasons, statuses, and invalidation notices, and the provider-health states.
- Localized the remaining window-layer dialogs and notices: rules add/clear/delete/import/export, proxy save and port-test feedback, sing-box browse/readiness, the configuration-recovery banner and its reset/import confirmations, shutdown notice, and process-page candidates. `MainWindow` now contains no hard-coded UI strings; AppService and runtime-error text remain Chinese.

## [0.5.0] - 2026-08-27

### Added

- Added localization infrastructure: Chinese-neutral and English resources with key-parity tests, a per-app language preference stored in `ui-preferences.json` outside the routing Configuration Workspace, applied at startup before any window is created, and a Settings-page pilot whose static texts follow the preference. The framework surfaces now follow it too — window title, navigation and status footer, page titles/subtitles, the rule context menu, and the About page. Other pages and dynamic messages remain Chinese; the preference applies after restart and defaults to Chinese.
- Extended localization to every static XAML surface: rule management (toolbar, headers, empty and drop states, recovery banner), the AI assistant page (provider/model, intent, editable draft columns, status), policy intelligence (statistics, findings and advice columns, privacy note), the route simulator (banner, query labels, decision and trace panels), runtime log, and process list columns. Dynamic strings set from code-behind — status messages, rule-row text, count labels, and runtime errors — remain Chinese.
- Declared Per-Monitor V2 DPI awareness in the embedded application manifest and extended the packaged-WPF smoke gate to assert the main window's Per-Monitor V2 context through `AreDpiAwarenessContextsEqual`. Visual layout validation across mixed-DPI displays remains unclaimed.

## [0.4.0] - 2026-08-27

### Added

- Added visible keyboard-focus states to the custom navigation, primary, and secondary button styles, UI Automation ids and assistive-technology names for navigation, window controls, and primary AI actions, and extended the packaged-WPF smoke gate to assert an assistive-technology window name, a minimum count of keyboard-focusable controls, Tab traversal, and arrow-key movement within the navigation group.
- Made every AI draft field editable in the preview (process, action, domains, IP/CIDR, ports, protocol, rationale). Each edit schedules an automatic revalidation through the same deterministic validator used for generated drafts — including the enabled-clone sing-box dry-run — and acceptance stays blocked until the edited draft passes.
- Added a credential-free AI provider health diagnostics panel in Settings. OpenAI checks report only whether `OPENAI_API_KEY` is present — the key is never displayed and no network request is sent; Ollama checks reuse the literal-loopback model listing to report service reachability, installed-model count, and whether the selected model is installed.
- Added a conservative `PartialOverlap` Policy Finding: two enabled rules whose scopes provably intersect without either containing the other are reported as an explicitly non-proven hint (Warning when outcomes differ, Info when they match). Domain-versus-IP constraints, disjoint ports, and different processes are never claimed as overlapping.

## [0.3.0] - 2026-08-27

### Fixed

- Include the WPF native runtime libraries in the self-contained single-file publish so the packaged executable can create its main window instead of terminating during native window subclass initialization.
- Render rule rows with their application, condition, routing mode, enabled state, and creation time instead of the model type name.
- Reject malformed UTF-8 configuration bytes and profile imports instead of accepting replacement characters that could later overwrite the original file.
- Show a yellow stale-runtime state when a replacement fails but the previously applied sing-box process remains healthy.
- Clear Policy Intelligence counts and show an explicit not-analyzed state when configuration recovery protection is active.

### Added

- Added a local-first AI Policy Intelligence page with deterministic duplicate, conflict, proven shadowing, broad-scope, disabled-invalid, inactive-duplicate, same-priority-overlap, and ProxyAll-posture findings.
- Added local finding navigation to affected rules plus optional OpenAI/Ollama plain-language explanation for 1–20 user-selected findings.
- Added a per-request confirmation dialog that displays the exact closed Policy Disclosure JSON before any policy-explanation request is sent.
- Added cancellation-aware background policy scans so large bounded analyses do not block the WPF dispatcher or delay safe shutdown.
- Added a Settings readiness panel that reports the resolved sing-box path, probes `sing-box version`, and requires a recognized v1.13+ release before configuration checking or process launch.
- Added an explicit file picker for a separately installed sing-box executable without bundling, downloading, or installing it.
- Added authenticated local proxy editing for SOCKS5, HTTP, and HTTPS listeners with DPAPI-protected passwords.
- Added a bounded local TCP port check that sends no proxy credentials and does not claim protocol, authentication, or internet reachability success.
- Added guided recovery actions for an unreadable configuration: open the data directory, import a valid configuration, or explicitly reset.
- Added a shared CI/Release compatibility gate that verifies the official sing-box v1.13.19 Windows archive SHA-256 and runs representative production-builder output through the real `sing-box check` without shipping the test dependency.
- Added a dedicated AI Route Decision Simulator page for one exact process/domain-or-IP/port/TCP-or-UDP what-if query, with a local evaluation trace and navigation to a proven matched rule.
- Added conservative matched-rule, global-fallback, indeterminate, invalid-query, and invalid-policy results, plus a policy-and-query fingerprint that hides stale decisions.

### Changed

- Use one Canonical Runtime Order across sing-box generation, the rules page, process-candidate display, and Policy Intelligence: priority ascending, creation timestamp ascending, then persisted source order.
- Make rule up/down actions move within that same canonical order before priorities are normalized.
- Canonicalize equivalent domain-suffix, adjacent/overlapping port-range, and mergeable CIDR unions before duplicate and containment analysis, reducing conservative false negatives without DNS or heuristic overlap claims.
- Emit empty legacy protocol, `Both`, and `TCP/UDP` explicitly as sing-box TCP plus UDP; reject `Any`, `ALL`, and `ICMP` instead of silently including v1.13 ICMP traffic.
- Label the process page as a process-name configuration candidate rather than a live routing status because destination, port, and protocol conditions are not evaluated there.
- Restrict supported proxy endpoints to literal loopback IP addresses; hostnames, LAN addresses, public addresses, and IPv4-mapped IPv6 forms are rejected.
- Drive the runtime status indicator from real probing, checking, starting, running, failed, and stopped states instead of a fixed green indicator.
- Show a clear dialog when another IntentRoute AI instance already owns the sing-box runtime lock.
- Route manual edits, imports, Profiles, recovery, and AI-draft acceptance through one Configuration Workspace candidate transaction; callers now receive detached snapshots instead of mutable active-state references.
- Evaluate static route queries in Canonical Runtime Order with the same destination-OR and cross-group-AND semantics as the production builder; stop at the first earlier rule whose domain/IP context cannot be disproved.
- Propagate cancellation through production configuration construction so large background simulations and runtime apply can stop cleanly during supersession or window shutdown.

### Security

- Keep local Policy Finding labels/evidence separate from the closed Policy Disclosure type; providers never receive existing process names, domains, IPs, ports, IDs, notes, paths, proxy data, credentials, logs, generated JSON, runtime identity, or process inventory.
- Require request-scoped selected-finding confirmation for policy explanation, recheck the local policy fingerprint before preview, after confirmation, and after the response, use strict code-referenced output with no tools or mutation interface, and never send or display a stale explanation.
- Preserve an unreadable `config.json` byte-for-byte, create a timestamped recovery copy when possible, block every configuration save path, and skip sing-box application until the user explicitly recovers or resets.
- Treat malformed or undecryptable `dpapi:` proxy passwords as an unusable configuration instead of silently replacing them with empty credentials.
- Fail closed when sing-box version output is missing, unreadable, timed out, or older than v1.13; no candidate configuration is written and no child process is started.
- Never execute a sing-box candidate found through environment variables, the application directory, or `PATH` until the user explicitly approves the exact file with the Settings file picker.
- Treat saved, migrated, and imported sing-box paths as unapproved on every elevated launch; only a file reselected in the current session may run `version`, `check`, or `run`.
- Validate rule imports against the complete candidate configuration before atomically replacing the current config; unsupported semantics no longer persist before runtime rejection.
- Treat null entries inside rules, proxy servers, or proxy chains as an unusable configuration instead of allowing a startup `NullReferenceException` outside the recovery path.
- Cancel and drain first-load/model-provider work before disposing providers, then queue the final WPF `Close` after the current closing frame, preventing managed crashes when the published app is closed immediately after launch.
- Validate and atomically persist complete configuration candidates before publishing in-memory state or queueing runtime replacement; validation, DPAPI, and filesystem failures now leave both memory and disk unchanged.
- Preserve current-session sing-box approval only for local transactions whose committed executable path is unchanged; Profile replacement, recovery import, and reset always clear approval.
- When approval is cleared, cancel any queued replacement apply and mark a preserved running process as `RunningStale` instead of leaving a green status for an older configuration.
- Make the startup-settle window cancellation-aware; cancellation after candidate promotion restores and restarts the previous generated configuration, and green-state publication is atomic with stale marking so a late Apply cannot overwrite revoked approval.
- Keep candidate probe identity separate from the managed process identity; failed checks leave the old PID/path/version aligned, while startup failure and cancellation rollback use the previous executable and version instead of the rejected candidate.
- Honor `INTENTROUTE_SMOKE_DIAGNOSTIC_PATH` only when it names an explicit absolute path without relative-traversal segments; relative or traversal-containing values are ignored.
- Converge cancellation during version probe, candidate write, or external check to `RunningStale` when the prior process remains active and `Failed` otherwise, instead of leaving a transient runtime state behind.
- Treat every in-memory proxy password as plaintext at the persistence and builder boundaries, so legitimate values beginning with `dpapi:` are encrypted and round-trip instead of being misread as stored ciphertext.
- Reject rules with null, empty, or whitespace-only process names at both the workspace and builder boundaries; only an explicit `*` represents a global rule. Persisted semantic failures now enter the same preservation-first recovery state as malformed JSON.
- Normalize optional imported strings defensively and reject missing or duplicate rule and proxy-server IDs before publication; UI matching remains null-safe as a second line of defense.
- Reject every non-empty proxy-chain collection at both workspace and direct-builder boundaries until a real sing-box runtime mapping exists; removed the unused service methods that implied chain support.
- Require `Id` to be present in serialized rule, proxy-server, and proxy-chain objects, preventing Json.NET property initializers from silently repairing omitted IDs with random GUIDs.
- Keep Route Decision Queries, local trace labels/IDs, resolved proxy identity, and simulated results entirely local; never invoke an AI provider, DNS, a proxy probe, process inspection, runtime logs, sing-box, persistence, or configuration apply from simulation.
- Disable route simulation during Recovery Protection and return no action for invalid policy, invalid query, missing cross-kind destination context, or the 500-rule evaluation bound.

### Tests

- Added pinned real sing-box coverage for DirectAll/ProxyAll, canonical rules, exact/suffix destinations, IPv4/IPv6 CIDRs, port/range, TCP/UDP/Both, Proxy/Direct/Block, authenticated loopback SOCKS5/HTTP/HTTPS, and explicit/default proxy selection.
- Added Route Decision Simulator coverage for canonical order, global and disabled rules, domain suffixes, IPv4/IPv6 CIDRs, port/protocol constraints, Direct/Proxy/Block/default outcomes, destination OR semantics, conservative cross-kind uncertainty, invalid inputs/policies, cancellation, stale fingerprints, read-only behavior, and evaluation bounds.
- Added direct production-builder cancellation coverage.
- Added canonical-order, explicit TCP/UDP, unsupported-protocol, domain-suffix, destination-union, CIDR/port/protocol containment, disabled-rule, selected-disclosure, privacy-canary, strict-output, and OpenAI/Ollama policy-payload coverage.
- Added a regression test proving that rule up/down actions follow the order shown by the UI and used by sing-box.
- Added equivalent-union and analyzer-cancellation coverage for domain, port, and CIDR policy shapes.
- Added coverage for corrupt JSON preservation, DPAPI failure, save blocking, explicit reset, loopback-only endpoint validation, local TCP checks, sing-box version compatibility, and prevention of check/start on unsupported versions.
- Added invalid UTF-8 preservation and unapproved discovery tests, plus a build-time package-content gate that rejects bundled sing-box and generated runtime files.
- Added null-collection-entry recovery coverage for rules, proxy servers, and proxy chains.
- Added a Windows CI and release smoke gate that starts the published single-file WPF executable, verifies its main window title, and requires a clean normal close.
- Added opt-in redacted managed-exception diagnostics for the packaged-WPF smoke gate so shutdown regressions fail with an actionable stack without exposing configuration or credentials.
- Surface the same redacted diagnostic when startup is caught by the WPF safety dialog or the smoke gate observes an unexpected main-window title.
- Added Configuration Workspace coverage for detached snapshots, filesystem-failure rollback, unsupported-mutation rollback, AI disabled-rule commits without runtime apply, and approval preservation/clearing semantics.
- Added regression coverage for approval-clearing while an older runtime remains active, plus DPAPI-marker-prefixed password round trips.
- Added builder, import-rollback, and startup-recovery coverage for null, empty, and whitespace-only executable names.
- Added deterministic cancellation-during-startup coverage at both runtime and AppService boundaries, plus direct-builder and persisted-recovery coverage for standalone proxy-chain definitions.
- Added two-executable runtime tests for candidate-check identity and rollback identity, plus deserialization and preservation-first startup tests for omitted `Id` properties.
- Added direct-runtime cancellation coverage for the pre-promotion probe window, including terminal state, unchanged PID/path/version, and unchanged generated configuration.

## [0.2.0] - 2026-08-26

### Added

- Rebranded the product and release artifacts as IntentRoute AI.
- Added a provider-neutral AI rule assistant with OpenAI Responses API and literal `127.0.0.1`/`::1` local Ollama support.
- Added strict structured-output parsing, bounded responses, provider error categories, and mocked HTTP test seams.
- Added deterministic AI draft validation for executable names, domains, CIDRs, ports, protocols, actions, duplicates, limits, and enabled proxy availability.
- Added an all-or-nothing preview and explicit acceptance flow that persists generated rules disabled without restarting sing-box.
- Added non-destructive migration of known configuration/profile files from `%APPDATA%\ProxyManager` to `%APPDATA%\IntentRouteAI`.
- Added an exclusive migration lock and interrupted-migration marker so concurrent or partial legacy copies safely retry without overwriting files already copied.

### Security

- OpenAI requests use `store=false`, no tools, a strict schema, and an API key read only from `OPENAI_API_KEY` at request time.
- Ollama requests accept only literal `127.0.0.1` or `::1`, reject credentialed endpoints, and disable system proxy use and redirects.
- Neither provider receives proxy credentials, proxy endpoints, existing rules, logs, filesystem paths, or the full process inventory.
- AI rules are temporarily enabled only in a cloned dry-run so the existing builder validates their actual semantics before disabled persistence.
- Unsupported protocol/action values are rejected at the strict parser boundary, and all projects compile with warnings treated as errors under the pinned .NET 8.0.424 SDK.

### Tests

- Expanded the suite beyond its original 15 cases with provider contracts, secret redaction, malformed/unexpected output, exact loopback enforcement, parser enum rejection, network-filter validation, interrupted/concurrent migration recovery, and disabled-rule persistence.

## [0.1.1] - 2026-08-25

### Security

- Added dual-stack IPv4/IPv6 TUN addresses so Windows strict routing covers both address families.
- Added a per-config-directory runtime lock and a PID/start-time lease for best-effort orphan recovery.
- Remove generated configs on stop and unexpected child exit, and clean stale candidates on the next launch.

### Fixed

- Detect a sing-box process that exits during its startup-settle window instead of reporting a false successful apply.
- Restore and restart the previous checked configuration when a checked replacement fails during startup.

### Tests and release

- Added lifecycle coverage for stale cleanup, concurrent ownership, orphan recovery, stop cleanup, dual-stack config, and failed-replacement rollback.
- Require release tags to match the project version and publish preview tags as GitHub prereleases.

## [0.1.0] - 2026-08-25

### Added

- WPF rule editor for exact process routing on Windows.
- sing-box v1.13+ TUN configuration builder and managed process runtime.
- Proxy, Direct, and Block rule actions with destination constraints.
- Pre-start `sing-box check` validation and redacted runtime logs.
- DPAPI-protected local password storage and redacted profile export.
- Unit tests for routing configuration, invalid inputs, and secret handling.
- CI, release automation, checksums, security policy, threat model, and contributor documentation.

### Changed

- Replaced the earlier system-proxy-only behavior with an explicit sing-box TUN data plane.
- Removed synthetic connection logs and unsupported UI controls.
