package com.longbridge.nsh;

import android.app.Activity;
import android.content.Intent;
import android.content.res.Configuration;
import android.os.Bundle;

import io.github.kaminarios.govue.shell.AiTransport;
import io.github.kaminarios.govue.shell.BrowserLaunches;
import io.github.kaminarios.govue.shell.BrowserTab;
import io.github.kaminarios.govue.shell.CaptionOverlay;
import io.github.kaminarios.govue.shell.ChatCamera;
import io.github.kaminarios.govue.shell.ChatSpeech;
import io.github.kaminarios.govue.shell.ChatReadAloud;
import io.github.kaminarios.govue.shell.DownloadExport;
import io.github.kaminarios.govue.shell.DownloadTransport;
import io.github.kaminarios.govue.shell.NowPlaying;
import io.github.kaminarios.govue.shell.PlayerRuntime;
import io.github.kaminarios.govue.shell.PlayerPip;
import io.github.kaminarios.govue.shell.PlayerWindow;
import io.github.kaminarios.govue.shell.TokenRuntime;
import io.github.kaminarios.govue.shell.VideoPlayer;

public final class GoVueActivity extends Activity {
    private final BrowserLaunches launches = new BrowserLaunches();
    private final PlayerWindow playerWindow = new PlayerWindow();
    private final PlayerPip pip = new PlayerPip();
    private NshView view;
    private ChatSpeech speech;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        DevActivity.edgeToEdge(this);
        view = new NshView(this);
        AiTransport.install(this);
        DownloadTransport.install(this);
        DownloadExport.register(view, this);
        view.useStandaloneLimits();
        view.ownSystemBars();
        BrowserTab.register(view);
        launches.register(view);
        CaptionOverlay.register(view);
        ChatCamera.register(view, this);
        view.registerComponent("govue/chat-speech", context -> {
            speech = new ChatSpeech(this, context);
            return speech;
        });
        view.registerComponent("govue/chat-read-aloud", context -> new ChatReadAloud(this, context));
        VideoPlayer.register(view);
        NowPlaying.register(view);
        playerWindow.register(view, this);
        pip.register(view, this);
        PlayerRuntime.register(view);
        TokenRuntime.register(view);
        setContentView(view);
        if (state == null) launches.receive(getIntent());
        view.runRustApp("nsh");
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        launches.receive(intent);
    }

    @Override
    protected void onResume() {
        super.onResume();
        pip.onResume();
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
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (!view.handleBack()) super.onBackPressed();
    }

    @Override
    protected void onDestroy() {
        view.destroy();
        super.onDestroy();
    }
}
