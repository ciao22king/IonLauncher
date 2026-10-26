//! Settings screen: store toggles, library behaviour and manual paths.

use crate::app::IonApp;
use crate::model::Store;
use crate::providers;
use crate::theme::Palette;
use crate::widgets;
use egui::{RichText, ScrollArea, Vec2};
use std::path::PathBuf;

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Settings")
            .size(22.0)
            .strong()
            .color(Palette::TEXT),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("IonLauncher reads each store's own files. Nothing is uploaded anywhere.")
            .size(12.5)
            .color(Palette::TEXT_MUTED),
    );
    ui.add_space(14.0);

    // Cards read better in a fixed column than stretched across a wide
    // window, so the form is centred and capped at a comfortable width.
    ScrollArea::vertical()
        .id_salt("settings-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width().min(760.0);
            ui.allocate_ui_with_layout(
                Vec2::new(width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    stores_section(app, ui);
                    ui.add_space(14.0);
                    library_section(app, ui);
                    ui.add_space(14.0);
                    updates_section(app, ui);
                    ui.add_space(14.0);
                    // Proton only exists on Linux, so the section is hidden
                    // entirely elsewhere rather than shown greyed out.
                    #[cfg(target_os = "linux")]
                    {
                        proton_section(app, ui);
                        ui.add_space(14.0);
                    }
                    paths_section(app, ui);
                    ui.add_space(14.0);
                    about_section(app, ui);
                    ui.add_space(20.0);
                },
            );
        });
}

fn stores_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Stores");
        ui.add_space(8.0);

        for store in Store::ALL {
            let mut enabled = app.config.enabled(store);
            let count = app.games.iter().filter(|g| g.store == store).count();
            let note = app
                .last_report
                .as_ref()
                .and_then(|r| r.note(store))
                .unwrap_or("not scanned yet")
                .to_string();

            ui.horizontal(|ui| {
                ui.painter().circle_filled(
                    ui.cursor().min + Vec2::new(6.0, 9.0),
                    5.0,
                    if enabled {
                        store.accent()
                    } else {
                        Palette::TEXT_FAINT
                    },
                );
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(store.name()).size(14.0).color(Palette::TEXT));
                    let detail = if enabled {
                        format!("{count} games · {note}")
                    } else {
                        "disabled".to_string()
                    };
                    ui.label(RichText::new(detail).size(11.5).color(Palette::TEXT_FAINT));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.checkbox(&mut enabled, "").changed() {
                        app.config.set_enabled(store, enabled);
                        app.config_dirty = true;
                        // A disabled store is dropped from the scan entirely,
                        // so its games simply stop appearing in the library.
                        app.view_cache = None;
                        app.rescan();
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
}

fn library_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Library");
        ui.add_space(8.0);

        let mut show_missing = app.config.show_missing;
        if ui
            .checkbox(
                &mut show_missing,
                "Show games whose install folder is missing",
            )
            .changed()
        {
            app.config.show_missing = show_missing;
            app.config_dirty = true;
            app.view_cache = None;
        }

        ui.add_space(6.0);
        let mut fetch_art = app.config.fetch_artwork;
        if ui
            .checkbox(
                &mut fetch_art,
                "Download cover artwork (needs a network connection)",
            )
            .changed()
        {
            app.config.fetch_artwork = fetch_art;
            app.art.set_fetch_enabled(fetch_art);
            app.config_dirty = true;
            if !fetch_art {
                app.art.clear_disk_cache();
            }
        }
        ui.label(
            RichText::new("Artwork is cached on disk, so covers appear instantly next time.")
                .size(11.0)
                .color(Palette::TEXT_FAINT),
        );

        ui.add_space(10.0);
        let mut card_width = app.config.card_width;
        ui.label(
            RichText::new("Card size")
                .size(12.0)
                .color(Palette::TEXT_MUTED),
        );
        if ui
            .add(egui::Slider::new(&mut card_width, 160.0..=320.0).suffix(" pt"))
            .changed()
        {
            app.config.card_width = card_width;
            app.config_dirty = true;
        }

        ui.add_space(6.0);
        let mut demo = app.config.demo_mode;
        if ui
            .checkbox(&mut demo, "Demo library (sample games, no scanning)")
            .changed()
        {
            app.config.demo_mode = demo;
            app.config_dirty = true;
            app.rescan();
        }

        if !app.config.favorites.is_empty() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} favourites saved", app.config.favorites.len()))
                        .size(11.5)
                        .color(Palette::TEXT_FAINT),
                );
                if widgets::secondary_button(ui, "Clear").clicked() {
                    app.config.favorites.clear();
                    app.config_dirty = true;
                    app.view_cache = None;
                }
            });
        }
    });
}

