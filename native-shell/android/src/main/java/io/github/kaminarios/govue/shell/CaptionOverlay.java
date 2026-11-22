package io.github.kaminarios.govue.shell;

import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.text.Layout;
import android.text.StaticLayout;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.TextView;

import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import com.longbridge.nsh.NshView;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Shows the current caption and measures the source's visual lines for Rust. */
public final class CaptionOverlay extends ViewGroup implements NshComponent {
    private record Measurement(String text, long segment, int width, float size, boolean bold) {}

    private final NshComponentContext context;
    private final TextView cue;
    private final GradientDrawable background = new GradientDrawable();
    private final float density;
    private String source;
    private long segment;
    private float maxWidth;
    private float left;
    private float vertical;
    private float clearance;
    private boolean bold;
    private Measurement lastMeasurement;

    public static void register(NshView view) {
        view.registerComponent("govue/caption-overlay", CaptionOverlay::new);
    }

    private CaptionOverlay(NshComponentContext context) {
        super(context.getContext());
        this.context = context;
        density = getResources().getDisplayMetrics().density;
        cue = new TextView(getContext());
        cue.setGravity(Gravity.CENTER_HORIZONTAL);
        cue.setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE);
        cue.setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE);
        cue.setMaxLines(2);
        cue.setVisibility(View.GONE);
        background.setCornerRadius(4 * density);
        cue.setBackground(background);
        addView(cue);
    }

    @Override
    public View view() {
        return this;
    }

    @Override
    public void setProp(String name, Object value) {
        if (!name.equals("display")) return;
        if (!(value instanceof Map<?, ?> input)
                || !(input.get("frame") instanceof Map<?, ?> frame)
                || !(frame.get("text") instanceof String text) || text.isEmpty()
                || !(frame.get("sourceText") instanceof String sourceText)
                || !(frame.get("segmentIndex") instanceof Number index)
                || !(input.get("style") instanceof Map<?, ?> style)) {
            cue.setVisibility(View.GONE);
            lastMeasurement = null;
            return;
        }
        source = sourceText;
        segment = index.longValue();
        maxWidth = number(input, "maxWidth", 0.92f);
        left = number(input, "left", 0.04f);
        vertical = number(style, "vertical", 1);
        clearance = number(input, "clearance", 12);
        float scale = number(style, "scale", 1);
        bold = Boolean.TRUE.equals(style.get("bold"));
        cue.setText(text);
        cue.setTextSize(TypedValue.COMPLEX_UNIT_SP, 14 * scale);
        cue.setTypeface(Typeface.DEFAULT, bold ? Typeface.BOLD : Typeface.NORMAL);
        cue.setTextColor(color(style.get("color")));
        Object mode = style.get("background");
        background.setColor("none".equals(mode) ? Color.TRANSPARENT
                : "opaque".equals(mode) ? Color.BLACK : 0x99000000);
        int horizontalPadding = dp(8 * scale);
        int verticalPadding = dp(2 * scale);
        cue.setPadding(horizontalPadding, verticalPadding, horizontalPadding, verticalPadding);
        cue.setVisibility(View.VISIBLE);
        requestLayout();
    }

    private static int color(Object value) {
        if (value instanceof String text) {
            try {
                return Color.parseColor(text);
            } catch (IllegalArgumentException invalid) {
                return Color.WHITE;
            }
        }
        return Color.WHITE;
    }

    private static float number(Map<?, ?> values, String key, float fallback) {
        if (values.get(key) instanceof Number value && Float.isFinite(value.floatValue())) {
            return value.floatValue();
        }
        return fallback;
    }

    private int dp(float size) {
        return Math.round(size * density);
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int width = MeasureSpec.getSize(widthSpec);
        int height = MeasureSpec.getSize(heightSpec);
        setMeasuredDimension(width, height);
        if (cue.getVisibility() == View.VISIBLE) {
            cue.measure(MeasureSpec.makeMeasureSpec(Math.round(width * maxWidth), MeasureSpec.AT_MOST),
                    MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED));
        }
    }

    @Override
    protected void onLayout(boolean changed, int leftEdge, int topEdge, int rightEdge, int bottomEdge) {
        if (cue.getVisibility() != View.VISIBLE) return;
        int width = rightEdge - leftEdge;
        int height = bottomEdge - topEdge;
        int captionWidth = Math.round(width * maxWidth);
        int clear = dp(clearance);
        int x = Math.round(width * left) + (captionWidth - cue.getMeasuredWidth()) / 2;
        int y = clear + Math.round(Math.max(0, height - 2 * clear - cue.getMeasuredHeight()) * vertical);
        cue.layout(x, y, x + cue.getMeasuredWidth(), y + cue.getMeasuredHeight());
        measureSource(captionWidth - cue.getPaddingLeft() - cue.getPaddingRight());
    }

    private void measureSource(int width) {
        if (width <= 0 || source.isEmpty()) return;
        Measurement measurement = new Measurement(source, segment, width, cue.getTextSize(), bold);
        if (measurement.equals(lastMeasurement)) return;
        lastMeasurement = measurement;
        StaticLayout layout = StaticLayout.Builder.obtain(source, 0, source.length(), cue.getPaint(), width)
                .setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE)
                .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
                .build();
        int count = layout.getLineCount();
        if (count > 4096) return;
        List<String> lines = new ArrayList<>(count);
        for (int line = 0; line < count; line++) {
            lines.add(source.substring(layout.getLineStart(line), layout.getLineEnd(line)));
        }
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("sourceText", source);
        data.put("segmentIndex", segment);
        data.put("lines", lines);
        context.emit("layout", data);
    }
}
