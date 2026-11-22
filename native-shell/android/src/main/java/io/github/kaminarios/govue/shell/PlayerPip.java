package io.github.kaminarios.govue.shell;

import android.app.Activity;
import android.app.PendingIntent;
import android.app.PictureInPictureParams;
import android.app.RemoteAction;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageManager;
import android.graphics.Rect;
import android.graphics.drawable.Icon;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.util.Rational;
import android.view.View;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** Presents Rust's picture-in-picture actions on Android's retained Activity. */
public final class PlayerPip {
    private static final List<String> KINDS = List.of("audio", "previous", "toggle", "next");
    private Receiver attached;

    public void register(NshView view, Activity activity) {
        view.registerComponent("govue/player-pip", context -> {
            attached = new Receiver(context, activity);
            return attached;
        });
    }

    public void onUserLeaveHint() {
        if (Build.VERSION.SDK_INT < 31 && attached != null && attached.autoEnter) attached.enter();
    }

    public void onModeChanged(boolean inPip) {
        if (attached != null) {
            attached.context.emit("mode", inPip);
            if (!inPip && attached.handoff) attached.finishHandoff();
        }
    }

    public void onResume() {
        if (attached != null && !attached.videoId.isEmpty()) attached.action("foreground");
    }

    private final class Receiver implements NshComponent {
        private final NshComponentContext context;
        private final Activity activity;
        private final View view;
        private final Handler main = new Handler(Looper.getMainLooper());
        private final String prefix;
        private final boolean supported;
        private final BroadcastReceiver controls;
        private List<RemoteAction> actions = List.of();
        private Set<String> enabled = Set.of();
        private String videoId = "";
        private long generation;
        private boolean available;
        private boolean autoEnter;
        private boolean handoff;
        private long deadline;
        private Rect frame;

        @SuppressWarnings("deprecation")
        Receiver(NshComponentContext context, Activity activity) {
            this.context = context;
            this.activity = activity;
            view = new View(activity);
            prefix = activity.getPackageName() + ".PIP.";
            supported = activity.getPackageManager().hasSystemFeature(PackageManager.FEATURE_PICTURE_IN_PICTURE);
            controls = new BroadcastReceiver() {
                @Override
                public void onReceive(Context ignored, Intent intent) {
                    String name = intent.getAction();
                    if (name == null || !name.startsWith(prefix)) return;
                    String kind = name.substring(prefix.length());
                    if (activity.isInPictureInPictureMode() && enabled.contains(kind)
                            && generation == intent.getLongExtra("generation", -1)
                            && videoId.equals(intent.getStringExtra("videoId"))) {
                        action(kind);
                    }
                }
            };
            IntentFilter filter = new IntentFilter();
            for (String action : KINDS) filter.addAction(prefix + action);
            if (Build.VERSION.SDK_INT >= 33) activity.registerReceiver(controls, filter, Context.RECEIVER_NOT_EXPORTED);
            else activity.registerReceiver(controls, filter,
                    activity.getPackageName() + ".internal.PIP", null);
        }

        @Override
        public View view() {
            return view;
        }

        @Override
        public void setProp(String name, Object value) {
            if (name.equals("ready") && Boolean.TRUE.equals(value)) {
                context.emit("supported", supported);
                context.emit("mode", activity.isInPictureInPictureMode());
            } else if (name.equals("config") && value instanceof Map<?, ?> options) {
                configure(options);
            }
        }

