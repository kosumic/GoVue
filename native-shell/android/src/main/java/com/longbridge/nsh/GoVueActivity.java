package com.longbridge.nsh;

import android.annotation.SuppressLint;
import android.app.Activity;
import android.content.Intent;
import android.content.res.Configuration;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

import androidx.annotation.OptIn;
import androidx.media3.common.util.UnstableApi;

import io.github.kaminarios.govue.shell.AiTransport;
import io.github.kaminarios.govue.shell.BrowserLaunches;
import io.github.kaminarios.govue.shell.BrowserTab;
import io.github.kaminarios.govue.shell.CaptionOverlay;
import io.github.kaminarios.govue.shell.CaptionWords;
import io.github.kaminarios.govue.shell.CastTransport;
import io.github.kaminarios.govue.shell.ChatCamera;
import io.github.kaminarios.govue.shell.ChatSpeech;
import io.github.kaminarios.govue.shell.ChatReadAloud;
import io.github.kaminarios.govue.shell.DownloadExport;
import io.github.kaminarios.govue.shell.GameRenderer;
import io.github.kaminarios.govue.shell.DownloadTransport;
import io.github.kaminarios.govue.shell.NowPlaying;
import io.github.kaminarios.govue.shell.PlayerRuntime;
import io.github.kaminarios.govue.shell.PlayerPip;
import io.github.kaminarios.govue.shell.PlayerWindow;
import io.github.kaminarios.govue.shell.ProviderQrScanner;
import io.github.kaminarios.govue.shell.R;
import io.github.kaminarios.govue.shell.ReplyNotifications;
import io.github.kaminarios.govue.shell.TokenRuntime;
import io.github.kaminarios.govue.shell.VideoPlayer;
import io.github.kaminarios.govue.shell.WordTranslation;

public final class GoVueActivity extends Activity {
    private final BrowserLaunches launches = new BrowserLaunches();
    private final PlayerWindow playerWindow = new PlayerWindow();
    private final PlayerPip pip = new PlayerPip();
    private NshView view;
    private FrameLayout launchCover;
    private ChatSpeech speech;
    private ReplyNotifications.Binding notifications;
    private ProviderQrScanner.Binding qrScanner;
    private OnBackInvokedCallback backCallback;

    @Override
    @OptIn(markerClass = UnstableApi.class)
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        DevActivity.edgeToEdge(this);
        view = new NshView(this);
        AiTransport.install(this);
        notifications = ReplyNotifications.register(view, this);
        qrScanner = ProviderQrScanner.register(view, this);
        CastTransport.register(view, this);
        DownloadTransport.install(this);
        DownloadExport.register(view, this);
        view.useStandaloneLimits();
        view.ownSystemBars();
        BrowserTab.register(view);
        launches.register(view);
        CaptionOverlay.register(view);
        CaptionWords.register(view);
        WordTranslation.register(view);
        ChatCamera.register(view, this);
        view.registerComponent("govue/chat-speech", context -> {
            speech = new ChatSpeech(this, context);
            return speech;
        });
        view.registerComponent("govue/chat-read-aloud", context -> new ChatReadAloud(this, context));
        VideoPlayer.register(view);
        GameRenderer.register(view);
        NowPlaying.register(view);
        playerWindow.register(view, this);
        pip.register(view, this);
        PlayerRuntime.register(view);
        TokenRuntime.register(view);
        setContentView(withLaunchCover());
        if (Build.VERSION.SDK_INT >= 33) {
            backCallback = () -> {
                if (!view.handleBack() && !moveTaskToBack(false)) finishAfterTransition();
            };
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, backCallback);
        }
        if (state == null) receive(getIntent());
        view.runRustApp("nsh");
    }

    private FrameLayout withLaunchCover() {
        FrameLayout root = new FrameLayout(this);
        root.addView(view, new FrameLayout.LayoutParams(-1, -1));
        launchCover = new FrameLayout(this);
        launchCover.setBackgroundColor(getColor(R.color.govue_launch_background));
        launchCover.setClickable(true);
        ImageView icon = new ImageView(this);
        icon.setImageResource(R.drawable.govue_icon);
        icon.setScaleType(ImageView.ScaleType.FIT_CENTER);
        icon.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
        int size = getResources().getDimensionPixelSize(R.dimen.govue_launch_icon_size);
        GradientDrawable badge = new GradientDrawable();
        badge.setShape(GradientDrawable.OVAL);
        badge.setColor(getColor(R.color.govue_background_light));
        icon.setBackground(badge);
        int inset = size / 5;
        icon.setPadding(inset, inset, inset, inset);
        launchCover.addView(icon, new FrameLayout.LayoutParams(size, size, Gravity.CENTER));
        root.addView(launchCover, new FrameLayout.LayoutParams(-1, -1));
        view.registerComponent("govue/launch-cover", context -> new NshComponent() {
            private final View marker = new View(GoVueActivity.this);

            @Override
            public View view() {
                return marker;
            }

            @Override
            public void setProp(String name, Object value) {
                if (launchCover == null) return;
                if (name.equals("dark")) {
                    launchCover.setBackgroundColor(getColor(Boolean.TRUE.equals(value)
                            ? R.color.govue_background_dark : R.color.govue_background_light));
                } else if (name.equals("ready") && Boolean.TRUE.equals(value)) {
                    FrameLayout cover = launchCover;
                    launchCover = null;
                    cover.postOnAnimation(() -> root.removeView(cover));
                }
            }
        });
        return root;
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        receive(intent);
        notifications.drain();
    }

    private void receive(Intent intent) {
        String scheme = intent == null ? null : intent.getScheme();
        if ("govue".equals(scheme) || "lingoplay".equals(scheme)) {
            view.handleUrlSchemeIntent(intent);
        } else {
            launches.receive(intent);
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        notifications.drain();
        pip.onResume();
        qrScanner.resume();
    }

    @Override
    protected void onUserLeaveHint() {
        super.onUserLeaveHint();
        pip.onUserLeaveHint();
    }

    @Override
    public void onPictureInPictureModeChanged(boolean inPip, Configuration config) {
        super.onPictureInPictureModeChanged(inPip, config);
        pip.onModeChanged(inPip);
    }

    @Override
    public void onConfigurationChanged(Configuration config) {
        super.onConfigurationChanged(config);
        DevActivity.systemBars(this);
        view.mounter().applySystemBars();
        playerWindow.applySystemBars();
    }

    @Override
    public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] grants) {
        super.onRequestPermissionsResult(requestCode, permissions, grants);
        if (requestCode == ChatSpeech.MICROPHONE && speech != null) speech.permission(grants);
        if (requestCode == ProviderQrScanner.REQUEST) qrScanner.permissionChanged();
    }

    @Override
    @SuppressLint("GestureBackNavigation")
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (!view.handleBack()) super.onBackPressed();
    }

    @Override
    protected void onDestroy() {
        if (Build.VERSION.SDK_INT >= 33 && backCallback != null) {
            getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(backCallback);
        }
        view.destroy();
        super.onDestroy();
    }
}
