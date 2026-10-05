import Cocoa
import FlutterMacOS

/// The window every desktop app opens for the gauntlet: 1280 x 820 points of
/// content, titled Gauntlet, as `desktop.py` measures it.
class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    self.contentViewController = flutterViewController
    self.title = "Gauntlet"
    self.setContentSize(NSSize(width: 1280, height: 820))
    self.styleMask.remove(.resizable)

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
  }
}
