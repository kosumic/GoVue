package io.github.kaminarios.govue.shell;

import android.graphics.Color;
import android.graphics.Matrix;
import android.os.Handler;
import android.os.Looper;
import android.view.TextureView;
import android.widget.FrameLayout;

import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.Format;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MimeTypes;
import androidx.media3.common.Player;
import androidx.media3.common.Timeline;
import androidx.media3.common.TrackSelectionOverride;
import androidx.media3.common.Tracks;
import androidx.media3.common.VideoSize;
import androidx.media3.common.util.Log;
import androidx.media3.common.util.UnstableApi;
import androidx.media3.datasource.DefaultDataSource;
import androidx.media3.datasource.DefaultHttpDataSource;
import androidx.media3.exoplayer.DefaultLoadControl;
import androidx.media3.exoplayer.DefaultRenderersFactory;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.exoplayer.analytics.AnalyticsListener;
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Media3 decoding and presentation for the Rust-owned GoVue player. */
@UnstableApi
public final class VideoPlayer implements NshComponent {
    private final NshComponentContext context;
    private final FrameLayout container;
    private final TextureView texture;
    private final ExoPlayer player;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            if (!hasSource || destroyed) return;
            emitStatus();
            main.postDelayed(this, 250);
        }
    };
    private long generation;
    private String mediaId;
    private String fit = "contain";
    private boolean hasSource;
    private boolean firstFrame;
    private boolean videoEnabled = true;
    private boolean destroyed;
    private int startupHeight;

    public static void register(NshView view) {
        view.registerComponent("govue/video-player", VideoPlayer::new);
    }

    private VideoPlayer(NshComponentContext context) {
        this.context = context;
        // Media3 errors can include media addresses from the underlying loader.
        Log.setLogLevel(Log.LOG_LEVEL_OFF);
        container = new FrameLayout(context.getContext());
        container.setBackgroundColor(Color.BLACK);
        texture = new TextureView(context.getContext());
        texture.setAlpha(0);
        container.addView(texture, new FrameLayout.LayoutParams(-1, -1));
        texture.addOnLayoutChangeListener((view, left, top, right, bottom, oldLeft, oldTop, oldRight, oldBottom) -> resize());

        DefaultRenderersFactory renderers = new DefaultRenderersFactory(context.getContext())
                .forceEnableMediaCodecAsynchronousQueueing().setEnableDecoderFallback(true);
        int buffer = DefaultLoadControl.DEFAULT_MAX_BUFFER_MS;
        player = new ExoPlayer.Builder(context.getContext(), renderers)
                .setLoadControl(new DefaultLoadControl.Builder()
                        .setBufferDurationsMs(buffer, buffer, 250, 250).build())
                .setAudioAttributes(new AudioAttributes.Builder().setUsage(C.USAGE_MEDIA)
                        .setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(), true)
                .setHandleAudioBecomingNoisy(true)
                .build();
        player.setVideoTextureView(texture);
        player.addListener(new Player.Listener() {
            @Override
            public void onEvents(Player owner, Player.Events events) {
                if (!isCurrent()) return;
                if (events.contains(Player.EVENT_VIDEO_SIZE_CHANGED)) resize();
                if (events.contains(Player.EVENT_TRACKS_CHANGED)) emitTracks();
                emitStatus();
            }
        });
        player.addAnalyticsListener(new AnalyticsListener() {
            @Override
            public void onRenderedFirstFrame(EventTime event, Object output, long renderTimeMs) {
                if (!isCurrent() || !videoEnabled || event.timeline.isEmpty()
                        || event.windowIndex < 0 || event.windowIndex >= event.timeline.getWindowCount()
                        || !mediaId.equals(event.timeline.getWindow(event.windowIndex, new Timeline.Window()).mediaItem.mediaId)) {
                    return;
                }
                texture.setAlpha(1);
                if (firstFrame) return;
                firstFrame = true;
                if (startupHeight > 0) {
                    startupHeight = 0;
                    player.setTrackSelectionParameters(player.getTrackSelectionParameters().buildUpon()
                            .setMaxVideoSize(Integer.MAX_VALUE, Integer.MAX_VALUE).build());
                }
                emit("firstFrame", Collections.emptyMap());
            }
        });
    }

    @Override
    public FrameLayout view() {
        return container;
    }

    @Override
    public void setProp(String name, Object value) {
        if (destroyed) return;
        if (name.equals("fit") && value instanceof String mode) {
            fit = mode;
            resize();
        }
        if (name.equals("source") && value instanceof Map<?, ?> load
                && load.get("generation") instanceof Number revision && revision.longValue() > generation) {
            replace(revision.longValue(), load);
        }
    }

    private void replace(long revision, Map<?, ?> load) {
        generation = revision;
        hasSource = false;
        firstFrame = false;
        texture.setAlpha(0);
        main.removeCallbacks(tick);
        player.stop();
        player.clearMediaItems();
        if (load.get("source") instanceof Map<?, ?> source && source.get("uri") instanceof String uri) {
            try {
                open(load, source, uri);
            } catch (RuntimeException failure) {
                hasSource = false;
                player.stop();
                player.clearMediaItems();
                emitStatus("InvalidSource");
            }
        }
        emit("applied", Collections.emptyMap());
        if (hasSource) main.post(tick);
        else if (load.get("source") == null) emitStatus();
    }

    private void open(Map<?, ?> load, Map<?, ?> source, String uri) {
        mediaId = "govue-media:" + generation;
        MediaItem.Builder item = new MediaItem.Builder().setUri(uri).setMediaId(mediaId);
        if ("dash".equals(source.get("contentType"))) item.setMimeType(MimeTypes.APPLICATION_MPD);
        if ("hls".equals(source.get("contentType"))) item.setMimeType(MimeTypes.APPLICATION_M3U8);

        Map<String, String> headers = new LinkedHashMap<>();
        if (source.get("headers") instanceof Map<?, ?> fields) {
            for (Map.Entry<?, ?> entry : fields.entrySet()) {
                if (entry.getKey() instanceof String name && entry.getValue() instanceof String value) {
                    headers.put(name, value);
                }
            }
        }
        DefaultHttpDataSource.Factory http = new DefaultHttpDataSource.Factory()
                .setAllowCrossProtocolRedirects(false).setDefaultRequestProperties(headers);
        DefaultMediaSourceFactory factory = new DefaultMediaSourceFactory(
                new DefaultDataSource.Factory(context.getContext().getApplicationContext(), http));

        startupHeight = source.get("startupHeight") instanceof Number height && height.intValue() > 0
                ? height.intValue() : 0;
        player.setTrackSelectionParameters(player.getTrackSelectionParameters().buildUpon()
                .clearOverridesOfType(C.TRACK_TYPE_VIDEO)
                .setMaxVideoSize(Integer.MAX_VALUE, startupHeight > 0 ? startupHeight : Integer.MAX_VALUE).build());
        player.setPlaybackSpeed(load.get("rate") instanceof Number speed ? speed.floatValue() : 1);
        long position = load.get("position") instanceof Number seconds ? millis(seconds) : C.TIME_UNSET;
        player.setMediaSource(factory.createMediaSource(item.build()), position);
        hasSource = true;
        player.setPlayWhenReady(Boolean.TRUE.equals(load.get("playing")));
        player.prepare();
    }

    private boolean isCurrent() {
        MediaItem current = player.getCurrentMediaItem();
        return hasSource && !destroyed && current != null && mediaId.equals(current.mediaId);
    }

    @Override
    public void command(String name, Object value) {
        if (!isCurrent() || !(value instanceof Map<?, ?> input)
                || !(input.get("generation") instanceof Number revision)
                || revision.longValue() != generation) return;
        Object option = input.get("value");
        switch (name) {
            case "play" -> player.play();
            case "pause" -> player.pause();
            case "seek" -> {
                if (option instanceof Number seconds) player.seekTo(millis(seconds));
            }
            case "rate" -> {
                if (option instanceof Number speed) player.setPlaybackSpeed(speed.floatValue());
            }
            case "volume" -> {
                if (option instanceof Number volume) player.setVolume(volume.floatValue());
            }
            case "videoEnabled" -> {
                videoEnabled = Boolean.TRUE.equals(option);
                if (!videoEnabled) texture.setAlpha(0);
                player.setTrackSelectionParameters(player.getTrackSelectionParameters().buildUpon()
                        .setTrackTypeDisabled(C.TRACK_TYPE_VIDEO, !videoEnabled).build());
            }
            case "videoTrack" -> selectTrack(option instanceof String id ? id : null);
            default -> { }
        }
    }

    private void selectTrack(String id) {
        var selection = player.getTrackSelectionParameters().buildUpon().clearOverridesOfType(C.TRACK_TYPE_VIDEO);
        if (id == null) {
            player.setTrackSelectionParameters(selection.build());
            return;
        }
        for (Tracks.Group group : player.getCurrentTracks().getGroups()) {
            if (group.getType() != C.TRACK_TYPE_VIDEO) continue;
            for (int index = 0; index < group.length; index++) {
                if (group.isTrackSupported(index) && id.equals(group.getTrackFormat(index).id)) {
                    player.setTrackSelectionParameters(selection.setOverrideForType(
                            new TrackSelectionOverride(group.getMediaTrackGroup(), index)).build());
                    return;
                }
            }
        }
    }

    private void resize() {
        VideoSize size = player.getVideoSize();
        int width = texture.getWidth(), height = texture.getHeight();
        Matrix transform = new Matrix();
        if (width > 0 && height > 0 && size.width > 0 && size.height > 0) {
            float ratio = (size.width * size.pixelWidthHeightRatio / size.height) / ((float) width / height);
            if (ratio > 0 && Float.isFinite(ratio)) {
                float x = 1, y = 1;
                if ("cover".equals(fit)) {
                    if (ratio > 1) x = ratio;
                    else y = 1 / ratio;
                } else if ("contain".equals(fit)) {
                    if (ratio > 1) y = 1 / ratio;
                    else x = ratio;
                }
                transform.setScale(x, y, width / 2f, height / 2f);
            }
        }
        texture.setTransform(transform);
    }

    private static long millis(Number seconds) {
        return Math.max(0, Math.round(seconds.doubleValue() * 1000));
    }

    private void emitStatus() {
        emitStatus(player.getPlayerError() == null ? null : player.getPlayerError().getErrorCodeName());
    }

    private void emitStatus(String error) {
        String state = error != null ? "error" : switch (player.getPlaybackState()) {
            case Player.STATE_BUFFERING -> "buffering";
            case Player.STATE_READY -> player.isPlaying() ? "playing" : "paused";
            case Player.STATE_ENDED -> "ended";
            default -> "idle";
        };
        Map<String, Object> status = new LinkedHashMap<>();
        status.put("state", state);
        status.put("position", Math.max(0, player.getCurrentPosition()) / 1000.0);
        status.put("bufferedPosition", Math.max(0, player.getBufferedPosition()) / 1000.0);
        status.put("duration", player.getDuration() >= 0 ? player.getDuration() / 1000.0 : null);
        status.put("playing", player.isPlaying());
        status.put("rate", player.getPlaybackParameters().speed);
        status.put("errorCode", error);
        emit("status", Collections.singletonMap("status", status));
    }

    private void emitTracks() {
        List<Map<String, Object>> descriptions = new ArrayList<>();
        for (Tracks.Group group : player.getCurrentTracks().getGroups()) {
            if (group.getType() != C.TRACK_TYPE_VIDEO) continue;
            for (int index = 0; index < group.length; index++) {
                Format track = group.getTrackFormat(index);
                if (!group.isTrackSupported(index) || track.id == null) continue;
                Map<String, Object> fields = new LinkedHashMap<>();
                fields.put("id", track.id);
                fields.put("width", Math.max(0, track.width));
                fields.put("height", Math.max(0, track.height));
                fields.put("frameRate", Float.isFinite(track.frameRate) ? Math.max(0, track.frameRate) : 0);
                fields.put("bitrate", Math.max(0, track.bitrate));
                descriptions.add(fields);
            }
        }
        emit("tracks", Collections.singletonMap("tracks", descriptions));
    }

    private void emit(String kind, Map<String, ?> fields) {
        Map<String, Object> event = new LinkedHashMap<>(fields);
        event.put("generation", generation);
        event.put("kind", kind);
        context.emit("playback", event);
    }

    @Override
    public void destroy() {
        destroyed = true;
        main.removeCallbacks(tick);
        player.clearVideoTextureView(texture);
        player.release();
    }
}
