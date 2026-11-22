package io.github.kaminarios.govue.shell;

import android.annotation.SuppressLint;
import android.webkit.ConsoleMessage;
import android.webkit.CookieManager;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;

import java.io.ByteArrayInputStream;

final class IsolatedWebView {
    static final String URL = "about:blank";
    static final String DOCUMENT = "<!doctype html><meta charset=utf-8>"
            + "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; "
            + "script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none'; "
            + "base-uri 'none'; form-action 'none'\">"
            + "<script>for (const name of ['RTCPeerConnection', 'webkitRTCPeerConnection']) "
            + "Object.defineProperty(window, name, {value: undefined, writable: false, configurable: false});</script>";

    private IsolatedWebView() {}

    @SuppressLint("SetJavaScriptEnabled")
    @SuppressWarnings("deprecation")
    static void configure(WebView view) {
        WebSettings settings = view.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setBlockNetworkLoads(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setAllowFileAccessFromFileURLs(false);
        settings.setAllowUniversalAccessFromFileURLs(false);
        settings.setDomStorageEnabled(false);
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setJavaScriptCanOpenWindowsAutomatically(false);
        settings.setSupportMultipleWindows(false);
        CookieManager.getInstance().setAcceptThirdPartyCookies(view, false);
        view.setWebChromeClient(new WebChromeClient() {
            @Override
            public boolean onConsoleMessage(ConsoleMessage message) {
                return true;
            }
        });
    }

    static class Client extends WebViewClient {
        @Override
        public final boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            return true;
        }

        @Override
        public final WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
            if (request.isForMainFrame() && URL.equals(request.getUrl().toString())) return null;
            return new WebResourceResponse("text/plain", "UTF-8", new ByteArrayInputStream(new byte[0]));
        }
    }
}
