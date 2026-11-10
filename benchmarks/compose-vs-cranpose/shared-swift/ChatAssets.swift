// The chat's pictures, loaded once from the folder `PERF_CHAT` names: the
// animated picture's frames, decoded with ImageIO, and the status line's
// spinner, which AppKit reads from its SVG.

import AppKit
import ImageIO

enum ChatAssets {
    /// The frames of `spin.gif` in `folder`.
    static func gifFrames(in folder: String) -> [CGImage] {
        let url = URL(fileURLWithPath: folder).appendingPathComponent("spin.gif")
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else {
            fputs("chat picture: cannot read \(url.path)\n", stderr)
            return []
        }
        return (0..<CGImageSourceGetCount(source)).compactMap { CGImageSourceCreateImageAtIndex(source, $0, nil) }
    }

    /// The spinner of `progress.svg` in `folder`, drawn at `side` points.
    static func spinner(in folder: String, side: CGFloat) -> CGImage? {
        let url = URL(fileURLWithPath: folder).appendingPathComponent("progress.svg")
        guard let image = NSImage(contentsOf: url) else {
            fputs("chat spinner: cannot read \(url.path)\n", stderr)
            return nil
        }
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        var rect = CGRect(x: 0, y: 0, width: side * scale, height: side * scale)
        image.size = CGSize(width: side * scale, height: side * scale)
        return image.cgImage(forProposedRect: &rect, context: nil, hints: nil)
    }
}
