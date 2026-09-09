import Foundation
import Vision

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
let handler = VNImageRequestHandler(url: URL(fileURLWithPath: CommandLine.arguments[1]), options: [:])
try handler.perform([request])
let rows = (request.results ?? []).compactMap { observation -> [String: Any]? in
    guard let candidate = observation.topCandidates(1).first else { return nil }
    let box = observation.boundingBox
    return ["text": candidate.string, "confidence": candidate.confidence, "bounds": [box.minX, box.minY, box.width, box.height]]
}
let data = try JSONSerialization.data(withJSONObject: rows, options: [.prettyPrinted])
print(String(decoding: data, as: UTF8.self))
