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

### Verifying a screenshot shows what you think

`import -window root` always succeeds, even when the click that was meant to
change the page never landed. Never trust a screenshot by looking at it: read
the page title back with OCR and assert on it.

```sh
convert shot.png -crop 1300x200+180+50 +repage \
  -colorspace Gray -resize 200% -normalize /tmp/h.png
tesseract /tmp/h.png - --psm 6
```

Two further traps, both seen here:

- ImageMagick writes a `tIME` chunk, so two captures of an *identical* screen
  differ in bytes while decoding to identical pixels. `md5sum` on the file is
  therefore useless for telling pages apart — hash the decoded pixels
  (`convert f.png -depth 8 ppm:- | md5sum`) or, better, compare OCR text.
- A single `xdotool click` after `mousemove` is often dropped before the app
  has settled. Click twice with a short pause, then move the pointer off the
  control so hover styling does not confuse OCR.

Set the starting view deterministically through `"view": "Grid"` or `"List"`
in the config instead of clicking the toolbar chips, which sit too close
together to hit reliably.

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
- When the user is wrong, say so directly and show why. Never echo a wrong fact,
  inference or calculation just to seem agreeable.
- If an approach is blocked, do not brute-force it. Try an alternative, or say
  what you need. Do not retry the same failing action in a loop.
- Own a mistake briefly: acknowledge it, correct it, move on. No long apology.
- Say no more than you did. Saved is not deployed, tested is not shipped.
- Trust a live source over your memory for anything volatile: API endpoints,
  release versions, prices. Query with the current year, not the one you recall.
- On a long task, say what you are doing as you go instead of going silent.
- Do not offer work you have no way to do.

### Voice

- Match the user's language, depth and formality.
- Answer a clear request directly. Do not open with "if you mean" or a
  clarifying question the request does not need.
- No fake "real-talk" openers: "Honestly", "To be blunt", "My honest take".
- No flattery. State what you found, then stop.
- No exclamation points and no emojis unless the user asks for them.

### Tracking a multi-step task

Use the task list for anything with several steps. Skip it for a single action
or plain conversation.

- Write tasks that are specific and actionable, and break large items down.
- Mark one task `in_progress` when you start it and `done` the moment it
  finishes. Do not batch the completions at the end.
- Mark a task done only when it is fully done: no failing test, no partial
  implementation, no unresolved dependency. Otherwise leave it in progress and
  add a task for the blocker.
- Remove tasks that no longer matter, and revise the list when the requirements
  change.

### Verifying before you present

- Inspect what you produced, not just that the command exited zero. For a UI
  change, look at the rendered result and check for clipped text, overflow, and
  text too close in colour to its background.
- Reproduce a bug before fixing it, and show the same case passing after.
- Before running a command that changes system state, check that the evidence
  supports that specific action. A signal that pattern-matches to a known
  failure may have a different cause.

### Using tools

- Prefer the dedicated file, search and edit tools over shell equivalents.
- Read the relevant file or skill before acting in its domain.
- Independent calls can run in parallel in one response.
- Do not make test calls or calls that do not move the task forward.
- Do not use OCR on an image you can read directly.

### Research and uncertainty

- Before answering, judge whether the fact is time-stable. If it could have
  changed, search first, and search the assumption rather than the answer you
  already have in mind.
- Do not search when you are working over text the user already gave you.
  Not searching is not licence to guess: state your basis, or ask.
- Break a multi-entity question into one query per entity and run them in
  parallel. A vague combined query returns nothing useful.
- Use what the sources say. Do not add inferred or invented detail to fill a
  gap.
- When information cannot be obtained, say so plainly rather than presenting a
  guess as a result.

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

## Windows packaging

- `build.rs` embeds the icon and version metadata. It runs `windres` for the
  target architecture and passes the compiled `.res` to the linker. A missing
  or failing `windres` only warns; the binary still builds, just without an
  icon. Nothing runs on non-Windows targets.
- `installer/ionlauncher.nsi` is a MUI2 wizard. Verify it under Wine with a
  throwaway prefix, then drive the pages with `xdotool` and read the result
  from the filesystem; do not trust a screenshot alone.
- Keep nsDialogs control handles in `Var`s, never in `$0`-`$9`. MUI reuses
  those registers between pages, so a handle kept there is clobbered before the
  leave function reads it and the user's shortcut choice is silently lost.
- The installer runs elevated, so set `SetShellVarContext all` before creating
  or deleting shortcuts. Without it `$DESKTOP` and `$SMPROGRAMS` point at the
  administrator's profile and the shortcuts never appear for the user.
- Build a 64-bit stub (`Target amd64-unicode`). The app is x86_64, so a 32-bit
  stub has nothing to install and mishandles the Program Files redirects.
- An already-running copy of the app locks `ionlauncher.exe`, so the file copy
  fails and the shortcut section is skipped. Close it before reinstalling.
- The CI workflow builds the installer on every push and attaches it to tagged
  releases. `dist/` is ignored; the installer is a build product.

## Linux and macOS packaging

- `assets/ionlauncher.svg` is the icon master. The `.ico` and `.icns` and the
  `assets/linux/*.png` set all come from it, so change the master and regenerate
  rather than editing a raster. The SVG was fitted to the original icon: a
  rounded rect with radius 56 and an eight-point bolt, verified to within 46
  pixels of the original silhouette.
- `installer/linux/build-deb.sh` and `build-appimage.sh` take an already-built
  binary; neither compiles anything. Both are run by
  `.github/workflows/linux-packages.yml`.
- `mktemp -d` returns a 0700 directory. The `.deb` staging root must be 0755 or
  the package installs with a root directory the user cannot read.
- The `.deb` control file needs its `DEBIAN/` directory to exist before the
  heredoc runs. An `install -D` of a file elsewhere does not create it.
- `dpkg-deb --build --root-owner-group` is what makes the archive root-owned
  without needing root to build.
- `appimagetool` refuses to run without `file` installed, and needs
  `APPIMAGE_EXTRACT_AND_RUN=1` on a machine with no FUSE (which includes most
  containers and CI images).
- The `.deb` `Depends` list must cover the libraries the binary `dlopen`s, not
  just the ones in `ldd`. `strings` on the binary is how you find them; on Linux
  that means the GL, X11 and Wayland libraries.
- macOS packaging needs macOS: `hdiutil`, `codesign` and `lipo` have no Linux
  equivalent. On Linux you can still test the script's layout logic by stubbing
  those three commands on `PATH`.
- A universal macOS build is two `cargo build`s plus `lipo`; pass the second
  slice with `UNIVERSAL_SECOND_BINARY`.
- The macOS app is ad-hoc signed, not notarised. That is enough to run, but the
  first launch after a download needs a right-click → Open.
- `cargo check --target x86_64-apple-darwin` fails on Linux inside `ring`'s C
  code, which needs an Apple SDK. The Rust code itself type-checks; shim `cc`
  and `ar` on `PATH` to prove it.

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
