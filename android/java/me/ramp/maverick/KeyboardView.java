package me.ramp.maverick;

import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.CompletionInfo;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

final class KeyboardView extends View {
    final int id;
    private final MaverickActivity activity;
    private boolean active = true;
    private Connection connection;

    KeyboardView(MaverickActivity activity, int id) {
        super(activity);
        this.activity = activity;
        this.id = id;
        setFocusable(true);
        setFocusableInTouchMode(true);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    void show() {
        if (active && hasWindowFocus()) {
            getContext().getSystemService(InputMethodManager.class).showSoftInput(this, 0);
        }
    }

    void close() {
        active = false;
        if (connection != null) connection.closeConnection();
        getContext().getSystemService(InputMethodManager.class)
            .hideSoftInputFromWindow(getWindowToken(), 0);
        clearFocus();
        if (getParent() instanceof ViewGroup) ((ViewGroup) getParent()).removeView(this);
    }

    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) show();
    }

    @Override public boolean onCheckIsTextEditor() {return active;}

    @Override public InputConnection onCreateInputConnection(EditorInfo info) {
        if (!active) return null;
        if (connection != null) connection.closeConnection();
        info.inputType = InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS;
        info.imeOptions = EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_EXTRACT_UI;
        info.initialSelStart = info.initialSelEnd = 0;
        connection = new Connection();
        return connection;
    }

    @Override public boolean dispatchKeyEvent(KeyEvent event) {
        if (!active) return false;
        if (event.getAction() == KeyEvent.ACTION_MULTIPLE && event.getCharacters() != null) {
            activity.sendText(event.getCharacters());
            return true;
        }
        return activity.sendKey(event.getKeyCode(), event.getUnicodeChar(),
            event.getAction(), event.getRepeatCount(), event.getMetaState()) || super.dispatchKeyEvent(event);
    }

    private final class Connection extends BaseInputConnection {
        private boolean closed;
        private boolean composingDelete;

        Connection() {super(KeyboardView.this, true);}
        private boolean live() {return active && !closed;}

        private void commit() {
            Editable text = getEditable();
            if (text.length() == 0) return;
            activity.sendText(text.toString());
            text.clear();
            Selection.setSelection(text, 0);
            getContext().getSystemService(InputMethodManager.class)
                .updateSelection(KeyboardView.this, 0, 0, -1, -1);
        }

        @Override public boolean commitText(CharSequence text, int position) {
            if (!live()) return false;
            super.commitText(text, position);
            commit();
            return true;
        }

        @Override public boolean setComposingText(CharSequence text, int position) {
            return live() && super.setComposingText(text, position);
        }

        @Override public boolean finishComposingText() {
            if (!live()) return false;
            super.finishComposingText();
            commit();
            return true;
        }

        @Override public boolean commitCompletion(CompletionInfo completion) {
            return commitText(completion.getText(), 1);
        }

        @Override public boolean deleteSurroundingText(int before, int after) {
            return delete(before, after, false);
        }

        @Override public boolean deleteSurroundingTextInCodePoints(int before, int after) {
            return delete(before, after, true);
        }

        private boolean delete(int before, int after, boolean codePoints) {
            if (!live() || before < 0 || after < 0) return false;
            Editable text = getEditable();
            int start = Math.max(0, Math.min(Selection.getSelectionStart(text), Selection.getSelectionEnd(text)));
            int end = Math.max(start, Math.max(Selection.getSelectionStart(text), Selection.getSelectionEnd(text)));
            int composingStart = getComposingSpanStart(text);
            int composingEnd = getComposingSpanEnd(text);
            if (composingStart >= 0 && composingEnd >= 0) {
                start = Math.min(start, Math.min(composingStart, composingEnd));
                end = Math.max(end, Math.max(composingStart, composingEnd));
            }
            int left = codePoints ? Character.codePointCount(text, 0, start) : start;
            int right = codePoints ? Character.codePointCount(text, end, text.length()) : text.length() - end;
            if (codePoints) super.deleteSurroundingTextInCodePoints(before, after);
            else super.deleteSurroundingText(before, after);
            for (int i = left; i < before; i++) key(KeyEvent.KEYCODE_DEL);
            for (int i = right; i < after; i++) key(KeyEvent.KEYCODE_FORWARD_DEL);
            return true;
        }

        private void key(int code) {
            for (int action : new int[] {KeyEvent.ACTION_DOWN, KeyEvent.ACTION_UP}) {
                activity.sendKey(code, 0, action, 0, 0);
            }
        }

        @Override public boolean sendKeyEvent(KeyEvent event) {
            if (!live()) return false;
            if (event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                if (event.getAction() == KeyEvent.ACTION_UP && composingDelete) {
                    composingDelete = false;
                    return true;
                }
                Editable text = getEditable();
                if (event.getAction() == KeyEvent.ACTION_DOWN && text.length() > 0) {
                    int end = Math.max(0, Selection.getSelectionEnd(text));
                    if (end > 0) text.delete(Character.offsetByCodePoints(text, end, -1), end);
                    composingDelete = true;
                    return true;
                }
            }
            return dispatchKeyEvent(event);
        }

        @Override public boolean performEditorAction(int action) {
            if (!live()) return false;
            finishComposingText();
            key(KeyEvent.KEYCODE_ENTER);
            return true;
        }

        @Override public void closeConnection() {
            closed = true;
            getEditable().clear();
            super.closeConnection();
        }
    }
}
