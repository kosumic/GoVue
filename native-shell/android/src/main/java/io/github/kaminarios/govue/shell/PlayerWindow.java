package io.github.kaminarios.govue.shell;

import android.app.Activity;
import android.content.pm.ActivityInfo;
import android.database.ContentObserver;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.provider.Settings;
import android.view.OrientationEventListener;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.util.Map;

/** Reports device orientation and applies the presentation chosen by Rust. */
public final class PlayerWindow {
    private Receiver receiver;

    public void register(NshView view, Activity activity) {
        view.registerComponent("govue/player-window", context -> {
            receiver = new Receiver(context, activity);
            return receiver;
        });
    }

    public void applySystemBars() {
        if (receiver != null && receiver.fullscreen) receiver.systemBars(true);
    }

    private final class Receiver implements NshComponent {
        private final NshComponentContext context;
        private final Activity activity;
        private final View view;
        private final OrientationEventListener orientation;
        private final ContentObserver autoRotate;
        private final int initialOrientation;
        private boolean observing;
        private boolean fullscreen;
        private int direction = -1;

        Receiver(NshComponentContext context, Activity activity) {
            this.context = context;
            this.activity = activity;
            view = new View(activity);
            initialOrientation = activity.getRequestedOrientation();
            orientation = new OrientationEventListener(activity) {
                @Override
                public void onOrientationChanged(int degrees) {
                    int next = orientationBucket(degrees);
                    if (direction != next) {
                        direction = next;
                        reportOrientation();
                    }
                }
            };
            autoRotate = new ContentObserver(new Handler(Looper.getMainLooper())) {
                @Override
                public void onChange(boolean selfChange) {
                    reportOrientation();
                }
            };
        }

        @Override
        public View view() {
            return view;
        }

        @Override
        public void setProp(String name, Object value) {
            if (name.equals("fullscreen")) {
                setFullscreen(Boolean.TRUE.equals(value));
            } else if (name.equals("observe")) {
                observe(Boolean.TRUE.equals(value));
            }
        }

        private void observe(boolean enabled) {
            if (observing == enabled) return;
            observing = enabled;
            if (enabled) {
                if (orientation.canDetectOrientation()) orientation.enable();
                activity.getContentResolver().registerContentObserver(
                        Settings.System.getUriFor(Settings.System.ACCELEROMETER_ROTATION), false, autoRotate);
            } else {
                orientation.disable();
                activity.getContentResolver().unregisterContentObserver(autoRotate);
                direction = -1;
            }
        }

        private void reportOrientation() {
            if (!observing) return;
            boolean rotating = Settings.System.getInt(activity.getContentResolver(),
                    Settings.System.ACCELEROMETER_ROTATION, 0) == 1;
            context.emit("orientation", Map.of("degrees", direction, "systemAutoRotate", rotating));
        }

        private void setFullscreen(boolean enabled) {
            if (fullscreen == enabled) return;
            fullscreen = enabled;
            activity.setRequestedOrientation(enabled
                    ? ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE : initialOrientation);
            systemBars(enabled);
        }

        @SuppressWarnings("deprecation")
        private void systemBars(boolean hidden) {
            Window window = activity.getWindow();
            if (Build.VERSION.SDK_INT >= 30) {
                WindowInsetsController bars = window.getInsetsController();
                if (bars == null) return;
                if (hidden) {
                    bars.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                    bars.hide(WindowInsets.Type.systemBars());
                } else {
                    bars.show(WindowInsets.Type.systemBars());
                    bars.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_DEFAULT);
                }
            } else {
                View decor = window.getDecorView();
                int mask = View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY;
                int current = decor.getSystemUiVisibility();
                decor.setSystemUiVisibility(hidden ? current | mask : current & ~mask);
            }
        }

        @Override
        public void destroy() {
            observe(false);
            setFullscreen(false);
            if (receiver == this) receiver = null;
        }
    }

    private static int orientationBucket(int degrees) {
        if (degrees < 0) return -1;
        if (degrees <= 30 || degrees >= 330 || (degrees >= 150 && degrees <= 210)) return 0;
        if (degrees >= 60 && degrees <= 120) return 90;
        if (degrees >= 240 && degrees <= 300) return 270;
        return -1;
    }
}
