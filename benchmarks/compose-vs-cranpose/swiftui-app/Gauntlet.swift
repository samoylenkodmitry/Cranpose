// The gauntlet in SwiftUI: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in
// SwiftUI's own idiom. The benchmark's README describes it; `desktop.py` runs
// it with the tier in `PERF_TIER` and the Roboto files in `PERF_FONTS`.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. A display link advances it once per frame of the display;
// the views that read it are the views that change.

import AppKit
import CoreText
import Observation
import QuartzCore
import SwiftUI

/// What `desktop.py` asked of the gauntlet.
enum Launch {
    static let environment = ProcessInfo.processInfo.environment
    static let tier = Int(environment["PERF_TIER"] ?? "") ?? 5
    static let freeze = Int(environment["PERF_FREEZE"] ?? "") ?? 0
    static let fonts = environment["PERF_FONTS"] ?? "fonts"
    /// The window every desktop app opens for the gauntlet, in points.
    static let window = CGSize(width: 1280, height: 820)
}

/// Writes a `PERF` line where `desktop.py` reads it.
func log(_ line: String) {
    print(line)
    fflush(stdout)
}

/// The frame index every per-frame value follows.
@Observable
final class Clock: NSObject {
    var frame = 0
    @ObservationIgnored private var link: CADisplayLink?

    func start() {
        guard link == nil, let screen = NSScreen.main else { return }
        let link = screen.displayLink(target: self, selector: #selector(tick))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    @objc private func tick(_ link: CADisplayLink) {
        if frame == 0 { log("PERF first_frame") }
        frame += 1
        if Launch.freeze > 0 && frame >= Launch.freeze {
            log("PERF frozen frame=\(frame)")
            link.invalidate()
        }
    }
}

extension Color {
    init(rgb: UInt32) {
        self.init(.sRGB, red: Double(rgb >> 16 & 0xFF) / 255, green: Double(rgb >> 8 & 0xFF) / 255,
                  blue: Double(rgb & 0xFF) / 255)
    }

    static let ink = Color(rgb: 0x111827)
    static let body = Color(rgb: 0x374151)
    static let muted = Color(rgb: 0x6B7280)
    static let hairline = Color(rgb: 0xE5E7EB)
    static let panel = Color(rgb: 0xE2E8F0)
    static let up = Color(rgb: 0x16A34A)
    static let down = Color(rgb: 0xDC2626)
    static let levelBackground = [Color(rgb: 0xF1F5F9), Color(rgb: 0xCBD5E1)]

    static func palette(_ index: Int) -> Color { Color(rgb: PerfData.paletteRGB[index % 8]) }
}

/// Text in Roboto at one tier's scale.
func roboto(_ size: CGFloat, _ s: CGFloat, bold: Bool = false) -> Font {
    .custom("Roboto", fixedSize: size * s).weight(bold ? .bold : .regular)
}

extension View {
    /// Lines 1.4 em apart: Roboto's own line spacing is 1.172 em.
    func paragraph(_ size: CGFloat, _ s: CGFloat, lines: Int) -> some View {
        lineSpacing(size * s * (1.4 - 1.172)).lineLimit(lines)
    }
}

/// Equal columns `spacing` apart, each as tall as it needs, tops aligned.
struct EqualColumns: Layout {
    var spacing: CGFloat

    private func column(_ width: CGFloat, _ count: Int) -> CGFloat {
        (width - spacing * CGFloat(count - 1)) / CGFloat(count)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 0
        let column = column(width, subviews.count)
        let height = subviews.map { $0.sizeThatFits(ProposedViewSize(width: column, height: nil)).height }.max()
        return CGSize(width: width, height: height ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let column = column(bounds.width, subviews.count)
        for (index, subview) in subviews.enumerated() {
            subview.place(at: CGPoint(x: bounds.minX + CGFloat(index) * (column + spacing), y: bounds.minY),
                          anchor: .topLeading, proposal: ProposedViewSize(width: column, height: nil))
        }
    }
}

/// Children left to right, wrapping, `spacing` apart both ways, as Compose's FlowRow.
struct FlowLayout: Layout {
    var spacing: CGFloat

