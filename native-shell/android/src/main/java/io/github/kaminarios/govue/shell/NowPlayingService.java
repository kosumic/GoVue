package io.github.kaminarios.govue.shell;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.media.MediaMetadata;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.os.IBinder;
import android.os.SystemClock;

import java.util.Objects;

/** Platform notification and controller endpoint for the one mounted GoVue player. */
public final class NowPlayingService extends Service {
    private static final String CHANNEL = "govue-playback";
    private static final int NOTIFICATION = 1048;
    static NowPlayingService running;

    private MediaSession session;
    private NowPlaying.Snapshot last;
    private PendingIntent open;
    private boolean foreground;

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public void onCreate() {
        super.onCreate();
        running = this;
        getSystemService(NotificationManager.class).createNotificationChannel(
                new NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW));
        session = new MediaSession(this, "GoVue");
        Intent launch = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (launch != null) {
            open = PendingIntent.getActivity(this, 0, launch, PendingIntent.FLAG_IMMUTABLE);
            session.setSessionActivity(open);
        }
        session.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() { control("play", 0); }
            @Override public void onPause() { control("pause", 0); }
            @Override public void onStop() { control("pause", 0); }
            @Override public void onSeekTo(long position) { control("seek", position); }
            @Override public void onSkipToPrevious() { control("previous", 0); }
            @Override public void onSkipToNext() { control("next", 0); }
        });
        session.setActive(true);
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        var owner = NowPlaying.selected();
        if (owner == null || owner.current() == null) {
            stopSelf(startId);
            return START_NOT_STICKY;
        }
        owner.serviceStarted();
        update(owner.current());
        if (intent != null && intent.getAction() != null
                && intent.getLongExtra("generation", 0) == owner.current().generation()) {
            switch (intent.getAction()) {
                case "play", "pause", "previous", "next" -> control(intent.getAction(), 0);
                default -> { }
            }
        }
        return START_NOT_STICKY;
    }

    void update(NowPlaying.Snapshot snapshot) {
        boolean metadataChanged = last == null || last.generation() != snapshot.generation()
                || last.duration() != snapshot.duration() || !Objects.equals(last.title(), snapshot.title())
                || !Objects.equals(last.author(), snapshot.author());
        if (metadataChanged) {
            session.setMetadata(new MediaMetadata.Builder()
                    .putString(MediaMetadata.METADATA_KEY_MEDIA_ID, snapshot.video())
                    .putString(MediaMetadata.METADATA_KEY_TITLE, snapshot.title())
                    .putString(MediaMetadata.METADATA_KEY_ARTIST, snapshot.author())
                    .putLong(MediaMetadata.METADATA_KEY_DURATION, snapshot.duration())
                    .build());
        }
        long actions = PlaybackState.ACTION_PLAY_PAUSE | PlaybackState.ACTION_PLAY | PlaybackState.ACTION_PAUSE
                | PlaybackState.ACTION_STOP;
        if (snapshot.duration() > 0) actions |= PlaybackState.ACTION_SEEK_TO;
        if (snapshot.previous()) actions |= PlaybackState.ACTION_SKIP_TO_PREVIOUS;
        if (snapshot.next()) actions |= PlaybackState.ACTION_SKIP_TO_NEXT;
        int state = snapshot.buffering() ? PlaybackState.STATE_BUFFERING
                : snapshot.playing() ? PlaybackState.STATE_PLAYING : PlaybackState.STATE_PAUSED;
        session.setPlaybackState(new PlaybackState.Builder().setActions(actions)
                .setState(state, snapshot.position(), snapshot.playing() ? snapshot.rate() : 0,
                        SystemClock.elapsedRealtime()).build());
        if (metadataChanged || last.playing() != snapshot.playing()
                || last.previous() != snapshot.previous() || last.next() != snapshot.next()) {
            Notification notice = notification(snapshot);
            if (!foreground) {
                startForeground(NOTIFICATION, notice);
                foreground = true;
            } else {
                getSystemService(NotificationManager.class).notify(NOTIFICATION, notice);
            }
        }
        last = snapshot;
    }

    private Notification notification(NowPlaying.Snapshot snapshot) {
        var style = new Notification.MediaStyle().setMediaSession(session.getSessionToken());
        var builder = new Notification.Builder(this, CHANNEL)
                .setContentTitle(snapshot.title()).setContentText(snapshot.author())
                .setSmallIcon(android.R.drawable.ic_media_play)
                .setContentIntent(open).setOnlyAlertOnce(true)
                .setVisibility(Notification.VISIBILITY_PUBLIC)
                .setShowWhen(false).setCategory(Notification.CATEGORY_TRANSPORT);
        if (snapshot.previous()) {
            builder.addAction(button(snapshot, "previous", android.R.drawable.ic_media_previous, "Previous"));
        }
        builder.addAction(snapshot.playing()
                ? button(snapshot, "pause", android.R.drawable.ic_media_pause, "Pause")
                : button(snapshot, "play", android.R.drawable.ic_media_play, "Play"));
        if (snapshot.next()) {
            builder.addAction(button(snapshot, "next", android.R.drawable.ic_media_next, "Next"));
        }
        if (snapshot.previous() && snapshot.next()) style.setShowActionsInCompactView(0, 1, 2);
        else if (snapshot.previous() || snapshot.next()) style.setShowActionsInCompactView(0, 1);
        else style.setShowActionsInCompactView(0);
        return builder.setStyle(style).build();
    }

    private Notification.Action button(NowPlaying.Snapshot snapshot, String action, int icon, String title) {
        Intent request = new Intent(this, NowPlayingService.class).setAction(action)
                .putExtra("generation", snapshot.generation());
        var pending = PendingIntent.getService(this, 0, request,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Action.Builder(android.graphics.drawable.Icon.createWithResource(this, icon),
                title, pending).build();
    }

    private void control(String action, long position) {
        var owner = NowPlaying.selected();
        if (owner != null) owner.action(action, position);
    }

    @Override
    public void onDestroy() {
        if (running == this) running = null;
        session.setActive(false);
        session.release();
        stopForeground(STOP_FOREGROUND_REMOVE);
        super.onDestroy();
    }
}
