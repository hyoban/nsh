//! `nsh`'s `MediaSession`: what the app plays, in the system's media session
//! ([`crate::media_session`]), the web's Media Session API in NSH's shape.
//! Needs the `mediaSession` capability when the app declares capabilities:
//! without it every function throws `ERR_ACCESS_DENIED` (checked here, in
//! the call; the core also refuses Rust components' requests).
//!
//! The VM keeps the session the app set ([`Session`]); each setter changes
//! its part and hands the whole to the core, which sends it to the platform
//! when it changed.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use crate::{
    element::{
        Callback, JsError, JsObject, Local, MediaSessionAction, ModuleCx, PropEnum, Stop, System,
        module,
    },
    media_session::{
        MediaAction, MediaMetadata, MediaPlaybackState, MediaPosition, MediaSessionState,
    },
};

/// Whether the app may use the media session: why not when it may not.
fn granted(sys: &System) -> Result<(), JsError> {
    if sys.access.allows_media_session() {
        return Ok(());
    }
    Err(JsError::access_denied(
        "the media session is not granted: add `\"mediaSession\": true` to capabilities in nsh.json",
        "mediaSession",
        "",
    ))
}

/// The system's media session: what plays, shown where the system shows it
/// (Android's media notification and lock screen), and the user's actions
/// on it (its buttons, a headset's).
#[module]
pub(crate) struct MediaSession;

#[module]
impl MediaSession {
    /// Whether the system shows the media session here (Android; elsewhere
    /// the setters do nothing and no action comes). Rejects with
    /// `ERR_ACCESS_DENIED` when the `mediaSession` capability is not
    /// granted.
    pub(crate) async fn available(sys: System) -> Result<bool, JsError> {
        granted(&sys)?;
        Ok(crate::prelude::media_session_available().await)
    }

    /// Sets what plays (null: nothing, and the session leaves the system's
    /// UI). Throws `ERR_ACCESS_DENIED` when the `mediaSession` capability
    /// is not granted, and a `RangeError` for an empty `title`.
    pub(crate) fn set_metadata(
        metadata: Option<MediaMetadataInit>,
        sys: System,
        cx: &mut ModuleCx,
    ) -> Result<(), JsError> {
        granted(&sys)?;
        let metadata = metadata.map(MediaMetadataInit::metadata).transpose()?;
        session(&sys).update(cx, |s| s.metadata = metadata);
        Ok(())
    }

    /// Sets whether what plays is playing: `"playing"` keeps the app
    /// running in the background on Android, `"paused"` shows the session
    /// with its play button, `"none"` removes it. Throws
    /// `ERR_ACCESS_DENIED` when the `mediaSession` capability is not
    /// granted.
    pub(crate) fn set_playback_state(
        state: MediaPlaybackState,
        sys: System,
        cx: &mut ModuleCx,
    ) -> Result<(), JsError> {
        granted(&sys)?;
        session(&sys).update(cx, |s| s.playback = state);
        Ok(())
    }

    /// Sets where playback is, in seconds (null: unknown): the system runs
    /// its own clock from it at `playbackRate`, so set it again on a seek, a
    /// pause or a new track, not as it plays. Throws `ERR_ACCESS_DENIED`
    /// when the `mediaSession` capability is not granted, and a
    /// `RangeError` for a negative or not finite time or rate.
    pub(crate) fn set_position_state(
        position: Option<MediaPositionInit>,
        sys: System,
        cx: &mut ModuleCx,
    ) -> Result<(), JsError> {
        granted(&sys)?;
        let position = position.map(MediaPositionInit::position).transpose()?;
        session(&sys).update(cx, |s| s.position = position);
        Ok(())
    }

    /// Listens to `event`. `action`: the user asked an action of what
    /// plays (`seekTime`, in seconds, for `seekTo`); the app does it and
    /// sets the session's state. Returns a function that stops listening.
    /// Throws `ERR_ACCESS_DENIED` when the `mediaSession` capability is not
    /// granted.
    pub(crate) fn on(
        event: MediaSessionEventName,
        #[args(action)] listener: Callback<MediaActionInfo>,
        sys: System,
        cx: crate::element::CallCx<'_>,
    ) -> Result<Stop, JsError> {
        let MediaSessionEventName::Action = event;
        granted(&sys)?;
        let session = session(&sys);
        let id = session.next.get();
        session.next.set(id.wrapping_add(1));
        session.listeners.borrow_mut().push((id, listener));
        if !session.started.replace(true) {
            cx.start(deliver(Rc::downgrade(&session)));
        }
        let session = Rc::downgrade(&session);
        Ok(Stop::new(move || {
            if let Some(session) = session.upgrade() {
                session.listeners.borrow_mut().retain(|(i, _)| *i != id);
            }
        }))
    }
}