    private func arrange(_ width: CGFloat, _ subviews: Subviews) -> (size: CGSize, points: [CGPoint]) {
        var points: [CGPoint] = []
        var (x, y, line, widest) = (CGFloat(0), CGFloat(0), CGFloat(0), CGFloat(0))
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width {
                y += line + spacing
                x = 0
                line = 0
            }
            points.append(CGPoint(x: x, y: y))
            x += size.width + spacing
            widest = max(widest, x - spacing)
            line = max(line, size.height)
        }
        return (CGSize(width: width.isFinite ? width : widest, height: y + line), points)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(proposal.width ?? .infinity, subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for (subview, point) in zip(subviews, arrange(bounds.width, subviews).points) {
            subview.place(at: CGPoint(x: bounds.minX + point.x, y: bounds.minY + point.y), proposal: .unspecified)
        }
    }
}

/// A rounded track filled to `share` in `fill`.
struct Bar: View {
    let share: Double
    let fill: Color
    let radius: CGFloat

    var body: some View {
        Canvas { context, size in
            context.fill(RoundedRectangle(cornerRadius: radius).path(in: CGRect(origin: .zero, size: size)),
                         with: .color(.hairline))
            let filled = CGRect(x: 0, y: 0, width: size.width * share, height: size.height)
            context.fill(RoundedRectangle(cornerRadius: radius).path(in: filled), with: .color(fill))
        }
    }
}

struct TickerTile: View {
    let ticker: PerfData.Ticker
    let index: Int
    let s: CGFloat
    let clock: Clock

    var body: some View {
        let cents = PerfData.tickerCents(ticker, clock.frame)
        let share = Double(cents - ticker.baseCents + ticker.swingCents) / Double(2 * ticker.swingCents)
        HStack(spacing: 4 * s) {
            Text(ticker.symbol).font(roboto(10, s, bold: true)).foregroundStyle(Color.ink)
            Text(PerfData.centsText(cents)).font(roboto(10, s)).foregroundStyle(Color.body)
            Text(PerfData.changeText(ticker, cents)).font(roboto(10, s))
                .foregroundStyle(cents >= ticker.baseCents ? Color.up : Color.down)
            Bar(share: min(max(share, 0), 1), fill: .palette(index), radius: 2 * s).frame(width: 20 * s, height: 4 * s)
        }
        .padding(.horizontal, 6 * s)
        .padding(.vertical, 3 * s)
        .background(RoundedRectangle(cornerRadius: 6 * s).fill(.white))
    }
}

/// A card's 48-point line over a fading fill, drawn from the frame.
struct Sparkline: View {
    let card: Int
    let color: Color
    let s: CGFloat
    let clock: Clock

    var body: some View {
        let frame = clock.frame
        Canvas { context, size in
            let step = size.width / CGFloat(PerfData.sparkPoints - 1)
            func point(_ index: Int) -> CGPoint {
                CGPoint(x: CGFloat(index) * step,
                        y: size.height * CGFloat(1 - PerfData.sparkValue(card, index, frame)))
            }
            var area = Path()
            var line = Path()
            area.move(to: CGPoint(x: 0, y: size.height))
            for index in 0..<PerfData.sparkPoints {
                area.addLine(to: point(index))
                if index == 0 { line.move(to: point(index)) } else { line.addLine(to: point(index)) }
            }
            area.addLine(to: CGPoint(x: size.width, y: size.height))
            area.closeSubpath()
            context.fill(area, with: .linearGradient(Gradient(colors: [color.opacity(0.25), color.opacity(0)]),
                                                     startPoint: .zero, endPoint: CGPoint(x: 0, y: size.height)))
            context.stroke(line, with: .color(color), lineWidth: 1.5 * s)
        }
    }
}

struct ProgressRow: View {
    let card: Int
    let s: CGFloat
    let clock: Clock

    var body: some View {
        let permille = PerfData.progressPermille(card, clock.frame)
        HStack(spacing: 6 * s) {
            Bar(share: Double(permille) / 1000, fill: .palette(card), radius: 3 * s).frame(height: 6 * s)
            Text("\(permille / 10)%").font(roboto(10, s)).foregroundStyle(Color.muted)
        }
    }
}

/// A translucent tag tilting with the frame: rotated, never laid out again.
struct Badge: View {
    let card: Int
    let s: CGFloat
    let clock: Clock

