package dev.perfcompare.web;

import android.os.Bundle;
import android.util.Log;
import android.view.ViewTreeObserver;
import android.webkit.WebSettings;
import com.getcapacitor.BridgeActivity;

/** The gauntlet in a web view: `--es scenario gauntlet --ei tier N [--ei freeze K]`. */
public class MainActivity extends BridgeActivity {
    static final String TAG = "PerfCompare";

    @Override
    public void onCreate(Bundle savedInstanceState) {
        registerPlugin(LaunchPlugin.class);
        super.onCreate(savedInstanceState);
        // The gauntlet's text goes far below the 8 px a web view draws at least.
        WebSettings settings = getBridge().getWebView().getSettings();
        settings.setMinimumFontSize(1);
        settings.setMinimumLogicalFontSize(1);
        getWindow().getDecorView().getViewTreeObserver().addOnDrawListener(new ViewTreeObserver.OnDrawListener() {
            private boolean logged;

            @Override
            public void onDraw() {
                if (!logged) {
                    logged = true;
                    Log.i(TAG, "PERF first_frame");
                }
            }
        });
    }
}