fn paths_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Manual locations");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Only needed when a store lives somewhere unusual. Leave blank for automatic detection.",
            )
            .size(11.5)
            .color(Palette::TEXT_MUTED),
        );
        ui.add_space(10.0);

        for store in Store::ALL {
            let mut text = app
                .config
                .override_root(store)
                .map(|p| p.display().to_string())
                .unwrap_or_default();

            ui.label(
                RichText::new(store.name())
                    .size(12.0)
                    .color(Palette::TEXT_MUTED),
            );
            ui.horizontal(|ui| {
                let response = ui.add_sized(
                    Vec2::new((ui.available_width() - 120.0).max(120.0), 26.0),
                    egui::TextEdit::singleline(&mut text)
                        .hint_text("automatic")
                        .desired_width(f32::INFINITY),
                );
                if response.changed() {
                    let trimmed = text.trim();
                    let value = if trimmed.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(trimmed))
                    };
                    app.config.set_override_root(store, value);
                    app.config_dirty = true;
                }
                if widgets::secondary_button(ui, "Detect").clicked() {
                    match providers::suggested_root(store) {
                        Some(path) => {
                            app.config.set_override_root(store, Some(path));
                            app.config_dirty = true;
                            app.rescan();
                        }
                        None => app.notify_error(&format!("{} not found on this machine", store.name())),
                    }
                }
            });
            ui.add_space(6.0);
        }

        ui.add_space(4.0);
        if widgets::secondary_button(ui, "Rescan now").clicked() {
            app.rescan();
        }
    });
}

/// Proton: running Windows games on Linux.
///
/// Split into "what is installed" and "what can be downloaded", because the
/// two answer different questions: which build do I use, and where do I get
/// another one.
#[cfg(target_os = "linux")]
fn proton_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Proton (Linux)");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Proton runs Windows games on Linux. Pick a build here and Windows titles \
                 launch through it, each with its own prefix.",
            )
            .size(11.5)
            .color(Palette::TEXT_MUTED),
        );
        ui.add_space(10.0);

        // Master switch. Without a build installed there is nothing to switch
        // to, so the checkbox explains that instead of silently doing nothing.
        let mut enabled = app.config.proton_enabled;
        let has_builds = !app.proton_installed.is_empty();
        ui.add_enabled_ui(has_builds, |ui| {
            if ui
                .checkbox(&mut enabled, "Use Proton for Windows games")
                .changed()
            {
                app.config.proton_enabled = enabled;
                app.config_dirty = true;
            }
        });
        if !has_builds {
            ui.label(
                RichText::new("No Proton build found yet — download one below.")
                    .size(11.0)
                    .color(Palette::TEXT_FAINT),
            );
        }

        ui.add_space(12.0);
        installed_proton_list(app, ui);

        ui.add_space(12.0);
        proton_download_list(app, ui);
    });
}

/// The builds already on this machine, as a selectable list.
#[cfg(target_os = "linux")]
fn installed_proton_list(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Installed ({})", app.proton_installed.len()))
                .size(12.5)
                .color(Palette::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::secondary_button(ui, "Rescan").clicked() {
                app.refresh_proton();
            }
        });
    });
    ui.add_space(6.0);

    if app.proton_installed.is_empty() {
        ui.label(
            RichText::new(
                "Nothing installed. Steam's own builds and community builds in \
                 compatibilitytools.d are both detected.",
            )
            .size(11.0)
            .color(Palette::TEXT_FAINT),
        );
        return;
    }

    // Collected up front so the list can be drawn while `app` is borrowed.
    let builds = app.proton_installed.clone();
    let selected = app.config.proton_version.clone();
    let active = app
        .active_proton()
        .map(|b| b.name.clone())
        .unwrap_or_default();

    for build in &builds {
        let is_selected = selected.as_deref() == Some(build.name.as_str());
        let is_active = active == build.name;
        let mut remove_request = false;

        ui.horizontal(|ui| {
            // A radio makes the "one at a time" rule obvious. The value is
            // driven by the config rather than by egui's own state, so the
            // selection stays correct after a build is removed.
            if ui.radio(is_selected, "").clicked() && !is_selected {
                app.config.proton_version = Some(build.name.clone());
                app.config_dirty = true;
            }

            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&build.name).size(13.0).color(Palette::TEXT));
                    if is_active {
                        ui.label(RichText::new("in use").size(10.5).color(Palette::OK));
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        build.origin.label(),
                        build.path.display()
                    ))
                    .size(10.5)
                    .color(Palette::TEXT_FAINT),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Steam's own builds are not ours to delete.
                if build.origin == crate::proton::ProtonOrigin::CompatibilityTool
                    && widgets::secondary_button(ui, "Remove").clicked()
                {
                    remove_request = true;
                }
            });
        });

        if remove_request {
            match crate::proton::remove(build) {
                Ok(()) => {
                    if is_selected {
                        app.config.proton_version = None;
                        app.config_dirty = true;
                    }
                    app.refresh_proton();
                    app.notify(&format!("{} removed", build.name));
                }
                Err(err) => app.notify_error(&format!("{}: {err}", build.name)),
            }
        }

        ui.add_space(6.0);
    }
}

