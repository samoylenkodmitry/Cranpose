package dev.cranpose.android;

import android.app.Activity;
import android.content.Context;
import android.graphics.Rect;
import android.text.Editable;
import android.text.InputType;
import android.text.Selection;
import android.text.SpannableStringBuilder;
import android.util.Log;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewTreeObserver;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;

import java.nio.charset.StandardCharsets;

/**
 * Invisible editor view giving cranpose text fields a real {@link InputConnection}.
 *
 * <p>A {@code NativeActivity} has no Java view overriding
 * {@code onCreateInputConnection}, so IMEs fall back to raw key events and
 * features that need a real editor - autocorrect/suggestions, swipe typing,
 * voice input and composing scripts such as CJK - do not work. This helper
 * attaches a focusable 1x1 view to the activity content view whenever a
 * cranpose text field gains focus; its {@link InputConnection} forwards
 * editing operations (commit, compose, delete-surrounding, key events,
 * editor actions) through JNI into the native text pipeline.
 *
 * <p>The Java side keeps an {@link Editable} mirror of the field so the IME's
 * synchronous text queries ({@code getTextBeforeCursor} and friends, served
 * by {@link BaseInputConnection}) never have to block on the native thread.
 * The native side owns the truth: it pushes state back via
 * {@link #updateState} after applying the forwarded operations, restarting
 * the input session whenever the two sides diverge (for example when the app
 * rejects or transforms input).
 *
 * <p>All methods are called from native code; view and IME operations are
 * posted to the UI thread. Include this source (and the {@code cranpose}
 * native library) in the Android app build - see
 * {@code cranpose/android/java}.
 */
public final class CranposeTextInput {
    private static final String TAG = "CranposeTextInput";

    private static volatile SessionState current;

    private CranposeTextInput() {
    }

    /**
     * Opens (or refreshes) the text-input session: attaches the editor view,
     * seeds the {@link Editable} mirror, focuses the view and asks the IME to
     * show. Selection offsets are UTF-16 code units into {@code text}.
     *
     * <p>{@code queueHandle} transfers ownership of one native queue
     * reference to this class; it is released when the session is hidden (or
     * immediately when a session already owns a handle).
     */
    public static int show(
            Activity activity,
            long queueHandle,
            String text,
            int selStart,
            int selEnd,
            boolean singleLine
    ) {
        try {
            activity.runOnUiThread(() -> {
                SessionState state = current;
                if (state == null) {
                    state = new SessionState(queueHandle);
                    state.editable = new SpannableStringBuilder(text);
                    state.singleLine = singleLine;
                    EditorView view = new EditorView(activity, state);
                    state.view = view;
                    setSelectionClamped(state.editable, selStart, selEnd);

                    ViewGroup content = activity.findViewById(android.R.id.content);
                    if (content == null) {
                        Log.w(TAG, "Activity has no content view; cannot install IME editor");
                        state.release();
                        return;
                    }
                    content.addView(view, new ViewGroup.LayoutParams(1, 1));
                    attachKeyboardListener(state, content);
                    current = state;
                } else {
                    // The session already owns a queue handle; drop the new one.
                    nativeImeReleaseQueue(queueHandle);
                    state.singleLine = singleLine;
                    state.editable.replace(0, state.editable.length(), text);
                    BaseInputConnection.removeComposingSpans(state.editable);
                    setSelectionClamped(state.editable, selStart, selEnd);
                    InputMethodManager imm = inputMethodManager(activity);
                    if (imm != null) {
                        // Re-seeding always restarts: the previous session's
                        // composition state is no longer meaningful (and the
                        // editor options may have changed).
                        imm.restartInput(state.view);
                    }
                }

                SessionState active = current;
                if (active == null || active.view == null) {
                    return;
                }
                active.view.requestFocus();
                InputMethodManager imm = inputMethodManager(activity);
                if (imm != null) {
                    imm.showSoftInput(active.view, 0);
                }
            });
            return 0;
        } catch (RuntimeException error) {
            Log.w(TAG, "Failed to show cranpose text input", error);
            return -1;
        }
    }

