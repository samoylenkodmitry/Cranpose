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
    @State private var text = ProcessInfo.processInfo.environment["REFERENCE_TEXT"] ?? ""
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
            // The scroll edge cases keep the label for the tests and draw no
            // pixels of it over the stripes.
            Text("Reference control").font(.caption).padding(.vertical, 24)
                .foregroundStyle(component == "scroll-edge" ? AnyShapeStyle(.clear) : AnyShapeStyle(.primary))
        }
        .overlay(alignment: .bottom) {
            // The toolbar's capsules sit where the count would.
            VStack {
                Text("Activations: \(activations)").opacity(["toolbar", "scroll-edge"].contains(component) ? 0 : 1)
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
        case "stepper":
            Stepper("Quantity", value: $value, in: 0...1, step: 0.1).labelsHidden()
                .accessibilityIdentifier("reference-stepper")
        case "progress":
            ProgressView(value: value).frame(width: 300).accessibilityIdentifier("reference-progress")
        case "activity":
            ProgressView().controlSize(value > 0.5 ? .large : .regular)
                .accessibilityIdentifier("reference-activity")
        case "gauge":
            Gauge(value: value) { Text("Level") }.frame(width: 300).accessibilityIdentifier("reference-gauge")
        case "disclosure":
            DisclosureGroup("Details", isExpanded: $checked) { Text("More information") }
                .frame(width: 300).accessibilityIdentifier("reference-disclosure")
        case "text-field":
            TextField("Name", text: $text).textFieldStyle(.roundedBorder).frame(width: 300)
                .accessibilityIdentifier("reference-text-field")
                .onAppear { if value == 1 { text = "Ada" } }
        case "wheel":
            Picker("Fruit", selection: $selection) {
                ForEach(Array(ReferenceChoices.fruits.enumerated()), id: \.offset) { index, fruit in
                    Text(fruit).tag(index)
                }
            }
            .pickerStyle(.wheel).frame(width: 300).accessibilityIdentifier("reference-wheel")
        case "wheel-same":
            ReferenceWheel(labels: Array(repeating: "Grape", count: 7), selected: 3, shift: 0)
                .frame(width: 300).accessibilityIdentifier("reference-wheel")
        case "wheel-held":
            ReferenceWheel(labels: ReferenceChoices.fruits, selected: 3, shift: 0.5)
                .frame(width: 300).accessibilityIdentifier("reference-wheel")
        case "time":
            DatePicker("Time", selection: .constant(ReferenceChoices.time), displayedComponents: .hourAndMinute)
                .datePickerStyle(.wheel).labelsHidden().accessibilityIdentifier("reference-time")
        case "time-12":
            DatePicker("Time", selection: .constant(ReferenceChoices.time), displayedComponents: .hourAndMinute)
                .datePickerStyle(.wheel).labelsHidden().environment(\.locale, Locale(identifier: "en_US"))
                .accessibilityIdentifier("reference-time")
        case "pop-up":
            Picker("Fruit", selection: $selection) {
                ForEach(Array(ReferenceChoices.fruits.enumerated()), id: \.offset) { index, fruit in
                    Text(fruit).tag(index)
                }
            }
            .pickerStyle(.menu).accessibilityIdentifier("reference-pop-up")
        case "color-well":
            ColorPicker("Color", selection: .constant(ReferenceChoices.colors[selection % ReferenceChoices.colors.count]))
                .labelsHidden().accessibilityIdentifier("reference-color-well")
        case "alert":
            Color.clear.frame(width: 300, height: 44)
                .alert("Delete Photo?", isPresented: .constant(true)) {
                    Button("Delete", role: .destructive) {}
                    Button("Cancel", role: .cancel) {}
                } message: {
                    Text("This photo will be deleted from all your devices.")
                }
        case "confirmation":
            // Value 0 centres a 300 by 44 source; 1 puts a 44 by 44 source near
            // the top-left corner and 2 near the bottom-right one.
            Color.clear.frame(width: selection == 0 ? 300 : 44, height: 44)
                .confirmationDialog("Delete Photo?", isPresented: .constant(true), titleVisibility: .visible) {
                    Button("Delete", role: .destructive) {}
                    Button("Cancel", role: .cancel) {}
                } message: {
                    Text("This photo will be deleted from all your devices.")
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity,
                       alignment: [Alignment.center, .topLeading, .bottomTrailing][min(selection, 2)])
                .padding(.horizontal, selection == 0 ? 0 : 24).padding(.vertical, selection == 0 ? 0 : 100)
        case "unavailable":
            ContentUnavailableView {
                Text("No Photos")
            } description: {
                Text("Photos you take appear here.")
            } actions: {
                Button("Take Photo") {}
            }
            .frame(width: 320).accessibilityIdentifier("reference-unavailable")
        case "control-group":
            ControlGroup {
                Button("Previous") {}
                Button("Next") {}
            }
            .frame(width: 300).accessibilityIdentifier("reference-control-group")
        case "page-control":
            ReferencePageControl(page: selection).frame(width: 300, height: 28)
                .accessibilityIdentifier("reference-page-control")
        case "toolbar":
            ReferenceToolbar(variant: selection).ignoresSafeArea()
        case "context-menu":
            ReferenceContextMenu(variant: selection).accessibilityIdentifier("reference-context-menu")
        case "swipe-actions":
            ReferenceSwipeRow().frame(width: 300, height: 44).accessibilityIdentifier("reference-swipe-actions")
        case "scroll-edge":
            ReferenceScrollEdge(variant: selection).ignoresSafeArea()
        default:
            Text("Unknown reference control: \(component)")
        }
    }
}

