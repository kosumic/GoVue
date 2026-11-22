package io.github.kaminarios.govue.shell;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.view.View;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.io.File;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Map;

/** Hands a Rust-prepared download artifact to Android's Save As picker. */
@SuppressWarnings("deprecation")
public final class DownloadExport implements NshComponent {
    private static final String TAG = "GoVueDownloadExport";
    private final Activity activity;
    private final NshComponentContext context;
    private final View view;

    public static void register(NshView views, Activity activity) {
        views.registerComponent("govue/download-export", context -> new DownloadExport(activity, context));
    }

    private DownloadExport(Activity activity, NshComponentContext context) {
        this.activity = activity;
        this.context = context;
        view = new View(activity);
    }

    @Override
    public View view() {
        return view;
    }

    @Override
    public void setProp(String name, Object value) {}

    @Override
    public void command(String name, Object value) {
        if (!name.equals("save") || !(value instanceof Map<?, ?> fields)
                || !(fields.get("uri") instanceof String address)
                || !(fields.get("name") instanceof String filename)
                || !(fields.get("mime") instanceof String mime)) return;
        try {
            Uri uri = Uri.parse(address);
            if (!"file".equals(uri.getScheme()) || !android.text.TextUtils.isEmpty(uri.getAuthority()) || uri.getPath() == null
                    || filename.trim().isEmpty() || filename.length() > 255
                    || !mime.matches("[a-zA-Z0-9.+-]+/[a-zA-Z0-9.+-]+")) {
                throw new IllegalArgumentException("Invalid saved file");
            }
            File folder = new File(activity.getCacheDir(), "download-exports").getCanonicalFile();
            File source = new File(uri.getPath()).getCanonicalFile();
            if (!folder.equals(source.getParentFile()) || !source.getName().equals(filename)
                    || !source.isFile()) {
                throw new IllegalArgumentException("Invalid saved file");
            }
            var manager = activity.getFragmentManager();
            var picker = (Picker) manager.findFragmentByTag(TAG);
            if (picker == null) {
                picker = new Picker();
                manager.beginTransaction().add(picker, TAG).commitNow();
            }
            if (!picker.open(source, mime, this::error)) {
                error("A file picker is already open.");
            }
        } catch (Exception failure) {
            error("Save As is unavailable.");
        }
    }

    private void error(String message) {
        context.emit("error", message);
    }

    public static final class Picker extends android.app.Fragment {
        private static final int CREATE = 7147;
        private File source;
        private boolean copying;
        private java.util.function.Consumer<String> error;

        @Override
        public void onCreate(android.os.Bundle state) {
            super.onCreate(state);
            setRetainInstance(true);
        }

        boolean open(File file, String mime, java.util.function.Consumer<String> onError) {
            if (source != null || copying) return false;
            source = file;
            error = onError;
            try {
                startActivityForResult(new Intent(Intent.ACTION_CREATE_DOCUMENT)
                        .addCategory(Intent.CATEGORY_OPENABLE).setType(mime)
                        .putExtra(Intent.EXTRA_TITLE, file.getName()), CREATE);
            } catch (RuntimeException failure) {
                source = null;
                error = null;
                throw failure;
            }
            return true;
        }

        @Override
        public void onActivityResult(int request, int result, Intent data) {
            if (request != CREATE) {
                super.onActivityResult(request, result, data);
                return;
            }
            File input = source;
            var onError = error;
            source = null;
            error = null;
            Uri output = data == null ? null : data.getData();
            if (result != Activity.RESULT_OK || input == null || output == null
                    || !"content".equals(output.getScheme()) || getActivity() == null) return;
            Activity owner = getActivity();
            var resolver = owner.getApplicationContext().getContentResolver();
            copying = true;
            Thread thread = new Thread(() -> {
                boolean saved = false;
                try (InputStream from = new java.io.FileInputStream(input);
                     OutputStream to = resolver.openOutputStream(output, "w")) {
                    if (to == null) throw new java.io.IOException("Cannot open the chosen file");
                    byte[] bytes = new byte[64 * 1024];
                    int count;
                    while ((count = from.read(bytes)) != -1) to.write(bytes, 0, count);
                    to.flush();
                    saved = true;
                } catch (Exception ignored) {}
                boolean success = saved;
                owner.runOnUiThread(() -> {
                    copying = false;
                    if (!success && onError != null) onError.accept("The saved file could not be exported.");
                });
            }, "GoVue file export");
            thread.start();
        }
    }
}
