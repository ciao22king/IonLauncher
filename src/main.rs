//! IonLauncher — one launcher for Steam, Epic Games and GOG.

// On Windows, suppress the console window that would otherwise appear
// alongside the GUI. Release builds only, so debug output stays visible.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod art;
mod config;
mod demo;
mod json_util;
mod model;
mod providers;
mod sqlite;
mod theme;
mod ui;
mod util;
mod vdf;
mod widgets;

use config::Config;

/// Parsed command-line options.
#[derive(Default)]
struct Args {
    demo: bool,
    print_library: bool,
    help: bool,
    version: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--demo" | "-d" => args.demo = true,
            "--print-library" | "--list" => args.print_library = true,
            "--help" | "-h" => args.help = true,
            "--version" | "-V" => args.version = true,
            _ => {}
        }
    }
    args
}

fn help_text() -> String {
    format!(
        "IonLauncher {version} — Steam, Epic Games and GOG in one place

USAGE:
    ionlauncher [OPTIONS]

OPTIONS:
    -d, --demo             Start with the built-in sample library
        --print-library    Print the detected library to stdout and exit
    -h, --help             Show this message
    -V, --version          Show the version
",
        version = env!("CARGO_PKG_VERSION")
    )
}

fn main() -> eframe::Result<()> {
    let args = parse_args();

    if args.help {
        println!("{}", help_text());
        return Ok(());
    }
    if args.version {
        println!("IonLauncher {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut config = Config::load();
    if args.demo {
        config.demo_mode = true;
    }

    // Headless mode: useful for scripting and for verifying detection on a
    // machine without a display.
    if args.print_library {
        return print_library(&config);
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("IonLauncher")
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([820.0, 560.0])
            .with_app_id("ionlauncher"),
        // The UI is 2D; a plain GL context avoids driver overhead on older GPUs.
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        ..Default::default()
    };

    eframe::run_native(
        "IonLauncher",
        native_options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::IonApp::new(config)))
        }),
    )
}

/// Prints the library as plain text and returns without opening a window.
fn print_library(config: &Config) -> eframe::Result<()> {
    let report = if config.demo_mode {
        demo::scan_report()
    } else {
        providers::scan_all(config)
    };
    println!("IonLauncher library ({} games)", report.games.len());
    for (store, note) in &report.notes {
        println!("  {:<12} {note}", store.name());
    }
    println!();
    for game in &report.games {
        let playtime = game.playtime_label().unwrap_or_else(|| "-".into());
        let size = game.size_label().unwrap_or_else(|| "-".into());
        println!(
            "  {:<40} {:<12} {:>9} {:>10}  {}",
            truncate(&game.title, 40),
            game.store.short(),
            playtime,
            size,
            match &game.launch {
                model::LaunchTarget::Url(url) => url.clone(),
                model::LaunchTarget::Direct { exe, .. } => exe.display().to_string(),
            }
        );
    }
    println!("\nscanned in {} ms", report.elapsed_ms);
    Ok(())
}

/// Truncates a title for column alignment, appending an ellipsis.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_titles_intact() {
        assert_eq!(truncate("Hades", 10), "Hades");
        assert_eq!(truncate("Exactly10!", 10), "Exactly10!");
    }

    #[test]
    fn truncate_adds_an_ellipsis_at_the_limit() {
        let out = truncate("The Witcher 3: Wild Hunt", 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("The Witch"));
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        // Multi-byte titles must not be cut mid-character.
        let out = truncate("Pokémon Café ReMix", 8);
        assert_eq!(out.chars().count(), 8);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn help_text_documents_every_flag() {
        let text = help_text();
        for flag in ["--demo", "--print-library", "--help", "--version"] {
            assert!(text.contains(flag), "help is missing {flag}");
        }
    }
}