        private void configure(Map<?, ?> options) {
            videoId = options.get("videoId") instanceof String id && id.matches("[A-Za-z0-9_-]{11}") ? id : "";
            generation = options.get("generation") instanceof Number owner ? owner.longValue() : 0;
            available = supported && !videoId.isEmpty() && Boolean.TRUE.equals(options.get("available"));
            autoEnter = available && Boolean.TRUE.equals(options.get("autoEnter"));
            frame = options.get("frame") instanceof Map<?, ?> bounds ? screenRect(bounds) : null;
            List<RemoteAction> rendered = new ArrayList<>();
            Set<String> allowed = new HashSet<>();
            if (available && options.get("actions") instanceof List<?> values && values.size() <= 3) {
                for (Object entry : values) {
                    if (!(entry instanceof Map<?, ?> item) || !(item.get("id") instanceof String action)
                            || !KINDS.contains(action) || !(item.get("label") instanceof String label)
                            || label.isEmpty() || label.length() > 80
                            || !(item.get("icon") instanceof String icon)) continue;
                    int drawable = icon(icon);
                    if (drawable == 0) continue;
                    Intent intent = new Intent(prefix + action).setPackage(activity.getPackageName())
                            .putExtra("generation", generation).putExtra("videoId", videoId);
                    int requestCode = (int) generation * KINDS.size() + KINDS.indexOf(action);
                    PendingIntent pending = PendingIntent.getBroadcast(activity, requestCode, intent,
                            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
                    RemoteAction control = new RemoteAction(Icon.createWithResource(activity, drawable),
                            label, label, pending);
                    control.setEnabled(Boolean.TRUE.equals(item.get("enabled")));
                    if (control.isEnabled()) allowed.add(action);
                    rendered.add(control);
                }
            }
            actions = rendered;
            enabled = allowed;
            apply();
        }

        private int icon(String name) {
            String approved = switch (name) {
                case "headphones", "previous", "next", "play", "pause" -> "govue_pip_" + name;
                default -> "";
            };
            return approved.isEmpty() ? 0
                    : activity.getResources().getIdentifier(approved, "drawable", activity.getPackageName());
        }

        private Rect screenRect(Map<?, ?> bounds) {
            if (!(bounds.get("left") instanceof Number left) || !(bounds.get("top") instanceof Number top)
                    || !(bounds.get("width") instanceof Number width) || !(bounds.get("height") instanceof Number height)) {
                return null;
            }
            float density = view.getResources().getDisplayMetrics().density;
            int[] origin = new int[2];
            view.getLocationOnScreen(origin);
            int x = origin[0] + Math.round(left.floatValue() * density);
            int y = origin[1] + Math.round(top.floatValue() * density);
            int w = Math.round(width.floatValue() * density), h = Math.round(height.floatValue() * density);
            return w > 0 && h > 0 ? new Rect(x, y, x + w, y + h) : null;
        }

        private PictureInPictureParams parameters() {
            PictureInPictureParams.Builder builder = new PictureInPictureParams.Builder().setActions(actions);
            if (frame != null && !activity.isInPictureInPictureMode()) {
                builder.setSourceRectHint(frame);
                float ratio = (float) frame.width() / frame.height();
                int width = Math.round(1000 * Math.max(0.45f, Math.min(2.3f, ratio)));
                builder.setAspectRatio(new Rational(width, 1000));
            }
            if (Build.VERSION.SDK_INT >= 31) {
                builder.setAutoEnterEnabled(autoEnter && !handoff);
            }
            return builder.build();
        }

        private void apply() {
            if (supported && !activity.isFinishing() && !activity.isDestroyed()) {
                try {
                    activity.setPictureInPictureParams(parameters());
                } catch (IllegalStateException | IllegalArgumentException ignored) {
                    // The Activity can cross a window transition while its props are updated.
                }
            }
        }

        private void enter() {
            if (!available || handoff || activity.isFinishing() || activity.isInPictureInPictureMode()) return;
            try {
                if (activity.enterPictureInPictureMode(parameters())) return;
            } catch (IllegalStateException | IllegalArgumentException ignored) {
                // The OS may reject PiP when another task owns it.
            }
            context.emit("error", "Picture-in-picture is unavailable.");
        }

        private void action(String action) {
            context.emit("action", Map.of(
                    "action", action, "videoId", videoId, "generation", generation, "handoff", handoff));
        }

        @Override
        public void command(String name, Object value) {
            if (!(value instanceof Number owner) || owner.longValue() != generation) return;
            if (name.equals("enter")) enter();
            if (name.equals("audio")) backgroundAudio();
        }

        private void backgroundAudio() {
            if (handoff || !enabled.contains("audio") || !activity.isInPictureInPictureMode()) {
                action("audio_failed");
                return;
            }
            handoff = true;
            deadline = SystemClock.elapsedRealtime() + 4000;
            apply();
            try {
                if (activity.moveTaskToBack(true)) {
                    main.postDelayed(this::finishHandoff, 80);
                    return;
                }
            } catch (RuntimeException ignored) { }
            handoff = false;
            apply();
            action("audio_failed");
        }

        private void finishHandoff() {
            if (!handoff) return;
            if (activity.isFinishing() || activity.isDestroyed() || SystemClock.elapsedRealtime() >= deadline) {
                handoff = false;
                apply();
                action("audio_failed");
            } else if (activity.isInPictureInPictureMode()) {
                main.postDelayed(this::finishHandoff, 80);
            } else {
                handoff = false;
                action("audio_complete");
            }
        }

        @Override
        public void destroy() {
            main.removeCallbacksAndMessages(null);
            activity.unregisterReceiver(controls);
            autoEnter = false;
            available = false;
            actions = List.of();
            apply();
            if (attached == this) attached = null;
        }
    }
}
