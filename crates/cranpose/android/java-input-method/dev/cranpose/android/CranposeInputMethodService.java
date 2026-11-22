package dev.cranpose.android;

import android.content.res.Configuration;
import android.inputmethodservice.InputMethodService;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.util.DisplayMetrics;
import android.view.Choreographer;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputMethodManager;
import android.widget.FrameLayout;

/**
 * The system keyboard of an application that names its board with
 * {@code AppLauncher::with_input_method}.
 *
 * <p>Its input view is a {@link SurfaceView}. Rust draws the board into the
 * view's surface on the GPU, on the main thread, once per {@link Choreographer}
 * frame while the board asks for frames. Touches go to Rust in physical
 * pixels. What the board types comes back through the static methods below,
 * which Rust calls on the main thread.
 */
public class CranposeInputMethodService extends InputMethodService
        implements SurfaceHolder.Callback, Choreographer.FrameCallback {
    private static CranposeInputMethodService current;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Runnable post = this::postFrame;
    private boolean framePosted;
    private boolean started;
    private SurfaceView surfaceView;
    private int bottomInset;

    private static native void nativeSurfaceChanged(Object surface, int width, int height, float density);
    private static native void nativeSurfaceDestroyed();
    private static native long nativeFrame(long frameTimeNanos);
    private static native void nativeTouch(int pointer, int phase, float x, float y);
    private static native void nativeEditor(
            int inputType, int imeOptions, boolean capitalizeNext, boolean start);
    private static native void nativeInsets(int left, int top, int right, int bottom, float density);
    private static native void nativeVisible(boolean visible);
    private static native float nativePreferredHeight(float screenWidth, float screenHeight);

    @Override
    public void onCreate() {
        super.onCreate();
        started = CranposeProcess.start(this);
        current = this;
    }

    @Override
    public void onDestroy() {
        if (started) {
            nativeSurfaceDestroyed();
        }
        if (current == this) {
            current = null;
        }
        Choreographer.getInstance().removeFrameCallback(this);
        handler.removeCallbacks(post);
        super.onDestroy();
    }

    @Override
    public boolean onEvaluateFullscreenMode() {
        return false;
    }

    @Override
    public View onCreateInputView() {
        FrameLayout frame = new FrameLayout(this);
        surfaceView = new SurfaceView(this) {
            @Override
            public boolean onTouchEvent(MotionEvent event) {
                return routeTouch(event);
            }
        };
        surfaceView.getHolder().addCallback(this);
        frame.addView(surfaceView, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, boardHeight()));
        // The board reaches under the navigation bar. The composition reads
        // the bar's height as its safe area and keeps its keys above it.
        frame.setOnApplyWindowInsetsListener((view, insets) -> {
            int inset = Build.VERSION.SDK_INT >= 30
                    ? insets.getInsets(WindowInsets.Type.navigationBars()).bottom
                    : insets.getSystemWindowInsetBottom();
            if (inset != bottomInset) {
                bottomInset = inset;
                if (started) {
                    nativeInsets(0, 0, 0, inset, getResources().getDisplayMetrics().density);
                }
                resizeBoard();
            }
            return insets;
        });
        return frame;
    }

    @Override
    public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        resizeBoard();
    }

    private void resizeBoard() {
        if (surfaceView != null) {
            ViewGroup.LayoutParams params = surfaceView.getLayoutParams();
            int height = boardHeight();
            if (params.height != height) {
                params.height = height;
                surfaceView.setLayoutParams(params);
            }
        }
    }

    // The board's height in pixels, the navigation bar below it included.
    private int boardHeight() {
        DisplayMetrics metrics = getResources().getDisplayMetrics();
        float density = metrics.density;
        if (!started) {
            return Math.round(260 * density) + bottomInset;
        }
        float logical = nativePreferredHeight(
                metrics.widthPixels / density, metrics.heightPixels / density);
        return Math.round(logical * density) + bottomInset;
    }

    @Override
    public void onStartInputView(EditorInfo info, boolean restarting) {
        super.onStartInputView(info, restarting);
        sendEditor(true);
    }

    @Override
    public void onUpdateSelection(int oldSelStart, int oldSelEnd, int newSelStart, int newSelEnd,
            int candidatesStart, int candidatesEnd) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd,
                candidatesStart, candidatesEnd);
        sendEditor(false);
    }

    @Override
    public void onWindowShown() {
        super.onWindowShown();
        if (started) {
            nativeVisible(true);
            requestFrame();
        }
    }

    @Override
    public void onWindowHidden() {
        if (started) {
            nativeVisible(false);
        }
        super.onWindowHidden();
    }

    // Tells the board about the focused field. `start` says that the field
    // just took the keyboard.
    private void sendEditor(boolean start) {
        EditorInfo info = getCurrentInputEditorInfo();
        if (!started || info == null) {
            return;
        }
        InputConnection connection = getCurrentInputConnection();
        boolean capitalize = connection != null
                && (info.inputType & InputType.TYPE_MASK_CLASS) == InputType.TYPE_CLASS_TEXT
                && connection.getCursorCapsMode(info.inputType) != 0;
        nativeEditor(info.inputType, info.imeOptions, capitalize, start);
        requestFrame();
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {}

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        if (started) {
            nativeSurfaceChanged(holder.getSurface(), width, height,
                    getResources().getDisplayMetrics().density);
            requestFrame();
        }
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        if (started) {
            nativeSurfaceDestroyed();
        }
    }

    @Override
    public void doFrame(long frameTimeNanos) {
        framePosted = false;
        if (!started) {
            return;
        }
        long next = nativeFrame(frameTimeNanos);
        if (next == 0) {
            postFrame();
        } else if (next > 0) {
            handler.postDelayed(post, next);
        }
    }

    private void postFrame() {
        if (!framePosted) {
            framePosted = true;
            Choreographer.getInstance().postFrameCallback(this);
        }
    }

    private boolean routeTouch(MotionEvent event) {
        if (!started) {
            return false;
        }
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_POINTER_DOWN:
                sendTouch(event, event.getActionIndex(), 0);
                break;
            case MotionEvent.ACTION_MOVE:
                for (int index = 0; index < event.getPointerCount(); index++) {
                    sendTouch(event, index, 1);
                }
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_POINTER_UP:
                sendTouch(event, event.getActionIndex(), 2);
                break;
            case MotionEvent.ACTION_CANCEL:
                for (int index = 0; index < event.getPointerCount(); index++) {
                    sendTouch(event, index, 3);
                }
                break;
            default:
                return false;
        }
        postFrame();
        return true;
    }

    private static void sendTouch(MotionEvent event, int index, int phase) {
        nativeTouch(event.getPointerId(index), phase, event.getX(index), event.getY(index));
    }

    /** Asks for a frame; Rust calls it from any thread when the board changes. */
    static void requestFrame() {
        CranposeInputMethodService service = current;
        if (service != null) {
            service.handler.post(service.post);
        }
    }

    /** Inserts {@code text} at the cursor, replacing the selection. */
    static void commitText(String text) {
        InputConnection connection = connection();
        if (connection != null) {
            connection.commitText(text, 1);
        }
    }

    /**
     * Deletes the selection, or the character before the cursor. A field that
     * shows no text to the keyboard gets the delete key itself.
     */
    static void deleteBackward() {
        CranposeInputMethodService service = current;
        InputConnection connection = connection();
        if (service == null || connection == null) {
            return;
        }
        CharSequence selected = connection.getSelectedText(0);
        if (selected != null && selected.length() > 0) {
            connection.commitText("", 1);
            return;
        }
        CharSequence before = connection.getTextBeforeCursor(2, 0);
        if (before == null || before.length() == 0) {
            service.sendDownUpKeyEvents(KeyEvent.KEYCODE_DEL);
            return;
        }
        int count = before.length() == 2
                && Character.isSurrogatePair(before.charAt(0), before.charAt(1)) ? 2 : 1;
        connection.deleteSurroundingText(count, 0);
    }

    /**
     * Runs the field's action, such as search or send, or sends the enter
     * key when the field takes several lines or names no action.
     */
    static void performImeAction() {
        CranposeInputMethodService service = current;
        if (service != null && !service.sendDefaultEditorAction(true)) {
            service.sendKeyChar('\n');
        }
    }

    /**
     * Hands over to the next keyboard the person enabled, or shows the list
     * of keyboards when the system names no next one.
     */
    static void switchToNextKeyboard() {
        CranposeInputMethodService service = current;
        if (service == null) {
            return;
        }
        if (Build.VERSION.SDK_INT >= 28 && service.switchToNextInputMethod(false)) {
            return;
        }
        InputMethodManager manager = service.getSystemService(InputMethodManager.class);
        if (manager != null) {
            manager.showInputMethodPicker();
        }
    }

    private static InputConnection connection() {
        CranposeInputMethodService service = current;
        return service == null ? null : service.getCurrentInputConnection();
    }
}
