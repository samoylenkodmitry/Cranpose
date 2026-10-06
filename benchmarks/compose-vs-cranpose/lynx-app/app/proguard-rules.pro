# Lynx, and Fresco behind its image service, reach much of their Java from
# native code and by reflection: under R8 their published rules lose the
# sparklines' SVG and the avatars, and the trace thread's native code aborts
# on a renamed `LynxLog.log`. Both are kept whole; R8 still shrinks the rest.
-keep class com.lynx.** { *; }
-keep class com.facebook.** { *; }

# The page's log module, which Lynx creates by reflection.
-keep class dev.perfcompare.lynx.PerfLog { *; }

# Optional libraries Lynx reaches only for features the page does not use:
# animated images, its debug description and the markdown element.
-dontwarn com.facebook.fresco.animation.**
-dontwarn com.google.gson.**
-dontwarn com.lynx.markdown.**
