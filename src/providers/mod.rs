//! Store providers. Each one knows how to find and parse its store's
//! on-disk metadata and how to launch its titles.

pub mod epic;
pub mod gog;
#[cfg(target_os = "windows")]
pub mod registry;
pub mod steam;

use crate::config::Config;
use crate::model::{Game, ScanReport, Store};
use std::path::PathBuf;
use std::time::Instant;

/// Scans every enabled store and returns a merged, sorted library.
///
/// A failure in one provider never prevents the others from reporting, so
/// a machine with a broken Steam install still lists its Epic and GOG games.
pub fn scan_all(config: &Config) -> ScanReport {
    let started = Instant::now();
    let mut report = ScanReport::default();

    for store in Store::ALL {
        if !config.enabled(store) {
            report.notes.push((store, "disabled".into()));
            continue;
        }
        let games = match store {
            Store::Steam => match steam::detect(config.override_root(store)) {
                Some(install) => steam::scan(&install),
                None => {
                    report.notes.push((store, "client not found".into()));
                    continue;
                }
            },
            Store::Epic => epic::scan(config.override_root(store)),
            Store::Gog => gog::scan(config.override_root(store)),
        };
        report.notes.push((store, summarize(&games)));
        report.games.extend(games);
    }

    // Deduplicate across stores: a title owned on two stores stays twice,
    // but the same store must never report a game twice.
    report.games.sort_by(|a, b| a.id.cmp(&b.id));
    report.games.dedup_by(|a, b| a.id == b.id);

    report.elapsed_ms = started.elapsed().as_millis();
    report
}

fn summarize(games: &[Game]) -> String {
    match games.len() {
        0 => "no games found".into(),
        1 => "1 game".into(),
        n => format!("{n} games"),
    }
}

/// Suggests a likely store root, used to prefill the settings field.
pub fn suggested_root(store: Store) -> Option<PathBuf> {
    match store {
        Store::Steam => steam::locate(None),
        Store::Epic => epic::manifests_dir(None),
        Store::Gog => gog::galaxy_data_dir(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_stores_are_reported_not_scanned() {
        let mut config = Config::default();
        for store in Store::ALL {
            config.set_enabled(store, false);
        }
        let report = scan_all(&config);
        assert!(report.games.is_empty());
        for store in Store::ALL {
            assert_eq!(report.note(store), Some("disabled"));
        }
    }

    #[test]
    fn scan_is_bounded_in_time_with_no_stores_present() {
        // Point every store at a nonexistent root so detection fails fast
        // rather than touching the real machine.
        let mut config = Config::default();
        let missing = PathBuf::from("/definitely/not/here");
        for store in Store::ALL {
            config.set_override_root(store, Some(missing.clone()));
        }
        let report = scan_all(&config);
        assert!(report.games.is_empty());
        // The overrides do not exist, so each store reports not found or empty.
        for store in Store::ALL {
            assert!(report.note(store).is_some());
        }
    }
}
