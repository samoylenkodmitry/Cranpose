# Web text input checks

The [browser and screen reader targets](../../docs/accessibility_validation.md#browser-and-screen-reader-targets)
define the reader interaction checks required in addition to these browser robots.

Build and package the release demo with `just web` and `apps/desktop-demo/package-web.sh <site>`.

- `just robot-accessibility-web <site> <output>` runs Chromium focus, accessibility and native composition checks. The selection regression defers the selection notification across rendering to verify that an unchanged application caret does not overwrite a newer browser selection.
- `just test-web-ime-firefox <site> <output> <driver-port> <http-port>` starts geckodriver and a local server, then runs the shared keyboard checks. On a Linux host without a display, run the recipe under `xvfb-run -a`.
- `just test-web-ime-webdriver <endpoint> safari <url> <output>` connects to a running `safaridriver --port <port>`. Enable Safari's **Allow remote automation**.
- Add `--ios-device <UDID>` to the WebDriver recipe for a USB-connected, trusted and unlocked iPhone. Enable both **Web Inspector** and **Remote Automation** under Settings → Apps → Safari → Advanced. The URL must be reachable from the phone. iOS WebDriver suppresses the software keyboard, so this verifies native editing through WebDriver, not the phone's software IME.

For physical Android, forward a port to `localabstract:chrome_devtools_remote`, open the test URL in Chrome, and run:

```sh
just test-web-ime-android <serial> <cdp-endpoint> <url> <output> <keyboard-layout.json>
```

The test temporarily selects the installed Gboard input method and restores the original input method in cleanup. `A11Y_ANDROID_IME` selects another installed input method. Run the recipe with `inspect` in place of the layout file to save a screenshot for calibration. Supply physical screen coordinates for the English keyboard:

```json
{"c":[431,1999],"a":[113,1840],"t":[488,1693],"Space":[591,2148],"swipe":[488,1693,916,1693,600]}
```

These coordinates are an example for one device, not defaults. The swipe is `[startX,startY,endX,endY,durationMs]` and must glide from **t** to **o**. The test verifies touch typing and glide against application state, then separately runs browser-native composition through CDP. It records the event source and does not treat direct keyboard commits as proof of OS composition.

Each runner writes a JSON report and screenshot. Preserve failed reports alongside successful runs when verifying a regression.

The password check compares Cranpose with a plain native password input. Chromium's internal Android accessibility tree contains unmasked password values; its Android accessibility adapter applies masking according to the platform policy. The Android runner additionally checks the actual platform tree for the protected flag and masked text with password speech disabled. See [Chromium's Android password adapter](https://github.com/chromium/chromium/blob/main/content/browser/accessibility/browser_accessibility_android.cc).
