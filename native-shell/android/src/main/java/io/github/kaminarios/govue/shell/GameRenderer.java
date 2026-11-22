package io.github.kaminarios.govue.shell;

import android.graphics.Color;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebView;
import android.widget.FrameLayout;

import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.util.Set;

import org.json.JSONObject;

/** Dedicated, network-disabled WebView for the sandboxed learning-game document. */
public final class GameRenderer implements NshComponent {
    private static final String BRIDGE = "GoVueGame";
    private final NshComponentContext context;
    private final FrameLayout container;
    private WebView web;

    public static void register(NshView view) {
        view.registerComponent("govue/game", GameRenderer::new);
    }

    private GameRenderer(NshComponentContext context) {
        this.context = context;
        container = new FrameLayout(context.getContext());
    }

    @Override
    public FrameLayout view() {
        return container;
    }

    @Override
    public void setProp(String name, Object value) {
        if (!name.equals("document") || !(value instanceof String html)) return;
        destroy();
        if (html.isEmpty() || html.length() > 1_000_000
                || !WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            context.emit("failed", true);
            return;
        }
        try {
            WebView view = new WebView(context.getContext());
            web = view;
            view.setBackgroundColor(Color.TRANSPARENT);
            IsolatedWebView.configure(view);
            view.setWebViewClient(new LocalGame());
            WebViewCompat.addWebMessageListener(view, BRIDGE, Set.of("*"), (owner, message, origin, main, unused) -> {
                if (owner != web || !main || !IsolatedWebView.URL.equals(owner.getUrl())) return;
                String source = origin.toString();
                if (!"null".equals(source) && !IsolatedWebView.URL.equals(source)) return;
                String raw = message.getData();
                if (raw != null && raw.length() <= 10_000) context.emit("message", raw);
            });
            container.addView(view, new FrameLayout.LayoutParams(-1, -1));
            view.loadDataWithBaseURL(IsolatedWebView.URL, html, "text/html", "UTF-8", null);
        } catch (RuntimeException failure) {
            destroy();
            context.emit("failed", true);
        }
    }

    @Override
    public void command(String name, Object value) {
        if (!name.equals("reply") || web == null || !(value instanceof String reply)
                || reply.length() > 100_000) return;
        String quoted = JSONObject.quote(reply).replace("\u2028", "\\u2028").replace("\u2029", "\\u2029");
        web.evaluateJavascript("window.__lingoGameReply?.(JSON.parse(" + quoted + "))", null);
    }

    @Override
    public void destroy() {
        WebView view = web;
        web = null;
        if (view != null) {
            container.removeView(view);
            WebViewCompat.removeWebMessageListener(view, BRIDGE);
            view.destroy();
        }
    }

    private final class LocalGame extends IsolatedWebView.Client {
        @Override
        public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
            if (view == web && request.isForMainFrame()) context.emit("failed", true);
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            if (view == web) {
                destroy();
                context.emit("failed", true);
            }
            return true;
        }
    }
}
