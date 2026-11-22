package io.github.kaminarios.govue.shell;

import android.app.Activity;
import android.app.Fragment;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.provider.MediaStore;
import android.view.View;

import androidx.core.content.FileProvider;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.io.File;
import java.io.IOException;
import java.util.Map;
import java.util.UUID;
import java.util.function.Consumer;

/** Owns the system camera request and its private output file until Rust imports it. */
@SuppressWarnings("deprecation")
public final class ChatCamera implements NshComponent {
    private static final String TAG = "GoVueChatCamera";
    private static final int ACCESS = Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
    private final Activity activity;
    private final NshComponentContext context;
    private final View view;
    private long lastRequest;

    public static void register(NshView host, Activity activity) {
        host.registerComponent("govue/chat-camera", context -> new ChatCamera(activity, context));
    }

    private ChatCamera(Activity activity, NshComponentContext context) {
        this.activity = activity;
        this.context = context;
        view = new View(activity);
    }

    @Override public View view() { return view; }

    @Override
    public void setProp(String name, Object value) {
        if (!name.equals("capture") || !(value instanceof Number number) || number.longValue() <= lastRequest) return;
        long request = lastRequest = number.longValue();
        try {
            var manager = activity.getFragmentManager();
            var camera = (Capture) manager.findFragmentByTag(TAG);
            if (camera == null) {
                camera = new Capture();
                manager.beginTransaction().add(camera, TAG).commitNow();
            }
            camera.takePhoto(result -> {
                if (result.error() != null) {
                    context.emit("capture", Map.of("id", request, "error", result.error()));
                } else if (result.file() != null) {
                    context.emit("capture", Map.of("id", request, "file", result.file()));
                } else {
                    context.emit("capture", Map.of("id", request));
                }
            });
        } catch (RuntimeException ignored) {
            context.emit("capture", Map.of("id", request, "error", "The camera is unavailable."));
        }
    }

    private record Result(String file, String error) {}

    public static final class Capture extends Fragment {
        private static final int REQUEST = 7148;
        private File output;
        private Uri uri;
        private Consumer<Result> completion;

        @Override
        public void onCreate(android.os.Bundle state) {
            super.onCreate(state);
            setRetainInstance(true);
        }

        private void takePhoto(Consumer<Result> reply) {
            if (completion != null) {
                reply.accept(new Result(null, "The camera is already open."));
                return;
            }
            completion = reply;
            try {
                Activity owner = getActivity();
                if (owner == null) throw new IOException();
                File directory = new File(owner.getCacheDir(), "govue-chat-camera");
                if (!directory.isDirectory() && !directory.mkdirs()) throw new IOException();
                output = new File(directory, UUID.randomUUID() + ".jpg");
                if (!output.createNewFile()) throw new IOException();
                uri = FileProvider.getUriForFile(owner, owner.getPackageName() + ".chat-camera", output);
                Intent intent = new Intent(MediaStore.ACTION_IMAGE_CAPTURE)
                        .putExtra(MediaStore.EXTRA_OUTPUT, uri).addFlags(ACCESS);
                intent.setClipData(ClipData.newRawUri("GoVue photo", uri));
                startActivityForResult(intent, REQUEST);
            } catch (IOException | RuntimeException ignored) {
                finish(false, "The camera is unavailable.");
            }
        }

        @Override
        public void onActivityResult(int request, int code, Intent data) {
            if (request != REQUEST) {
                super.onActivityResult(request, code, data);
                return;
            }
            boolean taken = code == Activity.RESULT_OK;
            boolean saved = taken && output != null && output.length() > 0;
            finish(saved, taken && !saved ? "Could not read the photo." : null);
        }

        private void finish(boolean keep, String error) {
            Activity owner = getActivity();
            if (uri != null && owner != null) owner.revokeUriPermission(uri, ACCESS);
            var reply = completion;
            String name = keep && output != null ? output.getName() : null;
            if ((!keep || reply == null) && output != null) output.delete();
            output = null;
            uri = null;
            completion = null;
            if (reply != null) reply.accept(new Result(name, error));
        }

        @Override public void onDestroy() {
            finish(false, null);
            super.onDestroy();
        }
    }

    public static final class Provider extends FileProvider {}
}
