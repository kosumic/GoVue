package io.github.kaminarios.govue.shell;

import android.graphics.Color;
import android.graphics.Matrix;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.os.Trace;
import android.view.TextureView;
import android.widget.FrameLayout;

import androidx.media3.common.AudioAttributes;
import androidx.media3.common.C;
import androidx.media3.common.Format;
import androidx.media3.common.MediaItem;
import androidx.media3.common.MimeTypes;
import androidx.media3.common.Player;
import androidx.media3.common.Timeline;
import androidx.media3.common.TrackGroup;
import androidx.media3.common.TrackSelectionOverride;
import androidx.media3.common.Tracks;
import androidx.media3.common.VideoSize;
import androidx.media3.common.util.Clock;
import androidx.media3.common.util.Log;
import androidx.media3.common.util.UnstableApi;
import androidx.media3.datasource.DefaultDataSource;
import androidx.media3.datasource.DefaultHttpDataSource;
import androidx.media3.exoplayer.DefaultLoadControl;
import androidx.media3.exoplayer.DefaultRenderersFactory;
import androidx.media3.exoplayer.DecoderCounters;
import androidx.media3.exoplayer.DecoderReuseEvaluation;
import androidx.media3.exoplayer.ExoPlayer;
import androidx.media3.exoplayer.analytics.AnalyticsListener;
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory;
import androidx.media3.exoplayer.source.MediaSource;
import androidx.media3.exoplayer.source.chunk.MediaChunk;
import androidx.media3.exoplayer.source.chunk.MediaChunkIterator;
import androidx.media3.exoplayer.source.preload.DefaultPreloadManager;
import androidx.media3.exoplayer.source.preload.PreloadException;
import androidx.media3.exoplayer.source.preload.PreloadManagerListener;
import androidx.media3.exoplayer.trackselection.AdaptiveTrackSelection;
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector;
import androidx.media3.exoplayer.upstream.BandwidthMeter;

