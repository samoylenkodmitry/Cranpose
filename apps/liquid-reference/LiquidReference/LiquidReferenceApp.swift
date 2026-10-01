import SwiftUI
import UIKit

@main
struct LiquidReferenceApp: App {
    var body: some Scene {
        WindowGroup {
            if let component = ProcessInfo.processInfo.environment["REFERENCE_COMPONENT"] {
                ReferenceControl(component: component)
            } else {
                ReferenceTabs()
            }
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

private struct ReferenceControl: View {
    let component: String
    @State private var trace = NativeTrace()
    @State private var value = Double(ProcessInfo.processInfo.environment["REFERENCE_VALUE"] ?? "0.5") ?? 0.5
    @State private var selection = Int(ProcessInfo.processInfo.environment["REFERENCE_VALUE"] ?? "0") ?? 0
    @State private var checked = ProcessInfo.processInfo.environment["REFERENCE_VALUE"] == "1"
    @State private var activations = 0
    private let environment = ProcessInfo.processInfo.environment

    var body: some View {
        ZStack {
            ReferenceBackdrop(pattern: environment["REFERENCE_BACKDROP"] ?? "checkerboard")
                .ignoresSafeArea()
            control
        }
        .tint(.blue)
        .preferredColorScheme(environment["REFERENCE_SCHEME"] == "dark" ? .dark : .light)
        .overlay(alignment: component == "nav-bar" ? .bottom : .top) {
            Text("Reference control").font(.caption).padding(.vertical, 24)
        }
        .overlay(alignment: .bottom) {
            VStack {
                Text("Activations: \(activations)")
                if let error = trace.error {
                    Text(error).accessibilityIdentifier("trace-error")
                }
            }.font(.caption).padding(.bottom, 24)
        }
        .overlay(alignment: .topLeading) {
            if environment["REFERENCE_CAPTURE_CONTROL_LAYERS"] == "1" || environment["REFERENCE_RECORDING"] == "1" {
                TouchProbe(trace: trace).frame(width: environment["REFERENCE_RECORDING"] == "1" ? 128 : 0, height: 8)
                    .offset(x: 16, y: 100).ignoresSafeArea().allowsHitTesting(false)
            }
        }
    }

    @ViewBuilder private var control: some View {
        switch component {
        case "slider":
            Slider(value: $value).frame(width: 300).accessibilityIdentifier("reference-slider")
                .onChange(of: value, initial: true) { _, current in trace.controlValue = current }
        case "toggle":
            Toggle("Enabled", isOn: $checked).labelsHidden().accessibilityIdentifier("reference-toggle")
        case "segmented":
            Picker("Scope", selection: $selection) {
                Text("All").tag(0)
                Text("Unread").tag(1)
                Text("Saved").tag(2)
            }
            .pickerStyle(.segmented).frame(width: 300).accessibilityIdentifier("reference-segmented")
        case "button":
            Button("Continue") { activations += 1 }.buttonStyle(.glass).controlSize(.large)
        case "prominent-button":
            Button("Continue") { activations += 1 }.buttonStyle(.glassProminent).controlSize(.large)
        case "chip":
            Button("Unread") { activations += 1 }.buttonStyle(.glass).controlSize(.small)
        case "filter-chip":
            Toggle("Unread", isOn: $checked).toggleStyle(.button)
                .buttonStyle(.glass).controlSize(.small)
                .onChange(of: checked) { _, _ in activations += 1 }
        case "icon-button":
            Button("Search", systemImage: "magnifyingglass") { activations += 1 }.labelStyle(.iconOnly)
                .foregroundStyle(.primary)
                .frame(width: 44, height: 44).glassEffect(.regular.interactive(), in: .circle)
        case "search":
            ReferenceSearchBar().frame(width: 316, height: 56)
        case "nav-bar":
            ReferenceNavigationBar().ignoresSafeArea()
        case "list-row":
            ReferenceListRow().frame(width: 300, height: 44)
        case "card":
            Text("Content").frame(width: 300, height: 160)
                .glassEffect(.regular, in: .rect(cornerRadius: 20))
        case "menu":
            Menu("Options") {
                if selection == 1 {
                    Section("Document") {
                        Button("Copy") {}
                        Button("Share") {}
                    }
                    Section("Danger") {
                        Button("Delete", role: .destructive) {}
                        Button("Cancel") {}
                    }
                } else {
                    Button("Copy") {}
                    Button("Share") {}
                    Button("Delete", role: .destructive) {}
                }
            }.buttonStyle(.glass)
        default:
            Text("Unknown reference control: \(component)")
        }
    }
}

private struct ReferenceSearchBar: UIViewRepresentable {
    func makeUIView(context: Context) -> UISearchBar {
        let bar = UISearchBar()
        bar.searchBarStyle = .minimal
        bar.placeholder = "Search"
        return bar
    }
    func updateUIView(_ uiView: UISearchBar, context: Context) {}
}

private struct ReferenceNavigationBar: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UINavigationController {
        let content = UIViewController()
        content.title = "Library"
        content.navigationItem.largeTitleDisplayMode = .always
        let scroll = UIScrollView()
        scroll.contentSize = CGSize(width: 300, height: 400)
        scroll.backgroundColor = .clear
        content.view = scroll
        let navigation = UINavigationController(rootViewController: content)
        navigation.navigationBar.prefersLargeTitles = true
        return navigation
    }
    func updateUIViewController(_ uiViewController: UINavigationController, context: Context) {}
}

private struct ReferenceListRow: UIViewRepresentable {
    final class Coordinator: NSObject, UITableViewDataSource {
        func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int { 1 }
        func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
            let cell = UITableViewCell(style: .default, reuseIdentifier: nil)
            cell.textLabel?.text = "Content"
            cell.backgroundColor = .clear
            return cell
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIView(context: Context) -> UITableView {
        let table = UITableView(frame: .zero, style: .plain)
        table.dataSource = context.coordinator
        table.rowHeight = 44
        table.isScrollEnabled = false
        table.backgroundColor = .clear
        return table
    }
    func updateUIView(_ uiView: UITableView, context: Context) {}
}

private struct ReferenceBackdrop: View {
    let pattern: String
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        Canvas { context, size in
            let gray = pattern.hasPrefix("gray-") ? Double(pattern.dropFirst(5)) : nil
            let rgb = pattern.hasPrefix("rgb-") ? pattern.dropFirst(4).split(separator: ",").compactMap { Double($0) } : []
            let background: Color = rgb.count == 3 ? Color(red: rgb[0], green: rgb[1], blue: rgb[2])
                : gray.map { Color(white: min(max($0, 0), 1)) } ?? (colorScheme == .dark ? .black : .white)
            context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(background))
            if pattern == "checkerboard" || pattern == "checkerboard-mono" {
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
                        let rect = CGRect(x: CGFloat(column) * cell, y: CGFloat(row) * cell,
                                          width: cell, height: cell)
                        if pattern == "checkerboard-mono" {
                            context.fill(Path(rect), with: .color((row + column).isMultiple(of: 2) ? .white : .black))
                        } else {
                            context.fill(Path(rect), with: .color(colors[(column + row) % colors.count]))
                            if (row + column).isMultiple(of: 2) {
                                context.fill(Path(rect), with: .color(.white.opacity(0.55)))
                            }
                        }
                    }
                }
            }
        }
        .accessibilityHidden(true)
    }
}
