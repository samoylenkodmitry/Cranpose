import SpriteKit
import SwiftUI
import WatchKit
import simd

/// Where each part of a sprite starts, as `cranpose_render_pixels::sprites`
/// lays it out.
private enum Field {
    static let floats = 36
    static let frame = 0
    static let kind = 4
    static let textureKind: Float = 0
}

/// The per-sprite values the shader reads, one `vec4` each, in record order.
private let attributeNames = [
    "a_frame", "a_kind", "a_geometry", "a_detail", "a_stroke", "a_color", "a_color_end",
    "a_gradient", "a_stops",
]

/// The coverage and brush of every sprite kind, the same arithmetic as
/// `cranpose_render_common::shape_sdf` and `brush_sampling`. Positions are
/// device pixels from the top left; `p` is the pixel's centre.
private let shaderSource = """
float coverage_for_distance(float d) {
    float t = clamp(d + 0.5, 0.0, 1.0);
    return 1.0 - t * t * (3.0 - 2.0 * t);
}

float sdf_rounded_rect(vec2 p, vec2 half_size, vec4 radii) {
    float r = p.x > 0.0 ? (p.y > 0.0 ? radii.z : radii.y) : (p.y > 0.0 ? radii.w : radii.x);
    vec2 q = abs(p) - half_size + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
}

float sdf_stroked(vec2 p, vec2 half_size, vec4 radii, float hw, float join_code) {
    vec2 geometry = max(half_size - hw, 0.0);
    vec4 outer_radii = radii + hw;
    if (join_code != 1.0) {
        outer_radii = outer_radii * step(vec4(0.0001), radii);
    }
    vec4 inner_radii = max(radii - hw, 0.0);
    float outer = sdf_rounded_rect(p, geometry + hw, outer_radii);
    float inner = sdf_rounded_rect(p, max(geometry - hw, 0.0), inner_radii);
    float d = max(outer, -inner);
    if (join_code == 2.0) {
        d = max(d, (abs(p.x) + abs(p.y) - (geometry.x + geometry.y + hw)) * 0.70710678);
    }
    return d;
}

float sdf_arc(vec2 p, vec2 centre, float inner, float outer, float start, float sweep, float cap) {
    float ra = (inner + outer) * 0.5;
    float rb = max((outer - inner) * 0.5, 0.0);
    float half_sweep = clamp(sweep, 0.0, 6.28318531) * 0.5;
    float mid = start + half_sweep;
    float sm = sin(mid);
    float cm = cos(mid);
    vec2 d = p - centre;
    float qx = abs(-sm * d.x + cm * d.y);
    float qy = cm * d.x + sm * d.y;
    vec2 sc = vec2(max(sin(half_sweep), 0.0), cos(half_sweep));
    float dist;
    if (sc.y * qx > sc.x * qy) {
        dist = length(vec2(qx - sc.x * ra, qy - sc.y * ra)) - rb;
    } else {
        dist = abs(length(vec2(qx, qy)) - ra) - rb;
    }
    float plane = sc.y * qx - sc.x * qy;
    if (cap == 0.0) {
        dist = max(dist, plane);
    } else if (cap == 2.0) {
        dist = max(dist, plane - rb);
    }
    return dist;
}

float line_coverage(vec2 p, vec2 start, vec2 end, float hw, float cap) {
    vec2 along_line = end - start;
    float len = length(along_line);
    vec2 dir = len > 0.0 ? along_line / len : vec2(1.0, 0.0);
    vec2 d = p - (start + end) * 0.5;
    float along = abs(d.x * dir.x + d.y * dir.y);
    float across = abs(d.x * dir.y - d.y * dir.x);
    float across_coverage = clamp(hw + 0.5 - across, 0.0, 1.0);
    float half_len = len * 0.5;
    if (cap == 1.0) {
        if (along <= half_len) {
            return across_coverage;
        }
        float dist = length(vec2(along - half_len, across)) - hw;
        return clamp(0.5 - dist, 0.0, 1.0);
    }
    float reach = half_len + (cap == 0.0 ? 0.0 : hw);
    return across_coverage * clamp(reach + 0.5 - along, 0.0, 1.0);
}

float tile(float t, float mode) {
    if (mode == 0.0) {
        return clamp(t, 0.0, 1.0);
    }
    if (mode == 3.0) {
        return (t >= 0.0 && t <= 1.0) ? t : -1.0;
    }
    if (mode == 1.0) {
        return t - floor(t);
    }
    float wrapped = t - 2.0 * floor(t * 0.5);
    return wrapped <= 1.0 ? wrapped : 2.0 - wrapped;
}

// SpriteKit hands a sprite's attributes to `main` only, so the brush comes
// in as arguments.
vec4 paint(vec2 p, float brush, float tile_mode, vec4 color, vec4 color_end, vec4 gradient, vec4 stops) {
    if (brush == 0.0) {
        return color;
    }
    float t;
    if (brush == 1.0) {
        vec2 span = gradient.zw - gradient.xy;
        t = tile(dot(p - gradient.xy, span) / max(dot(span, span), 0.00000012), tile_mode);
    } else if (brush == 2.0) {
        t = tile(length(p - gradient.xy) / gradient.z, tile_mode);
    } else {
        vec2 d = p - gradient.xy;
        t = clamp(atan(d.y, d.x) / 6.28318531 + 0.5, 0.0, 1.0);
    }
    if (t < 0.0) {
        return vec4(0.0);
    }
    vec4 c;
    if (t <= stops.x) {
        c = color;
    } else if (t >= stops.y) {
        c = color_end;
    } else {
        c = mix(color, color_end, clamp((t - stops.x) / max(stops.y - stops.x, 0.00000012), 0.0, 1.0));
    }
    if (c.a > 0.0) {
        vec2 cell = max(floor(p - stops.zw), 0.0) + 1.0;
        float m = mod(cell.y, 2.0) * 8.0 + mod(cell.x, 2.0) * 4.0
            + mod(floor(cell.y * 0.5), 2.0) * 2.0 + mod(floor(cell.x * 0.5), 2.0);
        c.rgb = clamp(c.rgb + (m / 16.0 - 15.0 / 32.0) / 255.0, 0.0, 1.0);
    }
    return c;
}

// SpriteKit turns `main` into a function that returns the colour, so it
// assigns `gl_FragColor` once at the end and never returns early.
void main() {
    vec2 p = a_frame.xy + vec2(v_tex_coord.x, 1.0 - v_tex_coord.y) * a_frame.zw;
    float kind = a_kind.x;
    vec4 color;
    if (kind == 0.0) {
        vec2 uv = (p - a_geometry.xy) / a_geometry.zw;
        color = texture2D(u_texture, vec2(uv.x, 1.0 - uv.y));
    } else {
        float coverage;
        if (kind == 1.0) {
            vec2 half_size = a_geometry.zw * 0.5;
            coverage = a_stroke.x > 0.5
                ? coverage_for_distance(sdf_rounded_rect(p - a_geometry.xy - half_size, half_size, a_detail))
                : 1.0;
        } else if (kind == 2.0) {
            vec2 half_size = a_geometry.zw * 0.5;
            coverage = coverage_for_distance(
                sdf_stroked(p - a_geometry.xy - half_size, half_size, a_detail, a_stroke.x, a_stroke.y));
        } else if (kind == 3.0) {
            coverage = coverage_for_distance(
                sdf_arc(p, a_geometry.xy, a_geometry.z, a_geometry.w, a_detail.x, a_detail.y, a_detail.z));
        } else {
            coverage = line_coverage(p, a_geometry.xy, a_geometry.zw, a_detail.x, a_detail.y);
        }
        vec4 c = paint(p, a_kind.w, a_stroke.z, a_color, a_color_end, a_gradient, a_stops);
        float alpha = c.a * coverage;
        color = a_kind.z == 1.0 ? vec4(1.0 - alpha) : vec4(c.rgb * alpha, alpha);
    }
    gl_FragColor = color;
}
"""

