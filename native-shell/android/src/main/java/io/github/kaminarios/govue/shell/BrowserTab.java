package io.github.kaminarios.govue.shell;

import android.annotation.SuppressLint;
import android.graphics.Bitmap;
import android.net.Uri;
import android.view.MotionEvent;
import android.webkit.CookieManager;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;

import androidx.webkit.ScriptHandler;
import androidx.webkit.WebMessageCompat;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import org.json.JSONObject;

import java.util.Arrays;
import java.util.Collections;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Set;
import java.util.UUID;

/** A mounted YouTube tab hosting GoVue's shared page scripts. */
public final class BrowserTab implements NshComponent {
    private static final Set<String> ORIGINS = Collections.unmodifiableSet(new HashSet<>(Arrays.asList(
            "https://youtube.com", "https://www.youtube.com",
            "https://m.youtube.com", "https://music.youtube.com")));
    private static final String BRIDGE = "GoVueAndroidPage";
    private static final int INTERNAL = 1;
    private static final int EXTERNAL = 2;
    private static final int MAX_MESSAGE = 8_100_000;

    private final NshComponentContext context;
    private final FrameLayout container;
    private WebView web;
    private ScriptHandler documentStart;
    private String address;
    private String documentStartAddress;
    private String bootstrap;
    private String pageBridge;
    private String configuration;
    private String token;
    private boolean bridgeReady;
    private long generation;
    private boolean active;
    private boolean loading;
    private boolean failed;

    public static void register(NshView view) {
        view.registerComponent("govue/browser-tab", BrowserTab::new);
    }

    private static native int urlPolicy(String address);
    private static native String browserUserAgent(String agent);

    private BrowserTab(NshComponentContext context) {
        this.context = context;
        container = new FrameLayout(context.getContext()) {
            @Override
            public boolean dispatchTouchEvent(MotionEvent event) {
                return active && super.dispatchTouchEvent(event);
            }
        };
    }

    @Override
    public FrameLayout view() {
        return container;
    }

    @Override
    public void setProp(String name, Object value) {
        if (!(value instanceof Map<?, ?> input)) return;
        if (name.equals("source") && address == null
                && input.get("url") instanceof String url
                && input.get("generation") instanceof Number revision
                && input.get("bootstrap") instanceof String startupScript
                && input.get("pageBridge") instanceof String pageScript) {
            address = url;
            generation = revision.longValue();
            bootstrap = startupScript;
            pageBridge = pageScript;
            startIfReady();
        }
        if (name.equals("options") && input.get("script") instanceof String script) {
            boolean visibilityChanged = active != Boolean.TRUE.equals(input.get("active"));
            boolean scriptChanged = !script.equals(configuration);
            active = Boolean.TRUE.equals(input.get("active"));
            configuration = script;
            container.setAlpha(active ? 1 : 0);
            container.setImportantForAccessibility(active ? FrameLayout.IMPORTANT_FOR_ACCESSIBILITY_AUTO
                    : FrameLayout.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
            if (web == null) {
                startIfReady();
            } else {
                if (scriptChanged) {
                    installDocumentStart();
                    evaluate(configuration);
                }
                if (visibilityChanged) evaluate(visibilityScript());
            }
        }
    }

    private void startIfReady() {
        if (web == null && !failed && address != null && configuration != null) {
            open(address);
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    @SuppressWarnings("deprecation")
    private void open(String url) {
        if (urlPolicy(url) != INTERNAL) return;
        web = new WebView(context.getContext());
        WebSettings settings = web.getSettings();
        String agent = browserUserAgent(settings.getUserAgentString());
        if (agent != null && !agent.isEmpty()) settings.setUserAgentString(agent);
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setTextZoom(100);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setSupportMultipleWindows(false);
        settings.setGeolocationEnabled(false);
        settings.setAllowFileAccess(false);
        settings.setAllowFileAccessFromFileURLs(false);
        settings.setAllowUniversalAccessFromFileURLs(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        CookieManager.getInstance().setAcceptThirdPartyCookies(web, true);
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            closeWebView();
            failed = true;
            emitError("Update Android System WebView to load this page.", true);
            return;
        }
        WebViewCompat.addWebMessageListener(web, BRIDGE, ORIGINS, (view, message, origin, main, reply) -> {
            if (view != web || !main || token == null || message.getType() != WebMessageCompat.TYPE_STRING
                    || !address.equals(view.getUrl())) return;
            String pageOrigin = allowedOrigin(origin);
            if (pageOrigin.isEmpty() || !pageOrigin.equals(allowedOrigin(Uri.parse(address)))) return;
            String data = message.getData();
            if (data == null || data.length() > MAX_MESSAGE + token.length() + 1) return;
            String prefix = token + ':';
            if (!data.startsWith(prefix)) return;
            Map<String, Object> fields = new LinkedHashMap<>();
            fields.put("document", document());
            fields.put("origin", pageOrigin);
            fields.put("raw", data.substring(prefix.length()));
            emit("pageMessage", fields);
        });
        web.setWebViewClient(new Client());
        web.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onProgressChanged(WebView view, int progress) {
                if (view == web) emit("progress", Collections.singletonMap("fraction", progress / 100.0));
            }
        });
        installDocumentStart();
        container.addView(web, new FrameLayout.LayoutParams(-1, -1));
        web.loadUrl(url);
    }

    private void installDocumentStart() {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) return;
        if (documentStart != null) documentStart.remove();
        documentStart = WebViewCompat.addDocumentStartJavaScript(web, bootstrap + '\n' + configuration, ORIGINS);
    }

    private static String allowedOrigin(Uri url) {
        String origin = "https://" + url.getHost();
        return "https".equals(url.getScheme()) && url.getUserInfo() == null
                && (url.getPort() == -1 || url.getPort() == 443) && ORIGINS.contains(origin) ? origin : "";
    }

