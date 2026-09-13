import SwiftUI

private struct RecordedKeyframe: Decodable, Identifiable {
    let id: String
    let route: Int
    let gestureLabel: String
    let phase: String
    let cranposePhase: String
    let time: Double
    let native: String
    let cranpose: String
    let nativeFrame: Int
    let cranposeFrame: Int
    let nativeTime: Double
    let cranposeTime: Double
    let nativeAvailable: Bool
    let cranposeAvailable: Bool
}

struct KeyframeInspector: View {
    @State private var index = 0.0
    @State private var route = 0
    @State private var playing = false
    @State private var rate = 0.1
    @Environment(\.dismiss) private var dismiss
    private let captured: [[RecordedKeyframe]]
    private var frames: [RecordedKeyframe] { captured[route] }

    init() {
        if let url = Bundle.main.url(forResource: "keyframes", withExtension: "json"),
           let data = try? Data(contentsOf: url),
           let decoded = try? JSONDecoder().decode([RecordedKeyframe].self, from: data) {
            let routes = Set(decoded.map(\.route)).sorted()
            captured = routes.isEmpty ? [[]] : routes.map { route in decoded.filter { $0.route == route } }
        } else { captured = [[]] }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if frames.isEmpty {
                        Text("No recording bundled").font(.headline)
                        Text("Build the comparison with liquid-reference-bundle to include every captured Native and Cranpose frame.")
                    } else {
                        let frame = frames[min(Int(index), frames.count - 1)]
                        Picker("Gesture", selection: $route) {
                            ForEach(captured.indices, id: \.self) { route in
                                Text(captured[route][0].gestureLabel).tag(route)
                                    .accessibilityIdentifier("recorded-route-\(route)")
                            }
                        }.pickerStyle(.segmented)
                        Text("Input \(frame.time, specifier: "%.3f") s").font(.title2.monospacedDigit().bold())
                        Text("Paired step \(Int(index) + 1) / \(frames.count)")
                            .font(.caption.monospacedDigit()).accessibilityIdentifier("frame-counter")
                        Text("Source time gap \(abs(frame.nativeTime - frame.cranposeTime) * 1000, specifier: "%.1f") ms")
                            .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                        Slider(value: $index, in: 0...Double(max(1, frames.count - 1)), step: 1,
                               onEditingChanged: { _ in playing = false })
                            .accessibilityLabel("Animation frame")
                        HStack {
                            Button { step(-1) } label: { Image(systemName: "backward.frame.fill") }
                                .accessibilityLabel("Previous frame").disabled(index == 0)
                            Button { playing.toggle() } label: { Image(systemName: playing ? "pause.fill" : "play.fill") }
                                .accessibilityLabel(playing ? "Pause" : "Play")
                            Button { step(1) } label: { Image(systemName: "forward.frame.fill") }
                                .accessibilityLabel("Next frame").disabled(Int(index) == frames.count - 1)
                            Spacer()
                            Picker("Speed", selection: $rate) {
                                Text("0.1×").tag(0.1)
                                Text("0.25×").tag(0.25)
                                Text("0.5×").tag(0.5)
                                Text("1×").tag(1.0)
                            }.pickerStyle(.menu)
                        }.buttonStyle(.bordered)
                        ScrollView(.horizontal) {
                            HStack {
                                boundaryButton("Before down", phase: "touch-down", before: true)
                                boundaryButton("After down", phase: "touch-down", before: false)
                                boundaryButton("Before up", phase: "touch-up", before: true)
                                boundaryButton("After up", phase: "touch-up", before: false)
                                Button("Final frame") {
                                    playing = false
                                    index = Double(frames.count - 1)
                                }.buttonStyle(.bordered)
                            }
                        }
                        ScrollView(.horizontal) {
                            HStack {
                                ForEach(["rest", "touch-down", "pressed", "sliding", "stopped", "idle-held", "touch-up", "settling"], id: \.self) { phase in
                                    Button(phase.replacingOccurrences(of: "-", with: " ")) {
                                        playing = false
                                        if let target = frames.firstIndex(where: { $0.phase == phase }) { index = Double(target) }
                                    }.buttonStyle(.bordered)
                                }
                            }
                        }
                        recordedImage("Native iOS", name: frame.native, phase: frame.phase,
                                      sourceFrame: frame.nativeFrame, time: frame.nativeTime,
                                      available: frame.nativeAvailable)
                        recordedImage("Cranpose", name: frame.cranpose, phase: frame.cranposePhase,
                                      sourceFrame: frame.cranposeFrame, time: frame.cranposeTime,
                                      available: frame.cranposeAvailable)
                        Text("Complete recordings include the frames before touch down and after touch up, the contact bounce, drag rebound and final settling. Uncaptured intervals are marked. There are no generated intermediate frames; physics and pixels still differ.")
                            .font(.footnote).foregroundStyle(.secondary)
                    }
                }.padding()
            }
            .navigationTitle("Animation frames")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
        }
        .onChange(of: route) { playing = false; index = 0 }
        .onChange(of: rate) { playing = false }
        .task(id: playing) {
            guard playing, !frames.isEmpty else { return }
            if Int(index) == frames.count - 1 { index = 0 }
            let began = Date()
            let origin = frames[Int(index)].time
            while !Task.isCancelled && playing {
                let time = origin + Date().timeIntervalSince(began) * rate
                while Int(index) + 1 < frames.count && frames[Int(index) + 1].time <= time { index += 1 }
                if Int(index) == frames.count - 1 { playing = false; break }
                do { try await Task.sleep(for: .milliseconds(8)) } catch { break }
            }
        }
    }

    private func step(_ amount: Int) {
        playing = false
        index = Double(max(0, min(frames.count - 1, Int(index) + amount)))
    }

    private func boundaryButton(_ title: String, phase: String, before: Bool) -> some View {
        Button(title) {
            playing = false
            if let first = frames.firstIndex(where: { $0.phase == phase }) {
                index = Double(max(0, first - (before ? 1 : 0)))
            }
        }.buttonStyle(.bordered)
    }

    @ViewBuilder
    private func recordedImage(_ title: String, name: String, phase: String, sourceFrame: Int, time: Double, available: Bool) -> some View {
        Text(title + " · " + phase).font(.headline)
        if !available {
            Text("Outside this recording’s captured interval")
                .frame(maxWidth: .infinity, minHeight: 100)
                .background(.black.opacity(0.08))
        } else if let url = Bundle.main.url(forResource: name, withExtension: nil),
           let image = UIImage(contentsOfFile: url.path) {
            Image(uiImage: image).resizable().interpolation(.none).scaledToFit()
                .accessibilityLabel(title + " recorded frame")
        }
        Text("Source frame \(sourceFrame) · input \(time, specifier: "%.3f") s")
            .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
    }
}
