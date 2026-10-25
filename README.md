# IonLauncher

A single, fast launcher for your Steam, Epic Games and GOG libraries.

IonLauncher reads the stores' own files directly and shows everything in one
place. It is written in Rust and draws its interface with OpenGL, so it starts
in well under a second, uses a few tens of megabytes of memory, and stays
smooth on older hardware.

![IonLauncher library view](docs/screenshot-library.png)

## Screenshots

| | |
|---|---|
| ![Library grid](docs/screenshot-library.png) | ![Library list](docs/screenshot-list.png) |
| Cover grid with the whole library mixed across stores | Compact list with playtime, size and last played |
| ![Search](docs/screenshot-search.png) | ![Favourites](docs/screenshot-favourites.png) |
| Search across all three stores at once | Only the games you starred |
| ![Game details](docs/screenshot-details.png) | ![Settings](docs/screenshot-settings.png) |
| Details, statistics and launch options | Store detection and Proton settings |

## What it does

- **One library, three stores.** Steam, Epic Games and GOG games appear in a
  single mixed list, each tagged with its store's colour — no separate
  per-store tabs to click through.
- **A left menu of pages.** Installed games, Search, Favourites and Settings,
  with Ctrl+F jumping straight to search.
- **Launch anything.** Games start through their store client by default, so
  cloud saves, achievements and overlays keep working.
- **Search and sort.** A dedicated search page covering all three stores at
  once, and sorting by name, playtime, size or how recently you played.
- **Grid and list views.** A cover grid for browsing, a compact list when you
  want to see a lot at once.
- **Favourites.** Star the games you actually play and find them in one click.
- **Real playtime and size.** Read from each store's metadata, not guessed.

## Supported platforms

| Platform | Steam | Epic Games | GOG |
|---|---|---|---|
| Linux | yes | yes (via Heroic) | yes (via GOG Galaxy or Heroic) |
| Windows | yes | yes | yes (via registry or GOG Galaxy) |
| macOS | yes | yes (via Heroic) | yes (via Heroic) |

## Install

You need a Rust toolchain (1.82 or newer).

```sh
git clone https://github.com/ciao22king/IonLauncher
cd IonLauncher
cargo build --release
./target/release/ionlauncher
```

The release binary is around 5 MB and has no runtime dependencies beyond your
graphics driver and the store clients you already have.

### Linux packages

`eframe` needs the usual desktop development headers to build:

```sh
# Debian / Ubuntu
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxrandr-dev \
                 libxi-dev libxkbcommon-dev libwayland-dev

# Fedora
sudo dnf install mesa-libGL-devel libX11-devel libXcursor-devel libXrandr-devel \
                 libXi-devel libxkbcommon-devel wayland-devel
```

## Try it without installing any games

```sh
ionlauncher --demo
```

This loads a built-in sample library so you can see the interface. Demo mode
lasts for that session only; it is never written to your settings.

### Command line

```
ionlauncher [OPTIONS]

  --demo            Show the built-in sample library
  --print-library   Print the detected library and exit (no window)
  --version         Print the version
  --help            Print this help
```

`--print-library` is handy for checking detection without a display:

```
$ ionlauncher --print-library
IonLauncher library (3 games)
  Steam        3 games

  Team Fortress 2        Steam      1560 h    24.0 GB  steam://runsappid/440
  Portal 2               Steam        60 h    12.0 GB  steam://runsappid/620
```

## How detection works

Nothing is uploaded anywhere. IonLauncher only reads files that are already on
your disk.

- **Steam** — `steamapps/libraryfolders.vdf` for every library folder,
  `appmanifest_*.acf` for the installed games, and
  `userdata/<id>/config/localconfig.vdf` for playtime and last-played times.
- **Epic Games** — the launcher's `Manifests/*.item` files, plus Heroic's
  `legendary` config and installed-game records.
- **GOG** — the Windows registry keys Galaxy writes, the Galaxy SQLite database
  (`galaxy-2.0.db`), and Heroic's `gog_store` records.

Store paths are found automatically. If yours live somewhere unusual, set them
in **Settings → Library paths**.

## Configuration

Settings are saved to `config.json` in your platform's config directory:

- Linux: `~/.config/IonLauncher/config.json`
- Windows: `%APPDATA%\IonLauncher\config.json`
- macOS: `~/Library/Application Support/IonLauncher/config.json`

Artwork is cached separately (under `~/.cache/IonLauncher/art` on Linux) and
trimmed at startup so it cannot grow without bound.

## Performance notes

The interface is built for machines that are not fast:

- Covers are downloaded once, resized on a worker thread, and cached to disk.
  Only the downscaled copy is uploaded to the GPU.
- The filtered, sorted game list is cached and only recomputed when a filter,
  the sort order or the library actually changes, so scrolling does not re-sort
  every frame.
- Scanning runs on a background thread; the window opens immediately and fills
  in as results arrive.
- Shadows, multisampling, depth and stencil buffers are disabled, and the app
  uses a plain OpenGL context rather than a heavier rendering backend.

## Development

```sh
cargo test      # 81 unit tests
cargo build     # debug build
cargo run -- --demo
```

The code is split by concern:

```
src/
  main.rs          entry point and command-line handling
  model.rs         Game, Store, LaunchTarget and the scan report
  vdf.rs           Valve VDF parser (Steam's config format)
  sqlite.rs        read-only SQLite reader (GOG Galaxy's database)
  json_util.rs     case- and separator-tolerant JSON lookups
  util.rs          filesystem, time and process helpers
  config.rs        settings and persistence
  art.rs           artwork download, decode, downscale and disk cache
  theme.rs         colours and egui style
  widgets.rs       reusable widgets (game card, buttons, chips)
  ui.rs            panel frames
  providers/       one module per store, plus the Windows registry reader
  app/             application state and the UI screens
```

## Licence

GPL-2.0-or-later. See [LICENSE](LICENSE).