    /** Closes the session: hides the IME, detaches the editor view. */
    public static void hide(Activity activity) {
        try {
            activity.runOnUiThread(() -> {
                SessionState state = current;
                if (state == null) {
                    return;
                }
                current = null;
                detachKeyboardListener(state);
                InputMethodManager imm = inputMethodManager(activity);
                if (imm != null && state.view != null) {
                    imm.hideSoftInputFromWindow(state.view.getWindowToken(), 0);
                }
                if (state.view != null && state.view.getParent() instanceof ViewGroup) {
                    ((ViewGroup) state.view.getParent()).removeView(state.view);
                }
                state.release();
            });
        } catch (RuntimeException error) {
            Log.w(TAG, "Failed to hide cranpose text input", error);
        }
    }

    /**
     * Pushes the native editor state into the mirror. Selection/composition
     * offsets are UTF-16 code units; composition -1/-1 means none.
     *
     * <p>If the text content already matches, only the selection (and the
     * IME's view of it via {@code updateSelection}) is refreshed - this is
     * the common case after the native side applied a forwarded operation
     * verbatim. A content mismatch means the two sides diverged, so the
     * whole input session is restarted.
     */
    public static void updateState(
            Activity activity,
            String text,
            int selStart,
            int selEnd,
            int compStart,
            int compEnd
    ) {
        try {
            activity.runOnUiThread(() -> {
                SessionState state = current;
                if (state == null || state.disposed || state.view == null) {
                    return;
                }
                boolean textChanged = !contentEquals(state.editable, text);
                if (textChanged) {
                    state.editable.replace(0, state.editable.length(), text);
                    BaseInputConnection.removeComposingSpans(state.editable);
                }
                setSelectionClamped(state.editable, selStart, selEnd);
                InputMethodManager imm = inputMethodManager(activity);
                if (imm == null) {
                    return;
                }
                if (textChanged) {
                    imm.restartInput(state.view);
                } else {
                    int length = state.editable.length();
                    int candStart = compStart >= 0 ? Math.min(compStart, length) : -1;
                    int candEnd = compEnd >= 0 ? Math.min(compEnd, length) : -1;
                    imm.updateSelection(
                            state.view,
                            Selection.getSelectionStart(state.editable),
                            Selection.getSelectionEnd(state.editable),
                            candStart,
                            candEnd
                    );
                }
            });
        } catch (RuntimeException error) {
            Log.w(TAG, "Failed to update cranpose text input state", error);
        }
    }

    private static InputMethodManager inputMethodManager(Context context) {
        return (InputMethodManager) context.getSystemService(Context.INPUT_METHOD_SERVICE);
    }

    private static boolean contentEquals(Editable editable, String text) {
        int length = editable.length();
        if (length != text.length()) {
            return false;
        }
        for (int i = 0; i < length; i++) {
            if (editable.charAt(i) != text.charAt(i)) {
                return false;
            }
        }
        return true;
    }

    private static void setSelectionClamped(Editable editable, int start, int end) {
        int length = editable.length();
        int clampedStart = Math.max(0, Math.min(start, length));
        int clampedEnd = Math.max(0, Math.min(end, length));
        Selection.setSelection(editable, clampedStart, clampedEnd);
    }

    private static int utf8Length(CharSequence text) {
        return text.toString().getBytes(StandardCharsets.UTF_8).length;
    }

    /** Session state shared between the editor view and its InputConnection. */
    private static final class SessionState {
        final long queueHandle;
        volatile View view;
        volatile boolean disposed;
        volatile boolean singleLine;
        Editable editable;
        /** Content view the keyboard-height listener is attached to. */
        View insetsView;
        ViewTreeObserver.OnGlobalLayoutListener keyboardListener;
        int lastKeyboardHeightPx = -1;

        SessionState(long queueHandle) {
            this.queueHandle = queueHandle;
        }

        void release() {
            if (!disposed) {
                disposed = true;
                view = null;
                nativeImeReleaseQueue(queueHandle);
            }
        }
    }

    /**
     * Attaches a global-layout listener that reports the on-screen keyboard's
     * height (physical px, 0 when hidden) via {@link #nativeImeInsetsChanged}.
     *
     * <p>Uses the visible-display-frame heuristic (the portion of the window not
     * covered by the keyboard), which works on every supported API level — the
     * {@code WindowInsets.Type.ime()} API only exists on API 30+. The reported
     * height is the distance from the visible frame's bottom to the window
     * bottom, i.e. how much the keyboard covers; small values (system bars with
     * no keyboard) are treated as hidden.
     */
    private static void attachKeyboardListener(SessionState state, final ViewGroup content) {
        final View root = content.getRootView();
        ViewTreeObserver.OnGlobalLayoutListener listener = () -> {
            if (state.disposed) {
                return;
            }
            Rect frame = new Rect();
            root.getWindowVisibleDisplayFrame(frame);
            int windowHeight = root.getHeight();
            int covered = windowHeight - frame.bottom;
            // Ignore small margins (navigation bar / rounding) that are not the
            // keyboard, so a hidden keyboard reports zero.
            int keyboardHeight = covered > windowHeight / 6 ? covered : 0;
            if (keyboardHeight != state.lastKeyboardHeightPx) {
                state.lastKeyboardHeightPx = keyboardHeight;
                nativeImeInsetsChanged(state.queueHandle, keyboardHeight);
            }
        };
        root.getViewTreeObserver().addOnGlobalLayoutListener(listener);
        state.insetsView = root;
        state.keyboardListener = listener;
    }

