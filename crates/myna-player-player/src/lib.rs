mod unavailable;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod vlc;

use myna_player_core::{PlayerCommand, PlayerSnapshot};
use thiserror::Error;

pub use unavailable::UnavailablePlayer;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub use vlc::LibVlcPlayer;

#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("player backend is unavailable: {0}")]
    Unavailable(String),
    #[error("media path is invalid: {0}")]
    InvalidPath(String),
    #[error("native player failed: {0}")]
    Backend(String),
    #[error("player lock was poisoned")]
    Poisoned,
}

pub trait PlayerEngine: Send + Sync {
    fn backend_name(&self) -> &'static str;
    fn available(&self) -> bool;
    fn open(&self, path: &str) -> Result<PlayerSnapshot, PlayerError>;
    fn command(&self, command: PlayerCommand) -> Result<PlayerSnapshot, PlayerError>;
    fn snapshot(&self) -> PlayerSnapshot;
    fn attach_surface(&self, native_handle: usize) -> Result<(), PlayerError>;
}

pub fn create_default_player() -> Box<dyn PlayerEngine> {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        match LibVlcPlayer::discover() {
            Ok(player) => return Box::new(player),
            Err(error) => return Box::new(UnavailablePlayer::new(error.to_string())),
        }
    }

    #[allow(unreachable_code)]
    Box::new(UnavailablePlayer::new(
        "Myna Player does not have a native player backend for this platform.",
    ))
}