/// Downloadable community builds, with a per-row download button.
#[cfg(target_os = "linux")]
fn proton_download_list(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Download builds")
                .size(12.5)
                .color(Palette::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::secondary_button(ui, "Refresh list").clicked() {
                app.proton_releases = None;
                app.fetch_proton_releases();
            }
        });
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("Community builds from GloriousEggroll/proton-ge-custom.")
            .size(10.5)
            .color(Palette::TEXT_FAINT),
    );
    ui.add_space(8.0);

    // Progress of an in-flight install takes over the section: while a build
    // is downloading, starting a second one would be confusing and wasteful.
    if let Some((tag, progress)) = &app.proton_download {
        ui.label(
            RichText::new(format!("Installing {tag}…"))
                .size(12.0)
                .color(Palette::TEXT),
        );
        ui.add_space(4.0);
        let fraction = progress.fraction();
        let text = if progress.total.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            "starting…".to_string()
        } else {
            format!("{:.0}%", fraction * 100.0)
        };
        ui.add(
            egui::ProgressBar::new(fraction)
                .desired_width(ui.available_width())
                .text(text),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "A build is a few hundred megabytes; the download runs in the background.",
            )
            .size(10.5)
            .color(Palette::TEXT_FAINT),
        );
        return;
    }

    if app.proton_releases.is_none() && !app.proton_fetching_releases {
        // First time the section is drawn, kick off the listing.
        app.fetch_proton_releases();
    }

    if app.proton_fetching_releases {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                RichText::new("Loading the list…")
                    .size(11.5)
                    .color(Palette::TEXT_MUTED),
            );
        });
        return;
    }

    if let Some(err) = &app.proton_releases_error {
        ui.label(
            RichText::new(format!("Could not load the list: {err}"))
                .size(11.0)
                .color(Palette::DANGER),
        );
        return;
    }

    let Some(releases) = app.proton_releases.clone() else {
        return;
    };

    let installed_names: Vec<String> = app
        .proton_installed
        .iter()
        .map(|b| b.name.clone())
        .collect();

    for release in releases {
        let already = installed_names.iter().any(|n| n == &release.tag);
        let mut start = false;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&release.tag).size(12.5).color(Palette::TEXT));
                let detail = if release.published.is_empty() {
                    release.size_label()
                } else {
                    format!("{} · {}", release.published, release.size_label())
                };
                ui.label(RichText::new(detail).size(10.5).color(Palette::TEXT_FAINT));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if already {
                    ui.label(RichText::new("installed").size(11.0).color(Palette::OK));
                } else if widgets::secondary_button(ui, "Download").clicked() {
                    start = true;
                }
            });
        });

        if start {
            let progress = crate::proton::DownloadProgress::new();
            app.proton_download = Some((release.tag.clone(), progress.clone()));
            crate::proton::download_release(release, progress);
        }

        ui.add_space(6.0);
    }
}