    private static void detachKeyboardListener(SessionState state) {
        if (state.insetsView != null && state.keyboardListener != null) {
            state.insetsView.getViewTreeObserver()
                    .removeOnGlobalLayoutListener(state.keyboardListener);
        }
        if (state.lastKeyboardHeightPx != 0 && !state.disposed) {
            nativeImeInsetsChanged(state.queueHandle, 0);
        }
        state.insetsView = null;
        state.keyboardListener = null;
    }

    /** Invisible focusable view whose InputConnection feeds the native pipeline. */
    private static final class EditorView extends View {
        private final SessionState state;

        EditorView(Context context, SessionState state) {
            super(context);
            this.state = state;
            setFocusable(true);
            setFocusableInTouchMode(true);
        }

        @Override
        public boolean onCheckIsTextEditor() {
            return true;
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            if (state.disposed) {
                return null;
            }
            outAttrs.inputType = state.singleLine
                    ? InputType.TYPE_CLASS_TEXT
                    : InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE;
            // NO_EXTRACT_UI/NO_FULLSCREEN: the fullscreen extract editor would
            // hide the composed UI entirely.
            outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_FULLSCREEN
                    | EditorInfo.IME_FLAG_NO_EXTRACT_UI
                    | (state.singleLine ? EditorInfo.IME_ACTION_DONE : EditorInfo.IME_ACTION_NONE);
            outAttrs.initialSelStart = Selection.getSelectionStart(state.editable);
            outAttrs.initialSelEnd = Selection.getSelectionEnd(state.editable);
            return new Connection(this, state);
        }
    }

    /**
     * Forwards IME editing operations to native code while keeping the local
     * {@link Editable} mirror in sync via {@link BaseInputConnection}'s
     * full-editor implementation (which also serves all text queries).
     */
    private static final class Connection extends BaseInputConnection {
        private final SessionState state;

        Connection(View view, SessionState state) {
            super(view, true);
            this.state = state;
        }

        @Override
        public Editable getEditable() {
            return state.editable;
        }

        @Override
        public boolean commitText(CharSequence text, int newCursorPosition) {
            if (!state.disposed) {
                nativeImeCommitText(state.queueHandle, text.toString(), newCursorPosition);
            }
            return super.commitText(text, newCursorPosition);
        }

        @Override
        public boolean setComposingText(CharSequence text, int newCursorPosition) {
            if (!state.disposed) {
                // The native side positions the cursor inside the preedit in
                // UTF-8 bytes. IMEs practically always pass 1 (cursor after
                // the composed text); other offsets degrade to the ends.
                int cursorBytes = newCursorPosition > 0 ? utf8Length(text) : 0;
                nativeImeSetComposingText(state.queueHandle, text.toString(), cursorBytes);
            }
            return super.setComposingText(text, newCursorPosition);
        }

        @Override
        public boolean setComposingRegion(int start, int end) {
            if (!state.disposed) {
                Editable editable = getEditable();
                int length = editable.length();
                int clampedStart = Math.max(0, Math.min(Math.min(start, end), length));
                int clampedEnd = Math.max(0, Math.min(Math.max(start, end), length));
                int startBytes = utf8Length(editable.subSequence(0, clampedStart));
                int endBytes =
                        startBytes + utf8Length(editable.subSequence(clampedStart, clampedEnd));
                nativeImeSetComposingRegion(state.queueHandle, startBytes, endBytes);
            }
            return super.setComposingRegion(start, end);
        }

