package dev.perfcompare.web;

import android.content.Intent;
import android.util.Log;
import com.getcapacitor.JSObject;
import com.getcapacitor.Plugin;
import com.getcapacitor.PluginCall;
import com.getcapacitor.PluginMethod;
import com.getcapacitor.annotation.CapacitorPlugin;

/** What `am start` asked of the gauntlet, and its `PERF` lines under the other apps' log tag. */
@CapacitorPlugin(name = "Launch")
public class LaunchPlugin extends Plugin {
    @PluginMethod
    public void get(PluginCall call) {
        Intent intent = getActivity().getIntent();
        JSObject launch = new JSObject();
        launch.put("tier", intent.getIntExtra("tier", 5));
        launch.put("freeze", intent.getIntExtra("freeze", 0));
        call.resolve(launch);
    }

    @PluginMethod
    public void log(PluginCall call) {
        Log.i(MainActivity.TAG, call.getString("message", ""));
        call.resolve();
    }
}
