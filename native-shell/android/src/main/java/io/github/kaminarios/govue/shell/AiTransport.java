package io.github.kaminarios.govue.shell;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.InputStream;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.util.Arrays;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.ScheduledFuture;
import java.util.concurrent.TimeUnit;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

import okhttp3.Call;
import okhttp3.OkHttpClient;
import okhttp3.Request;
import okhttp3.RequestBody;
import okhttp3.Response;
import okhttp3.ResponseBody;

/** Android Keystore and a streaming HTTP client for GoVue's Rust providers. */
public final class AiTransport {
    // These names and the ciphertext format must match the existing app.
    private static final String PREFERENCES = "lingoplay_ai_secrets";
    private static final String KEY_ALIAS = "io.github.kaminarios.lingoplay.ai";
    private static final String CIPHER = "AES/GCM/NoPadding";
    private static final int NONCE_BYTES = 12;
    private static final int TAG_BITS = 128;

    private final SharedPreferences preferences;
    private final OkHttpClient client = new OkHttpClient.Builder()
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(600, TimeUnit.SECONDS)
            .callTimeout(900, TimeUnit.SECONDS)
            .followRedirects(false)
            .followSslRedirects(false)
            .build();
    private final ScheduledExecutorService cancellation = Executors.newSingleThreadScheduledExecutor(work -> {
        Thread thread = new Thread(work, "GoVue AI cancellation");
        thread.setDaemon(true);
        return thread;
    });

    public static void install(Context context) {
        System.loadLibrary("nsh");
        installNative(new AiTransport(context.getApplicationContext()));
    }

    private static native void installNative(AiTransport adapter);
    private static native boolean active(long id);
    private static native void start(long id, int status);
    private static native boolean push(long id, byte[] bytes, int count);

    private AiTransport(Context context) {
        preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE);
    }

    private synchronized SecretKey encryptionKey() throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        if (store.getKey(KEY_ALIAS, null) instanceof SecretKey key) return key;
        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        generator.init(new KeyGenParameterSpec.Builder(KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build());
        return generator.generateKey();
    }

    private static boolean validKey(String key) {
        return key.startsWith("ai.") && key.length() <= 100
                && key.chars().allMatch(c -> Character.isLetterOrDigit(c) || c == '.' || c == '-' || c == '_');
    }

    // A null value signals a storage error. Empty values mean the key is absent.
    private String readSecret(String key) {
        if (!validKey(key)) return null;
        try {
            String encoded = preferences.getString(key, null);
            if (encoded == null) return "";
            byte[] stored = Base64.decode(encoded, Base64.NO_WRAP);
            if (stored.length <= NONCE_BYTES + TAG_BITS / Byte.SIZE) return null;
            Cipher cipher = Cipher.getInstance(CIPHER);
            cipher.init(Cipher.DECRYPT_MODE, encryptionKey(), new GCMParameterSpec(TAG_BITS, stored, 0, NONCE_BYTES));
            cipher.updateAAD(key.getBytes(StandardCharsets.UTF_8));
            return new String(cipher.doFinal(stored, NONCE_BYTES, stored.length - NONCE_BYTES), StandardCharsets.UTF_8);
        } catch (Exception failure) {
            return null;
        }
    }

    private boolean writeSecret(String key, String value) {
        if (!validKey(key) || value.length() > 100_000) return false;
        try {
            if (value.isEmpty()) return preferences.edit().remove(key).commit();
            Cipher cipher = Cipher.getInstance(CIPHER);
            cipher.init(Cipher.ENCRYPT_MODE, encryptionKey());
            cipher.updateAAD(key.getBytes(StandardCharsets.UTF_8));
            byte[] nonce = cipher.getIV();
            if (nonce.length != NONCE_BYTES) return false;
            byte[] encrypted = cipher.doFinal(value.getBytes(StandardCharsets.UTF_8));
            byte[] stored = Arrays.copyOf(nonce, NONCE_BYTES + encrypted.length);
            System.arraycopy(encrypted, 0, stored, NONCE_BYTES, encrypted.length);
            return preferences.edit().putString(key, Base64.encodeToString(stored, Base64.NO_WRAP)).commit();
        } catch (Exception failure) {
            return false;
        }
    }

    private boolean request(long id, String url, String method, String headers, String body) {
        try {
            URI address = new URI(url);
            boolean local = "127.0.0.1".equals(address.getHost()) || "localhost".equals(address.getHost());
            if (address.getRawUserInfo() != null || address.getHost() == null
                    || !("https".equals(address.getScheme()) || local && "http".equals(address.getScheme()))) return false;
            Request.Builder builder = new Request.Builder().url(url);
            JSONArray fields = new JSONArray(headers);
            for (int i = 0; i < fields.length(); i++) {
                JSONObject header = fields.getJSONObject(i);
                builder.header(header.getString("name"), header.getString("value"));
            }
            if (method.equals("POST")) builder.post(RequestBody.create(body.getBytes(StandardCharsets.UTF_8), null));
            else if (!method.equals("GET")) return false;
            if (!active(id)) return true;
            Call call = client.newCall(builder.build());
            ScheduledFuture<?> watch = cancellation.scheduleAtFixedRate(() -> {
                if (!active(id)) call.cancel();
            }, 200, 200, TimeUnit.MILLISECONDS);
            try (Response reply = call.execute()) {
                start(id, reply.code());
                ResponseBody response = reply.body();
                if (response != null) {
                    try (InputStream input = response.byteStream()) {
                        byte[] buffer = new byte[16 * 1024];
                        int count;
                        while ((count = input.read(buffer)) != -1) {
                            if (count > 0 && !push(id, buffer, count)) break;
                        }
                    }
                }
            } finally {
                watch.cancel(false);
            }
            return true;
        } catch (Exception failure) {
            return false;
        }
    }
}
