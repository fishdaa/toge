# Agent workflow

## UI testing

When testing a UI change, run the ordinary automated tests first, then perform a visual unit test of the affected flow.

The visual test must:

1. Start the smallest suitable local app or fixture.
2. Exercise the changed interaction with a real browser/window (including scrolling, focus, keyboard input, and loading states when relevant).
3. Capture a video covering the test from before the interaction through the final assertion.
4. Inspect the recording or captured frames for layout, clipping, focus, scroll position, and visible errors.
5. Save the recording outside version control, under `visual-test-artifacts/` or a temporary directory.
6. Share the recording as a tailnet-only URL when Tailscale is available. Use a localhost HTTP server and `tailscale serve --bg`; do not use Funnel or expose the artifact publicly.
7. Report the visual flow, the recording URL, and any limitation of the visual environment in the final handoff.

For Slint changes, the target is `toge-slint` and the shared `toge-slint/ui/main.slint` UI. Prefer a native-window recording; if desktop input or capture is unavailable, use Slint's headless software-renderer backend against that same UI and label the recording as offscreen Slint verification. A Tauri/Vite browser fixture is not equivalent and must not be reported as Slint verification.

If the native desktop app cannot be automated in the current environment, use the closest supported browser/fixture for the affected UI and state the limitation. Do not claim visual verification from unit tests alone.

For non-UI changes, ordinary automated tests are sufficient unless the change affects a user-visible rendering or interaction.
