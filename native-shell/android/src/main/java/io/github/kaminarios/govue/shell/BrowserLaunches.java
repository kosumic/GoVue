package io.github.kaminarios.govue.shell;

import android.content.Intent;
import android.view.View;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;

/** Delivers cold and warm Android browser launches to the Rust URL policy. */
public final class BrowserLaunches {
    private static final int MAX_MESSAGE = 100_000;
    private static final int MAX_PENDING = 16;

    private final ArrayDeque<String> pending = new ArrayDeque<>();
    private Receiver receiver;

    public void register(NshView host) {
        host.registerComponent("govue/browser-launches", Receiver::new);
    }

    public void receive(Intent intent) {
        if (intent == null) return;
        String action = intent.getAction();
        String text;
        if (Intent.ACTION_VIEW.equals(action)) {
            text = intent.getDataString();
        } else if (Intent.ACTION_SEND.equals(action) && "text/plain".equals(intent.getType())) {
            CharSequence content = intent.getCharSequenceExtra(Intent.EXTRA_TEXT);
            text = content == null ? null : content.toString();
        } else {
            return;
        }
        if (text == null || text.isEmpty() || text.length() > MAX_MESSAGE
                || text.getBytes(StandardCharsets.UTF_8).length > MAX_MESSAGE) return;
        if (pending.size() == MAX_PENDING) pending.removeFirst();
        pending.addLast(text);
        deliver();
    }

    private void deliver() {
        while (receiver != null && !pending.isEmpty()) {
            receiver.context.emit("launch", pending.removeFirst());
        }
    }

    private final class Receiver implements NshComponent {
        final NshComponentContext context;
        private final View view;

        Receiver(NshComponentContext context) {
            this.context = context;
            view = new View(context.getContext());
            view.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
        }

        @Override
        public View view() {
            return view;
        }

        @Override
        public void setProp(String key, Object value) {
            if (key.equals("ready") && Boolean.TRUE.equals(value)) {
                receiver = this;
                deliver();
            }
        }

        @Override
        public void destroy() {
            if (receiver == this) receiver = null;
        }
    }
}
