# Architecture

## Supported path

`ProxyManager.Standalone` is the retained .NET 8 WPF project/namespace for the IntentRoute AI control plane. It does not capture packets itself.

1. The UI reads detached configuration snapshots and sends edit intents to `AppService`; it never receives a mutable reference to the active configuration.
2. `ConfigurationWorkspace` clones the active configuration into a candidate, applies the requested mutation, normalizes and validates the complete candidate, and asks `AppConfigStore` to persist it atomically with DPAPI-protected passwords.
3. Only after persistence succeeds does `ConfigurationWorkspace` publish the candidate as the new active configuration. Validation or filesystem failure leaves both active memory state and persisted bytes unchanged.
4. `PolicyRuntimeOrder` supplies one Canonical Runtime Order to the builder, Route Decision Simulator, and read-only views: priority ascending, creation timestamp ascending, then persisted source order.
5. `SingBoxConfigBuilder` validates supported semantics and produces full and redacted JSON representations. Empty legacy protocol, `Both`, and `TCP/UDP` are emitted explicitly as `network: ["tcp", "udp"]`; unsupported `Any`, `ALL`, or `ICMP` inputs are rejected rather than broadened silently.
6. `SingBoxRuntime` discovers a separately installed sing-box candidate, but only an exact path explicitly re-approved in Settings for the current elevated session may run the bounded, no-shell `sing-box version` probe.
7. Only a recognized v1.13+ result crosses the readiness gate. Missing, old, timed-out, or unrecognized binaries do not receive configuration data and are not started.
8. A candidate runtime configuration is written atomically and checked with `sing-box check -c`. This validates schema and configuration, not live network reachability.
9. A checked candidate is promoted and the managed process is replaced. If the replacement exits during its startup-settle window, IntentRoute AI atomically restores and restarts the previous checked configuration when one exists.
10. sing-box creates dual-stack IPv4/IPv6 Windows TUN routes and applies process/destination route rules.

CI and Release use a shared, explicit test-only adapter to download the official pinned sing-box v1.13.19 Windows archive into runner temp, verify its SHA-256, and run representative production `SingBoxConfigBuilder` output through the real `sing-box check`. The adapter removes the executable and generated test configurations afterward, and package verification independently rejects bundled sing-box files. This is compatibility evidence for the builder schema, not a product download path or a network-reachability test.

## AI authoring path

1. The AI page creates an `AiRuleRequest` containing only the user-entered intent and selected model.
2. `OpenAiRuleProvider` uses the Responses API with strict JSON Schema and `store=false`, or `OllamaRuleProvider` uses a non-streaming literal-`127.0.0.1`/`::1` `/api/chat` request with the same schema.
3. Provider-specific JSON is converted to the shared `AiRuleSuggestion` model with unknown properties rejected.
4. `AiRuleDraftValidator` enforces local size, process, host, IP/CIDR, port, protocol, action, duplicate, and proxy-availability rules.
5. New rules are temporarily enabled only in a cloned candidate and passed through `SingBoxConfigBuilder`; this prevents disabled-rule filtering from turning validation into a no-op. This AI-preview dry-run is in-process construction, not execution of the external `sing-box` binary.
6. After explicit user confirmation, `ConfigurationWorkspace` commits the whole draft through the same candidate-validation and atomic-persistence path as manual edits, while keeping every accepted rule disabled and not replacing the running sing-box process.
7. Enabling remains a separate manual action through the supported path above, where a candidate file is checked by `sing-box check -c` before process replacement.

## Policy Intelligence path

1. The WPF page requests a detached Configuration Snapshot and passes it to `PolicyIntelligence.Analyze` on a cancellation-aware background task; newer snapshots and shutdown supersede the old scan without blocking the dispatcher.
2. The deep local module canonicalizes redundant domain suffixes, adjacent/overlapping integer port ranges, and mergeable sibling CIDRs, then uses Canonical Runtime Order plus the supported sing-box default-rule algebra: destination matcher types form one OR group, port and port-range matchers form another OR group, and process/network/other groups combine with AND. It reports containment only when every required relation is proven.
3. Enabled rules produce runtime-order findings. Disabled rules are never described as active; each is temporarily enabled only in a cloned in-memory candidate and passed through `SingBoxConfigBuilder` to identify prospective enable failures.
4. The local `PolicyAnalysisReport` contains display labels and rule IDs for navigation. It never crosses a provider seam.
5. The user selects 1–20 findings. `PolicyIntelligence.ToDisclosure` creates a closed allowlist object containing aggregate counts and structural finding fields only.
6. A confirmation dialog displays the exact logical JSON that will be sent and the provider-specific data notice. Cancel produces no request and no persistent authorization exists.
7. `OpenAiRuleProvider` or `OllamaRuleProvider` implements the separate `IAiPolicyExplainer` seam. OpenAI uses strict Responses API output with `store=false` and no tools; Ollama uses the existing literal-loopback-only, non-streaming transport.
8. `AiPolicyContract` rejects unknown output properties, unknown/duplicate finding codes, null/oversize fields, and out-of-range confidence. Model text is rendered as plain text and has no Configuration Workspace interface.
9. A local fingerprint binds explanation to the analyzed snapshot. It is checked before preview, after confirmation but before sending, and after the response; a stale summary is never sent and a stale result is discarded while keeping the newest local report.

