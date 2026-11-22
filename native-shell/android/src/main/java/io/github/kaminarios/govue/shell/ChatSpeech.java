package io.github.kaminarios.govue.shell;

import android.Manifest;
import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.speech.RecognitionListener;
import android.speech.RecognizerIntent;
import android.speech.SpeechRecognizer;
import android.view.View;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;

import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/** Owns one system recognizer and scopes every callback to the Rust request. */
public final class ChatSpeech implements NshComponent {
    public static final int MICROPHONE = 9733;
    private final Activity activity;
    private final View view;
    private final NshComponentContext context;
    private SpeechRecognizer recognizer;
    private long active;
    private long permissionPending;
    private ArrayList<String> hints = new ArrayList<>();

    public ChatSpeech(Activity activity, NshComponentContext context) {
        this.activity = activity;
        this.context = context;
        view = new View(activity);
    }

    @Override public View view() { return view; }
    @Override public void setProp(String name, Object value) {}

    @Override public void command(String name, Object args) {
        if (name.equals("settings")) {
            try {
                activity.startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                        Uri.fromParts("package", activity.getPackageName(), null)));
            } catch (ActivityNotFoundException ignored) {}
            return;
        }
        if (!(args instanceof Map<?, ?> values) || !(values.get("id") instanceof Number id)) return;
        long session = id.longValue();
        switch (name) {
            case "start" -> start(session, values.get("hints"));
            case "finish" -> {
                if (active == session && recognizer != null) recognizer.stopListening();
            }
            case "cancel" -> {
                if (active == session) disposeSession();
            }
        }
    }

    private void start(long id, Object words) {
        if (id <= 0) return;
        if (active != 0 || permissionPending != 0) {
            sendError(id, "Speech recognition is finishing. Tap the microphone to try again.", false);
            return;
        }
        if (!SpeechRecognizer.isRecognitionAvailable(activity)) {
            sendError(id, "Speech recognition is unavailable. Enable a speech recognition service on your device.", false);
            return;
        }
        active = id;
        hints = new ArrayList<>();
        if (words instanceof List<?> list) {
            for (Object item : list) {
                if (hints.size() == 50) break;
                if (item instanceof String word && word.length() <= 128) hints.add(word);
            }
        }
        if (activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            listen(id);
        } else {
            permissionPending = id;
            activity.requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, MICROPHONE);
        }
    }

    public void permission(int[] grants) {
        long id = permissionPending;
        permissionPending = 0;
        if (id == 0 || active != id) return;
        if (grants.length > 0 && grants[0] == PackageManager.PERMISSION_GRANTED) {
            listen(id);
        } else {
            boolean settings = !activity.shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO);
            fail(id, settings
                    ? "Allow microphone and speech recognition access in Settings to use voice input."
                    : "Microphone and speech recognition access are needed for voice input. Tap the microphone to try again.", settings);
        }
    }

    private void listen(long id) {
        if (active != id || activity.isFinishing()) return;
        try {
            recognizer = SpeechRecognizer.createSpeechRecognizer(activity);
            recognizer.setRecognitionListener(new Events(id));
            Intent intent = new Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
                    .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
                    .putExtra(RecognizerIntent.EXTRA_LANGUAGE, Locale.getDefault().toLanguageTag())
                    .putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
                    .putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1);
            if (Build.VERSION.SDK_INT >= 33) {
                intent.putStringArrayListExtra(RecognizerIntent.EXTRA_BIASING_STRINGS, hints);
                intent.putExtra(RecognizerIntent.EXTRA_ENABLE_FORMATTING, RecognizerIntent.FORMATTING_OPTIMIZE_QUALITY);
            }
            send(id, "starting");
            recognizer.startListening(intent);
        } catch (RuntimeException error) {
            fail(id, "Speech recognition is unavailable. Tap the microphone to try again.", false);
        }
    }

    private final class Events implements RecognitionListener {
        private final long id;
        Events(long id) { this.id = id; }
        @Override public void onReadyForSpeech(Bundle values) { send(id, "listening"); }
        @Override public void onBeginningOfSpeech() {}
        @Override public void onRmsChanged(float value) {}
        @Override public void onBufferReceived(byte[] buffer) {}
        @Override public void onEndOfSpeech() {}
        @Override public void onEvent(int type, Bundle values) {}
        @Override public void onPartialResults(Bundle values) { result(id, values, false); }
        @Override public void onResults(Bundle values) {
            if (active != id) return;
            result(id, values, true);
            send(id, "end");
            disposeSession();
        }
        @Override public void onError(int error) {
            if (active != id) return;
            if (error == SpeechRecognizer.ERROR_NO_MATCH || error == SpeechRecognizer.ERROR_SPEECH_TIMEOUT) {
                send(id, "end");
                disposeSession();
                return;
            }
            String message = switch (error) {
                case SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS ->
                    "Allow microphone and speech recognition access in Settings to use voice input.";
                case SpeechRecognizer.ERROR_NETWORK, SpeechRecognizer.ERROR_NETWORK_TIMEOUT ->
                    "Speech recognition could not connect. Check your connection and try again.";
                case SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED, SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE ->
                    "Speech recognition does not support your device language.";
                default -> "Speech recognition is unavailable. Tap the microphone to try again.";
            };
            fail(id, message, error == SpeechRecognizer.ERROR_INSUFFICIENT_PERMISSIONS);
        }
    }

    private void result(long id, Bundle values, boolean complete) {
        if (active != id || values == null) return;
        ArrayList<String> results = values.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION);
        if (results == null || results.isEmpty()) return;
        String text = results.get(0);
        if (text != null && text.length() <= 16000) {
            context.emit("voice", Map.of("id", id, "kind", "result", "text", text, "final", complete));
        }
    }

    private void send(long id, String kind) {
        if (active == id) context.emit("voice", Map.of("id", id, "kind", kind));
    }

    private void fail(long id, String error, boolean settings) {
        if (active != id) return;
        disposeSession();
        sendError(id, error, settings);
    }

    private void sendError(long id, String error, boolean settings) {
        context.emit("voice", Map.of("id", id, "kind", "error", "message", error, "settings", settings));
    }

    private void disposeSession() {
        active = 0;
        hints.clear();
        if (recognizer != null) {
            recognizer.cancel();
            recognizer.destroy();
            recognizer = null;
        }
    }

    @Override public void destroy() { disposeSession(); }
}