    var body: some View {
        Text("HOT").font(roboto(9, s, bold: true)).foregroundStyle(.white)
            .padding(.horizontal, 6 * s)
            .padding(.vertical, 2 * s)
            .background(RoundedRectangle(cornerRadius: 8 * s).fill(Color.palette(0)))
            .opacity(0.9)
            .rotationEffect(.degrees(PerfData.badgeDegrees(card, clock.frame)))
    }
}

struct CardView: View {
    static let footerLabels = ["likes", "replies", "shares"]

    let card: Int
    let post: PerfData.Post
    let avatar: Image
    let s: CGFloat
    let clock: Clock

    var body: some View {
        let (tag, tagColor) = post.tags[0]
        VStack(alignment: .leading, spacing: 6 * s) {
            HStack(spacing: 8 * s) {
                avatar.resizable().frame(width: 32 * s, height: 32 * s).clipShape(Circle())
                VStack(alignment: .leading, spacing: 0) {
                    Text(post.title).font(roboto(13, s, bold: true)).foregroundStyle(Color.ink)
                        .paragraph(13, s, lines: 2)
                    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
                    Text("by \(Text(post.author).bold()) · \(Text(tag).foregroundStyle(Color(rgb: PerfData.gradientEndRGB[tagColor])))")
                        .font(roboto(11, s)).foregroundStyle(Color.muted).lineLimit(1)
                }
            }
            Text(post.body).font(roboto(12, s)).foregroundStyle(Color.body).paragraph(12, s, lines: 4)
            ProgressRow(card: card, s: s, clock: clock)
            Sparkline(card: card, color: .palette(post.color), s: s, clock: clock).frame(height: 36 * s)
            FlowLayout(spacing: 4 * s) {
                ForEach(0..<post.tags.count, id: \.self) { index in
                    let (text, color) = post.tags[index]
                    Text(text).font(roboto(10, s)).foregroundStyle(Color(rgb: PerfData.gradientEndRGB[color]))
                        .padding(.horizontal, 8 * s)
                        .padding(.vertical, 3 * s)
                        .background(RoundedRectangle(cornerRadius: 10 * s).fill(Color(rgb: PerfData.chipBackgroundRGB[color])))
                }
            }
            // Three counters split by dividers as tall as the row.
            HStack(spacing: 0) {
                ForEach(0..<Self.footerLabels.count, id: \.self) { index in
                    if index > 0 { Rectangle().fill(Color.hairline).frame(width: 1) }
                    VStack(spacing: 0) {
                        Text(post.stats[index]).font(roboto(12, s, bold: true)).foregroundStyle(Color.ink)
                        Text(Self.footerLabels[index]).font(roboto(9, s)).foregroundStyle(Color.muted)
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .fixedSize(horizontal: false, vertical: true)
        }
        .padding(10 * s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            RoundedRectangle(cornerRadius: 12 * s).fill(.white).shadow(color: .black.opacity(0.24), radius: 1.5 * s, y: s)
        }
        .overlay { RoundedRectangle(cornerRadius: 12 * s).strokeBorder(Color.hairline, lineWidth: 1) }
        .overlay(alignment: .topTrailing) {
            if card % 5 == 0 { Badge(card: card, s: s, clock: clock).padding(6 * s) }
        }
    }
}

/// Levels nested inside one another, each a row of three labels above the
/// next level.
struct LevelView: View {
    let cluster: Int
    let remaining: Int
    let s: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: s) {
            EqualColumns(spacing: 2 * s) {
                ForEach(0..<3, id: \.self) { chip in
                    Text("C\(cluster).L\(remaining).\(chip)").font(roboto(9, s)).foregroundStyle(.white)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 2 * s).fill(Color.palette(remaining + chip + cluster)))
                }
            }
            if remaining > 0 { AnyView(LevelView(cluster: cluster, remaining: remaining - 1, s: s)) }
        }
        .padding(EdgeInsets(top: s, leading: 3 * s, bottom: s, trailing: s))
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 4 * s).fill(Color.levelBackground[remaining % 2]))
    }
}

struct RowView: View {
    let row: Int
    let model: Model
    let clock: Clock

