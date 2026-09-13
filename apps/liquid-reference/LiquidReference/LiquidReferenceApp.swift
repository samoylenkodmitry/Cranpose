import SwiftUI

@main
struct LiquidReferenceApp: App {
    var body: some Scene {
        WindowGroup {
            ReferenceTabs()
        }
    }
}

private struct ReferenceContent: Decodable {
    let titles: [String]
    let icons: [Int]
    let accent: [Double]?
    let palette: [[Double]]?

    static let active: ReferenceContent = {
        guard let json = ProcessInfo.processInfo.environment["REFERENCE_CONTENT"] else {
            return ReferenceContent(titles: ["Discover", "Browse", "Saved", "Account"], icons: [0, 1, 2, 3], accent: nil, palette: nil)
        }
        let value = try! JSONDecoder().decode(ReferenceContent.self, from: Data(json.utf8))
        precondition(value.titles.count == 4 && value.icons.count == 4 && value.icons.allSatisfy { (0..<4).contains($0) })
        return value
    }()

    static func color(_ rgb: [Double]) -> Color {
        precondition(rgb.count == 3 && rgb.allSatisfy { $0.isFinite && (0...1).contains($0) })
        return Color(red: rgb[0], green: rgb[1], blue: rgb[2])
    }
}

private enum Destination: Int, CaseIterable, Identifiable {
    case discover, browse, saved, account

    var id: Int { rawValue }

    var title: String {
        ReferenceContent.active.titles[rawValue]
    }

    var symbol: String {
        ["star", "bag", "bookmark", "person.crop.circle"][ReferenceContent.active.icons[rawValue]]
    }
}

private struct ReferenceTabs: View {
    @State private var selection = Destination(rawValue: Int(ProcessInfo.processInfo.environment["REFERENCE_INITIAL_DESTINATION"] ?? "0") ?? 0) ?? .discover
    @State private var trace = NativeTrace()
    @State private var inspecting = false
    private let environment = ProcessInfo.processInfo.environment

    var body: some View {
        TabView(selection: $selection) {
            ForEach(Destination.allCases) { destination in
                Tab(destination.title, systemImage: destination.symbol, value: destination) {
                    ZStack {
                        if let pattern = trace.optics.pattern, pattern.kind != "checkerboard" {
                            OpticalBackdrop(pattern: pattern).ignoresSafeArea()
                        } else {
                            ReferenceBackdrop(pattern: environment["REFERENCE_BACKDROP"] ?? "checkerboard")
                                .ignoresSafeArea()
                        }
                        VStack(spacing: 12) {
                            Text(destination.title)
                                .font(.largeTitle.bold())
                                .accessibilityIdentifier("destination")
                            Text("Native Liquid Glass · 8 pt checkerboard")
                                .font(.subheadline)
                            if trace.optics.enabled {
                                Text(trace.optics.label).accessibilityIdentifier("optical-probe")
                                Button("Next optical probe") { trace.optics.next() }
                            }
                            Text(trace.phase)
                                .font(.title3.monospaced())
                                .accessibilityIdentifier("touch-phase")
                            Text("\(trace.speed, specifier: "%.0f") pt/s · \(trace.frameCount) frames")
                                .font(.caption.monospacedDigit())
                            Text("Hold a tab, slide, stop, wait, then release")
                                .font(.caption)
                            Button("Inspect animation keyframes") { inspecting = true }
                                .buttonStyle(.borderedProminent)
                            if let error = trace.error { Text(error).font(.caption).foregroundStyle(.red).accessibilityIdentifier("trace-error") }
                        }
                    }
                }
            }
        }
        .overlay(alignment: .topLeading) {
            TouchProbe(trace: trace).frame(width: environment["REFERENCE_RECORDING"] == "1" ? 128 : 0, height: 8)
                .offset(x: 16, y: 100).ignoresSafeArea().allowsHitTesting(false)
        }
        .sheet(isPresented: $inspecting) { KeyframeInspector() }
        .tint(ReferenceContent.active.accent.map(ReferenceContent.color) ?? .blue)
        .preferredColorScheme(environment["REFERENCE_SCHEME"] == "dark" ? .dark : .light)
    }
}

private struct ReferenceBackdrop: View {
    let pattern: String
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        Canvas { context, size in
            let background: Color = colorScheme == .dark ? .black : .white
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(background))
            if pattern == "checkerboard" {
                let colors: [Color] = ReferenceContent.active.palette.map { $0.map(ReferenceContent.color) } ?? [
                    Color(red: 1, green: 0.23, blue: 0.19),
                    Color(red: 1, green: 0.58, blue: 0),
                    Color(red: 1, green: 0.80, blue: 0),
                    Color(red: 0.20, green: 0.78, blue: 0.35),
                    Color(red: 0.20, green: 0.68, blue: 0.90),
                    Color(red: 0, green: 0.48, blue: 1),
                    Color(red: 0.69, green: 0.32, blue: 0.87)
                ]
                let cell: CGFloat = 8
                for row in 0..<Int(ceil(size.height / cell)) {
                    for column in 0..<Int(ceil(size.width / cell)) {
                        let color = colors[(column + row) % colors.count]
                        let rect = CGRect(x: CGFloat(column) * cell, y: CGFloat(row) * cell,
                                          width: cell, height: cell)
                        context.fill(Path(rect), with: .color(color))
                        if (row + column).isMultiple(of: 2) {
                            context.fill(Path(rect), with: .color(.white.opacity(0.55)))
                        }
                    }
                }
            }
        }
        .accessibilityHidden(true)
    }
}
