// Deterministic benchmark data: `perf-data/src/lib.rs`,
// `shared-kotlin/dev/perfcompare/shared/PerfData.kt` and `shared-cs/PerfData.cs`
// implement the same generator bit for bit, so every app draws identical
// content. Only what the gauntlet shows is kept.

import Foundation

enum PerfData {
    static let postCount = 5000
    static let barCount = 24
    static let cardRowsPerCluster = 5
    static let avatarCount = 8
    static let avatarSize = 64
    static let sparkPoints = 48

    static let words = [
        "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do",
        "eiusmod", "tempor", "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua", "enim",
        "ad", "minim", "veniam", "quis", "nostrud", "exercitation", "ullamco", "laboris", "nisi",
        "aliquip", "ex", "ea", "commodo", "consequat", "duis", "aute", "irure", "in", "reprehenderit",
        "voluptate", "velit", "esse", "cillum", "fugiat", "nulla", "pariatur",
    ]
    static let first = [
        "Ada", "Linus", "Grace", "Alan", "Barbara", "Dennis", "Ken", "Margaret", "Edsger", "Donald",
        "Frances", "John", "Radia", "Tim", "Guido", "Bjarne",
    ]
    static let last = [
        "Lovelace", "Torvalds", "Hopper", "Turing", "Liskov", "Ritchie", "Thompson", "Hamilton",
        "Dijkstra", "Knuth", "Allen", "McCarthy", "Perlman", "Berners", "Rossum", "Stroustrup",
    ]
    static let tags = [
        "#rust", "#kotlin", "#compose", "#android", "#gpu", "#layout", "#text", "#perf", "#wgpu",
        "#skia", "#ui", "#mobile",
    ]

    static let paletteRGB: [UInt32] = [0xEF4444, 0xF97316, 0xEAB308, 0x22C55E, 0x14B8A6, 0x3B82F6, 0x8B5CF6, 0xEC4899]
    static let chipBackgroundRGB: [UInt32] = [0xFEE2E2, 0xFFEDD5, 0xFEF9C3, 0xDCFCE7, 0xCCFBF1, 0xDBEAFE, 0xEDE9FE, 0xFCE7F3]
    static let gradientEndRGB: [UInt32] = [0x7F1D1D, 0x7C2D12, 0x713F12, 0x14532D, 0x134E4A, 0x1E3A8A, 0x4C1D95, 0x831843]

    /// 32-bit xorshift, as in the other apps.
    struct Rng {
        var state: UInt32

        init(_ seed: Int) {
            state = (UInt32(truncatingIfNeeded: seed) &* 0x9E37_79B9) ^ 0xA5A5_A5A5
            if state == 0 { state = 1 }
        }

        mutating func next() -> UInt32 {
            var x = state
            x ^= x << 13
            x ^= x >> 17
            x ^= x << 5
            state = x
            return x
        }

        mutating func below(_ bound: Int) -> Int { Int(next() % UInt32(bound)) }

        mutating func unit() -> Float { Float(next() >> 8) / 16_777_216 }
    }

    struct Post {
        let author: String
        let color: Int
        let title: String
        let body: String
        let tags: [(text: String, color: Int)]
        let stats: [String]
    }

    static func sentence(_ rng: inout Rng, _ min: Int, _ max: Int) -> String {
        let count = min + rng.below(max - min + 1)
        let text = (0..<count).map { _ in words[rng.below(46)] }.joined(separator: " ")
        return text.prefix(1).uppercased() + text.dropFirst()
    }

    static func compactCount(_ value: Int) -> String {
        value >= 1000 ? "\(value / 1000).\(value % 1000 / 100)k" : "\(value)"
    }

    static func post(_ index: Int) -> Post {
        var rng = Rng(index + 1)
        let firstName = first[rng.below(16)]
        let lastName = last[rng.below(16)]
        _ = rng.below(59) // minutes, in the handle the gauntlet does not show
        _ = rng.below(1000) // the handle's number
        let color = rng.below(8)
        let title = sentence(&rng, 4, 8)
        let body = sentence(&rng, 22, 38) + "."
        for _ in 0..<barCount { _ = rng.unit() }
        let postTags = (0..<3).map { _ -> (text: String, color: Int) in
            let which = rng.below(tags.count)
            return (tags[which], which % 8)
        }
        let stats = (0..<4).map { _ in compactCount(rng.below(20_000)) }
        return Post(author: "\(firstName) \(lastName)", color: color, title: title, body: body, tags: postTags,
                    stats: stats)
    }

    static func posts() -> [Post] { (0..<postCount).map(post) }

    struct Tier {
        let columns: Int
        let scale: CGFloat
        let tickers: Int
        let depth: Int
    }

    static let tiers = [
        Tier(columns: 1, scale: 1.0, tickers: 8, depth: 6), Tier(columns: 2, scale: 0.85, tickers: 12, depth: 8),
        Tier(columns: 2, scale: 0.7, tickers: 16, depth: 10), Tier(columns: 3, scale: 0.6, tickers: 20, depth: 12),
        Tier(columns: 3, scale: 0.5, tickers: 28, depth: 14), Tier(columns: 4, scale: 0.45, tickers: 36, depth: 16),
        Tier(columns: 4, scale: 0.4, tickers: 44, depth: 20), Tier(columns: 5, scale: 0.35, tickers: 56, depth: 24),
        Tier(columns: 6, scale: 0.3, tickers: 72, depth: 28), Tier(columns: 7, scale: 0.27, tickers: 96, depth: 32),
        Tier(columns: 8, scale: 0.25, tickers: 120, depth: 40), Tier(columns: 10, scale: 0.2, tickers: 160, depth: 48),
        Tier(columns: 12, scale: 0.18, tickers: 200, depth: 56),
        Tier(columns: 14, scale: 0.16, tickers: 240, depth: 64),
        Tier(columns: 16, scale: 0.14, tickers: 300, depth: 72),
        Tier(columns: 20, scale: 0.12, tickers: 400, depth: 80),
    ]

