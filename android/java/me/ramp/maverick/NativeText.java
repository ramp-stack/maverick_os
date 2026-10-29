package me.ramp.maverick;

import android.app.Dialog;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.fonts.Font;
import android.graphics.fonts.FontFamily;
import android.os.Looper;
import android.text.Editable;
import android.text.Layout;
import android.text.Selection;
import android.text.Spannable;
import android.text.SpannableString;
import android.text.TextPaint;
import android.text.style.LineHeightSpan;
import android.text.style.MetricAffectingSpan;
import android.util.TypedValue;
import android.view.ActionMode;
import android.view.Gravity;
import android.view.Menu;
import android.view.MenuItem;
import android.view.MotionEvent;
import android.view.ViewConfiguration;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.TextView;
import org.json.JSONArray;
import org.json.JSONObject;

import java.nio.ByteBuffer;
import java.util.concurrent.FutureTask;

public final class NativeText extends TextView implements ActionMode.Callback {
    private final MaverickActivity activity;
    private final String identity;
    private final Dialog overlay;
    private long events;
    private volatile boolean active = true;
    private boolean applying;
    private boolean editable;
    private boolean focused;
    private boolean selecting;
    private ActionMode selectionMode;
    private boolean pressed;
    private boolean scrolling;
    private float downX;
    private float downY;
    private float lastX;
    private float lastY;
    private float scale = 1;
    private String appearance;
    private int inputBatches;
    private long revision;

    public static NativeText create(MaverickActivity activity, String id) throws Exception {
        if (Looper.myLooper() == Looper.getMainLooper()) return new NativeText(activity, id);
        FutureTask<NativeText> result = new FutureTask<>(() -> new NativeText(activity, id));
        activity.runOnUiThread(result);
        return result.get();
    }

    public static Typeface font(byte[] bytes) throws Exception {
        ByteBuffer buffer = ByteBuffer.allocateDirect(bytes.length);
        buffer.put(bytes).flip();
        Font font = new Font.Builder(buffer).build();
        return new Typeface.CustomFallbackBuilder(new FontFamily.Builder(font).build()).build();
    }

    public void update(String data, Typeface[] fonts) {
        activity.runOnUiThread(() -> {
            if (!active || activity.isDestroyed()) return;
            try {apply(new JSONObject(data), fonts);}
            catch (org.json.JSONException error) {throw new IllegalArgumentException(error);}
        });
    }

    public synchronized void close() {
        active = false;
        activity.runOnUiThread(() -> {
            activity.editText(this, false);
            overlay.dismiss();
        });
    }

    Editable document() {return active && editable ? getEditableText() : null;}
    void beginInput() {inputBatches++; super.beginBatchEdit();}
    void cancelInput() {super.endBatchEdit(); inputBatches--;}
    void endInput() {
        super.endBatchEdit();
        if (--inputBatches == 0) publish();
    }

    private synchronized void publish() {
        if (!active || applying || !editable || inputBatches != 0 || events == 0) return;
        int start = getSelectionStart(), end = getSelectionEnd();
        if (start < 0 || end < 0) return;
        revision = activity.nextTextRevision();
        textReplaced(identity, getText().toString(), Math.min(start, end), Math.max(start, end), revision);
        activity.updateText(this, false);
    }

    synchronized void sendCommand(int code, int meta) {
        if (active && events != 0) command(code, meta);
    }

