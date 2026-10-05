# AccessKit's Android adapter finds the view it serves through JNI, by this
# field's name and type.
-keep class com.google.androidgamesdk.GameActivity$InputEnabledSurfaceView
-keepclassmembers class com.google.androidgamesdk.GameActivity {
    com.google.androidgamesdk.GameActivity$InputEnabledSurfaceView mSurfaceView;
}
