package me.ramp.maverick;

import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.text.method.TextKeyListener;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.CompletionInfo;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

import java.util.ArrayList;

final class KeyboardView extends View {
    final int id;
    private final MaverickActivity activity;
    private boolean active = true;
    private Connection connection;
    private NativeText editor;

    KeyboardView(MaverickActivity activity, int id) {
        super(activity);
        this.activity = activity;
        this.id = id;
        setFocusable(true);
        setFocusableInTouchMode(true);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
    }

    void edit(NativeText value) {
        if (editor == value) return;
        if (connection != null) connection.closeConnection();
        editor = value;
        if (isFocused()) getContext().getSystemService(InputMethodManager.class).restartInput(this);
    }

    void update(NativeText value, boolean replaced) {
        if (editor != value || !isFocused()) return;
        InputMethodManager input = getContext().getSystemService(InputMethodManager.class);
        if (replaced) {
            if (connection != null) connection.closeConnection();
            input.restartInput(this);
        } else if (connection != null && connection.batches == 0) {
            Editable text = value.document();
            if (text != null) input.updateSelection(this,
                Selection.getSelectionStart(text), Selection.getSelectionEnd(text),
                BaseInputConnection.getComposingSpanStart(text),
                BaseInputConnection.getComposingSpanEnd(text));
        }
    }

    void show() {
        if (active && isFocused() && hasWindowFocus()) {
            getContext().getSystemService(InputMethodManager.class).showSoftInput(this, 0);
        }
    }

    void close() {
        active = false;
        hide();
        if (getParent() instanceof ViewGroup) ((ViewGroup) getParent()).removeView(this);
    }

