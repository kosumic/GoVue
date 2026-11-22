package io.github.kaminarios.govue.shell;

import android.annotation.SuppressLint;
import android.webkit.ConsoleMessage;
import android.webkit.CookieManager;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.io.ByteArrayInputStream;
import java.util.LinkedHashMap;
import java.util.Map;

import org.json.JSONException;
import org.json.JSONObject;

/** Executes extracted player code in a local document without a JavaScript bridge. */
public final class PlayerRuntime implements NshComponent {
    private static final String COMPONENT = "govue/player-runtime";
    private static final String LOCAL_URL = "about:blank";
    private static final String DOCUMENT = "<!doctype html>"
            + "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; "
            + "script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none'; "
            + "base-uri 'none'; form-action 'none'\">"
            + "<script>"
            + "for (const name of ['RTCPeerConnection', 'webkitRTCPeerConnection']) "
            + "Object.defineProperty(window, name, {value: undefined, writable: false, configurable: false});"
            + "</script>";
    private static final int MAX_SOURCE = 2_000_000;
    private static final int MAX_SIGNATURE = 10_000;
    private static final int MAX_REPLY = 130_000;

    private final NshComponentContext context;
    private final FrameLayout container;
    private WebView renderer;
    private Job pending;

    public static void register(NshView view) {
        view.registerComponent(COMPONENT, PlayerRuntime::new);
    }

    private PlayerRuntime(NshComponentContext context) {
        this.context = context;
        container = new FrameLayout(context.getContext());
        container.setImportantForAccessibility(FrameLayout.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        container.setVisibility(FrameLayout.INVISIBLE);
    }

    @Override
    public FrameLayout view() {
        return container;
    }

    @Override
    public void setProp(String name, Object value) {}

    @Override
    public void command(String name, Object args) {
        if (name.equals("cancel")) {
            if (pending != null && pending.taskId.equals(args)) reset();
            return;
        }
        if (!name.equals("evaluate") || !(args instanceof Map<?, ?> input)
                || !(input.get("requestId") instanceof String requestId)
                || !(input.get("taskId") instanceof String taskId)) return;
        if (requestId.isEmpty() || requestId.length() > 100 || taskId.isEmpty()
                || taskId.length() > 20) return;

        Job job = new Job(requestId, taskId);
        if (pending != null || !(input.get("source") instanceof String source)
                || source.isEmpty() || source.length() > MAX_SOURCE) {
            emit(job, null);
            return;
        }
        pending = job;
        try {
            renderer = new WebView(context.getContext());
            configure(renderer);
            renderer.setWebChromeClient(new WebChromeClient() {
                @Override
                public boolean onConsoleMessage(ConsoleMessage message) {
                    return true;
                }
            });
            renderer.setWebViewClient(new LocalDocument(job, source));
            container.addView(renderer, new FrameLayout.LayoutParams(1, 1));
            renderer.loadDataWithBaseURL(null, DOCUMENT, "text/html", "UTF-8", null);
        } catch (RuntimeException failure) {
            complete(job, null);
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    @SuppressWarnings("deprecation")
    private static void configure(WebView view) {
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
    }

    private final class LocalDocument extends WebViewClient {
        private final Job job;
        private String source;

        LocalDocument(Job job, String source) {
            this.job = job;
            this.source = source;
        }

        @Override
        public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            return true;
        }

        @Override
        public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
            if (request.isForMainFrame() && LOCAL_URL.equals(request.getUrl().toString())) return null;
            return new WebResourceResponse("text/plain", "UTF-8", new ByteArrayInputStream(new byte[0]));
        }

        @Override
        public void onPageFinished(WebView view, String url) {
            if (view != renderer || pending != job || source == null || !LOCAL_URL.equals(url)) return;
            String quoted = JSONObject.quote(source).replace("\u2028", "\\u2028").replace("\u2029", "\\u2029");
            source = null;
            view.evaluateJavascript("(() => { try { const result = Function(" + quoted + ")(); "
                    + "const values = Object.create(null); for (const key of ['n', 'sig']) { "
                    + "const value = result == null ? null : result[key]; "
                    + "if (typeof value === 'string' && value.length <= " + MAX_SIGNATURE
                    + ") values[key] = value; } return values; } catch (_) { return null; } })()",
                    raw -> complete(job, signatures(raw)));
        }

        @Override
        public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
            if (request.isForMainFrame() && view == renderer) complete(job, null);
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            if (view == renderer) complete(job, null);
            return true;
        }
    }

    private static Map<String, String> signatures(String raw) {
        if (raw == null || raw.length() > MAX_REPLY) return null;
        try {
            JSONObject decoded = new JSONObject(raw);
            Map<String, String> result = new LinkedHashMap<>();
            for (String key : new String[] {"n", "sig"}) {
                if (decoded.opt(key) instanceof String value && value.length() <= MAX_SIGNATURE) {
                    result.put(key, value);
                }
            }
            return result;
        } catch (JSONException invalid) {
            return null;
        }
    }

    private void complete(Job job, Map<String, String> signatures) {
        if (pending != job) return;
        reset();
        emit(job, signatures);
    }

    private void emit(Job job, Map<String, String> signatures) {
        Map<String, Object> reply = new LinkedHashMap<>();
        reply.put("requestId", job.requestId);
        reply.put("taskId", job.taskId);
        if (signatures != null) reply.put("signatures", signatures);
        context.emit("evaluated", reply);
    }

    private void reset() {
        WebView previous = renderer;
        renderer = null;
        pending = null;
        if (previous != null) {
            container.removeView(previous);
            previous.destroy();
        }
    }

    @Override
    public void destroy() {
        reset();
    }

    private record Job(String requestId, String taskId) {}
}