/// `MediaSession.setMetadata`'s metadata (the web's `MediaMetadataInit`,
/// one artwork).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
#[prop(exact)]
pub(crate) struct MediaMetadataInit {
    /// Its title.
    title: String,
    /// Its artist.
    artist: Option<String>,
    /// Its album.
    album: Option<String>,
    /// Its artwork's URL (`https:`, `data:`), loaded under the app's
    /// `network` grant.
    artwork: Option<String>,
}

impl MediaMetadataInit {
    fn metadata(self) -> Result<MediaMetadata, JsError> {
        if self.title.is_empty() {
            return Err(JsError::Range(
                "MediaSession.setMetadata: `title` must not be empty".into(),
            ));
        }
        Ok(MediaMetadata::new(self.title)
            .artist(self.artist.unwrap_or_default())
            .album(self.album.unwrap_or_default())
            .artwork(self.artwork.unwrap_or_default()))
    }
}

/// `MediaSession.setPositionState`'s position (the web's
/// `MediaPositionState`), in seconds.
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
#[prop(exact)]
pub(crate) struct MediaPositionInit {
    /// The length of what plays.
    duration: f64,
    /// Where it is (default 0).
    position: Option<f64>,
    /// How fast it plays (default 1).
    playback_rate: Option<f64>,
}

impl MediaPositionInit {
    fn position(self) -> Result<MediaPosition, JsError> {
        let position = self.position.unwrap_or(0.0);
        let rate = self.playback_rate.unwrap_or(1.0);
        if !(self.duration.is_finite()
            && self.duration >= 0.0
            && position.is_finite()
            && position >= 0.0)
            || !(rate.is_finite() && rate >= 0.0)
        {
            return Err(JsError::Range(
                "MediaSession.setPositionState: `duration`, `position` and `playbackRate` must be finite and not negative"
                    .into(),
            ));
        }
        Ok(MediaPosition {
            duration: std::time::Duration::from_secs_f64(self.duration),
            position: std::time::Duration::from_secs_f64(position.min(self.duration)),
            rate: rate as f32,
        })
    }
}

/// `MediaSession.on`'s events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PropEnum)]
pub(crate) enum MediaSessionEventName {
    /// The user asked an action.
    #[default]
    Action,
}

/// An action the user asked (the `action` event; the web's
/// `MediaSessionActionDetails`).
#[derive(Debug, Clone, Default, PartialEq, JsObject)]
pub(crate) struct MediaActionInfo {
    /// `play`, `pause`, `stop`, `previousTrack`, `nextTrack` or `seekTo`.
    action: MediaAction,
    /// For `seekTo`: where to go, in seconds.
    seek_time: Option<f64>,
}

/// The VM's session and its `action` listeners.
#[derive(Default)]
struct Session {
    state: RefCell<MediaSessionState>,
    listeners: RefCell<Vec<(u32, Callback<MediaActionInfo>)>>,
    next: Cell<u32>,
    /// The task waiting for the core's actions started; it lasts as the VM
    /// does.
    started: Cell<bool>,
}

impl Session {
    /// Changes the session and hands the whole to the core, after the
    /// call, in order with the UI.
    fn update(&self, cx: &mut ModuleCx, change: impl FnOnce(&mut MediaSessionState)) {
        let state = {
            let mut state = self.state.borrow_mut();
            change(&mut state);
            state.clone()
        };
        cx.act(crate::element::Action::MediaSession(
            MediaSessionAction::Update(state),
        ));
    }
}

impl Local for Session {
    fn close(&self) {
        self.listeners.borrow_mut().clear();
    }
}

fn session(sys: &System) -> Rc<Session> {
    sys.locals.get::<Session>()
}

/// Posts the core's actions to the listeners, while the VM lasts.
async fn deliver(session: Weak<Session>) {
    loop {
        let details = crate::prelude::next_media_action().await;
        let Some(session) = session.upgrade() else {
            return;
        };
        let info = MediaActionInfo {
            action: details.action,
            seek_time: details.seek_time.map(|t| t.as_secs_f64()),
        };
        for (_, listener) in &*session.listeners.borrow() {
            listener.post(info.clone());
        }
    }
}
