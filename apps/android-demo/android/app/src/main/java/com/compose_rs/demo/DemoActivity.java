package com.compose_rs.demo;

import dev.cranpose.android.CranposeActivity;

/**
 * Diagnostics-only subclass used for on-device frame-pacing measurement.
 *
 * <p>{@code NativeActivity} loads the native library through libnativeloader's
 * {@code OpenNativeLibrary}, which does NOT register the library with ART's JNI
 * method resolver. Any {@code native} method declared in Java therefore fails
 * with {@code UnsatisfiedLinkError} even though the symbol is exported. Calling
 * {@code System.loadLibrary} from a static initializer registers the same
 * library with ART before {@code NativeActivity.onCreate} runs, so Java->Rust
 * calls such as {@code nativeOnNetworkStatus} resolve.
 */
public class DemoActivity extends CranposeActivity {
    static {
        System.loadLibrary("desktop_app");
    }
}
