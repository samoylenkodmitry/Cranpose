# The Rust cdylib resolves these classes and methods by name over JNI, so a
# shrinker that cannot see the call sites must not remove or rename them.
-keep class dev.cranpose.android.** { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}
