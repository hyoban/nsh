//! The system's media session (`nsh`'s `MediaSession`): what the app plays,
//! shown where the system shows what plays (Android's media notification and
//! lock screen), the system's transport controls sent back to the app (the
//! notification's buttons, headset and Bluetooth buttons), and playback kept
//! going while the app is in the background (Android's `mediaPlayback`
//! foreground service). Its names are the web's Media Session API's.
//!
//! The app tells the core what plays ([`crate::element::update_media_session`]);
//! the core keeps the whole session and sends it to the platform as one
//! [`MediaSessionCommand::Update`] on the root
//! ([`Command::MediaSession`](crate::Command::MediaSession)) each time it
//! changes. The platform reports the user's actions as the root's
//! [`EventKind::MediaSession`](crate::EventKind::MediaSession)
//! ([`MediaSessionEvent`] gives its payload's fields) once the core sent
//! [`MediaSessionCommand::Listen`]; the core keeps them until the app's
//! listener takes them. Where
//! [`Environment::media_session`](crate::platform::Environment::media_session)
//! is false nothing is sent and no action comes.
//!
//! The app plays the sound itself (with `AudioPlayer`, or its own engine):
//! the session only says what plays and hears what the user asks.

use std::time::Duration;

use crate::element::PropEnum;

/// Whether what the session shows is playing (the web's
/// `MediaSessionPlaybackState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PropEnum)]
pub enum MediaPlaybackState {
    /// Nothing plays: the system shows no session (and Android lets the app
    /// go from the foreground).
    #[default]
    None,
    /// Paused: the session shows, the play button in it.
    Paused,
    /// Playing: the session shows; on Android the app keeps running in the
    /// background.
    Playing,
}

impl MediaPlaybackState {
    /// Its name in the platforms' commands.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Paused => "paused",
            Self::Playing => "playing",
        }
    }
}

/// What plays (the web's `MediaMetadata`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MediaMetadata {
    /// Its title.
    pub title: String,
    /// Its artist, `""` for none.
    pub artist: String,
    /// Its album, `""` for none.
    pub album: String,
    /// Its artwork's URL (`https:`, `data:`), `""` for none. The platform
    /// loads it as an image, under the app's `network` grant.
    pub artwork: String,
}

impl MediaMetadata {
    /// What plays, titled `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Its artist.
    pub fn artist(mut self, artist: impl Into<String>) -> Self {
        self.artist = artist.into();
        self
    }

    /// Its album.
    pub fn album(mut self, album: impl Into<String>) -> Self {
        self.album = album.into();
        self
    }

    /// Its artwork's URL.
    pub fn artwork(mut self, url: impl Into<String>) -> Self {
        self.artwork = url.into();
        self
    }
}

/// Where playback is (the web's `MediaPositionState`): the system runs its
/// own clock from it at `rate`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MediaPosition {
    /// The length of what plays.
    pub duration: Duration,
    /// Where it is.
    pub position: Duration,
    /// How fast it plays (1: normal).
    pub rate: f32,
}

/// The whole session as the platform shows it.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct MediaSessionState {
    /// What plays; `None`: nothing to show.
    pub metadata: Option<MediaMetadata>,
    /// Whether it plays.
    pub playback: MediaPlaybackState,
    /// Where it is; `None`: unknown (no position in the system's UI).
    pub position: Option<MediaPosition>,
}

/// An action the user asks of what plays (the web's
/// `MediaSessionAction`, in NSH's camel case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PropEnum)]
pub enum MediaAction {
    /// Play.
    #[default]
    Play,
    /// Pause.
    Pause,
    /// Stop: the session ends.
    Stop,
    /// The previous track.
    PreviousTrack,
    /// The next track.
    NextTrack,
    /// Go to [`MediaActionDetails::seek_time`].
    SeekTo,
}

impl MediaAction {
    /// Its name in the platforms' events.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Stop => "stop",
            Self::PreviousTrack => "previousTrack",
            Self::NextTrack => "nextTrack",
            Self::SeekTo => "seekTo",
        }
    }

    /// The action named `name` ([`Self::as_str`]).
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Play,
            Self::Pause,
            Self::Stop,
            Self::PreviousTrack,
            Self::NextTrack,
            Self::SeekTo,
        ]
        .into_iter()
        .find(|a| a.as_str() == name)
    }
}

/// An action the user asked, with its details.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaActionDetails {
    /// The action.
    pub action: MediaAction,
    /// For [`MediaAction::SeekTo`]: where to go.
    pub seek_time: Option<Duration>,
}

/// What the core asks of the platform (on the root,
/// [`crate::Command::MediaSession`]). Sent only where
/// [`Environment::media_session`](crate::platform::Environment::media_session)
/// holds.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MediaSessionCommand {
    /// Show the session as it is now: what plays, whether it plays, where.
    /// [`MediaPlaybackState::None`] removes it from the system's UI.
    Update(Box<MediaSessionState>),
    /// Report the user's actions as [`EventKind::MediaSession`](crate::EventKind::MediaSession).
    Listen,
}

/// The fields of a platform's [`EventKind::MediaSession`](crate::EventKind::MediaSession)
/// payload (an [`EventPayload::Map`](crate::EventPayload::Map)).
#[non_exhaustive]
pub struct MediaSessionEvent;

impl MediaSessionEvent {
    /// The action's name ([`MediaAction::as_str`]).
    pub const ACTION: &'static str = "action";
    /// For `seekTo`: where to go, in milliseconds (a number).
    pub const SEEK_TIME: &'static str = "seekTime";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_names_round_trip() {
        for action in [
            MediaAction::Play,
            MediaAction::Pause,
            MediaAction::Stop,
            MediaAction::PreviousTrack,
            MediaAction::NextTrack,
            MediaAction::SeekTo,
        ] {
            assert_eq!(MediaAction::from_name(action.as_str()), Some(action));
        }
        assert_eq!(MediaAction::from_name("skipad"), None);
    }

    // Android reports the event by its number (`Native.MEDIA_SESSION`).
    #[test]
    fn the_event_keeps_its_number() {
        assert_eq!(crate::EventKind::MediaSession as u8, 76);
    }
}