    var body: some View {
        let s = model.tier.scale
        switch PerfData.row(row, columns: model.tier.columns) {
        case .cards(let first):
            EqualColumns(spacing: 8 * s) {
                ForEach(first..<first + model.tier.columns, id: \.self) { card in
                    CardView(card: card, post: model.posts[card % PerfData.postCount],
                             avatar: model.avatars[card % PerfData.avatarCount], s: s, clock: clock)
                }
            }
        case .cluster(let cluster):
            LevelView(cluster: cluster, remaining: model.tier.depth, s: s)
        }
    }
}

/// The data the screen draws, made once.
final class Model {
    let tier = PerfData.tier(Launch.tier)
    let posts = PerfData.posts()
    let tickers: [PerfData.Ticker]
    let avatars: [Image]

    init() {
        tickers = PerfData.tickers(tier.tickers)
        avatars = (0..<PerfData.avatarCount).map { index in
            let size = PerfData.avatarSize
            let data = Data(PerfData.avatarRGBA(index))
            guard let provider = CGDataProvider(data: data as CFData),
                  let image = CGImage(width: size, height: size, bitsPerComponent: 8, bitsPerPixel: 32,
                                      bytesPerRow: size * 4, space: CGColorSpaceCreateDeviceRGB(),
                                      bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                                      provider: provider, decode: nil, shouldInterpolate: true,
                                      intent: .defaultIntent)
            else { return Image(systemName: "circle") }
            return Image(decorative: image, scale: 1)
        }
    }
}

/// The list's offset follows the frame: 3 points a frame.
struct GauntletList: View {
    let model: Model
    let clock: Clock
    @State private var position = ScrollPosition(y: 0)

    var body: some View {
        let s = model.tier.scale
        ScrollView(.vertical, showsIndicators: false) {
            LazyVStack(spacing: 8 * s) {
                ForEach(0..<2000 * (PerfData.cardRowsPerCluster + 1), id: \.self) { row in
                    RowView(row: row, model: model, clock: clock)
                }
            }
            .padding(8 * s)
        }
        .scrollPosition($position)
        .onChange(of: clock.frame) { _, frame in position.scrollTo(y: CGFloat(frame) * 3) }
    }
}

/// The content's width follows the frame.
struct WidthFollowsFrame<Content: View>: View {
    let clock: Clock
    @ViewBuilder let content: Content

    var body: some View {
        content.frame(width: (Launch.window.width * PerfData.widthFraction(clock.frame) * 2).rounded() / 2)
    }
}

struct GauntletView: View {
    let model: Model
    let clock: Clock

    var body: some View {
        let s = model.tier.scale
        VStack(alignment: .leading, spacing: 0) {
            Text("Gauntlet").font(roboto(20, 1, bold: true)).foregroundStyle(.white)
                .padding(.horizontal, 16)
                .frame(maxWidth: .infinity, minHeight: 56, maxHeight: 56, alignment: .leading)
                .background(Color(rgb: 0x1E2A4A))
            WidthFollowsFrame(clock: clock) {
                VStack(spacing: 0) {
                    FlowLayout(spacing: 4 * s) {
                        ForEach(0..<model.tickers.count, id: \.self) { index in
                            TickerTile(ticker: model.tickers[index], index: index, s: s, clock: clock)
                        }
                    }
                    .padding(6 * s)
                    .background(Color.panel)
                    GauntletList(model: model, clock: clock)
                }
            }
        }
        .frame(width: Launch.window.width, height: Launch.window.height, alignment: .topLeading)
        .background(Color(rgb: 0xEEF0F5))
    }
}

@main
struct GauntletApp: App {
    @State private var clock = Clock()
    private let model: Model

    init() {
        // Roboto is no macOS system font: the app registers the files every
        // desktop app loads, for this process only.
        for file in ["Roboto-Regular.ttf", "Roboto-Bold.ttf"] {
            let url = URL(fileURLWithPath: Launch.fonts).appendingPathComponent(file)
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
        model = Model()
    }

    var body: some Scene {
        Window("Gauntlet", id: "gauntlet") {
            GauntletView(model: model, clock: clock).onAppear { clock.start() }
        }
        .windowResizability(.contentSize)
    }
}
