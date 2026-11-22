package io.github.kaminarios.govue.shell;

import android.app.Activity;
import android.media.AudioAttributes;
import android.os.Handler;
import android.os.Looper;
import android.speech.tts.TextToSpeech;
import android.speech.tts.UtteranceProgressListener;
import android.view.View;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** Reads Rust-prepared response text using Android's current speech engine. */
public final class ChatReadAloud implements NshComponent {
    private final View view;
    private final NshComponentContext context;
    private final Handler main = new Handler(Looper.getMainLooper());
    private TextToSpeech speech;
    private boolean ready;
    private boolean closed;
    private long active;
    private int index;
    private List<String> chunks = List.of();

    public ChatReadAloud(Activity activity, NshComponentContext context) {
        this.context = context;
        view = new View(activity);
    }

    @Override public View view() { return view; }
    @Override public void setProp(String name, Object value) {}

    @Override public void command(String name, Object raw) {
        if (closed || !(raw instanceof Map<?, ?> values) || !(values.get("id") instanceof Number number)) return;
        long id = number.longValue();
        if (name.equals("stop") && id == active) {
            stop();
        } else if (name.equals("read") && id > 0 && values.get("chunks") instanceof List<?> list) {
            stop();
            active = id;
            if (list.isEmpty() || list.size() > 4096) {
                finish("Could not read this response. Please try again.");
                return;
            }
            var checked = new ArrayList<String>(list.size());
            for (Object part : list) {
                if (!(part instanceof String text) || text.isEmpty() || text.length() > TextToSpeech.getMaxSpeechInputLength()) {
                    finish("Could not read this response. Please try again.");
                    return;
                }
                checked.add(text);
            }
            chunks = checked;
            if (speech == null) {
                speech = new TextToSpeech(view.getContext(), status -> main.post(() -> initialized(status)));
            } else if (ready) {
                next();
            }
        }
    }

    private void initialized(int status) {
        if (closed || speech == null) return;
        var voices = status == TextToSpeech.SUCCESS ? speech.getVoices() : null;
        if (voices == null || voices.isEmpty()) {
            if (active != 0) finish("No text-to-speech voice is available. Add one in your device’s text-to-speech settings.");
            speech.shutdown();
            speech = null;
            return;
        }
        speech.setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build());
        speech.setOnUtteranceProgressListener(new UtteranceProgressListener() {
            @Override public void onStart(String id) {}
            @Override public void onDone(String utterance) { main.post(() -> completed(utterance, false)); }
            @Override public void onError(String utterance) { main.post(() -> completed(utterance, true)); }
        });
        ready = true;
        if (active != 0) next();
    }

    private String utterance() { return "govue-read-" + active + "-" + index; }

    private void next() {
        if (index == chunks.size()) {
            finish(null);
        } else if (speech.speak(chunks.get(index), TextToSpeech.QUEUE_FLUSH, null, utterance()) != TextToSpeech.SUCCESS) {
            finish("Could not read this response. Please try again.");
        }
    }

    private void completed(String utterance, boolean failed) {
        if (active == 0 || !utterance().equals(utterance)) return;
        if (failed) {
            finish("Could not read this response. Please try again.");
        } else {
            index++;
            next();
        }
    }

    private void finish(String error) {
        long id = active;
        stop();
        context.emit("readAloud", error == null ? Map.of("id", id) : Map.of("id", id, "error", error));
    }

    private void stop() {
        active = 0;
        chunks = List.of();
        index = 0;
        if (speech != null && ready) speech.stop();
    }

    @Override public void destroy() {
        closed = true;
        stop();
        if (speech != null) speech.shutdown();
        speech = null;
    }
}
