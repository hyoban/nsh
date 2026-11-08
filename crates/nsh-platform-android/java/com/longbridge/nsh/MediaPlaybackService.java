package com.longbridge.nsh;

import android.app.Notification;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.util.Log;

/**
 * The {@code mediaPlayback} foreground service that keeps an app playing in the background while its
 * {@link MediaSessions} plays: started with the session's notification when it plays, detached from it (the
 * notification stays, the user may swipe it away) when it pauses, stopped when it ends. It also takes the actions of
 * the notification's own buttons (before Android 13) and its dismissal, and reports them as the session's.
 *
 * <p>{@code nsh package} declares it, with {@code FOREGROUND_SERVICE} and {@code FOREGROUND_SERVICE_MEDIA_PLAYBACK},
 * for apps granted {@code mediaSession}. Where it is not declared (an embedding app), starting it fails quietly and
 * the session shows from its notification alone.
 */
public final class MediaPlaybackService extends Service {
    private static final String TAG = "nsh";
    /** The notification to start in the foreground with: the session's latest. */
    private static Notification pending;
    /** The service is in the foreground with the session's notification. */
    private static boolean foreground;

    /** Puts the service in the foreground with {@code notification}, or updates it there. UI thread. */
    static void start(Context context, Notification notification) {
        pending = notification;
        if (foreground) {
            MediaSessions.manager(context).notify(MediaSessions.NOTIFICATION_ID, notification);
            return;
        }
        if (instance != null) {
            instance.enterForeground();
            return;
        }
        try {
            Intent intent = new Intent(context, MediaPlaybackService.class);
            if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(intent);
            else context.startService(intent);
        } catch (RuntimeException e) {
            // Not declared, or not allowed to start from the background: the notification alone.
            Log.w(TAG, "media playback service: " + e);
            MediaSessions.manager(context).notify(MediaSessions.NOTIFICATION_ID, notification);
        }
    }

    /** Leaves the foreground, keeping the notification. UI thread. */
    static void detach(Context context) {
        if (instance != null && foreground) {
            if (Build.VERSION.SDK_INT >= 24) instance.stopForeground(STOP_FOREGROUND_DETACH);
            else instance.stopForeground(false);
        }
        foreground = false;
    }

    /** Stops the service. UI thread. */
    static void stop(Context context) {
        pending = null;
        if (instance != null) instance.stopSelf();
    }

    /** A pending intent that reports {@code action} as the session's. */
    static PendingIntent action(Context context, String action) {
        Intent intent = new Intent(context, MediaPlaybackService.class)
                .setAction(MediaSessions.ACTION)
                .putExtra(MediaSessions.EXTRA_ACTION, action);
        return PendingIntent.getService(context, action.hashCode(), intent,
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
    }

    private static MediaPlaybackService instance;

    @Override
    public void onCreate() {
        super.onCreate();
        instance = this;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && MediaSessions.ACTION.equals(intent.getAction())) {
            String action = intent.getStringExtra(MediaSessions.EXTRA_ACTION);
            if (action != null) MediaSessions.report(action, -1);
            // An action of a session that does not play: nothing to keep in the foreground.
            if (!foreground && pending == null) {
                stopSelf();
                return START_NOT_STICKY;
            }
            if (foreground) return START_NOT_STICKY;
        }
        enterForeground();
        return START_NOT_STICKY;
    }

    /** Enters the foreground with the session's latest notification. */
    private void enterForeground() {
        Notification n = pending;
        if (n == null) return;
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(MediaSessions.NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
            } else {
                startForeground(MediaSessions.NOTIFICATION_ID, n);
            }
            foreground = true;
        } catch (RuntimeException e) {
            // Not allowed from the background here: the notification alone.
            Log.w(TAG, "media playback service: " + e);
            MediaSessions.manager(this).notify(MediaSessions.NOTIFICATION_ID, n);
        }
    }

    @Override
    public void onDestroy() {
        foreground = false;
        instance = null;
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
