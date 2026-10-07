use std::sync::Mutex;

use myna_player_core::{PlayerCommand, PlayerSnapshot};

use crate::{PlayerEngine, PlayerError};

pub struct UnavailablePlayer {
    snapshot: Mutex<PlayerSnapshot>,
}

impl UnavailablePlayer {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            snapshot: Mutex::new(PlayerSnapshot::unavailable(reason)),
        }
    }
}

impl PlayerEngine for UnavailablePlayer {
    fn backend_name(&self) -> &'static str {
        "unavailable"
    }

    fn available(&self) -> bool {
        false
    }

    fn open(&self, _path: &str) -> Result<PlayerSnapshot, PlayerError> {
        Err(PlayerError::Unavailable(
            self.snapshot()
                .error
                .unwrap_or_else(|| "libVLC is unavailable".into()),
        ))
    }

    fn command(&self, _command: PlayerCommand) -> Result<PlayerSnapshot, PlayerError> {
        Err(PlayerError::Unavailable(
            self.snapshot()
                .error
                .unwrap_or_else(|| "libVLC is unavailable".into()),
        ))
    }

    fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| PlayerSnapshot::unavailable("player lock was poisoned"))
    }

    fn attach_surface(&self, _native_handle: usize) -> Result<(), PlayerError> {
        Err(PlayerError::Unavailable(
            self.snapshot()
                .error
                .unwrap_or_else(|| "libVLC is unavailable".into()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_player_never_fakes_playback() {
        let player = UnavailablePlayer::new("missing");
        assert!(!player.available());
        assert!(!player.snapshot().available);
        assert!(player.open("/tmp/video.mp4").is_err());
    }
}
