package io.github.kaminarios.govue.shell;

import android.content.Context;
import android.content.Intent;
import android.view.View;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.lang.ref.WeakReference;
import java.util.Map;

/** Mirrors playback state to the Android media session. Commands stay scoped to Rust's source. */
public final class NowPlaying implements NshComponent {
    private static WeakReference<NowPlaying> selected = new WeakReference<>(null);

    record Snapshot(long generation, String video, String title, String author, long position,
            long duration, float rate, boolean playing, boolean buffering, boolean previous,
            boolean next, boolean foreground) {
        static Snapshot read(Object raw) {
            if (!(raw instanceof Map<?, ?> fields) || !(fields.get("generation") instanceof Number generation)
                    || generation.longValue() <= 0 || !(fields.get("video") instanceof String id)
                    || !id.matches("[A-Za-z0-9_-]{11}")) return null;
            float rate = fields.get("rate") instanceof Number value ? value.floatValue() : 1;
            return new Snapshot(generation.longValue(), id, text(fields.get("title"), "GoVue"),
                    text(fields.get("author"), ""), time(fields.get("position")), time(fields.get("duration")),
                    Float.isFinite(rate) && rate >= 0.25f && rate <= 4 ? rate : 1,
                    Boolean.TRUE.equals(fields.get("playing")), Boolean.TRUE.equals(fields.get("buffering")),
                    Boolean.TRUE.equals(fields.get("previous")), Boolean.TRUE.equals(fields.get("next")),
                    Boolean.TRUE.equals(fields.get("foreground")));
        }

        private static String text(Object value, String fallback) {
            return value instanceof String text && !text.isEmpty()
                    ? text.substring(0, Math.min(text.length(), 1000)) : fallback;
        }

        private static long time(Object value) {
            double seconds = value instanceof Number number ? number.doubleValue() : 0;
            return Double.isFinite(seconds) ? Math.round(Math.min(604800, Math.max(0, seconds)) * 1000) : 0;
        }
    }

    private final NshComponentContext context;
    private final Context app;
    private final View view;
    private Snapshot current;
    private boolean starting;

    public static void register(NshView view) {
        view.registerComponent("govue/now-playing", NowPlaying::new);
    }

    private NowPlaying(NshComponentContext context) {
        this.context = context;
        app = context.getContext().getApplicationContext();
        view = new View(context.getContext());
        selected = new WeakReference<>(this);
    }

    static NowPlaying selected() {
        return selected.get();
    }

    Snapshot current() {
        return current;
    }

    void serviceStarted() {
        starting = false;
    }

    @Override
    public View view() {
        return view;
    }

    @Override
    public void setProp(String name, Object value) {
        if (!"snapshot".equals(name) || selected() != this) return;
        current = Snapshot.read(value);
        Intent service = new Intent(app, NowPlayingService.class);
        if (current == null) {
            starting = false;
            app.stopService(service);
        } else if (NowPlayingService.running != null) {
            NowPlayingService.running.update(current);
        } else if (!starting && current.foreground()) {
            try {
                app.startForegroundService(service);
                starting = true;
            } catch (RuntimeException ignored) {
                // The next foreground playback update can retry the system service.
            }
        }
    }

    void action(String action, long position) {
        if (current == null || selected() != this) return;
        context.emit("control", Map.of("action", action, "generation", current.generation(),
                "video", current.video(), "position", position / 1000.0));
    }

    @Override
    public void destroy() {
        if (selected() != this) return;
        selected.clear();
        current = null;
        app.stopService(new Intent(app, NowPlayingService.class));
    }
}