// The choices the picker references offer, shared with the Cranpose app.
private enum ReferenceChoices {
    static let fruits = ["Apple", "Banana", "Cherry", "Grape", "Lemon", "Mango", "Orange"]
    static let colors: [Color] = [.blue, .red, .green]
    static let time = Calendar.current.date(from: DateComponents(year: 2026, month: 1, day: 1, hour: 10, minute: 42))!
}

// A UIKit wheel of `labels` resting on `selected`, turned `shift` rows past
// it without a touch, as a finger holds it between two rows.
private struct ReferenceWheel: UIViewRepresentable {
    let labels: [String]
    let selected: Int
    let shift: CGFloat

    func makeCoordinator() -> Source { Source(labels: labels) }

    func makeUIView(context: Context) -> UIPickerView {
        let picker = UIPickerView()
        picker.dataSource = context.coordinator
        picker.delegate = context.coordinator
        picker.selectRow(selected, inComponent: 0, animated: false)
        if shift != 0 {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
                let height = picker.rowSize(forComponent: 0).height
                for view in scrollViews(in: picker) {
                    view.setContentOffset(CGPoint(x: view.contentOffset.x, y: view.contentOffset.y + shift * height), animated: false)
                }
            }
        }
        return picker
    }

    func updateUIView(_ picker: UIPickerView, context: Context) {}

    final class Source: NSObject, UIPickerViewDataSource, UIPickerViewDelegate {
        let labels: [String]
        init(labels: [String]) { self.labels = labels }
        func numberOfComponents(in pickerView: UIPickerView) -> Int { 1 }
        func pickerView(_ pickerView: UIPickerView, numberOfRowsInComponent component: Int) -> Int { labels.count }
        func pickerView(_ pickerView: UIPickerView, titleForRow row: Int, forComponent component: Int) -> String? { labels[row] }
    }
}

private func scrollViews(in view: UIView) -> [UIScrollView] {
    var found: [UIScrollView] = []
    for subview in view.subviews {
        if let scroll = subview as? UIScrollView {
            found.append(scroll)
        }
        found += scrollViews(in: subview)
    }
    return found
}

// Five pages with `page` current, as a pager under a carousel shows them.
private struct ReferencePageControl: UIViewRepresentable {
    let page: Int

    func makeUIView(context: Context) -> UIPageControl {
        let control = UIPageControl()
        control.numberOfPages = 5
        control.currentPage = page
        return control
    }

