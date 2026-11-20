# The Rust cdylib resolves these classes and methods by name over JNI, so a
# shrinker that cannot see the call sites must not remove or rename them.
-keep class dev.cranpose.android.** { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}
# The Meta Platform SDK, which the `meta-entitlement` service adds. Its native library
# builds the SDK's Java objects by name, so a shrinker must not rename or remove them. An
# application without the service has no such class. The rule matches nothing then.
-keep class com.meta.horizon.platform.ovr.** { *; }
