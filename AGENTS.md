# AGENTS.md

Guidance for agents working on IonLauncher.

## Project

IonLauncher is a unified game launcher for Steam, Epic Games and GOG. Rust,
native OpenGL UI via `eframe`/`egui` 0.37. The whole point is that it is light
and responsive on old hardware while still looking good, so performance and
visual quality are both requirements, not nice-to-haves.

## Commands

```sh
cargo build                    # debug
cargo build --release          # ~5 MB stripped binary
cargo test                     # unit tests
cargo run -- --demo            # GUI with the built-in sample library
cargo run -- --print-library   # headless: print the detected library and exit
```

`--print-library` is the fastest way to check provider changes: it needs no
display and no real store installation.

## Testing on a headless machine

The GUI can be verified without a physical display:

```sh
Xvfb :99 -screen 0 1400x900x24 &
DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 ./target/debug/ionlauncher --demo &
sleep 20
DISPLAY=:99 import -window root /tmp/shot.png
```

Set `LIBGL_ALWAYS_SOFTWARE=1` so the software rasteriser is used; there is no
real GPU in CI.

To test provider code against realistic data without installing a store, build
a fake tree under `/tmp` and point `steam_root` / `epic_root` / `gog_root` at it
in `~/.config/IonLauncher/config.json`. Always delete that config afterwards:
if it still points at the fake tree, later runs report nonsense.

## Architecture

Data flows one way: `providers::scan_all` produces a `ScanReport`, which the
app stores as `Vec<Game>`. The UI never touches the filesystem directly.

- `providers/` — one module per store. Each returns `Vec<Game>` and must never
  panic on missing, truncated or malformed files; a broken store file means
  "no games from that store", not a crash.
- `model.rs` — `Game` is the single currency of the app. `LaunchTarget` decides
  whether a title starts via a store URL or a direct executable.
- `app/mod.rs` — `IonApp` holds state and owns the view cache. Filtering and
  sorting are cached and only recomputed when `ViewKey` changes.
- `app/library.rs`, `details.rs`, `search.rs`, `settings.rs` — pages, plus
  `sidebar.rs` for the left menu and `toolbar.rs` for the display controls.
  These are plain functions taking `&mut IonApp` and `&mut Ui`; they are not
  widgets. `library::grid` and `library::list` are shared with the search page
  so both render results identically.
- `art.rs` — resolves a game's `ArtSource` list in order (disk cache, local
  store files, then network) and caches the decoded, downscaled cover to disk.
  Only the downscaled image reaches the GPU. Steam cover URLs are built from
  the app id; GOG and Epic use their public APIs because their CDN paths are
  content hashes that cannot be guessed. Epic app names are internal codenames,
  so the human title is tried as a second slug.
- `proton.rs` — lists and installs Proton (Windows-on-Linux) builds. Installed
  builds are read from every `compatibilitytools.d` directory; community builds
  come from the GloriousEggroll release listing. A download runs on a worker
  thread and unpacks through a staging directory so a partial install is never
  visible to Steam.

## egui 0.37 specifics

This version differs from most tutorials, so do not copy older examples:

- `eframe::App` has `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame)`,
  not `update(&mut self, ctx, frame)`.
- Panels are unified: `egui::Panel::left/right/top/bottom(id)` and
  `CentralPanel`. `SidePanel` and `TopBottomPanel` no longer exist.
- Panels take a `&mut Ui` and are shown with `.show(ui, ..)`, not
  `.show(ctx, ..)`. The central panel uses `.show(ui, ..)` too.
- `Context::style()`/`set_style()` are gone. Use `all_styles_mut`,
  `style_mut_of` or `set_visuals`.
- `egui::Area::new` needs an `Id`; build it with `Id::unique("name")`.
- egui only repaints on input. Any background work (a scan, an artwork fetch,
  a Proton download) needs `Context::request_repaint_after` from the frame
  loop, or results arrive on their channel but the window never redraws.

## How to work

How to approach the job. The next section is code style.

- Act on the request as stated. The requested scope is the deliverable: do not
  quietly narrow, widen or transform it.
- When you have enough information to act, act. Do not re-derive facts already
  settled, re-open a decision the user has made, or list options you will not
  take. If you are weighing a choice, give a recommendation, not a survey.
- Make routine judgment calls yourself. Ask only when two readings would lead to
  materially different work.
- Finish the whole task, not just the easy parts. Report completion only when it
  is complete.
- If one part is blocked, finish every other part in full, then say plainly what
  you left out and why. Shrinking the scope is the user's call, not yours.
- On an uncertainty mid-task, do everything that does not depend on the answer
  first, then ask at the point where it actually matters.
- Report outcomes faithfully. If tests fail, say so and show the output. If a
  step was skipped, say that. When something is done and verified, state it
  plainly without hedging.
- Before deleting or overwriting anything, look at the target first. For actions
  that are hard to reverse or outward-facing, confirm first unless durably
  authorized. Approval in one context does not extend to the next.
- Reference code as `file_path:line_number` so the reader can click it.
- Never end a turn on a plan, an analysis or a promise. If the last paragraph is
  next steps or "I will", do that work now with tool calls.

### Writing the final message

The user may not see the tool calls, so the last message has to stand alone.

- Lead with the outcome. If something could not be verified, say that first.
- One idea per sentence, about twenty words. A sentence beats a label with a
  colon.
- State facts and conclusions. Do not narrate your own reasoning, and do not
  open by announcing that no tools were needed.
- Do not close by restating what you did or offering a follow-up.
- Use a list for parallel items: findings, steps, files to look at. A single
  point stays in prose.
- Keep code out of prose. Put commands, snippets and error text in a fenced
  block. Name a file or flag only when the reader has to go there.
- Keep numbers in a short table or on their own line, and only when they change
  what the reader does.

## Style

- Comment the why, never the what. Do not narrate the diff or restate the code.
- No `unwrap()` on anything read from disk, the network, or the registry.
- Providers must stay panic-free on malformed input. Prefer `Option`/`Result`
  and skip bad entries rather than propagating an error to the UI.
- Keep the dependency list short. This project's selling point is being small
  and fast; weigh every new crate against that.
- Store-specific knowledge (file formats, registry keys, database schemas)
  belongs in `providers/`, not in the UI or model.

## Git

- Commit or push only when the user asks, unless the session already
  established that as the working agreement.
- Keep each fix minimal: what the failure or the review comment needs, no more.
  Do not widen the change on your own.
- Before pushing, run the repo's own fast checks and prove the change is sound:
  `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Re-read your
  own diff adversarially and fix what you find first. One validated push beats
  three speculative ones.
- Never skip, disable or quarantine a test to get a green build. Never push an
  empty commit to kick CI.
- A failing test is never an infrastructure flake. Root-cause it.
- Write a clear, descriptive commit message that says what changed and why.
