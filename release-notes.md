### Added

- Slint search controls for case, whole-word, path, and regex matching, plus
  file-type presets that edit the raw query and stay in sync with typed modifiers.
- Immediate-parent and depth query filters, quoted filter values, and hidden-name
  attribute matching.
- CLI JSON Lines output (`--json`/`-jsonl`), nonblocking readiness checks
  (`--no-wait`), and `--` to separate query text from options.
- Noctalia launcher plugin with file search, daemon progress, index rebuilding,
  and an optional action to open the Slint client.
- Slint Options window for recording, clearing, and saving application shortcuts,
  plus systemwide window shortcuts through the desktop Global Shortcuts portal.
- Selected-file previews for images, SVG, PDFs, text/code, office documents,
  spreadsheets, audio metadata/artwork, and sampled video stills using optional
  installed tools; reuse of valid freedesktop thumbnails.

### Changed

- PDF previews render at the pane's physical pixel width, load the visible page
  before neighboring pages, and support scrolling through the document.
- Text/code previews show line numbers, wrapping controls, horizontal scrolling,
  keyboard navigation, copying, and optional syntax colors from `bat`/`batcat`.

### Fixed

- Recognized unsupported search filters and attributes report errors instead of
  silently accepting them; regex matching respects the case setting.
- A leading `!` negates search terms instead of matching it literally.
- Extension filters and file-type presets match uppercase extensions, including
  restored indexes; narrow Slint windows retain horizontal result scrolling.
- Video previews display the first frame while samples load and keep it visible
  if duration inspection or sampling fails.
- Slint keeps selected-file details and a compact result summary on one footer
  row, eliding long details to keep status visible.

### Removed

- The unused `exclude_fstypes` (`[roots]`) and `interval_secs` (`[polling]`) config options. Existing config files that set them still load; the values are ignored.
- The CLI flags `-pause`/`-more` and `-config`, which were parsed but had no effect. Passing them is now an unknown-flag error.
- The preview Support button, its popup, and unused diagnostic summary.