import com.google.common.collect.ImmutableList;
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
    private record StagedMedia(MediaItem item, String uri, long position) {}

    private final NshComponentContext context;
    private final FrameLayout container;
    private final TextureView texture;
    private final ExoPlayer player;
    private final DefaultPreloadManager.Builder preloadBuilder;
    private DefaultPreloadManager preloadManager;
    private StagedMedia staged;
    private MediaItem adopted;
    private long preloadSequence;
    private boolean preloadFailed;
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
    private volatile int startupHeight;
    private long sourceStartedAt;
    private Long firstFrameMs;
    private String videoDecoder;
    private String audioDecoder;

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
        preloadBuilder = new DefaultPreloadManager.Builder(context.getContext(), ignored ->
                staged == null ? DefaultPreloadManager.PreloadStatus.PRELOAD_STATUS_NOT_PRELOADED
                        : DefaultPreloadManager.PreloadStatus.specifiedRangeLoaded(staged.position(), 2000))
                .setRenderersFactory(renderers)
                .setTrackSelectorFactory(ctx -> new DefaultTrackSelector(ctx, new StartupTrackSelection()))
                .setLoadControl(new DefaultLoadControl.Builder()
                        .setBufferDurationsMsForLocalPlayback(buffer, buffer, 250, 250)
                        .setBufferDurationsMsForStreaming(buffer, buffer, 750, 1500)
                        .build());
        player = preloadBuilder.buildExoPlayer(new ExoPlayer.Builder(context.getContext())
                .setAudioAttributes(new AudioAttributes.Builder().setUsage(C.USAGE_MEDIA)
                        .setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(), true)
                .setHandleAudioBecomingNoisy(true));
        player.setVideoTextureView(texture);
        player.addListener(new Player.Listener() {
            @Override
            public void onEvents(Player owner, Player.Events events) {
                if (!isCurrent()) return;
                if (events.contains(Player.EVENT_VIDEO_SIZE_CHANGED)) resize();
                if (events.contains(Player.EVENT_TRACKS_CHANGED)) emitTracks();
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && Trace.isEnabled()) {
                    if (events.contains(Player.EVENT_PLAYBACK_STATE_CHANGED)) {
                        mark("GoVue playback: state " + owner.getPlaybackState());
                        mark("GoVue playback: buffered " + owner.getTotalBufferedDuration());
                    }
                    if (events.contains(Player.EVENT_IS_PLAYING_CHANGED)) {
                        mark("GoVue playback: playing " + owner.isPlaying());
                    }
                }
                emitStatus();
            }
        });
        player.addAnalyticsListener(new AnalyticsListener() {
            @Override
            public void onVideoDecoderInitialized(EventTime event, String name, long time, long duration) {
                if (isCurrent(event)) videoDecoder = name;
            }

            @Override
            public void onAudioDecoderInitialized(EventTime event, String name, long time, long duration) {
                if (isCurrent(event)) audioDecoder = name;
            }

            @Override
            public void onVideoInputFormatChanged(EventTime event, Format format, DecoderReuseEvaluation reuse) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && Trace.isEnabled() && isCurrent(event)) {
                    mark("GoVue playback: selected height " + format.height);
                }
            }

            @Override
            public void onRenderedFirstFrame(EventTime event, Object output, long renderTimeMs) {
                if (!isCurrent(event) || !videoEnabled) return;
                texture.setAlpha(1);
                if (firstFrame) return;
                firstFrame = true;
                startupHeight = 0;
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q && Trace.isEnabled()) {
                    mark("GoVue playback: first frame");
                    mark("GoVue playback: first position " + Math.max(0, player.getCurrentPosition()));
                    mark("GoVue playback: first height " + player.getVideoSize().height);
                }
                firstFrameMs = Math.max(0, renderTimeMs - sourceStartedAt);
                emit("firstFrame", Collections.emptyMap());
            }
        });
    }

    private final class StartupTrackSelection extends AdaptiveTrackSelection.Factory {
        @Override
        protected AdaptiveTrackSelection createAdaptiveTrackSelection(TrackGroup group, int[] tracks,
                int type, BandwidthMeter bandwidth, ImmutableList<AdaptiveTrackSelection.AdaptationCheckpoint> checkpoints) {
            int height = startupHeight;
            if (height <= 0 || group.type != C.TRACK_TYPE_VIDEO) {
                return super.createAdaptiveTrackSelection(group, tracks, type, bandwidth, checkpoints);
            }
            return new AdaptiveTrackSelection(group, tracks, type, bandwidth,
                    AdaptiveTrackSelection.DEFAULT_MIN_DURATION_FOR_QUALITY_INCREASE_MS,
                    AdaptiveTrackSelection.DEFAULT_MAX_DURATION_FOR_QUALITY_DECREASE_MS,
                    AdaptiveTrackSelection.DEFAULT_MIN_DURATION_TO_RETAIN_AFTER_DISCARD_MS,
                    AdaptiveTrackSelection.DEFAULT_MAX_WIDTH_TO_DISCARD,
                    AdaptiveTrackSelection.DEFAULT_MAX_HEIGHT_TO_DISCARD,
                    AdaptiveTrackSelection.DEFAULT_BANDWIDTH_FRACTION,
                    AdaptiveTrackSelection.DEFAULT_BUFFERED_FRACTION_TO_LIVE_EDGE_FOR_QUALITY_INCREASE,
                    checkpoints, Clock.DEFAULT) {
                private boolean initial = true;

                @Override
                public void updateSelectedTrack(long position, long buffered, long available,
                        List<? extends MediaChunk> queue, MediaChunkIterator[] iterators) {
                    super.updateSelectedTrack(position, buffered, available, queue, iterators);
                    initial = false;
                }

                @Override
                protected boolean canSelectFormat(Format format, int bitrate, long budget) {
                    // Start near the prefetched rendition. Normal adaptation can upgrade
                    // once enough media is buffered, within the same sample stream.
                    return initial ? format.height > 0 && format.height <= height
                            : super.canSelectFormat(format, bitrate, budget);
                }
            };
        }
    }

    private static void mark(String phase) {
        Trace.beginSection(phase);
        Trace.endSection();
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
        if (name.equals("preload")) {
            try {
                stage(value);
            } catch (IllegalStateException | IllegalArgumentException failure) {
                clearStaged();
                mark("GoVue playback: preload unavailable");
            }
        }
    }

    private void stage(Object value) {
        clearStaged();
        if (hasSource || !(value instanceof Map<?, ?> load)
                || !(load.get("source") instanceof Map<?, ?> source)
                || !(source.get("uri") instanceof String uri) || !uri.startsWith("http://127.0.0.1:")
                || !(load.get("position") instanceof Number position)) return;
        if (preloadManager == null) {
            preloadManager = preloadBuilder.build();
            preloadManager.addListener(new PreloadManagerListener() {
                @Override
                public void onCompleted(MediaItem item) {
                    if (staged != null && staged.item().equals(item)) mark("GoVue playback: preload ready");
                }

                @Override
                public void onError(PreloadException error) {
                    if (staged != null && staged.item().equals(error.mediaItem)) {
                        preloadFailed = true;
                        mark("GoVue playback: preload failed");
                    }
                }
            });
        }
        preloadFailed = false;
        startupHeight = startupHeight(source);
        MediaItem item = mediaItem(source, uri, "govue-preload:" + ++preloadSequence);
        staged = new StagedMedia(item, uri, millis(position));
        mark("GoVue playback: preload samples");
        preloadManager.add(mediaSource(source, item), 0);
        preloadManager.invalidate();
    }

    private void clearStaged() {
        if (staged == null) return;
        MediaItem item = staged.item();
        staged = null;
        mark("GoVue playback: drop preloaded samples");
        preloadManager.remove(item);
    }

    private MediaSource takeStaged(String uri, long position) {
        if (staged == null) return null;
        // Media3 reuses a queued period only at the exact requested position.
        if (preloadFailed || !uri.equals(staged.uri()) || position != staged.position()) {
            if (!preloadFailed) {
                mark(uri.equals(staged.uri()) ? "GoVue playback: preload resume changed"
                        : "GoVue playback: preload source changed");
            }
            clearStaged();
            return null;
        }
        adopted = staged.item();
        staged = null;
        mark("GoVue playback: reuse preloaded samples");
        return preloadManager.getMediaSource(adopted);
    }

    private void replace(long revision, Map<?, ?> load) {
        generation = revision;
        hasSource = false;
        firstFrame = false;
        startupHeight = 0;
        firstFrameMs = null;
        videoDecoder = null;
        audioDecoder = null;
        texture.setAlpha(0);
        main.removeCallbacks(tick);
        player.stop();
        player.clearMediaItems();
        if (adopted != null) {
            preloadManager.remove(adopted);
            adopted = null;
        }
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
        long position = load.get("position") instanceof Number seconds ? millis(seconds) : C.TIME_UNSET;
        MediaSource media = takeStaged(uri, position);
        if (media == null) media = mediaSource(source, mediaItem(source, uri, "govue-media:" + generation));
        mediaId = media.getMediaItem().mediaId;
        startupHeight = startupHeight(source);
        player.setTrackSelectionParameters(player.getTrackSelectionParameters().buildUpon()
                .clearOverridesOfType(C.TRACK_TYPE_VIDEO)
                .build());
        player.setPlaybackSpeed(load.get("rate") instanceof Number speed ? speed.floatValue() : 1);
        player.setMediaSource(media, position);
        hasSource = true;
        sourceStartedAt = SystemClock.elapsedRealtime();
        player.setPlayWhenReady(Boolean.TRUE.equals(load.get("playing")));
        Trace.beginSection("GoVue playback: prepare");
        try {
            player.prepare();
        } finally {
            Trace.endSection();
        }
    }

    private static int startupHeight(Map<?, ?> source) {
        return source.get("startupHeight") instanceof Number height ? Math.max(0, height.intValue()) : 0;
    }

    private static MediaItem mediaItem(Map<?, ?> source, String uri, String id) {
        MediaItem.Builder item = new MediaItem.Builder().setUri(uri).setMediaId(id);
        if ("dash".equals(source.get("contentType"))) item.setMimeType(MimeTypes.APPLICATION_MPD);
        if ("hls".equals(source.get("contentType"))) item.setMimeType(MimeTypes.APPLICATION_M3U8);
        return item.build();
    }

    private MediaSource mediaSource(Map<?, ?> source, MediaItem item) {
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
        return new DefaultMediaSourceFactory(new DefaultDataSource.Factory(
                context.getContext().getApplicationContext(), http)).createMediaSource(item);
    }

    private boolean isCurrent() {
        MediaItem current = player.getCurrentMediaItem();
        return hasSource && !destroyed && current != null && mediaId.equals(current.mediaId);
    }

    private boolean isCurrent(AnalyticsListener.EventTime event) {
        return isCurrent() && !event.timeline.isEmpty()
                && event.windowIndex >= 0 && event.windowIndex < event.timeline.getWindowCount()
                && mediaId.equals(event.timeline.getWindow(event.windowIndex, new Timeline.Window()).mediaItem.mediaId);
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
                } else if ("width".equals(fit)) {
                    y = 1 / ratio;
                } else if ("height".equals(fit)) {
                    x = ratio;
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
        long liveOffset = player.getCurrentLiveOffset();
        status.put("liveOffset", liveOffset >= 0 ? liveOffset / 1000.0 : null);
        status.put("diagnostics", diagnosticSample());
        emit("status", Collections.singletonMap("status", status));
    }

    private Map<String, Object> diagnosticSample() {
        Map<String, Object> sample = new LinkedHashMap<>();
        sample.put("sampledAt", SystemClock.elapsedRealtime());
        sample.put("firstFrameMs", firstFrameMs);
        sample.put("videoDecoder", videoDecoder);
        sample.put("audioDecoder", audioDecoder);
        DecoderCounters counts = player.getVideoDecoderCounters();
        if (counts != null) {
            counts.ensureUpdated();
            sample.put("renderedFrames", counts.renderedOutputBufferCount);
            sample.put("droppedFrames", counts.droppedBufferCount);
        }
        Format video = player.getVideoFormat();
        if (video != null) {
            sample.put("width", video.width);
            sample.put("height", video.height);
            sample.put("nominalFps", Float.isFinite(video.frameRate) ? video.frameRate : null);
            sample.put("videoCodec", video.codecs == null ? video.sampleMimeType : video.codecs);
            sample.put("videoBitrate", video.bitrate);
        }
        Format audio = player.getAudioFormat();
        if (audio != null) {
            sample.put("audioCodec", audio.codecs == null ? audio.sampleMimeType : audio.codecs);
            sample.put("audioBitrate", audio.bitrate);
            sample.put("audioChannels", audio.channelCount);
            sample.put("audioSampleRate", audio.sampleRate);
        }
        var display = container.getDisplay();
        if (display != null && Float.isFinite(display.getRefreshRate())) {
            sample.put("refreshRate", display.getRefreshRate());
        }
        return sample;
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
        if (preloadManager != null) preloadManager.release();
    }
}
