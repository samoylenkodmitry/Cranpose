import Cocoa
import FlutterMacOS

/// The window every desktop app opens: 1280 x 820 points of content, titled
/// after the screen in `PERF_SCENARIO`, as `desktop.py` measures it.
class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    self.contentViewController = flutterViewController
    self.title = ProcessInfo.processInfo.environment["PERF_SCENARIO"] == "wall" ? "Still wall" : "Gauntlet"
    self.setContentSize(NSSize(width: 1280, height: 820))
    self.styleMask.remove(.resizable)

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
  }
}