    /// Tier 1 to 16; anything else is clamped into that range.
    static func tier(_ tier: Int) -> Tier { tiers[min(max(tier, 1), tiers.count) - 1] }

    struct Ticker {
        let symbol: String
        let baseCents: Int
        let swingCents: Int
        let step: Int
        let phase: Int
    }

    static func tickers(_ count: Int) -> [Ticker] {
        var rng = Rng(31_337)
        return (0..<count).map { _ in
            let length = 3 + rng.below(2)
            let symbol = String((0..<length).map { _ in Character(UnicodeScalar(UInt8(65 + rng.below(26)))) })
            let baseCents = 1_000 + rng.below(99_000)
            let swingCents = 50 + rng.below(950)
            let step = 1 + rng.below(9)
            let phase = rng.below(2_000)
            return Ticker(symbol: symbol, baseCents: baseCents, swingCents: swingCents, step: step, phase: phase)
        }
    }

    /// A triangle wave over the period: 0 at the ends, half the period in the middle.
    static func triangle(_ value: Int, _ period: Int) -> Int {
        let position = (value % period + period) % period
        return period / 2 - abs(position - period / 2)
    }

    /// The quote's price on the frame, in cents.
    static func tickerCents(_ ticker: Ticker, _ frame: Int) -> Int {
        let wave = triangle(frame * ticker.step + ticker.phase, 2_000)
        return ticker.baseCents + ticker.swingCents * (wave - 500) / 500
    }

    static func twoDecimals(_ hundredths: Int) -> String {
        let value = abs(hundredths)
        let fraction = value % 100
        return "\(value / 100).\(fraction < 10 ? "0" : "")\(fraction)"
    }

    /// 1234 → 12.34.
    static func centsText(_ cents: Int) -> String { (cents < 0 ? "-" : "") + twoDecimals(cents) }

    /// The change from the base price as a signed percent: +1.25%.
    static func changeText(_ ticker: Ticker, _ cents: Int) -> String {
        let basisPoints = (cents - ticker.baseCents) * 10_000 / ticker.baseCents
        return (basisPoints < 0 ? "-" : "+") + twoDecimals(basisPoints) + "%"
    }

    /// A card's progress on the frame, in thousandths.
    static func progressPermille(_ card: Int, _ frame: Int) -> Int { (frame * 3 + card * 37) % 1_000 }

    /// The tilt of a card's badge on the frame, in degrees: -5 to 5.
    static func badgeDegrees(_ card: Int, _ frame: Int) -> Double {
        Double(Float(triangle(frame * 2 + card * 30, 40)) * 0.5 - 5)
    }

    /// The content's share of the window width on the frame: 0.92 to 1.
    static func widthFraction(_ frame: Int) -> CGFloat {
        CGFloat(0.92 + 0.08 * Float(triangle(frame * 3, 200)) / 100)
    }

    /// The sparkline's height at the point, as a fraction of the chart, on the frame.
    static func sparkValue(_ card: Int, _ point: Int, _ frame: Int) -> Float {
        let phase = (Float(frame) + Float(card) * 7) * 0.11
        return 0.5 + 0.38 * sinf(Float(point) * 0.32 + phase) + 0.08 * sinf(Float(point) * 1.7 + Float(card))
    }

    static let avatarColors: [[Int]] = [
        [0xEF, 0x44, 0x44, 0x7F, 0x1D, 0x1D], [0xF9, 0x73, 0x16, 0x7C, 0x2D, 0x12],
        [0xEA, 0xB3, 0x08, 0x71, 0x3F, 0x12], [0x22, 0xC5, 0x5E, 0x14, 0x53, 0x2D],
        [0x14, 0xB8, 0xA6, 0x13, 0x4E, 0x4A], [0x3B, 0x82, 0xF6, 0x1E, 0x3A, 0x8A],
        [0x8B, 0x5C, 0xF6, 0x4C, 0x1D, 0x95], [0xEC, 0x48, 0x99, 0x83, 0x18, 0x43],
    ]

    /// Avatar pixels as RGBA bytes: a radial gradient crossed by diagonal stripes.
    static func avatarRGBA(_ index: Int) -> [UInt8] {
        let colors = avatarColors[index % avatarCount]
        let size = avatarSize
        var pixels = [UInt8]()
        pixels.reserveCapacity(size * size * 4)
        for y in 0..<size {
            for x in 0..<size {
                let dx = x - size / 2
                let dy = y - size * 3 / 8
                let t = min(255, (dx * dx + dy * dy) * 255 / (40 * 40))
                let stripe = (x + y + index * 3) / 6 % 2 == 0
                for channel in 0..<3 {
                    var value = (colors[channel] * (255 - t) + colors[channel + 3] * t) / 255
                    if stripe { value += (255 - value) / 6 }
                    pixels.append(UInt8(value))
                }
                pixels.append(0xFF)
            }
        }
        return pixels
    }

    /// What a list row holds: a row of cards from `first`, or every sixth row a cluster.
    enum Row {
        case cards(first: Int)
        case cluster(Int)
    }

    static func row(_ row: Int, columns: Int) -> Row {
        let block = row / (cardRowsPerCluster + 1)
        let within = row % (cardRowsPerCluster + 1)
        if within == cardRowsPerCluster { return .cluster(block) }
        return .cards(first: (block * cardRowsPerCluster + within) * columns)
    }
}
