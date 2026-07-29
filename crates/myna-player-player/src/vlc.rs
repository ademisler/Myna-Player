use std::{
    env,
    ffi::{CStr, CString, c_char, c_float, c_int, c_longlong, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, Mutex},
};

use libloading::Library;
use myna_player_core::{PlayerCommand, PlayerSnapshot, PlayerState, TrackDescriptor, TrackKind};

use crate::{PlayerEngine, PlayerError};

type VlcInstance = c_void;
type VlcMedia = c_void;
type VlcMediaPlayer = c_void;

#[repr(C)]
struct VlcTrackDescription {
    id: c_int,
    name: *mut c_char,
    next: *mut VlcTrackDescription,
}

type NewInstance = unsafe extern "C" fn(c_int, *const *const c_char) -> *mut VlcInstance;
type ReleaseInstance = unsafe extern "C" fn(*mut VlcInstance);
type NewMediaPath = unsafe extern "C" fn(*mut VlcInstance, *const c_char) -> *mut VlcMedia;
type ReleaseMedia = unsafe extern "C" fn(*mut VlcMedia);
type ParseMedia = unsafe extern "C" fn(*mut VlcMedia);
type GetMediaDuration = unsafe extern "C" fn(*mut VlcMedia) -> c_longlong;
type NewMediaPlayer = unsafe extern "C" fn(*mut VlcInstance) -> *mut VlcMediaPlayer;
type ReleaseMediaPlayer = unsafe extern "C" fn(*mut VlcMediaPlayer);
type SetMedia = unsafe extern "C" fn(*mut VlcMediaPlayer, *mut VlcMedia);
type Play = unsafe extern "C" fn(*mut VlcMediaPlayer) -> c_int;
type SetPause = unsafe extern "C" fn(*mut VlcMediaPlayer, c_int);
type Stop = unsafe extern "C" fn(*mut VlcMediaPlayer);
type GetTime = unsafe extern "C" fn(*mut VlcMediaPlayer) -> c_longlong;
type SetTime = unsafe extern "C" fn(*mut VlcMediaPlayer, c_longlong);
type GetLength = unsafe extern "C" fn(*mut VlcMediaPlayer) -> c_longlong;
type GetState = unsafe extern "C" fn(*mut VlcMediaPlayer) -> c_int;
type SetVolume = unsafe extern "C" fn(*mut VlcMediaPlayer, c_int) -> c_int;
type SetMute = unsafe extern "C" fn(*mut VlcMediaPlayer, c_int);
type SetRate = unsafe extern "C" fn(*mut VlcMediaPlayer, c_float) -> c_int;
type GetTrack = unsafe extern "C" fn(*mut VlcMediaPlayer) -> c_int;
type SetTrack = unsafe extern "C" fn(*mut VlcMediaPlayer, c_int) -> c_int;
type GetTrackDescription = unsafe extern "C" fn(*mut VlcMediaPlayer) -> *mut VlcTrackDescription;
type ReleaseTrackDescription = unsafe extern "C" fn(*mut VlcTrackDescription);
type ErrorMessage = unsafe extern "C" fn() -> *const c_char;
#[cfg(target_os = "macos")]
type SetNSObject = unsafe extern "C" fn(*mut VlcMediaPlayer, *mut c_void);
#[cfg(target_os = "windows")]
type SetHwnd = unsafe extern "C" fn(*mut VlcMediaPlayer, *mut c_void);
#[cfg(target_os = "linux")]
type SetXWindow = unsafe extern "C" fn(*mut VlcMediaPlayer, u32);