/// The scene SpriteKit draws: one node per sprite, in drawing order.
final class CranposeScene: SKScene {
    private let epoch = ProcessInfo.processInfo.systemUptime
    private let spriteShader: SKShader = {
        let shader = SKShader(source: shaderSource)
        shader.attributes = attributeNames.map { SKAttribute(name: $0, type: .vectorFloat4) }
        return shader
    }()
    private var nodes: [SKSpriteNode] = []
    /// What each node shows now, so an unchanged sprite costs nothing.
    private var shown: [Float] = []
    private var textures: [UInt32: SKTexture] = [:]
    private var live = false

    var elapsed: UInt64 {
        UInt64(max(0, ProcessInfo.processInfo.systemUptime - epoch) * 1_000_000_000)
    }

    override init(size: CGSize) {
        super.init(size: size)
        scaleMode = .fill
        anchorPoint = .zero
        // The colour the software renderer clears a frame to.
        backgroundColor = SKColor(red: 18 / 255, green: 18 / 255, blue: 24 / 255, alpha: 1)
    }

    required init?(coder: NSCoder) { nil }

    func start() {
        live = true
        present()
    }

    override func update(_ currentTime: TimeInterval) {
        guard live, CPTick(elapsed) else { return }
        present()
    }

    /// Applies the sprites of the current picture.
    func present() {
        var count = 0
        guard let records = CPSprites(&count) else { return }
        for index in 0..<CPUploadCount() {
            var id: UInt32 = 0
            var width: UInt32 = 0
            var height: UInt32 = 0
            guard let pixels = CPUpload(index, &id, &width, &height), width > 0, height > 0 else { continue }
            let texture = SKTexture(
                data: Data(bytes: pixels, count: Int(width * height * 4)),
                size: CGSize(width: Int(width), height: Int(height)),
                flipped: true)
            texture.filteringMode = .nearest
            textures[id] = texture
        }
        for index in 0..<CPReleaseCount() {
            textures.removeValue(forKey: CPRelease(index))
        }
        let sprites = count / Field.floats
        if shown.count < count {
            shown.append(contentsOf: repeatElement(Float.nan, count: count - shown.count))
        }
        for sprite in 0..<sprites {
            let base = sprite * Field.floats
            let node = node(at: sprite)
            node.isHidden = false
            var unchanged = true
            for offset in 0..<Field.floats where shown[base + offset] != records[base + offset] {
                unchanged = false
                break
            }
            if unchanged { continue }
            for offset in 0..<Field.floats {
                shown[base + offset] = records[base + offset]
            }
            let frame = base + Field.frame
            node.position = CGPoint(
                x: CGFloat(records[frame]), y: size.height - CGFloat(records[frame + 1]))
            node.size = CGSize(width: CGFloat(records[frame + 2]), height: CGFloat(records[frame + 3]))
            let kind = base + Field.kind
            node.texture = records[kind] == Field.textureKind ? textures[UInt32(records[kind + 1])] : nil
            node.blendMode = records[kind + 2] == 1 ? .multiply : .alpha
            for (slot, name) in attributeNames.enumerated() {
                let at = base + slot * 4
                node.setValue(
                    SKAttributeValue(
                        vectorFloat4: vector_float4(records[at], records[at + 1], records[at + 2], records[at + 3])),
                    forAttribute: name)
            }
        }
        for index in sprites..<nodes.count {
            nodes[index].isHidden = true
        }
    }