    private String visibilityScript() {
        return "window.dispatchEvent(new CustomEvent('tabVisibilityChanged',{detail:{active:" + active + "}}));"
                + (active ? "" : "document.querySelectorAll('video,audio').forEach(v=>v.pause());");
    }

    private void bindDocument() {
        if (failed || token != null || !address.equals(web.getUrl()) || allowedOrigin(Uri.parse(address)).isEmpty()) return;
        token = UUID.randomUUID().toString();
        String binding = token;
        String quotedToken = JSONObject.quote(token);
        // A message queued by a replaced document carries that document's token.
        web.evaluateJavascript("(() => { if(location.href !== " + JSONObject.quote(address) + ") return false;"
                + " const token = " + quotedToken + ";"
                + " const endpoint = window." + BRIDGE + ";"
                + " if (!endpoint) return false;"
                + " window.__govuePageTransport = {"
                + " postMessage: raw => endpoint.postMessage(token + ':' + raw),"
                + " matches: stamp => stamp === token };\n" + pageBridge + '\n' + configuration + '\n'
                + "window.__govueBrowser?.setDocumentGeneration(" + generation + ");" + visibilityScript()
                + " return true; })();", result -> {
                    if (!binding.equals(token)) return;
                    bridgeReady = "true".equals(result);
                    if (bridgeReady) emitReady();
                    else token = null;
                });
    }

    private void evaluate(String script) {
        if (web == null || token == null) return;
        web.evaluateJavascript("if (location.href === " + JSONObject.quote(address)
                + " && window.__govuePageTransport?.matches(" + JSONObject.quote(token) + ")) {\n"
                + script + "\n}", null);
    }

    private Map<String, Object> document() {
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("url", address);
        fields.put("generation", generation);
        fields.put("canGoBack", web.canGoBack());
        return fields;
    }

    private void emit(String kind, Map<String, ?> fields) {
        Map<String, Object> event = new LinkedHashMap<>(fields);
        event.put("kind", kind);
        context.emit("browser", event);
    }

    private void emitNavigation() {
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("document", document());
        fields.put("loading", loading);
        emit("navigation", fields);
    }

    private void emitReady() {
        emit("ready", Collections.singletonMap("document", document()));
    }

    private void emitError(String message, boolean terminated) {
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("message", message);
        fields.put("terminated", terminated);
        emit("error", fields);
    }

    private final class Client extends WebViewClient {
        @Override
        public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            if (view != web) return true;
            String url = request.getUrl().toString();
            if (url.equals("about:blank")) return false;
            int policy = urlPolicy(url);
            if (policy == EXTERNAL && request.isForMainFrame()) {
                emit("externalUrl", Collections.singletonMap("url", url));
            }
            return policy != INTERNAL;
        }

        @Override
        public void onPageStarted(WebView view, String url, Bitmap favicon) {
            if (view != web) return;
            token = null;
            bridgeReady = false;
            failed = false;
            loading = true;
            documentStartAddress = url;
            updateAddress(url);
            if (!WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
                    && !allowedOrigin(Uri.parse(url)).isEmpty()) {
                view.evaluateJavascript(bootstrap + '\n' + configuration, null);
            }
        }

        @Override
        public void doUpdateVisitedHistory(WebView view, String url, boolean reload) {
            if (view == web && url != null && !url.equals(address)) {
                updateAddress(url);
                evaluate("window.__govueBrowser?.setDocumentGeneration(" + generation + ");");
                if (!loading && bridgeReady) emitReady();
            }
        }

        @Override
        public void onPageCommitVisible(WebView view, String url) {
            if (view == web && (url.equals(address) || url.equals(documentStartAddress))) bindDocument();
        }

        @Override
        public void onPageFinished(WebView view, String url) {
            if (view != web || failed || !(url.equals(address) || url.equals(documentStartAddress))) return;
            loading = false;
            bindDocument();
            emitNavigation();
            if (bridgeReady || allowedOrigin(Uri.parse(address)).isEmpty()) emitReady();
        }

        @Override
        public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
            if (view != web || !request.isForMainFrame() || !address.equals(request.getUrl().toString())) return;
            token = null;
            bridgeReady = false;
            failed = true;
            loading = false;
            emitError(error.getDescription().toString(), false);
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            if (view == web) {
                failed = true;
                loading = false;
                closeWebView();
                emitError("The browser stopped. Reload this page to continue.", true);
            }
            return true;
        }
    }

    private void updateAddress(String url) {
        address = url;
        generation = Math.max(generation + 1, System.currentTimeMillis());
        if (urlPolicy(url) == INTERNAL) emitNavigation();
    }

    @Override
    public void command(String name, Object args) {
        if (bootstrap == null || configuration == null) return;
        if (name.equals("navigate") && args instanceof String url && urlPolicy(url) == INTERNAL) {
            if (web == null) open(url);
            else web.loadUrl(url);
        } else if (name.equals("reload") && address != null) {
            if (web == null) open(address);
            else web.reload();
        } else if (name.equals("back") && web != null && web.canGoBack()) {
            web.goBack();
        } else if (name.equals("inject") && args instanceof Map<?, ?> input
                && input.get("generation") instanceof Number revision && revision.longValue() == generation
                && address != null && address.equals(input.get("url"))
                && input.get("script") instanceof String script) {
            evaluate(script);
        }
    }

    private void closeWebView() {
        if (web == null) return;
        WebView previous = web;
        web = null;
        token = null;
        bridgeReady = false;
        documentStart = null;
        container.removeView(previous);
        previous.destroy();
    }

    @Override
    public void destroy() {
        closeWebView();
    }
}
