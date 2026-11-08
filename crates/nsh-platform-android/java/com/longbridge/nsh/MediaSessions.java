package com.longbridge.nsh;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.util.Log;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.ByteBuffer;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * {@code MediaSession} of {@code nsh}: what the app plays, in the system's media session. One {@link MediaSession} for
 * the process, shown as a media-style notification (and on the lock screen) while the app plays or is paused, and kept
 * playing in the background by {@link MediaPlaybackService}, a {@code mediaPlayback} foreground service, while it plays.
 * The user's actions (the notification's buttons, a headset's, the lock screen's) are reported as
 * {@code Native.MEDIA_SESSION} events on the root of the mounter that listens.
 *
 * <p>The core sends the whole session each time it changes ({@code encode.rs}'s {@code Command::MediaSession}):
 * metadata (title, artist, album, artwork URL), the playback state (none, paused, playing) and the position. The
 * artwork loads off the UI thread and is kept for its URL. {@code nsh package} declares the service and its
 * permissions for apps granted {@code mediaSession}; without them (an embedding app that does not declare the
 * service) the session still shows, from the notification alone, and the app is not kept in the background.
 *
 * <p>Android 13 and later draw the notification's buttons from the session's actions; earlier versions get them as
 * the notification's actions, which go to the service.
 */
final class MediaSessions {
    private static final String TAG = "nsh";
    private static final String CHANNEL = "com.longbridge.nsh.media";
    static final int NOTIFICATION_ID = 0x4e5348;
    static final String ACTION = "com.longbridge.nsh.MEDIA_ACTION";
    static final String EXTRA_ACTION = "action";

    private static final int NONE = 0;
    private static final int PAUSED = 1;
    private static final int PLAYING = 2;

    private static MediaSession session;
    /** The mounter that listens to actions. UI thread. */
    private static Mounter listener;
    private static Context app;

    private static String title = "";
    private static String artist = "";
    private static String album = "";
    private static String artworkUrl = "";
    private static Bitmap artwork;
    private static int state = NONE;
    private static boolean hasPosition;
    private static float durationMs;
    private static float positionMs;
    private static float rate = 1f;

    private static final Handler handler = new Handler(Looper.getMainLooper());
    private static final ExecutorService loader = Executors.newSingleThreadExecutor();

    private MediaSessions() {}

    /** A command, as {@code encode.rs} wrote it. UI thread. */
    static void command(Mounter mounter, ByteBuffer b, Object[] objects) {
        app = mounter.context.getApplicationContext();
        switch (b.getInt()) {
            case 0: {
                if (b.getInt() != 0) {
                    title = (String) objects[b.getInt()];
                    artist = (String) objects[b.getInt()];
                    album = (String) objects[b.getInt()];
                    String url = (String) objects[b.getInt()];
                    if (!url.equals(artworkUrl)) {
                        artworkUrl = url;
                        artwork = null;
                        loadArtwork(url);
                    }
                } else {
                    title = artist = album = artworkUrl = "";
                    artwork = null;
                }
                state = b.getInt();
                hasPosition = b.getInt() != 0;
                if (hasPosition) {
                    durationMs = b.getFloat();
                    positionMs = b.getFloat();
                    rate = b.getFloat();
                }
                apply();
                return;
            }
            case 1:
                listener = mounter;
                return;
            default:
        }
    }

    /** The mounter goes: it hears no more actions. The session stays as the app left it. UI thread. */
    static void forget(Mounter mounter) {
        if (listener == mounter) listener = null;
    }

    /** Reports {@code action} (a {@code MediaAction} name) to the app. UI thread. */
    static void report(String action, double seekTimeMs) {
        Mounter m = listener;
        if (m == null) return;
        if (seekTimeMs >= 0) {
            m.eventMap(1, Native.MEDIA_SESSION, new String[] {"action", "seekTime"}, new Object[] {action, seekTimeMs});
        } else {
            m.eventMap(1, Native.MEDIA_SESSION, new String[] {"action"}, new Object[] {action});
        }
    }

    /** The session, made with the first command. UI thread. */
    static MediaSession session() {
        if (session == null && app != null) {
            session = new MediaSession(app, "nsh");
            session.setCallback(new MediaSession.Callback() {
                @Override public void onPlay() { report("play", -1); }
                @Override public void onPause() { report("pause", -1); }
                @Override public void onStop() { report("stop", -1); }
                @Override public void onSkipToNext() { report("nextTrack", -1); }
                @Override public void onSkipToPrevious() { report("previousTrack", -1); }
                @Override public void onSeekTo(long pos) { report("seekTo", pos); }
            }, handler);
        }
        return session;
    }

    /** Shows the session as it is now: the system's session, the notification and the service. UI thread. */
    private static void apply() {
        MediaSession s = session();
        if (s == null) return;
        if (state == NONE || title.isEmpty()) {
            s.setActive(false);
            MediaPlaybackService.stop(app);
            manager(app).cancel(NOTIFICATION_ID);
            return;
        }
        MediaMetadata.Builder meta = new MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, title)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, artist)
                .putString(MediaMetadata.METADATA_KEY_ALBUM, album);
        if (hasPosition) meta.putLong(MediaMetadata.METADATA_KEY_DURATION, (long) durationMs);
        if (artwork != null) meta.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, artwork);
        s.setMetadata(meta.build());
        long actions = PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE | PlaybackState.ACTION_PLAY_PAUSE
                | PlaybackState.ACTION_STOP | PlaybackState.ACTION_SKIP_TO_NEXT | PlaybackState.ACTION_SKIP_TO_PREVIOUS;
        if (hasPosition) actions |= PlaybackState.ACTION_SEEK_TO;
        s.setPlaybackState(new PlaybackState.Builder()
                .setActions(actions)
                .setState(state == PLAYING ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED,
                        hasPosition ? (long) positionMs : PlaybackState.PLAYBACK_POSITION_UNKNOWN,
                        state == PLAYING ? rate : 0f, SystemClock.elapsedRealtime())
                .build());
        s.setActive(true);
        Notification notification = notification(app);
        if (state == PLAYING) {
            MediaPlaybackService.start(app, notification);
        } else {
            // Paused: the app may go, and the user may swipe the session away.
            MediaPlaybackService.detach(app);
            manager(app).notify(NOTIFICATION_ID, notification);
        }
    }

    /** The session's notification as it is now. UI thread. */
    static Notification notification(Context context) {
        NotificationManager manager = manager(context);
        if (Build.VERSION.SDK_INT >= 26 && manager.getNotificationChannel(CHANNEL) == null) {
            NotificationChannel channel = new NotificationChannel(CHANNEL, "Media", NotificationManager.IMPORTANCE_LOW);
            channel.setShowBadge(false);
            manager.createNotificationChannel(channel);
        }
        Notification.Builder n = Build.VERSION.SDK_INT >= 26
                ? new Notification.Builder(context, CHANNEL)
                : new Notification.Builder(context);
        int icon = context.getApplicationInfo().icon;
        n.setSmallIcon(icon != 0 ? icon : android.R.drawable.ic_media_play)
                .setContentTitle(title)
                .setContentText(artist.isEmpty() ? album : artist)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setOngoing(state == PLAYING)
                .setShowWhen(false);
        if (artwork != null) n.setLargeIcon(artwork);
        Intent launch = context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
        if (launch != null) {
            launch.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_REORDER_TO_FRONT);
            n.setContentIntent(PendingIntent.getActivity(context, 0, launch, PendingIntent.FLAG_IMMUTABLE));
        }
        n.setDeleteIntent(MediaPlaybackService.action(context, "stop"));
        // Before Android 13 the buttons are the notification's own; later the system draws them from the session.
        n.addAction(new Notification.Action.Builder(android.R.drawable.ic_media_previous, "Previous",
                MediaPlaybackService.action(context, "previousTrack")).build());
        n.addAction(state == PLAYING
                ? new Notification.Action.Builder(android.R.drawable.ic_media_pause, "Pause",
                        MediaPlaybackService.action(context, "pause")).build()
                : new Notification.Action.Builder(android.R.drawable.ic_media_play, "Play",
                        MediaPlaybackService.action(context, "play")).build());
        n.addAction(new Notification.Action.Builder(android.R.drawable.ic_media_next, "Next",
                MediaPlaybackService.action(context, "nextTrack")).build());
        MediaSession s = session();
        n.setStyle(new Notification.MediaStyle()
                .setMediaSession(s != null ? s.getSessionToken() : null)
                .setShowActionsInCompactView(0, 1, 2));
        return n.build();
    }

    static NotificationManager manager(Context context) {
        return (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
    }

    /** Loads the artwork at {@code url} off the UI thread, then shows it if it is still the session's. */
    private static void loadArtwork(String url) {
        if (url.isEmpty()) return;
        loader.execute(() -> {
            Bitmap bitmap = null;
            try {
                if (url.startsWith("data:")) {
                    int comma = url.indexOf(',');
                    byte[] bytes = android.util.Base64.decode(url.substring(comma + 1), android.util.Base64.DEFAULT);
                    bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.length);
                } else {
                    HttpURLConnection c = (HttpURLConnection) new URL(url).openConnection();
                    c.setConnectTimeout(10000);
                    c.setReadTimeout(10000);
                    try (InputStream in = c.getInputStream()) {
                        bitmap = BitmapFactory.decodeStream(in);
                    } finally {
                        c.disconnect();
                    }
                }
            } catch (Exception e) {
                Log.w(TAG, "media session artwork: " + e);
            }
            Bitmap loaded = bitmap;
            handler.post(() -> {
                if (loaded == null || !url.equals(artworkUrl)) return;
                artwork = loaded;
                apply();
            });
        });
    }
}