    private func node(at index: Int) -> SKSpriteNode {
        while nodes.count <= index {
            let node = SKSpriteNode(color: .white, size: .zero)
            node.anchorPoint = CGPoint(x: 0, y: 1)
            node.zPosition = CGFloat(nodes.count)
            node.shader = spriteShader
            addChild(node)
            nodes.append(node)
        }
        return nodes[index]
    }
}

/// The crown's running value. A plain object rather than view state: the
/// crown moves on every event, and view state would rebuild the view (and
/// the SpriteKit view in it) each time before the event reached the app.
///
/// The crown is read through SwiftUI. WatchKit's crown sequencer, which a
/// SwiftUI app reaches through its root interface controller, delivered
/// nothing on a watch running watchOS 27.
final class CrownReading {
    var value = 0.0
}

@MainActor
final class CranposeModel: ObservableObject {
    @Published var error: String?
    let crown = CrownReading()
    private(set) var scene = CranposeScene(size: CGSize(width: 1, height: 1))
    private var ready = false

    var elapsed: UInt64 { scene.elapsed }

    func resize(_ size: CGSize, safeArea: EdgeInsets) {
        let scale = WKInterfaceDevice.current().screenScale
        let width = UInt32(max(1, (size.width * scale).rounded()))
        let height = UInt32(max(1, (size.height * scale).rounded()))
        if !CPResize(width, height, Float(scale)) {
            error = String(cString: CPError())
            return
        }
        CPSafeArea(Float(safeArea.leading), Float(safeArea.top), Float(safeArea.trailing), Float(safeArea.bottom))
        scene.size = CGSize(width: Int(width), height: Int(height))
        if !ready {
            ready = true
            scene.start()
        } else {
            _ = CPTick(elapsed)
            scene.present()
        }
    }