struct VlcApi {
    #[cfg(target_os = "macos")]
    _core_library: Library,
    _library: Library,
    new_instance: NewInstance,
    release_instance: ReleaseInstance,
    new_media_path: NewMediaPath,
    release_media: ReleaseMedia,
    parse_media: ParseMedia,
    media_get_duration: GetMediaDuration,
    new_media_player: NewMediaPlayer,
    release_media_player: ReleaseMediaPlayer,
    set_media: SetMedia,
    play: Play,
    set_pause: SetPause,
    stop: Stop,
    get_time: GetTime,
    set_time: SetTime,
    get_length: GetLength,
    get_state: GetState,
    set_volume: SetVolume,
    set_mute: SetMute,
    set_rate: SetRate,
    audio_get_track: GetTrack,
    audio_set_track: SetTrack,
    audio_get_track_description: GetTrackDescription,
    video_get_spu: GetTrack,
    video_set_spu: SetTrack,
    video_get_spu_description: GetTrackDescription,
    track_description_release: ReleaseTrackDescription,
    error_message: ErrorMessage,
    #[cfg(target_os = "macos")]
    set_nsobject: SetNSObject,
    #[cfg(target_os = "windows")]
    set_hwnd: SetHwnd,
    #[cfg(target_os = "linux")]
    set_xwindow: SetXWindow,
}

impl VlcApi {
    unsafe fn load(path: &Path) -> Result<Self, PlayerError> {
        #[cfg(target_os = "macos")]
        let core_library = {
            use libloading::os::unix::{Library as UnixLibrary, RTLD_GLOBAL, RTLD_NOW};

            let core_path = path.with_file_name("libvlccore.dylib");
            // SAFETY: libvlccore is the matching dependency shipped beside the selected
            // libvlc. RTLD_GLOBAL lets libvlc resolve its @rpath install name.
            let library = unsafe { UnixLibrary::open(Some(&core_path), RTLD_NOW | RTLD_GLOBAL) }
                .map_err(|error| {
                    PlayerError::Unavailable(format!("{}: {error}", core_path.display()))
                })?;
            Library::from(library)
        };

        // SAFETY: The library remains owned by VlcApi for at least as long as every copied
        // function pointer. Each symbol type below matches the stable libVLC 3 C header.
        #[cfg(target_os = "macos")]
        let library = {
            use libloading::os::unix::{Library as UnixLibrary, RTLD_GLOBAL, RTLD_NOW};

            // SAFETY: The matching core library is already loaded and both handles are retained.
            Library::from(
                unsafe { UnixLibrary::open(Some(path), RTLD_NOW | RTLD_GLOBAL) }.map_err(
                    |error| PlayerError::Unavailable(format!("{}: {error}", path.display())),
                )?,
            )
        };
        #[cfg(not(target_os = "macos"))]
        let library = unsafe { Library::new(path) }
            .map_err(|error| PlayerError::Unavailable(format!("{}: {error}", path.display())))?;

        macro_rules! symbol {
            ($name:literal, $type:ty) => {{
                // SAFETY: Symbol names and signatures are taken from libvlc_media_player.h.
                unsafe {
                    *library
                        .get::<$type>(concat!($name, "\0").as_bytes())
                        .map_err(|error| {
                            PlayerError::Unavailable(format!(
                                "libVLC symbol {} is unavailable: {error}",
                                $name
                            ))
                        })?
                }
            }};
        }

        Ok(Self {
            #[cfg(target_os = "macos")]
            _core_library: core_library,
            new_instance: symbol!("libvlc_new", NewInstance),
            release_instance: symbol!("libvlc_release", ReleaseInstance),
            new_media_path: symbol!("libvlc_media_new_path", NewMediaPath),
            release_media: symbol!("libvlc_media_release", ReleaseMedia),
            parse_media: symbol!("libvlc_media_parse", ParseMedia),
            media_get_duration: symbol!("libvlc_media_get_duration", GetMediaDuration),
            new_media_player: symbol!("libvlc_media_player_new", NewMediaPlayer),
            release_media_player: symbol!("libvlc_media_player_release", ReleaseMediaPlayer),
            set_media: symbol!("libvlc_media_player_set_media", SetMedia),
            play: symbol!("libvlc_media_player_play", Play),
            set_pause: symbol!("libvlc_media_player_set_pause", SetPause),
            stop: symbol!("libvlc_media_player_stop", Stop),
            get_time: symbol!("libvlc_media_player_get_time", GetTime),
            set_time: symbol!("libvlc_media_player_set_time", SetTime),
            get_length: symbol!("libvlc_media_player_get_length", GetLength),
            get_state: symbol!("libvlc_media_player_get_state", GetState),
            set_volume: symbol!("libvlc_audio_set_volume", SetVolume),
            set_mute: symbol!("libvlc_audio_set_mute", SetMute),
            set_rate: symbol!("libvlc_media_player_set_rate", SetRate),
            audio_get_track: symbol!("libvlc_audio_get_track", GetTrack),
            audio_set_track: symbol!("libvlc_audio_set_track", SetTrack),
            audio_get_track_description: symbol!(
                "libvlc_audio_get_track_description",
                GetTrackDescription
            ),
            video_get_spu: symbol!("libvlc_video_get_spu", GetTrack),
            video_set_spu: symbol!("libvlc_video_set_spu", SetTrack),
            video_get_spu_description: symbol!(
                "libvlc_video_get_spu_description",
                GetTrackDescription
            ),
            track_description_release: symbol!(
                "libvlc_track_description_list_release",
                ReleaseTrackDescription
            ),
            error_message: symbol!("libvlc_errmsg", ErrorMessage),
            #[cfg(target_os = "macos")]
            set_nsobject: symbol!("libvlc_media_player_set_nsobject", SetNSObject),
            #[cfg(target_os = "windows")]
            set_hwnd: symbol!("libvlc_media_player_set_hwnd", SetHwnd),
            #[cfg(target_os = "linux")]
            set_xwindow: symbol!("libvlc_media_player_set_xwindow", SetXWindow),
            _library: library,
        })
    }

