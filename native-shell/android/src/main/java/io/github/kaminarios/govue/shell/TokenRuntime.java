package io.github.kaminarios.govue.shell;

import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebView;
import android.widget.FrameLayout;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import org.json.JSONException;
import org.json.JSONObject;
import org.json.JSONTokener;

import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.regex.Pattern;

/** Runs the bundled token interpreter in local documents with no native JS interface. */
public final class TokenRuntime implements NshComponent {
    private static final int MAX_SESSIONS = 4;
    private static final int MAX_TOKEN = 2_000_000;
    private static final Pattern SCRIPT_END = Pattern.compile("</script", Pattern.CASE_INSENSITIVE);
    private static final String OUTPUT = """
            (() => {
              Object.defineProperty(Document.prototype, 'cookie', { get: () => '', set: () => {} });
              let value = null;
              Object.defineProperty(window, '__govueTokenTransport', { value: {
                postMessage: raw => { if (typeof raw === 'string' && raw.length <= 2100000) value = raw; }
              } });
              Object.defineProperty(window, '__govueTakeToken', { value: () => {
                const raw = value; value = null; return raw;
              } });
            })();
            """;

    private final NshComponentContext context;
    private final FrameLayout view;
    private final Map<String, Session> sessions = new HashMap<>();
    private String document;

    public static void register(NshView view) {
        view.registerComponent("govue/token-runtime", TokenRuntime::new);
    }

    private TokenRuntime(NshComponentContext context) {
        this.context = context;
        view = new FrameLayout(context.getContext());
        view.setImportantForAccessibility(FrameLayout.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
        view.setVisibility(FrameLayout.INVISIBLE);
    }

    @Override
    public FrameLayout view() {
        return view;
    }

    @Override
    public void setProp(String key, Object value) {
        if (document == null && key.equals("script") && value instanceof String script) {
            document = IsolatedWebView.DOCUMENT + "<script>" + OUTPUT
                    + SCRIPT_END.matcher(script).replaceAll(java.util.regex.Matcher.quoteReplacement("<\\/script"))
                    + "</script>";
            context.emit("ready", null);
        }
    }

    @Override
    public void command(String name, Object args) {
        if (name.equals("release") && args instanceof String requestId) {
            close(requestId);
            return;
        }
        if (!name.equals("evaluate") || document == null || !(args instanceof Map<?, ?> input)
                || !(input.get("requestId") instanceof String requestId)
                || !(input.get("task") instanceof Map<?, ?> params)
                || !(params.get("taskId") instanceof Number id)
                || !(params.get("stage") instanceof String stage)) return;
        long task = id.longValue();
        Session session = sessions.get(requestId);
        boolean snapshot = stage.equals("snapshot");
        if ((snapshot && (session != null || sessions.size() >= MAX_SESSIONS))
                || (!snapshot && (!stage.equals("mint") || session == null || !session.ready || session.pending != 0))) {
            emit(requestId, task, null);
            return;
        }
        try {
            String json = new JSONObject(params).toString();
            if (snapshot) {
                session = new Session(requestId);
                sessions.put(requestId, session);
                session.pending = task;
                session.input = json;
                view.addView(session.web, new FrameLayout.LayoutParams(1, 1));
                session.web.loadDataWithBaseURL(null, document, "text/html", "UTF-8", null);
            } else {
                session.pending = task;
                session.input = json;
                session.mint = true;
                session.dispatch();
            }
        } catch (RuntimeException failure) {
            close(requestId);
            emit(requestId, task, null);
        }
    }

    private final class Session extends IsolatedWebView.Client {
        private final String request;
        private final WebView web = new WebView(context.getContext());
        private String input;
        private long pending;
        private boolean ready;
        private boolean mint;

        Session(String request) {
            this.request = request;
            IsolatedWebView.configure(web);
            web.setWebViewClient(this);
        }

        @Override
        public void onPageFinished(WebView renderer, String url) {
            if (active() && !ready && IsolatedWebView.URL.equals(url)) {
                ready = true;
                dispatch();
            }
        }

        @Override
        public void onReceivedError(WebView renderer, WebResourceRequest request, WebResourceError error) {
            if (request.isForMainFrame()) finish(null);
        }

        @Override
        public boolean onRenderProcessGone(WebView renderer, RenderProcessGoneDetail detail) {
            finish(null);
            return true;
        }

        private boolean active() {
            return sessions.get(request) == this;
        }

        private void dispatch() {
            if (!active() || pending == 0 || input == null) return;
            String args = input;
            input = null;
            web.evaluateJavascript("window.__govueTakeToken?.();window.runExtractionToken?.("
                    + JSONObject.quote(request) + "," + args + ");true;", unused -> poll(pending));
        }

        private void poll(long task) {
            if (!active() || task == 0 || task != pending) return;
            web.evaluateJavascript("window.__govueTakeToken?.()", encoded -> {
                if (!active() || task != pending) return;
                if (encoded == null || encoded.equals("null")) {
                    web.postDelayed(() -> poll(task), 50);
                    return;
                }
                finish(value(encoded, request, task));
            });
        }

        private void finish(String token) {
            if (!active()) return;
            long task = pending;
            pending = 0;
            if (mint || token == null) close(request);
            if (task != 0) emit(request, task, token);
        }
    }

    private static String value(String encoded, String request, long task) {
        if (encoded.length() > MAX_TOKEN * 2 + 1000) return null;
        try {
            Object decoded = new JSONTokener(encoded).nextValue();
            if (!(decoded instanceof String raw) || raw.length() > MAX_TOKEN + 1000) return null;
            JSONObject reply = new JSONObject(raw);
            if (!request.equals(reply.optString("requestId")) || reply.optLong("taskId") != task
                    || !(reply.opt("value") instanceof String token) || token.isEmpty()
                    || token.length() > MAX_TOKEN || reply.optBoolean("error")) return null;
            return token;
        } catch (JSONException invalid) {
            return null;
        }
    }

    private void emit(String request, long task, String token) {
        Map<String, Object> fields = new LinkedHashMap<>();
        fields.put("requestId", request);
        fields.put("taskId", task);
        fields.put("value", token);
        context.emit("token", fields);
    }

    private void close(String request) {
        Session session = sessions.remove(request);
        if (session == null) return;
        view.removeView(session.web);
        session.web.destroy();
    }

    @Override
    public void destroy() {
        for (String request : new java.util.ArrayList<>(sessions.keySet())) close(request);
    }
}
