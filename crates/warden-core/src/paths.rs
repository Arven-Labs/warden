//! Where Warden keeps its files (P143). Every file the hub writes — config, devices, API keys, members, conversations,
//! the models it downloads — lives under one folder, which is `<config dir>/warden` unless the process was started with
//! `--data-dir`: a service (systemd, a container) then puts everything on one volume.
//!
//! Set once at startup, before any path is asked for; without it nothing moves.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// From now on every Warden file lives under `dir` instead of `<config dir>/warden`. Only the first call counts, and it
/// has to come before anything asks for a path.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// The folder given to `set_data_dir`, if any.
pub fn data_dir() -> Option<&'static Path> {
    DATA_DIR.get().map(PathBuf::as_path)
}

/// The folder every Warden file lives in: the one given to `set_data_dir`, else `<config dir>/warden`. `None` when
/// neither exists (no `$HOME`).
pub fn warden_dir() -> Option<PathBuf> {
    resolve(DATA_DIR.get().map(PathBuf::as_path), dirs::config_dir())
}

/// Where the downloaded embedding models go: `<data dir>/models` when there is a data dir, else the OS cache folder
/// (`<cache dir>/warden/models`), which is not vault content and is shared by every vault.
pub fn model_cache_dir() -> PathBuf {
    match DATA_DIR.get() {
        Some(dir) => dir.join("models"),
        None => dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("warden").join("models"),
    }
}

fn resolve(data_dir: Option<&Path>, config_dir: Option<PathBuf>) -> Option<PathBuf> {
    match data_dir {
        Some(dir) => Some(dir.to_path_buf()),
        None => config_dir.map(|dir| dir.join("warden")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_data_dir_warden_lives_under_the_config_dir() {
        assert_eq!(resolve(None, Some(PathBuf::from("/home/ana/.config"))), Some(PathBuf::from("/home/ana/.config/warden")));
        assert_eq!(resolve(None, None), None, "no config dir and no data dir: nowhere to put anything");
    }

    #[test]
    fn a_data_dir_is_the_folder_itself_and_wins_over_the_config_dir() {
        assert_eq!(resolve(Some(Path::new("/data")), Some(PathBuf::from("/home/ana/.config"))), Some(PathBuf::from("/data")));
        assert_eq!(resolve(Some(Path::new("/data")), None), Some(PathBuf::from("/data")), "even where there is no home");
    }
}