## Route Decision Simulation path

1. The WPF page creates a `RouteDecisionQuery` from one exact process name, one concrete domain or literal IPv4/IPv6 address, one port, and TCP or UDP. Wildcards, executable paths, CIDR query values, scoped IPv6 addresses, ambiguous short IPv4 forms, and unsupported enum values are rejected.
2. The page takes a detached Configuration Snapshot and invokes `PolicyIntelligence.SimulateRoute` on a cancellation-aware background task. Recovery Protection blocks the page; the unreadable configuration placeholder is never evaluated as if it were active state.
3. The simulator first calls `SingBoxConfigBuilder.Build` against that snapshot. A builder failure becomes `InvalidPolicy`, returns no action, and does not execute sing-box or write the generated JSON.
4. Enabled rules are evaluated in Canonical Runtime Order. Process/network/port groups combine with the destination group using AND; domains and CIDRs inside the destination group use the same OR semantics as the builder. A known matching member proves that group, but a domain miss cannot exclude a rule that also has CIDRs without a resolved IP, and an IP miss cannot exclude a rule that also has domains without domain context.
5. Evaluation stops at the first proven match or the first earlier rule that cannot be excluded. If all rules are proven misses, the configured global fallback is proven. A 500-rule budget, invalid query, invalid policy, or missing cross-kind context never produces a guessed action.
6. `RouteDecisionReport` includes a local-only rule trace for UI explanation and a SHA-256 fingerprint that binds the policy fingerprint plus normalized query. Query or configuration changes hide the old decision, and completion rechecks the current snapshot before display.
7. The simulator has no provider interface and no runtime interface. It performs no provider request, DNS resolution/reverse lookup, proxy connection, process enumeration, runtime-log read, packet observation, filesystem write, sing-box probe, or configuration mutation/apply.

## Trust boundaries

| Boundary | Responsibility |
| --- | --- |
| WPF input to config builder | Validate process patterns, hosts, IP/CIDR, ports, protocols, proxy references, and server endpoints |
| Config builder to file system | Never log full JSON; write atomically to the per-user application directory |
| File system to sing-box | Display saved/imported/discovered paths only as unapproved hints; execute only a path reselected in the current elevated session, without a command shell, then validate before run |
| sing-box output to UI | Redact common password, secret, token, and credential patterns |
| Profile export | Remove passwords rather than exporting DPAPI ciphertext |
| Runtime ownership | Hold an exclusive per-config-directory lock and record the child PID, start time, and executable path without credentials |
| Legacy data to current directory | Hold an exclusive migration lock, copy only missing known files atomically, retain the source, and resume only when an in-progress marker proves ownership |
| Persisted configuration to application state | Preserve unreadable input, create a recovery copy when possible, block all writes and runtime apply, and require explicit import/reset recovery |
| Configuration snapshot to mutation | Expose detached snapshots only; clone, validate, persist, and publish complete candidates through the Configuration Workspace |
| Proxy settings to outbound | Accept only literal loopback IP addresses and valid ports; protect stored passwords with DPAPI; never send credentials during the TCP-listener check |
| User intent to AI provider | Send only intent plus static schema/instructions; never include proxy settings, existing rules, logs, paths, or process inventory |
| Local Policy Finding to AI provider | Require per-request selected-finding confirmation; send only the exact closed Policy Disclosure, never local finding text, rule values/IDs, builder JSON, proxy data, paths, logs, runtime state, or process inventory |
| AI policy explanation to UI | Strictly parse references to disclosed finding codes, display plain text only, discard stale results, and expose no mutation or runtime interface |
| Route Decision Query to local simulator | Evaluate only a detached snapshot with bounded conservative semantics; keep query values, trace, rule/proxy identifiers, and results local, and never describe them as observed traffic |
| AI output to domain model | Reject oversized, malformed, missing, or additional fields; treat all model text as untrusted data |
| Ollama client to local service | Permit only literal HTTP `127.0.0.1` or `::1`; disable proxy use and redirects; never pull models or launch a process |
| Accepted AI draft to config | Validate all-or-nothing, dry-run enabled clones, persist disabled rules once, and require a second action to enable |

## Failure behavior

Invalid replacement configuration does not intentionally terminate a healthy managed process. Candidate version/path results stay local during build and check, so a rejected candidate cannot overwrite the identity shown for the process that remains active. Once a candidate process starts, runtime path/version/PID move together to that process. A checked replacement that immediately fails to run triggers a best-effort rollback with both the prior generated configuration and prior executable identity. The startup-settle delay observes caller cancellation; if cancellation arrives after candidate promotion or launch, the same rollback path restores and restarts the prior configuration before the runtime remains `RunningStale`. Process output is bounded in memory.

