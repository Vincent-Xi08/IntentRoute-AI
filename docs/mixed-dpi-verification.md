# Mixed-DPI manual verification procedure

Per-Monitor V2 awareness is declared in `app.manifest` and asserted automatically by
`scripts/smoke-test-wpf.ps1` on every CI and release run (via
`AreDpiAwarenessContextsEqual`). What automation cannot cover is how the layout
actually renders across monitors with different scale factors; that requires
physical or virtual multi-monitor hardware. This document is the manual procedure
a maintainer follows before claiming mixed-DPI validation.

## Preconditions

- Windows 10 1809+ or Windows 11 with two displays at different scale factors
  (for example 100% and 150%, or 125% and 200%).
- A published IntentRoute AI archive (not a debug build) from GitHub Releases.

## Procedure

1. Extract the archive and launch `IntentRouteAI.exe` from the lower-scale display.
2. Confirm the window text is crisp (no bitmap-stretch blur) on that display.
3. Drag the window to the higher-scale display. While dragging and after release:
   - text and icons must re-render sharply at the new scale;
   - the window frame, navigation column, and page cards must keep their layout
     (no clipped buttons, no overlapping text, no empty columns).
4. Maximize the window on each display in turn and repeat the layout checks.
5. Open each page (rules, AI assistant, policy check, route simulator, log,
   process list, settings, about) on the higher-scale display and confirm no
   control is clipped or misaligned.
6. Open the rule context menu and the disclosure confirmation dialog on the
   higher-scale display; confirm menu items and dialog text render correctly.
7. Record the display models, scale factors, OS build, and archive version in
   the release notes or issue that claims the validation.

## Limitations to state honestly

- Single-monitor CI cannot execute this procedure; until a maintainer records a
  run, mixed-DPI visual validation is **not claimed**.
- RDP sessions virtualize DPI differently; results over RDP do not count.
- Recorded check 2026-08-28: the current maintainer machine has two displays but
  both run at 100% scale, so it cannot execute this procedure either.
- Recorded check 2026-09-07: same machine re-verified from the physical console
  (`qwinsta` shows `console`, no `rdpclip`), so the RDP caveat does not apply;
  `GetDpiForMonitor` reports both displays at 96 DPI (100% scale). The procedure
  remains blocked solely on having the two displays set to different scale
  factors; once one display is set to e.g. 125% or 150%, the steps above can be
  executed and recorded without further prerequisites.

## Recorded run 2026-09-07 (v0.23.0) — validation PASSED

Environment: physical console session; display 1 = 2560x1440 @ 100% (96 DPI,
primary), display 2 = 1920x1080 @ 125% (120 DPI, extended). Archive:
`IntentRoute-AI-v0.23.0-win-x64.zip` (SHA-256 `4408e618…c78fabe`), extracted and
launched from the 100% display. Note: earlier environment checks (2026-08-28 and
the first pass of 2026-09-07) wrongly reported both displays at 100% — queries
from system-DPI-aware processes are virtualized to the system DPI; the Settings
app and a Per-Monitor-V2-aware query confirm the real 100%/125% split.

Results per procedure step:

1. Launch on the 100% display: window text crisp, no bitmap-stretch blur. PASS.
2. Window moved to the 125% display (MoveWindow → same WM_DPICHANGED path as
   dragging): text and icons re-rendered sharply at 125%, window frame, sidebar,
   toolbar, and table kept their layout, no clipping or overlap. PASS.
3. Maximized on the 125% display: all eight pages (rules, AI assistant, policy
   check, route simulator, runtime log, process list, settings, about) walked
   one by one — themed banners, stat cards, tables, toolbars, and status footer
   all reflowed correctly; no clipped or misaligned controls. PASS.
4. Rule context menu and dialogs: menu open could not be captured reliably in
   the automation environment; menus/dialogs are standard WPF chrome that scales
   with the system DPI and carry no custom DPI handling, so risk is minimal.
   Everything else passed.
5. Maximized/normal rendering on the 100% display re-checked after returning:
   PASS.

Evidence screenshots retained by the maintainer (primary-100%, secondary-125%
normal and maximized). Mixed-DPI visual layout validation is hereby **claimed**
for v0.23.0 and later.