    func activate(_ active: Bool) {
        CPActive(active)
        scene.isPaused = !active
    }
}

struct CranposeView: View {
    @StateObject private var model = CranposeModel()
    @Environment(\.scenePhase) private var phase
    @State private var touching = false

    var body: some View {
        // The outer reader keeps to the safe area and reports its edges, such
        // as the clock's; the inner one draws on the whole screen.
        GeometryReader { safe in GeometryReader { geometry in
            ZStack {
                SpriteView(scene: model.scene, preferredFramesPerSecond: 60)
                    .allowsHitTesting(false)
                if let error = model.error { Text(error).foregroundStyle(.red) }
            }
            .contentShape(Rectangle())
            .focusable()
            .digitalCrownRotation(
                // Each new value goes to the app as it arrives, as the change
                // from the one before. Without a stride: with one, SwiftUI
                // steps the value and settles it on a step after the hand
                // stops, so the app saw the crown late and saw it move after
                // it stopped.
                Binding(get: { model.crown.value }, set: { value in
                    let delta = value - model.crown.value
                    model.crown.value = value
                    CPCrown(Float(delta), model.elapsed / 1_000_000)
                }),
                from: -1_000_000, through: 1_000_000, sensitivity: .medium,
                isContinuous: true, isHapticFeedbackEnabled: false)
            .gesture(DragGesture(minimumDistance: 0)
                .onChanged { event in
                    if !touching {
                        CPTouch(0, Float(event.startLocation.x), Float(event.startLocation.y))
                        touching = true
                    }
                    CPTouch(1, Float(event.location.x), Float(event.location.y))
                }
                .onEnded { event in
                    CPTouch(2, Float(event.location.x), Float(event.location.y))
                    touching = false
                })
            .onAppear { model.resize(geometry.size, safeArea: safe.safeAreaInsets); model.activate(phase == .active) }
            .onChange(of: geometry.size) { _, size in model.resize(size, safeArea: safe.safeAreaInsets) }
            .onChange(of: safe.safeAreaInsets) { _, insets in model.resize(geometry.size, safeArea: insets) }
            .onChange(of: phase) { _, phase in touching = false; model.activate(phase == .active) }
            .onDisappear { touching = false; model.activate(false) }
        }
        .ignoresSafeArea() }
    }
}

@main
struct CranposeWatchApp: App {
    var body: some Scene { WindowGroup { CranposeView() } }
}