    void hide() {
        if (connection != null) connection.closeConnection();
        editor = null;
        setFocusable(false);
        getContext().getSystemService(InputMethodManager.class)
            .hideSoftInputFromWindow(getWindowToken(), 0);
    }

    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) show();
    }

    @Override public boolean onCheckIsTextEditor() {return active;}

    @Override public InputConnection onCreateInputConnection(EditorInfo info) {
        if (!active) return null;
        if (connection != null) connection.closeConnection();
        info.inputType = InputType.TYPE_CLASS_TEXT | (editor == null ?
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS : InputType.TYPE_TEXT_FLAG_AUTO_CORRECT
                | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        info.imeOptions = EditorInfo.IME_ACTION_DONE | EditorInfo.IME_FLAG_NO_EXTRACT_UI;
        Editable document = editor == null ? null : editor.document();
        info.initialSelStart = document == null ? 0 : Selection.getSelectionStart(document);
        info.initialSelEnd = document == null ? 0 : Selection.getSelectionEnd(document);
        if (document != null) info.setInitialSurroundingText(document);
        connection = new Connection(editor);
        return connection;
    }

    @Override public boolean dispatchKeyEvent(KeyEvent event) {
        if (!active) return false;
        if (connection != null && editor != null && connection.keyEvent(event)) return true;
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
        private final NativeText owner;
        private int batches;
        private final ArrayList<KeyEvent> commands = new ArrayList<>();

        Connection(NativeText owner) {super(KeyboardView.this, true); this.owner = owner;}
        private boolean live() {return active && !closed && (owner == null || owner.document() != null);}
        @Override public Editable getEditable() {
            if (closed) return null;
            return owner == null ? super.getEditable() : owner.document();
        }
        @Override public boolean beginBatchEdit() {
            if (!live()) return false;
            batches++;
            if (owner != null) owner.beginInput();
            return true;
        }
        @Override public boolean endBatchEdit() {
            if (!live() || batches == 0) return false;
            batches--;
            if (owner != null) owner.endInput();
            if (batches == 0 && owner != null) {
                for (KeyEvent event : commands) owner.sendCommand(event.getKeyCode(), event.getMetaState());
                commands.clear();
            }
            return batches > 0;
        }

        private void command(int code, int meta) {
            finishComposingText();
            if (owner == null) {key(code); return;}
            if (batches == 0) owner.sendCommand(code, meta);
            else commands.add(new KeyEvent(0, 0, KeyEvent.ACTION_DOWN, code, 0, meta));
        }

        private boolean keyEvent(KeyEvent event) {
            if (!live() || owner == null) return false;
            int code = event.getKeyCode(), action = event.getAction();
            if (code == KeyEvent.KEYCODE_ENTER || code == KeyEvent.KEYCODE_TAB) {
                if (action == KeyEvent.ACTION_DOWN) command(code, event.getMetaState());
                return true;
            }
            if (event.isCtrlPressed() || event.isMetaPressed()) {
                int command;
                switch (code) {
                    case KeyEvent.KEYCODE_A: command = android.R.id.selectAll; break;
                    case KeyEvent.KEYCODE_C: command = android.R.id.copy; break;
                    case KeyEvent.KEYCODE_X: command = android.R.id.cut; break;
                    case KeyEvent.KEYCODE_V: command = android.R.id.paste; break;
                    case KeyEvent.KEYCODE_Z:
                        command = event.isShiftPressed() ? android.R.id.redo : android.R.id.undo; break;
                    case KeyEvent.KEYCODE_Y: command = android.R.id.redo; break;
                    default: return false;
                }
                if (action == KeyEvent.ACTION_DOWN) owner.onTextContextMenuItem(command);
                return true;
            }
            if (event.isAltPressed()) return false;
            if (code != KeyEvent.KEYCODE_DEL && code != KeyEvent.KEYCODE_FORWARD_DEL
                && event.getUnicodeChar() == 0 && event.getCharacters() == null) return false;
            beginBatchEdit();
            try {
                TextKeyListener listener = TextKeyListener.getInstance(true, TextKeyListener.Capitalize.NONE);
                if (action == KeyEvent.ACTION_DOWN) return listener.onKeyDown(KeyboardView.this,
                    getEditable(), code, event);
                if (action == KeyEvent.ACTION_UP) {
                    listener.onKeyUp(KeyboardView.this, getEditable(), code, event);
                    return true;
                }
                return listener.onKeyOther(KeyboardView.this, getEditable(), event);
            } finally {endBatchEdit();}
        }

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
            if (owner != null && (text.toString().equals("\n") || text.toString().equals("\r")
                || text.toString().equals("\r\n") || text.toString().equals("\t"))) {
                command(text.charAt(0) == '\t' ? KeyEvent.KEYCODE_TAB : KeyEvent.KEYCODE_ENTER, 0);
                return true;
            }
            super.commitText(text, position);
            if (owner == null) commit();
            return true;
        }

        @Override public boolean setComposingText(CharSequence text, int position) {
            return live() && super.setComposingText(text, position);
        }

        @Override public boolean finishComposingText() {
            if (!live()) return false;
            super.finishComposingText();
            if (owner == null) commit();
            return true;
        }

        @Override public boolean setComposingRegion(int start, int end) {
            return live() && super.setComposingRegion(start, end);
        }

        @Override public boolean setSelection(int start, int end) {
            if (!live()) return false;
            beginBatchEdit();
            try {return super.setSelection(start, end);}
            finally {endBatchEdit();}
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
            if (owner != null) return codePoints ?
                super.deleteSurroundingTextInCodePoints(before, after) :
                super.deleteSurroundingText(before, after);
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
            if (owner != null) return KeyboardView.this.dispatchKeyEvent(event);
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

        @Override public boolean performContextMenuAction(int command) {
            return live() && owner != null && owner.onTextContextMenuItem(command);
        }

        @Override public boolean performEditorAction(int action) {
            if (!live()) return false;
            command(KeyEvent.KEYCODE_ENTER, 0);
            return true;
        }

        @Override public void closeConnection() {
            if (closed) return;
            Editable text = getEditable();
            if (text != null) {
                removeComposingSpans(text);
                if (owner == null) text.clear();
            }
            if (owner != null) while (batches > 0) {batches--; owner.cancelInput();}
            commands.clear();
            closed = true;
            super.closeConnection();
        }
    }
}
