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
- `app/library.rs`, `details.rs`, `settings.rs`, `sidebar.rs` — screens. These
  are plain functions taking `&mut IonApp` and `&mut Ui`; they are not widgets.
- `art.rs` — downloads, decodes and downscales covers on a worker thread, then
  caches them to disk. Only the downscaled image reaches the GPU.

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

## Style

- Comment the why, never the what. Do not narrate the diff or restate the code.
- No `unwrap()` on anything read from disk, the network, or the registry.
- Providers must stay panic-free on malformed input. Prefer `Option`/`Result`
  and skip bad entries rather than propagating an error to the UI.
- Keep the dependency list short. This project's selling point is being small
  and fast; weigh every new crate against that.
- Store-specific knowledge (file formats, registry keys, database schemas)
  belongs in `providers/`, not in the UI or model.