/// Updates: the only channel this launcher has to tell you a new build exists.
fn updates_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Updates");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("IonLauncher {}", crate::update::current_version()))
                    .size(12.5)
                    .color(Palette::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.update_checking {
                    ui.spinner();
                    ui.label(
                        RichText::new("Checking…")
                            .size(11.5)
                            .color(Palette::TEXT_MUTED),
                    );
                } else if widgets::secondary_button(ui, "Check now").clicked() {
                    app.check_for_updates(true);
                }
            });
        });

        ui.add_space(6.0);
        let mut enabled = app.config.check_updates;
        if ui
            .checkbox(&mut enabled, "Check for updates on startup")
            .changed()
        {
            app.config.check_updates = enabled;
            app.config_dirty = true;
        }
        ui.label(
            RichText::new(
                "One request to the public GitHub releases page. Nothing else is sent, and \
                 downloading an update never installs it without asking.",
            )
            .size(10.5)
            .color(Palette::TEXT_FAINT),
        );

        // An in-flight download owns the card: showing the banner underneath
        // would invite a second click on a download already running.
        if let Some(progress) = app.update_download.clone() {
            ui.add_space(10.0);
            let fraction = progress.fraction();
            let text = if progress.total.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                "starting…".to_string()
            } else {
                format!("{:.0}%", fraction * 100.0)
            };
            ui.label(
                RichText::new("Downloading the update…")
                    .size(12.0)
                    .color(Palette::TEXT),
            );
            ui.add_space(4.0);
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .text(text),
            );
            return;
        }

        // Downloaded and waiting: the file is in the download folder, and the
        // last step is the user's to take.
        if let Some(path) = app.update_ready.clone() {
            ui.add_space(10.0);
            ui.label(
                RichText::new("Ready to install")
                    .size(12.5)
                    .strong()
                    .color(Palette::OK),
            );
            ui.add_space(2.0);
            ui.label(RichText::new(&path).size(10.5).color(Palette::TEXT_FAINT));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::secondary_button(ui, "Run installer").clicked() {
                    match crate::update::run_installer(&path) {
                        Ok(()) => app.notify("Installer opened — follow its steps"),
                        Err(err) => app.notify_error(&format!("Could not run it: {err}")),
                    }
                }
                if widgets::secondary_button(ui, "Open folder").clicked() {
                    if let Some(parent) = std::path::Path::new(&path).parent() {
                        let _ = crate::util::open_url(&parent.display().to_string());
                    }
                }
            });
            return;
        }

        let Some(update) = app.update_available.clone() else {
            if app.update_error.is_some() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("The last check could not reach GitHub.")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED),
                );
            } else if app.update_checked && !app.update_checking {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("You are on the latest version.")
                        .size(11.0)
                        .color(Palette::TEXT_MUTED),
                );
            }
            return;
        };

        ui.add_space(10.0);
        ui.label(
            RichText::new(format!("Version {} is available", update.version))
                .size(12.5)
                .strong()
                .color(Palette::ACCENT),
        );
        if update.size_bytes > 0 {
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!("Download size: {}", update.size_label()))
                    .size(10.5)
                    .color(Palette::TEXT_FAINT),
            );
        }

        if !update.notes.trim().is_empty() {
            ui.add_space(6.0);
            // Release notes can be long; cap the height so the card cannot
            // push the rest of the settings off-screen.
            egui::ScrollArea::vertical()
                .id_salt("update-notes")
                .max_height(120.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(update.notes.trim())
                            .size(11.0)
                            .color(Palette::TEXT_MUTED),
                    );
                });
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if update.asset_url.is_some() {
                if widgets::secondary_button(ui, "Download update").clicked() {
                    app.download_update();
                }
            } else {
                // No file for this platform: the release page is the only way.
                if widgets::secondary_button(ui, "Open release page").clicked() {
                    let _ = crate::util::open_url(&update.page_url);
                }
                ui.label(
                    RichText::new("No installer for this platform yet.")
                        .size(10.5)
                        .color(Palette::TEXT_FAINT),
                );
            }
            if widgets::secondary_button(ui, "Skip this version").clicked() {
                app.config.skipped_version = Some(update.version.clone());
                app.config_dirty = true;
                app.update_available = None;
                app.notify("Version skipped");
            }
        });
    });
}

fn about_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "About");
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("IonLauncher {}", env!("CARGO_PKG_VERSION")))
                .size(13.0)
                .color(Palette::TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "A single, fast launcher for Steam, Epic Games and GOG. Written in Rust with an \
                 immediate-mode GPU interface, so it starts instantly and stays responsive on \
                 older machines.",
            )
            .size(11.5)
            .color(Palette::TEXT_MUTED),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("GPL-2.0-or-later")
                .size(11.0)
                .color(Palette::TEXT_FAINT),
        );
        ui.add_space(6.0);
        if let Some(dir) = crate::art::art_cache_dir() {
            ui.label(
                RichText::new(format!("Artwork cache: {}", dir.display()))
                    .size(10.5)
                    .color(Palette::TEXT_FAINT),
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, "Clear artwork cache").clicked() {
                app.art.clear_disk_cache();
                app.notify("Artwork cache cleared");
            }
            if widgets::secondary_button(ui, "Open config folder").clicked() {
                match crate::config::Config::path() {
                    Some(path) => {
                        if let Some(parent) = path.parent() {
                            let _ = crate::util::open_url(&parent.display().to_string());
                        }
                    }
                    None => app.notify_error("Config path unavailable"),
                }
            }
        });
    });
}