    fn last_error(&self, fallback: &str) -> String {
        // SAFETY: libvlc_errmsg returns a thread-local, null-terminated string or null.
        let pointer = unsafe { (self.error_message)() };
        if pointer.is_null() {
            return fallback.into();
        }
        // SAFETY: libVLC guarantees a valid C string until the next libVLC error call.
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }
}

struct VlcInner {
    instance: *mut VlcInstance,
    player: *mut VlcMediaPlayer,
    media_path: Option<String>,
    duration_ms: u64,
    error: Option<String>,
    volume: u8,
    muted: bool,
    rate: f32,
}

// libVLC media-player calls are serialized through the surrounding Mutex. The pointers are
// created and destroyed by libVLC and are never dereferenced by Rust.
unsafe impl Send for VlcInner {}

pub struct LibVlcPlayer {
    api: Arc<VlcApi>,
    inner: Mutex<VlcInner>,
    library_path: PathBuf,
}

impl LibVlcPlayer {
    pub fn discover() -> Result<Self, PlayerError> {
        let mut errors = Vec::new();
        for candidate in library_candidates() {
            if !candidate.exists() && candidate.components().count() > 1 {
                continue;
            }
            match unsafe { Self::from_library(&candidate) } {
                Ok(player) => return Ok(player),
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(PlayerError::Unavailable(if errors.is_empty() {
            "libVLC 3 was not found. Install VLC or bundle libVLC with Myna Player.".into()
        } else {
            errors.join("; ")
        }))
    }

    unsafe fn from_library(path: &Path) -> Result<Self, PlayerError> {
        // SAFETY: VlcApi verifies all required symbols before returning.
        let api = Arc::new(unsafe { VlcApi::load(path) }?);
        if let Some(directory) = plugin_directory(path) {
            // SAFETY: Player construction is serialized during application startup, before any
            // worker threads call libVLC. libVLC 3 reads this documented process variable.
            unsafe { env::set_var("VLC_PLUGIN_PATH", directory) };
        }
        #[cfg(not(test))]
        let arguments = [
            CString::new("--no-video-title-show").expect("static argument"),
            CString::new("--quiet").expect("static argument"),
        ];
        #[cfg(test)]
        let arguments = {
            // Unit tests do not own an NSView/HWND. libVLC still opens and decodes the real
            // media, while dummy outputs keep the smoke test independent from a window server.
            let mut arguments = vec![
                CString::new("--no-video-title-show").expect("static argument"),
                CString::new("--quiet").expect("static argument"),
            ];
            arguments.push(CString::new("--vout=dummy").expect("static argument"));
            arguments.push(CString::new("--aout=dummy").expect("static argument"));
            arguments
        };
        let argument_pointers = arguments
            .iter()
            .map(|argument| argument.as_ptr())
            .collect::<Vec<_>>();

        // SAFETY: Pointers remain valid for the duration of libvlc_new.
        let instance = unsafe {
            (api.new_instance)(argument_pointers.len() as c_int, argument_pointers.as_ptr())
        };
        if instance.is_null() {
            return Err(PlayerError::Unavailable(
                api.last_error("libvlc_new returned null"),
            ));
        }
        // SAFETY: instance is a live libVLC instance.
        let player = unsafe { (api.new_media_player)(instance) };
        if player.is_null() {
            // SAFETY: instance is live and owned by this constructor.
            unsafe { (api.release_instance)(instance) };
            return Err(PlayerError::Unavailable(
                api.last_error("libvlc_media_player_new returned null"),
            ));
        }

        Ok(Self {
            api,
            inner: Mutex::new(VlcInner {
                instance,
                player,
                media_path: None,
                duration_ms: 0,
                error: None,
                volume: 100,
                muted: false,
                rate: 1.0,
            }),
            library_path: path.to_path_buf(),
        })
    }

    pub fn library_path(&self) -> &Path {
        &self.library_path
    }

    fn snapshot_locked(&self, inner: &VlcInner) -> PlayerSnapshot {
        // SAFETY: player is alive while inner is locked.
        let position = unsafe { (self.api.get_time)(inner.player) }.max(0) as u64;
        // SAFETY: player is alive while inner is locked.
        let player_duration = unsafe { (self.api.get_length)(inner.player) }.max(0) as u64;
        let duration = player_duration.max(inner.duration_ms);
        // SAFETY: player is alive while inner is locked.
        let state = map_state(unsafe { (self.api.get_state)(inner.player) });
        let tracks = self.track_descriptors(inner.player);

        PlayerSnapshot {
            available: true,
            backend: format!("libVLC ({})", self.library_path.display()),
            state,
            media_path: inner.media_path.clone(),
            file_name: inner.media_path.as_deref().and_then(|path| {
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            }),
            position_ms: position,
            duration_ms: duration,
            volume: inner.volume,
            muted: inner.muted,
            rate: if inner.rate.is_finite() && inner.rate > 0.0 {
                inner.rate
            } else {
                1.0
            },
            tracks,
            error: inner.error.clone(),
        }
    }

    fn track_descriptors(&self, player: *mut VlcMediaPlayer) -> Vec<TrackDescriptor> {
        // SAFETY: player is live; descriptions are released below.
        let selected_audio = unsafe { (self.api.audio_get_track)(player) };
        // SAFETY: player is live.
        let audio = unsafe { (self.api.audio_get_track_description)(player) };
        let mut tracks = unsafe {
            collect_tracks(
                audio,
                TrackKind::Audio,
                selected_audio,
                self.api.track_description_release,
            )
        };

        // SAFETY: player is live; descriptions are released below.
        let selected_subtitle = unsafe { (self.api.video_get_spu)(player) };
        // SAFETY: player is live.
        let subtitle = unsafe { (self.api.video_get_spu_description)(player) };
        tracks.extend(unsafe {
            collect_tracks(
                subtitle,
                TrackKind::Subtitle,
                selected_subtitle,
                self.api.track_description_release,
            )
        });
        tracks
    }

    fn restart_if_ended(&self, inner: &mut VlcInner) {
        // libVLC 3 does not reliably restart an ended item with a plain `play` call.
        let state = map_state(unsafe { (self.api.get_state)(inner.player) });
        let position = unsafe { (self.api.get_time)(inner.player) }.max(0) as u64;
        let length =
            (unsafe { (self.api.get_length)(inner.player) }.max(0) as u64).max(inner.duration_ms);
        let at_end = length > 0 && position.saturating_add(250) >= length;
        if state == PlayerState::Ended || at_end {
            unsafe {
                (self.api.stop)(inner.player);
                (self.api.set_time)(inner.player, 0);
            }
        }
    }

    fn play_locked(&self, inner: &mut VlcInner) -> Result<(), PlayerError> {
        self.restart_if_ended(inner);
        if unsafe { (self.api.play)(inner.player) } != 0 {
            let error = self.api.last_error("libVLC play failed");
            inner.error = Some(error.clone());
            return Err(PlayerError::Backend(error));
        }
        Ok(())
    }
}

impl PlayerEngine for LibVlcPlayer {
    fn backend_name(&self) -> &'static str {
        "libvlc"
    }

    fn available(&self) -> bool {
        true
    }

    fn open(&self, path: &str) -> Result<PlayerSnapshot, PlayerError> {
        let path = Path::new(path);
        if !path.is_file() {
            return Err(PlayerError::InvalidPath(path.display().to_string()));
        }
        #[cfg(unix)]
        let raw_path = {
            use std::os::unix::ffi::OsStrExt;
            CString::new(path.as_os_str().as_bytes())
        };
        #[cfg(not(unix))]
        let raw_path = CString::new(path.to_string_lossy().as_bytes());
        let raw_path = raw_path
            .map_err(|_| PlayerError::InvalidPath("media path contains a null character".into()))?;

        let mut inner = self.inner.lock().map_err(|_| PlayerError::Poisoned)?;
        // SAFETY: instance and path are valid for this call.
        let media = unsafe { (self.api.new_media_path)(inner.instance, raw_path.as_ptr()) };
        if media.is_null() {
            return Err(PlayerError::Backend(
                self.api.last_error("libVLC could not create media"),
            ));
        }
        // SAFETY: media is live and local. Synchronous parsing resolves duration and track
        // metadata before the descriptor is handed to the media player.
        unsafe { (self.api.parse_media)(media) };
        // SAFETY: media remains live until after set_media below.
        let parsed_duration = unsafe { (self.api.media_get_duration)(media) }.max(0) as u64;
        // SAFETY: player and media are live. set_media retains the media.
        unsafe {
            (self.api.set_media)(inner.player, media);
            (self.api.release_media)(media);
        }
        inner.media_path = Some(path.to_string_lossy().into_owned());
        inner.duration_ms = parsed_duration;
        inner.error = None;
        Ok(self.snapshot_locked(&inner))
    }

    fn command(&self, command: PlayerCommand) -> Result<PlayerSnapshot, PlayerError> {
        let mut inner = self.inner.lock().map_err(|_| PlayerError::Poisoned)?;
        match command {
            PlayerCommand::Play => {
                self.play_locked(&mut inner)?;
            }
            PlayerCommand::Pause => {
                // SAFETY: player is live.
                unsafe { (self.api.set_pause)(inner.player, 1) };
            }
            PlayerCommand::TogglePlayback => {
                let state = map_state(unsafe { (self.api.get_state)(inner.player) });
                if matches!(state, PlayerState::Playing | PlayerState::Buffering) {
                    unsafe { (self.api.set_pause)(inner.player, 1) };
                } else {
                    self.play_locked(&mut inner)?;
                }
            }
            PlayerCommand::Stop => {
                // SAFETY: player is live.
                unsafe { (self.api.stop)(inner.player) };
            }
            PlayerCommand::Seek { position_ms } => {
                // SAFETY: player is live.
                let player_duration = unsafe { (self.api.get_length)(inner.player) }.max(0) as u64;
                let duration = player_duration.max(inner.duration_ms);
                let target = if duration > 0 {
                    position_ms.min(duration)
                } else {
                    position_ms
                };
                // SAFETY: player is live.
                unsafe { (self.api.set_time)(inner.player, target as c_longlong) };
            }
            PlayerCommand::SetVolume { volume } => {
                // SAFETY: player is live.
                if unsafe { (self.api.set_volume)(inner.player, volume.min(100) as c_int) } != 0 {
                    return Err(PlayerError::Backend(
                        self.api.last_error("libVLC volume change failed"),
                    ));
                }
                inner.volume = volume.min(100);
            }
            PlayerCommand::SetMuted { muted } => {
                // SAFETY: player is live.
                unsafe { (self.api.set_mute)(inner.player, i32::from(muted)) };
                inner.muted = muted;
            }
            PlayerCommand::SetRate { rate } => {
                let rate = rate.clamp(0.25, 4.0);
                // SAFETY: player is live.
                if unsafe { (self.api.set_rate)(inner.player, rate) } != 0 {
                    return Err(PlayerError::Backend(
                        self.api.last_error("libVLC playback-rate change failed"),
                    ));
                }
                inner.rate = rate;
            }
            PlayerCommand::SelectTrack { kind, id } => {
                // SAFETY: player is live.
                let result = unsafe {
                    match kind {
                        TrackKind::Audio => (self.api.audio_set_track)(inner.player, id),
                        TrackKind::Subtitle => (self.api.video_set_spu)(inner.player, id),
                    }
                };
                if result != 0 {
                    return Err(PlayerError::Backend(
                        self.api.last_error("libVLC track selection failed"),
                    ));
                }
            }
        }
        inner.error = None;
        Ok(self.snapshot_locked(&inner))
    }

    fn snapshot(&self) -> PlayerSnapshot {
        match self.inner.lock() {
            Ok(inner) => self.snapshot_locked(&inner),
            Err(_) => PlayerSnapshot::unavailable("player lock was poisoned"),
        }
    }

    fn attach_surface(&self, native_handle: usize) -> Result<(), PlayerError> {
        if native_handle == 0 {
            return Err(PlayerError::Backend(
                "native video surface handle is null".into(),
            ));
        }
        let inner = self.inner.lock().map_err(|_| PlayerError::Poisoned)?;
        #[cfg(target_os = "macos")]
        {
            // SAFETY: The caller owns a live NSView for the lifetime of the application.
            unsafe { (self.api.set_nsobject)(inner.player, native_handle as *mut c_void) };
        }
        #[cfg(target_os = "windows")]
        {
            // SAFETY: The caller owns a live child HWND for the lifetime of the application.
            unsafe { (self.api.set_hwnd)(inner.player, native_handle as *mut c_void) };
        }
        #[cfg(target_os = "linux")]
        {
            // SAFETY: The caller supplies a live X11 window identifier.
            unsafe { (self.api.set_xwindow)(inner.player, native_handle as u32) };
        }
        Ok(())
    }
}

impl Drop for LibVlcPlayer {
    fn drop(&mut self) {
        if let Ok(inner) = self.inner.get_mut() {
            // SAFETY: This is the unique final owner; release order follows libVLC docs.
            unsafe {
                (self.api.stop)(inner.player);
                (self.api.release_media_player)(inner.player);
                (self.api.release_instance)(inner.instance);
            }
            inner.player = ptr::null_mut();
            inner.instance = ptr::null_mut();
        }
    }
}

unsafe fn collect_tracks(
    head: *mut VlcTrackDescription,
    kind: TrackKind,
    selected: c_int,
    release: ReleaseTrackDescription,
) -> Vec<TrackDescriptor> {
    let mut tracks = Vec::new();
    let mut current = head;
    while !current.is_null() {
        // SAFETY: current is a node in the libVLC-owned linked list.
        let description = unsafe { &*current };
        let label = if description.id < 0 {
            match kind {
                TrackKind::Audio => "Audio off".into(),
                TrackKind::Subtitle => "Embedded subtitles off".into(),
            }
        } else if description.name.is_null() {
            match kind {
                TrackKind::Audio => format!("Audio {}", description.id),
                TrackKind::Subtitle => format!("Subtitle {}", description.id),
            }
        } else {
            let raw = unsafe { CStr::from_ptr(description.name) }
                .to_string_lossy()
                .into_owned();
            if raw.eq_ignore_ascii_case("disable") {
                match kind {
                    TrackKind::Audio => "Audio off".into(),
                    TrackKind::Subtitle => "Embedded subtitles off".into(),
                }
            } else {
                raw
            }
        };
        tracks.push(TrackDescriptor {
            id: description.id,
            kind,
            label,
            language: None,
            selected: description.id == selected,
        });
        current = description.next;
    }
    if !head.is_null() {
        // SAFETY: head was returned by libVLC and has not been released yet.
        unsafe { release(head) };
    }
    tracks
}

fn map_state(state: c_int) -> PlayerState {
    match state {
        1 => PlayerState::Opening,
        2 => PlayerState::Buffering,
        3 => PlayerState::Playing,
        4 => PlayerState::Paused,
        5 => PlayerState::Stopped,
        6 => PlayerState::Ended,
        7 => PlayerState::Error,
        _ => PlayerState::Idle,
    }
}

fn library_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("MYNA_PLAYER_LIBVLC_PATH") {
        candidates.push(PathBuf::from(path));
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(executable) = env::current_exe()
            && let Some(macos_directory) = executable.parent()
        {
            candidates.push(macos_directory.join("../Frameworks").join("libvlc.dylib"));
        }
        candidates
            .push(PathBuf::from("/Applications/VLC.app").join("Contents/MacOS/lib/libvlc.dylib"));
        candidates.push(PathBuf::from(
            "/opt/homebrew/Caskroom/vlc/3.0.21/VLC.app/Contents/MacOS/lib/libvlc.dylib",
        ));
        candidates.push(PathBuf::from("libvlc.dylib"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(executable) = env::current_exe()
            && let Some(directory) = executable.parent()
        {
            candidates.push(directory.join("vlc").join("lib").join("libvlc.dll"));
            candidates.push(
                directory
                    .join("resources")
                    .join("vlc")
                    .join("lib")
                    .join("libvlc.dll"),
            );
            candidates.push(directory.join("libvlc.dll"));
        }
        candidates.push(PathBuf::from("libvlc.dll"));
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push(PathBuf::from("libvlc.so.5"));
        candidates.push(PathBuf::from("libvlc.so"));
    }
    candidates
}

fn plugin_directory(library_path: &Path) -> Option<PathBuf> {
    if let Some(path) = env::var_os("MYNA_PLAYER_VLC_PLUGIN_PATH") {
        return Some(PathBuf::from(path));
    }
    #[cfg(target_os = "macos")]
    {
        if library_path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "Frameworks")
        {
            let contents = library_path.parent()?.parent()?;
            let bundled = contents.join("Resources").join("vlc").join("plugins");
            if bundled.is_dir() {
                return Some(bundled);
            }
        }
        let macos_directory = library_path.parent()?.parent()?;
        let candidate = macos_directory.join("plugins");
        candidate.is_dir().then_some(candidate)
    }
    #[cfg(target_os = "windows")]
    {
        let vlc_root = library_path.parent()?.parent()?;
        let plugins = vlc_root.join("plugins");
        plugins.is_dir().then_some(plugins)
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_mapping_is_explicit() {
        assert_eq!(map_state(3), PlayerState::Playing);
        assert_eq!(map_state(6), PlayerState::Ended);
        assert_eq!(map_state(99), PlayerState::Idle);
    }

    #[test]
    fn candidates_include_environment_override_first() {
        // SAFETY: This test does not run concurrently with code that reads this test-only key.
        unsafe { env::set_var("MYNA_PLAYER_LIBVLC_PATH", "/tmp/custom-libvlc") };
        assert_eq!(
            library_candidates().first(),
            Some(&PathBuf::from("/tmp/custom-libvlc"))
        );
        // SAFETY: See note above.
        unsafe { env::remove_var("MYNA_PLAYER_LIBVLC_PATH") };
    }

    #[test]
    #[ignore = "requires an installed or bundled libVLC runtime"]
    fn discovers_real_libvlc_runtime() {
        let player = LibVlcPlayer::discover().expect("libVLC should load");
        assert!(player.available());
        assert!(player.library_path().exists());
    }

    #[test]
    #[ignore = "requires libVLC and MYNA_PLAYER_TEST_MEDIA"]
    fn exercises_real_playback_controls_and_eof() {
        use std::{thread, time::Duration};

        let media = env::var("MYNA_PLAYER_TEST_MEDIA")
            .expect("MYNA_PLAYER_TEST_MEDIA must point to a short media file");
        let player = LibVlcPlayer::discover().expect("libVLC should load");
        let opened = player.open(&media).expect("media should open");
        assert_eq!(opened.media_path.as_deref(), Some(media.as_str()));

        player.command(PlayerCommand::Play).expect("play");
        let mut playing = player.snapshot();
        for _ in 0..200 {
            if playing.duration_ms > 0 && playing.state == PlayerState::Playing {
                break;
            }
            thread::sleep(Duration::from_millis(50));
            playing = player.snapshot();
        }
        assert!(
            playing.duration_ms > 0,
            "duration never resolved: {playing:?}; runtime={}",
            player.library_path().display()
        );
        assert_eq!(playing.state, PlayerState::Playing);

        player
            .command(PlayerCommand::SetVolume { volume: 37 })
            .expect("volume");
        thread::sleep(Duration::from_millis(50));
        let volume = player.snapshot();
        assert_eq!(volume.volume, 37);
        let rate = player
            .command(PlayerCommand::SetRate { rate: 1.25 })
            .expect("rate");
        assert!((rate.rate - 1.25).abs() < 0.01);

        player.command(PlayerCommand::Pause).expect("pause");
        let seek_target = playing.duration_ms / 2;
        player
            .command(PlayerCommand::Seek {
                position_ms: seek_target,
            })
            .expect("seek");
        let mut seeked = player.snapshot();
        for _ in 0..40 {
            if seeked.position_ms >= seek_target.saturating_sub(100) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
            seeked = player.snapshot();
        }
        assert!(
            seeked.position_ms >= seek_target.saturating_sub(100),
            "seek did not reach target: {seeked:?}"
        );
        player.command(PlayerCommand::Play).expect("resume");
        let mut ended = false;
        let mut last_snapshot = player.snapshot();
        for _ in 0..200 {
            let snapshot = player.snapshot();
            if snapshot.state == PlayerState::Ended {
                ended = true;
                break;
            }
            last_snapshot = snapshot;
            thread::sleep(Duration::from_millis(50));
        }
        assert!(
            ended,
            "player did not report EOF; final snapshot: {last_snapshot:?}"
        );

        player
            .command(PlayerCommand::Play)
            .expect("replay after EOF");
        let mut replayed = player.snapshot();
        for _ in 0..80 {
            if replayed.state == PlayerState::Playing
                && replayed.position_ms < playing.duration_ms / 2
            {
                break;
            }
            thread::sleep(Duration::from_millis(50));
            replayed = player.snapshot();
        }
        assert_eq!(
            replayed.state,
            PlayerState::Playing,
            "replay did not start: {replayed:?}"
        );
        assert!(
            replayed.position_ms < playing.duration_ms / 2,
            "replay did not return near the beginning: {replayed:?}"
        );
    }
}