        @Override
        public boolean setSelection(int start, int end) {
            if (!state.disposed) {
                // Move the caret/selection natively (BaseInputConnection's
                // default only touches the local Editable mirror). This is how
                // Gboard's spacebar-swipe scrubs the cursor.
                Editable editable = getEditable();
                int length = editable.length();
                int clampedStart = Math.max(0, Math.min(Math.min(start, end), length));
                int clampedEnd = Math.max(0, Math.min(Math.max(start, end), length));
                int startBytes = utf8Length(editable.subSequence(0, clampedStart));
                int endBytes =
                        startBytes + utf8Length(editable.subSequence(clampedStart, clampedEnd));
                nativeImeSetSelection(state.queueHandle, startBytes, endBytes);
            }
            return super.setSelection(start, end);
        }

        @Override
        public boolean finishComposingText() {
            if (!state.disposed) {
                nativeImeFinishComposing(state.queueHandle);
            }
            return super.finishComposingText();
        }

        @Override
        public boolean deleteSurroundingText(int beforeLength, int afterLength) {
            if (!state.disposed) {
                // Convert the UTF-16 char counts into UTF-8 byte counts using
                // the mirror (the native side counts bytes).
                Editable editable = getEditable();
                int selStart = Selection.getSelectionStart(editable);
                int selEnd = Selection.getSelectionEnd(editable);
                if (selStart >= 0 && selEnd >= 0) {
                    int start = Math.min(selStart, selEnd);
                    int end = Math.max(selStart, selEnd);
                    int beforeChars = Math.max(0, Math.min(beforeLength, start));
                    int afterChars = Math.max(0, Math.min(afterLength, editable.length() - end));
                    int beforeBytes = utf8Length(editable.subSequence(start - beforeChars, start));
                    int afterBytes = utf8Length(editable.subSequence(end, end + afterChars));
                    nativeImeDeleteSurrounding(state.queueHandle, beforeBytes, afterBytes);
                }
            }
            return super.deleteSurroundingText(beforeLength, afterLength);
        }

        @Override
        public boolean sendKeyEvent(KeyEvent event) {
            if (state.disposed) {
                return false;
            }
            // Forward through the IME queue instead of super (which would
            // inject into the window input queue and double-deliver through
            // the native key path).
            nativeImeSendKeyEvent(
                    state.queueHandle,
                    event.getAction(),
                    event.getKeyCode(),
                    event.getMetaState(),
                    event.getUnicodeChar()
            );
            // Mirror the common editing keys BaseInputConnection would have
            // delivered so the Editable stays in sync until the next native
            // state push.
            if (event.getAction() == KeyEvent.ACTION_DOWN
                    && event.getKeyCode() == KeyEvent.KEYCODE_DEL) {
                Editable editable = getEditable();
                int selStart = Selection.getSelectionStart(editable);
                int selEnd = Selection.getSelectionEnd(editable);
                if (selStart >= 0 && selEnd >= 0) {
                    int start = Math.min(selStart, selEnd);
                    int end = Math.max(selStart, selEnd);
                    if (start != end) {
                        editable.delete(start, end);
                    } else if (start > 0) {
                        int deleteFrom = start - 1;
                        // Keep surrogate pairs intact.
                        if (deleteFrom > 0
                                && Character.isLowSurrogate(editable.charAt(deleteFrom))
                                && Character.isHighSurrogate(editable.charAt(deleteFrom - 1))) {
                            deleteFrom -= 1;
                        }
                        editable.delete(deleteFrom, start);
                    }
                }
            }
            return true;
        }

        @Override
        public boolean performEditorAction(int actionCode) {
            if (!state.disposed) {
                nativeImePerformEditorAction(state.queueHandle, actionCode);
            }
            return true;
        }
    }

    private static native void nativeImeCommitText(
            long queueHandle, String text, int newCursorPosition);

    private static native void nativeImeSetComposingText(
            long queueHandle, String text, int cursorBytes);

    private static native void nativeImeSetComposingRegion(
            long queueHandle, int startBytes, int endBytes);

    private static native void nativeImeSetSelection(
            long queueHandle, int startBytes, int endBytes);

    private static native void nativeImeFinishComposing(long queueHandle);

    private static native void nativeImeDeleteSurrounding(
            long queueHandle, int beforeBytes, int afterBytes);

    private static native void nativeImeSendKeyEvent(
            long queueHandle, int action, int keyCode, int metaState, int unicodeChar);

    private static native void nativeImePerformEditorAction(long queueHandle, int actionCode);

    private static native void nativeImeInsetsChanged(long queueHandle, int bottomPx);

    private static native void nativeImeReleaseQueue(long queueHandle);
}