Cancellation or shutdown kills any in-flight `sing-box version` or `sing-box check` process tree before propagating cancellation. Apply cancellation before candidate promotion converges to `RunningStale` when the prior process survives or `Failed` when none exists; a direct runtime caller cannot leave `Starting`, `Probing`, or `Checking` as the final state. Window shutdown cancels and drains local policy-analysis and route-simulation work, cancels outstanding readiness/apply and AI-provider operations before disposing their clients, and asynchronously waits for runtime ownership and secret-bearing generated files to be released instead of synchronously blocking the WPF dispatcher.

An unreadable or semantically unsafe persisted configuration does not fall back to an active empty/default configuration. Malformed or invalid-UTF-8 input, null documents or collection entries, missing required rule identities/process names, duplicate rule/server IDs, an omitted `Id` JSON member, non-empty proxy-chain collections, and unavailable DPAPI credentials all enter the same fail-closed recovery state. `Id` is marked required at the JSON boundary so object initializers can create identities for new domain objects without silently repairing imported objects. Proxy-chain data remains in the schema only to parse and explicitly reject legacy/imported definitions; no runtime mapping exists. A missing process name is rejected rather than interpreted as a global route; global intent requires the explicit `*` literal. The UI remains available while configuration mutations and sing-box application are blocked. The original file is not rewritten until the user explicitly imports a valid replacement or confirms reset.

Every supported mutation is transactional at the application-state level. A candidate that fails normalization, supported-semantics validation, DPAPI serialization, or atomic filesystem replacement is never published as the active configuration and never queues a runtime apply. Rule and proxy edits preserve a current-session sing-box approval only when the committed executable path is unchanged. Profile replacement, recovery import, and reset clear that approval even when they contain the same path. Clearing approval also cancels any queued apply. If cancellation precedes candidate launch, the previous process is untouched; if it lands during startup settling, the candidate is terminated and the previous generated configuration/process is restored. Cancellation checking and green `Running` publication share the runtime lock with stale marking, preventing an older Apply from overwriting `RunningStale` after approval is revoked.

Passwords are plaintext only inside the in-memory domain model. `AppConfigStore.Deserialize` removes the DPAPI envelope at the file boundary, `SingBoxConfigBuilder` consumes the resulting plaintext directly, and every non-redacted serialization creates a fresh DPAPI envelope. This keeps a legitimate plaintext value beginning with `dpapi:` distinct from the reserved marker used in persisted JSON. The Rust core (migration phase 3c) reproduces this boundary independently: `intentroute_core::dpapi` produces and consumes the identical `"dpapi:"` envelope via CryptProtectData/CurrentUser with no entropy, and `intentroute_core::workspace` implements the same load (recovery copies, fail-closed on undecryptable values) and transactional commit (normalize → invariants → builder dry-run → DPAPI serialize → temp + atomic `ReplaceFileW`) for future Rust tooling. The Rust console's bounded edits (rule enable/mode/constraints, add, delete, proxy-server fields) run one such commit while holding the `sing-box.runtime.lock` exclusive handle ported by `intentroute_core::runtime_lock`, so a running WPF instance blocks the edit (and vice versa) rather than racing on `config.json`; the constraints editor gates its Save on the shared live validator exactly like the WPF RuleEditWindow, rule adds are refused on full-identity duplicates checked against the freshly loaded on-disk state inside the transaction, deletes require an explicit irreversible-action confirmation, and the server editor gates on the core loopback/port validation with passwords re-encrypted into a fresh DPAPI envelope on every save. Rule moves (up/down) replicate the WPF `MoveRule` canonical-order swap with full priority renumbering. Remaining page parity (monitor, policy, simulator) is not yet in the Rust shell.

On stop, normal shutdown, or an unexpected managed-child exit, IntentRoute AI removes the generated configuration. On the next launch after an application or OS crash, it verifies a recorded orphan by PID and start time, checks the executable path when Windows permits it, terminates that process tree, and removes stale configs/candidates. Recovery and deletion remain best effort under administrator interference, filesystem failure, or corrupted state.

Provider failure, cancellation, rate limiting, missing local models, malformed output, or local validation failure leaves configuration unchanged. Raw provider error bodies are not surfaced. Provider/model controls remain locked while a generation or policy-explanation request is in flight so a response cannot be attributed to a newly selected provider. Policy-explanation failure never clears the local deterministic report; a changed policy invalidates and discards only the old model explanation.

## Dependency boundary

sing-box, Ollama, local models, and OpenAI service access are separate dependencies selected/configured by the user. None are linked, vendored, downloaded, or redistributed by IntentRoute AI. Upstream behavior, availability, pricing, data handling, and licensing remain the responsibility of their respective projects/providers.

The repository's pinned sing-box compatibility adapter is invoked only by an explicit developer/CI test command. It does not sit behind an application interface and its temporary dependency is excluded from every package.
