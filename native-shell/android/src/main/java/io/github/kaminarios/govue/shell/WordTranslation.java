package io.github.kaminarios.govue.shell;

import android.view.View;

import com.google.mlkit.common.model.DownloadConditions;
import com.google.mlkit.nl.translate.TranslateLanguage;
import com.google.mlkit.nl.translate.Translation;
import com.google.mlkit.nl.translate.Translator;
import com.google.mlkit.nl.translate.TranslatorOptions;
import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.util.Map;

/** App-owned adapter around the Android on-device translation models. */
public final class WordTranslation implements NshComponent {
    private final NshComponentContext context;
    private final View view;
    private Translator active;
    private String request;

    public static void register(NshView view) {
        view.registerComponent("govue/word-translation", WordTranslation::new);
    }

    private WordTranslation(NshComponentContext context) {
        this.context = context;
        view = new View(context.getContext());
        view.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    @Override
    public View view() {
        return view;
    }

    @Override
    public void setProp(String name, Object value) {}

    @Override
    public void command(String name, Object value) {
        if (name.equals("cancel")) {
            if (request != null && request.equals(value)) destroy();
            return;
        }
        if (!name.equals("translate") || !(value instanceof Map<?, ?> fields)
                || !(fields.get("request") instanceof String id) || id.length() > 100
                || !(fields.get("input") instanceof String input) || input.length() > 4_000
                || !(fields.get("source") instanceof String sourceTag) || sourceTag.length() > 32
                || !(fields.get("target") instanceof String targetTag) || targetTag.length() > 32) return;
        destroy();
        String source = TranslateLanguage.fromLanguageTag(sourceTag);
        String target = TranslateLanguage.fromLanguageTag(targetTag);
        if (source == null || target == null) {
            error(id, "Translation is unavailable for this language pair.");
            return;
        }
        try {
            Translator client = Translation.getClient(new TranslatorOptions.Builder()
                    .setSourceLanguage(source).setTargetLanguage(target).build());
            active = client;
            request = id;
            client.downloadModelIfNeeded(new DownloadConditions.Builder().build())
                    .addOnSuccessListener(ignored -> {
                        if (active == client && id.equals(request)) {
                            client.translate(input)
                                    .addOnSuccessListener(text -> finish(client, id, text, null))
                                    .addOnFailureListener(failure -> finish(client, id, null, "Could not translate this word. Please try again."));
                        }
                    })
                    .addOnFailureListener(failure -> finish(client, id, null,
                            "Could not download the language model. Check your connection and try again."));
        } catch (RuntimeException failure) {
            destroy();
            error(id, "Could not translate this word. Please try again.");
        }
    }

    private void finish(Translator client, String id, String text, String error) {
        if (active != client || !id.equals(request)) return;
        destroy();
        if (text != null && !text.isBlank()) {
            context.emit("translation", Map.of("request", id, "text", text));
        } else {
            error(id, error == null ? "No translation was returned. Please try again." : error);
        }
    }

    private void error(String id, String error) {
        context.emit("translation", Map.of("request", id, "error", error));
    }

    @Override
    public void destroy() {
        Translator client = active;
        active = null;
        request = null;
        if (client != null) client.close();
    }
}
