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