    func updateUIView(_ control: UIPageControl, context: Context) {
        control.currentPage = page
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

// A bottom toolbar. Variant 0 shows a lone item and a pair at the far end;
// variant 1 shows a lone item and a prominent one; variant 2 shows a pair of
// image items and a lone one; variant 3 shows three image items, then a text
// item beside an image item.
private struct ReferenceToolbar: UIViewControllerRepresentable {
    let variant: Int
    func makeUIViewController(context: Context) -> UINavigationController {
        let content = UIViewController()
        content.view.backgroundColor = .clear
        let item = { (title: String) in UIBarButtonItem(title: title, style: .plain, target: nil, action: nil) }
        let image = { (name: String) in
            UIBarButtonItem(image: UIImage(systemName: name), style: .plain, target: nil, action: nil)
        }
        let done = UIBarButtonItem(title: "Done", style: .prominent, target: nil, action: nil)
        switch variant {
        case 1: content.toolbarItems = [item("Edit"), .flexibleSpace(), done]
        case 2: content.toolbarItems = [image("square.and.arrow.up"), image("trash"), .flexibleSpace(), image("plus")]
        case 3:
            content.toolbarItems = [
                image("square.and.arrow.up"), image("trash"), image("folder"), .flexibleSpace(), item("Edit"), image("plus"),
            ]
        default: content.toolbarItems = [item("Edit"), .flexibleSpace(), item("Share"), item("Delete")]
        }
        let navigation = UINavigationController(rootViewController: content)
        navigation.setNavigationBarHidden(true, animated: false)
        navigation.setToolbarHidden(false, animated: false)
        navigation.view.backgroundColor = .clear
        return navigation
    }
    func updateUIViewController(_ uiViewController: UINavigationController, context: Context) {}
}

// A 160 by 100 tile with a context menu of three actions. Variant 0 sits
// in the middle of the screen, 1 near its bottom edge and 2 against its left
// edge. Variant 3 lifts a custom 240 by 160 preview in place of the tile.
// Variant 4 sits 71 points left of the middle, so its preview fits whole
// while a menu centred under it would not.
private struct ReferenceContextMenu: View {
    let variant: Int

    var body: some View {
        let tile = RoundedRectangle(cornerRadius: 16, style: .continuous).fill(Color.blue)
            .overlay(Text("Photo").foregroundStyle(.white))
            .frame(width: 160, height: 100)
        let actions = Group {
            Button("Copy") {}
            Button("Share") {}
            Button("Delete", role: .destructive) {}
        }
        switch variant {
        case 1: tile.contextMenu { actions }.offset(y: 330)
        case 2: tile.contextMenu { actions }.offset(x: -110)
        case 4: tile.contextMenu { actions }.offset(x: -71)
        case 3:
            tile.contextMenu { actions } preview: {
                Text("Preview").foregroundStyle(.white).frame(width: 240, height: 160).background(Color.orange)
            }
        default: tile.contextMenu { actions }
        }
    }
}

// Stripes and solids under the scroll view's edges, which show the edge
// effect's blur and tint row by row. Four columns on whole points, 100
// points wide: black, stripes that alternate black and white every 3
// points, stripes that alternate every 12, and white to the right edge. Variant 0 rests at the top with no
// bars and every other variant scrolls 200 points. Variant 1 keeps the
// automatic style, 2 the hard one; 3 and 4 add a navigation bar, automatic
// and hard; 5 adds a bottom toolbar; 6 and 7 stand a custom bar 44 and 100
// points tall over the top edge. Variant 8 shows the toolbar at rest, for a
// test that scrolls it by a drag.
private struct ReferenceScrollEdge: UIViewControllerRepresentable {
    let variant: Int

    final class Content: UIViewController {
        let variant: Int
        let scroll = UIScrollView()
        var placed = false

        init(variant: Int) {
            self.variant = variant
            super.init(nibName: nil, bundle: nil)
        }

        required init?(coder: NSCoder) { nil }

        override func viewDidLoad() {
            super.viewDidLoad()
            view.backgroundColor = .clear
            title = "Title"
            navigationItem.largeTitleDisplayMode = .never
            toolbarItems = [
                UIBarButtonItem(title: "Edit", style: .plain, target: nil, action: nil), .flexibleSpace(),
                UIBarButtonItem(title: "Share", style: .plain, target: nil, action: nil),
            ]
            scroll.backgroundColor = .clear
            if variant == 2 || variant == 4 { scroll.topEdgeEffect.style = .hard }
            view.addSubview(scroll)
            guard variant >= 6 else { return }
            let bar = UILabel()
            bar.text = "Bar"
            bar.textAlignment = .center
            bar.tag = variant == 6 ? 44 : 100
            let container = UIScrollEdgeElementContainerInteraction()
            container.scrollView = scroll
            container.edge = .top
            bar.addInteraction(container)
            view.addSubview(bar)
        }

        override func viewDidLayoutSubviews() {
            super.viewDidLayoutSubviews()
            scroll.frame = view.bounds
            for bar in view.subviews where bar is UILabel {
                bar.frame = CGRect(x: 0, y: view.safeAreaInsets.top, width: view.bounds.width, height: CGFloat(bar.tag))
            }
            guard !placed, view.bounds.width > 0 else { return }
            placed = true
            let width = view.bounds.width
            let height: CGFloat = 3000
            let columns = [UIColor.black, UIColor(patternImage: Self.stripe(period: 6)),
                           UIColor(patternImage: Self.stripe(period: 24)), UIColor.white]
            for (index, fill) in columns.enumerated() {
                let x = CGFloat(index) * 100
                let column = UIView(frame: CGRect(x: x, y: 0, width: index == 3 ? width - x : 100, height: height))
                column.backgroundColor = fill
                scroll.addSubview(column)
            }
            scroll.contentSize = CGSize(width: width, height: height)
            scroll.contentOffset.y = -scroll.adjustedContentInset.top + ([0, 8].contains(variant) ? 0 : 200)
        }

        static func stripe(period: CGFloat) -> UIImage {
            UIGraphicsImageRenderer(size: CGSize(width: period, height: 1)).image { context in
                UIColor.black.setFill()
                context.fill(CGRect(x: 0, y: 0, width: period / 2, height: 1))
                UIColor.white.setFill()
                context.fill(CGRect(x: period / 2, y: 0, width: period / 2, height: 1))
            }
        }
    }

    func makeUIViewController(context: Context) -> UINavigationController {
        let navigation = UINavigationController(rootViewController: Content(variant: variant))
        navigation.setNavigationBarHidden(variant != 3 && variant != 4, animated: false)
        navigation.setToolbarHidden(variant != 5 && variant != 8, animated: false)
        navigation.view.backgroundColor = .clear
        return navigation
    }

    func updateUIViewController(_ uiViewController: UINavigationController, context: Context) {}
}

// A list row with Flag and a destructive Delete at its trailing edge and Pin
// at its leading edge. A full swipe only opens the row, so a test's drag
// shows the actions instead of running one.
private struct ReferenceSwipeRow: UIViewRepresentable {
    final class Coordinator: NSObject, UITableViewDataSource, UITableViewDelegate {
        func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int { 1 }
        func tableView(_ tableView: UITableView, cellForRowAt indexPath: IndexPath) -> UITableViewCell {
            let cell = UITableViewCell(style: .default, reuseIdentifier: nil)
            cell.textLabel?.text = "Content"
            cell.backgroundColor = .clear
            return cell
        }
        func tableView(
            _ tableView: UITableView, trailingSwipeActionsConfigurationForRowAt indexPath: IndexPath
        ) -> UISwipeActionsConfiguration? {
            let delete = UIContextualAction(style: .destructive, title: "Delete") { _, _, done in done(false) }
            let flag = UIContextualAction(style: .normal, title: "Flag") { _, _, done in done(false) }
            flag.backgroundColor = .systemOrange
            let configuration = UISwipeActionsConfiguration(actions: [delete, flag])
            configuration.performsFirstActionWithFullSwipe = false
            return configuration
        }
        func tableView(
            _ tableView: UITableView, leadingSwipeActionsConfigurationForRowAt indexPath: IndexPath
        ) -> UISwipeActionsConfiguration? {
            let pin = UIContextualAction(style: .normal, title: "Pin") { _, _, done in done(false) }
            pin.backgroundColor = .systemBlue
            let configuration = UISwipeActionsConfiguration(actions: [pin])
            configuration.performsFirstActionWithFullSwipe = false
            return configuration
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIView(context: Context) -> UITableView {
        let table = UITableView(frame: .zero, style: .plain)
        table.dataSource = context.coordinator
        table.delegate = context.coordinator
        table.rowHeight = 44
        table.isScrollEnabled = false
        table.backgroundColor = .clear
        return table
    }
    func updateUIView(_ uiView: UITableView, context: Context) {}
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
            if ["checkerboard", "checkerboard-mono", "checkerboard-dark-rainbow"].contains(pattern) {
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
                            let darkRainbow = pattern == "checkerboard-dark-rainbow"
                            if darkRainbow {
                                context.fill(Path(rect), with: .color(.black.opacity(0.65)))
                            }
                            if (row + column).isMultiple(of: 2) {
                                context.fill(Path(rect), with: .color((darkRainbow ? Color.black : Color.white).opacity(0.55)))
                            }
                        }
                    }
                }
            }
        }
        .accessibilityHidden(true)
    }
}
