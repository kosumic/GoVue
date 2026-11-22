package com.longbridge.nsh;

import android.app.Activity;
import android.content.Intent;
import android.content.res.Configuration;
import android.os.Bundle;

import io.github.kaminarios.govue.shell.AiTransport;
import io.github.kaminarios.govue.shell.BrowserLaunches;
import io.github.kaminarios.govue.shell.BrowserTab;
import io.github.kaminarios.govue.shell.CaptionOverlay;
import io.github.kaminarios.govue.shell.PlayerRuntime;
import io.github.kaminarios.govue.shell.TokenRuntime;
import io.github.kaminarios.govue.shell.VideoPlayer;

public final class GoVueActivity extends Activity {
    private final BrowserLaunches launches = new BrowserLaunches();
    private NshView view;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        DevActivity.edgeToEdge(this);
        view = new NshView(this);
        AiTransport.install(this);
        view.useStandaloneLimits();
        view.ownSystemBars();
        BrowserTab.register(view);
        launches.register(view);
        CaptionOverlay.register(view);
        VideoPlayer.register(view);
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
    public void onConfigurationChanged(Configuration config) {
        super.onConfigurationChanged(config);
        DevActivity.systemBars(this);
        view.mounter().applySystemBars();
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