    @Override public boolean onTouchEvent(MotionEvent event) {
        if (!active) return false;
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                downX = lastX = event.getRawX();
                downY = lastY = event.getRawY();
                scrolling = false;
                pressed = !selecting;
                if (pressed) {
                    sendPointer(event, 0, 0, 0);
                    sendPointer(event, 1, 0, 0);
                }
                break;
            case MotionEvent.ACTION_MOVE:
                if (pressed && !selecting) {
                    float distance = (float) Math.hypot(event.getRawX() - downX, event.getRawY() - downY);
                    if (!scrolling && distance > ViewConfiguration.get(activity).getScaledTouchSlop()) {
                        scrolling = true;
                        cancelLongPress();
                        MotionEvent cancel = MotionEvent.obtain(event);
                        cancel.setAction(MotionEvent.ACTION_CANCEL);
                        super.onTouchEvent(cancel);
                        cancel.recycle();
                    }
                    sendPointer(event, 3, lastX - event.getRawX(), lastY - event.getRawY());
                    lastX = event.getRawX();
                    lastY = event.getRawY();
                }
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_CANCEL:
                pressed = false;
                if (event.getActionMasked() == MotionEvent.ACTION_CANCEL && selectionMode == null) {
                    selecting = false;
                }
                break;
            default: break;
        }
        if (scrolling) return true;
        return super.onTouchEvent(event);
    }

    @Override public boolean performLongClick() {
        boolean handled = super.performLongClick();
        if (handled) {
            // Android starts ActionMode after finger-up; selection owns the drag before then.
            selecting = true;
            pressed = false;
        }
        return handled;
    }

    @Override public boolean onCreateActionMode(ActionMode mode, Menu menu) {
        selecting = true;
        selectionMode = mode;
        pressed = false;
        return true;
    }

    @Override public ActionMode startActionMode(ActionMode.Callback callback, int type) {
        return ((ViewGroup) activity.getWindow().getDecorView())
            .startActionModeForChild(this, callback, type);
    }

    @Override public boolean onPrepareActionMode(ActionMode mode, Menu menu) {
        if (editable) {
            if (menu.findItem(android.R.id.cut) == null) {
                menu.add(0, android.R.id.cut, 1, android.R.string.cut);
            }
            if (activity.getSystemService(ClipboardManager.class).hasPrimaryClip()
                && menu.findItem(android.R.id.paste) == null) {
                menu.add(0, android.R.id.paste, 2, android.R.string.paste);
            }
        }
        return true;
    }

    @Override public boolean onActionItemClicked(ActionMode mode, MenuItem item) {
        if (item.getItemId() != android.R.id.cut && item.getItemId() != android.R.id.paste) return false;
        boolean handled = onTextContextMenuItem(item.getItemId());
        if (handled) mode.finish();
        return handled;
    }

    @Override public void onDestroyActionMode(ActionMode mode) {
        selectionMode = null;
        selecting = false;
    }

    @Override public boolean onTextContextMenuItem(int command) {
        if (editable && (command == android.R.id.undo || command == android.R.id.redo)) {
            beginInput();
            try {return super.onTextContextMenuItem(command);}
            finally {endInput();}
        }
        if (!editable || (command != android.R.id.cut && command != android.R.id.paste)) {
            return super.onTextContextMenuItem(command);
        }
        String text = getText().toString();
        int start = Math.max(0, Math.min(getSelectionStart(), getSelectionEnd()));
        int end = Math.max(start, Math.max(getSelectionStart(), getSelectionEnd()));
        ClipboardManager clipboard = activity.getSystemService(ClipboardManager.class);
        String replacement = "";
        if (command == android.R.id.cut) {
            clipboard.setPrimaryClip(ClipData.newPlainText("", text.substring(start, end)));
        } else {
            ClipData clip = clipboard.getPrimaryClip();
            if (clip == null || clip.getItemCount() == 0) return false;
            replacement = clip.getItemAt(0).coerceToText(activity).toString();
        }
        beginInput();
        try {
            Editable document = getEditableText();
            android.view.inputmethod.BaseInputConnection.removeComposingSpans(document);
            document.replace(start, end, replacement);
            Selection.setSelection(document, start + replacement.length());
        } finally {endInput();}
        return true;
    }

    @Override protected void onSelectionChanged(int start, int end) {
        super.onSelectionChanged(start, end);
        synchronized (this) {
            if (!active || applying || !editable || inputBatches != 0
                || events == 0 || start < 0 || end < 0) return;
            revision = activity.nextTextRevision();
            selectionChanged(identity, getText().toString(),
                Math.min(start, end), Math.max(start, end), revision);
            activity.updateText(this, false);
        }
    }

    private native void selectionChanged(String id, String text, int start, int end, long revision);
    private native void textReplaced(String id, String text, int start, int end, long revision);
    private native void command(int code, int meta);
    private native void pointer(float x, float y, int state, float dx, float dy);

    private NativeText(MaverickActivity activity, String id) {
        super(activity);
        this.activity = activity;
        identity = id;
        setTextIsSelectable(true);
        setTextColor(Color.TRANSPARENT);
        setBackgroundColor(Color.TRANSPARENT);
        setPadding(0, 0, 0, 0);
        setIncludeFontPadding(false);
        setCursorVisible(false);
        setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE);
        setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE);
        setCustomSelectionActionModeCallback(this);
        setCustomInsertionActionModeCallback(this);
        FrameLayout clip = new FrameLayout(activity);
        clip.setClipChildren(true);
        clip.addView(this);
        // Android disables native text handles inside PopupWindow subwindows.
        overlay = new Dialog(activity);
        overlay.requestWindowFeature(Window.FEATURE_NO_TITLE);
        overlay.setContentView(clip);
        Window window = overlay.getWindow();
        window.setBackgroundDrawable(new ColorDrawable(Color.TRANSPARENT));
        window.clearFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND);
        window.addFlags(WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
            | WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
            | WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH
            | WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN
            | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS);
        window.setDecorFitsSystemWindows(false);
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_NOTHING);
        window.setGravity(Gravity.TOP | Gravity.LEFT);
        window.setWindowAnimations(0);
        window.getDecorView().setOnTouchListener((view, event) -> {
            if (event.getActionMasked() != MotionEvent.ACTION_OUTSIDE) return false;
            applying = true;
            try {
                if (selectionMode != null) selectionMode.finish();
                selecting = false;
                clearFocus();
            } finally {applying = false;}
            return false;
        });
    }

    private void apply(JSONObject data, Typeface[] fonts) throws org.json.JSONException {
        applying = true;
        try {
            scale = (float) data.getDouble("scale");
            if (data.getLong("revision") >= revision) {
                editable = !data.isNull("cursor");
                if (focused && !data.getBoolean("focused")) clearFocus();
                focused = data.getBoolean("focused");
                String value = data.getString("text");
                int cursor = editable ? data.getInt("cursor") : 0;
                int anchor = editable && !data.isNull("anchor") ? data.getInt("anchor") : cursor;
                boolean replaced = !getText().toString().equals(value);
                boolean documentChanged = replaced
                    || getSelectionStart() != anchor || getSelectionEnd() != cursor;
                JSONArray spans = data.getJSONArray("spans");
                JSONObject first = spans.getJSONObject(0);
                float size = (float) first.getDouble("size");
                setTextSize(TypedValue.COMPLEX_UNIT_PX, size);
                setTypeface(fonts[0]);
                setLetterSpacing((float) first.getDouble("kerning") / size);
                setGravity(Gravity.TOP | new int[] {Gravity.LEFT, Gravity.CENTER_HORIZONTAL, Gravity.RIGHT}
                    [data.getInt("align")]);
                if (!first.isNull("height")) setLineHeight((int) Math.ceil(first.getDouble("height")));
                String style = spans.toString();
                if (replaced && selectionMode != null) selectionMode.finish();
                if (replaced) selecting = false;
                if (replaced || !style.equals(appearance)
                    || (editable && getEditableText() == null)) {
                    int selectionStart = getSelectionStart(), selectionEnd = getSelectionEnd();
                    SpannableString styled = new SpannableString(value);
                    for (int index = 0; index < spans.length(); index++) {
                        JSONObject span = spans.getJSONObject(index);
                        int start = span.getInt("start"), end = span.getInt("end");
                        if (start == end) continue;
                        styled.setSpan(new Style(fonts[index], (float) span.getDouble("size"),
                            (float) span.getDouble("kerning")), start, end,
                            Spannable.SPAN_EXCLUSIVE_EXCLUSIVE);
                        if (!span.isNull("height")) {
                            styled.setSpan(new LineHeightSpan.Standard(
                                (int) Math.ceil(span.getDouble("height"))), start, end,
                                Spannable.SPAN_EXCLUSIVE_EXCLUSIVE);
                        }
                    }
                    if (!replaced && getText() instanceof Editable && editable) {
                        Editable document = getEditableText();
                        for (Style old : document.getSpans(0, document.length(), Style.class)) {
                            document.removeSpan(old);
                        }
                        for (LineHeightSpan old : document.getSpans(
                            0, document.length(), LineHeightSpan.class)) {
                            document.removeSpan(old);
                        }
                        for (Object span : styled.getSpans(0, styled.length(), Object.class)) {
                            document.setSpan(span, styled.getSpanStart(span), styled.getSpanEnd(span),
                                styled.getSpanFlags(span));
                        }
                    } else {setText(styled, editable ? BufferType.EDITABLE : BufferType.SPANNABLE);}
                    appearance = style;
                    if (!editable && selecting && selectionStart >= 0 && selectionEnd >= 0) {
                        Selection.setSelection((Spannable) getText(),
                            Math.min(selectionStart, value.length()), Math.min(selectionEnd, value.length()));
                    }
                }
                if (editable && documentChanged && getText() instanceof Spannable) {
                    cursor = Math.min(cursor, getText().length());
                    anchor = Math.min(anchor, getText().length());
                    if (anchor == cursor && selectionMode != null) selectionMode.finish();
                    if (anchor == cursor) selecting = false;
                    Selection.setSelection((Spannable) getText(), anchor, cursor);
                }
                activity.editText(this, focused && editable);
                if (editable && documentChanged) activity.updateText(this, replaced);
            }
            if (!data.getBoolean("visible")) {overlay.hide(); return;}
            int left = (int) Math.floor(data.getDouble("left"));
            int top = (int) Math.floor(data.getDouble("top"));
            int width = (int) Math.ceil(data.getDouble("right")) - left;
            int height = (int) Math.ceil(data.getDouble("bottom")) - top;
            FrameLayout.LayoutParams bounds = new FrameLayout.LayoutParams(
                (int) Math.ceil(data.getDouble("width")), (int) Math.ceil(data.getDouble("height")));
            bounds.leftMargin = (int) Math.floor(data.getDouble("x")) - left;
            bounds.topMargin = (int) Math.floor(data.getDouble("y")) - top;
            setLayoutParams(bounds);
            scrollTo(0, 0);
            int[] origin = new int[2];
            activity.getWindow().getDecorView().getLocationOnScreen(origin);
            left += origin[0];
            top += origin[1];
            Window window = overlay.getWindow();
            WindowManager.LayoutParams position = window.getAttributes();
            position.setFitInsetsTypes(0);
            position.x = left;
            position.y = top;
            position.width = width;
            position.height = height;
            window.setAttributes(position);
            if (!overlay.isShowing()) overlay.show();
        } finally {applying = false;}
    }

    private synchronized void sendPointer(MotionEvent event, int state, float dx, float dy) {
        if (!active || events == 0) return;
        int[] origin = new int[2];
        activity.getWindow().getDecorView().getLocationOnScreen(origin);
        pointer((event.getRawX() - origin[0]) / scale, (event.getRawY() - origin[1]) / scale,
            state, dx / scale, dy / scale);
    }

    private static final class Style extends MetricAffectingSpan {
        private final Typeface font;
        private final float size;
        private final float kerning;

        Style(Typeface font, float size, float kerning) {
            this.font = font;
            this.size = size;
            this.kerning = kerning;
        }

        @Override public void updateDrawState(TextPaint paint) {updateMeasureState(paint);}
        @Override public void updateMeasureState(TextPaint paint) {
            paint.setTypeface(font);
            paint.setTextSize(size);
            paint.setLetterSpacing(kerning / size);
        }
    }
}
