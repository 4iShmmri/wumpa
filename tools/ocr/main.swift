// wumpa-ocr: on-device text recognition through Apple Vision (macOS).
// Usage: wumpa-ocr <image> [lang,lang,...] [--raw]
// --raw disables language correction, which otherwise drops lone digits and codes.
// Prints one JSON document: lines with normalised boxes (origin bottom-left,
// as Vision reports them), confidences and per-word boxes.
import Foundation
import Vision
import AppKit

struct Box: Encodable { let x: Double; let y: Double; let w: Double; let h: Double }
struct Word: Encodable { let text: String; let box: Box }
struct Line: Encodable { let text: String; let conf: Double; let box: Box; let words: [Word] }
struct Output: Encodable { let width: Int; let height: Int; let lines: [Line] }

func box(_ r: CGRect) -> Box { Box(x: r.minX, y: r.minY, w: r.width, h: r.height) }

let args = CommandLine.arguments
guard args.count >= 2, let image = NSImage(contentsOfFile: args[1]),
      let tiff = image.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff), let cg = rep.cgImage else {
  FileHandle.standardError.write("usage: wumpa-ocr <image> [languages]\n".data(using: .utf8)!)
  exit(2)
}
let raw = args.contains("--raw")
let langArg = args.dropFirst(2).first { !$0.hasPrefix("--") }
let languages = langArg.map { $0.split(separator: ",").map(String.init) } ?? ["ar", "en"]

var lines: [Line] = []
let request = VNRecognizeTextRequest { req, _ in
  for o in (req.results as? [VNRecognizedTextObservation]) ?? [] {
    guard let top = o.topCandidates(1).first else { continue }
    var words: [Word] = []
    let text = top.string
    var start = text.startIndex
    for part in text.split(separator: " ", omittingEmptySubsequences: true) {
      let range = text.range(of: String(part), range: start..<text.endIndex)!
      if let wb = try? top.boundingBox(for: range) { words.append(Word(text: String(part), box: box(wb.boundingBox))) }
      start = range.upperBound
    }
    lines.append(Line(text: text, conf: Double(top.confidence), box: box(o.boundingBox), words: words))
  }
}
request.recognitionLevel = .accurate
request.usesLanguageCorrection = !raw
request.recognitionLanguages = languages
do { try VNImageRequestHandler(cgImage: cg, options: [:]).perform([request]) } catch {
  FileHandle.standardError.write("recognition failed: \(error)\n".data(using: .utf8)!)
  exit(1)
}
let out = Output(width: cg.width, height: cg.height, lines: lines)
let enc = JSONEncoder()
FileHandle.standardOutput.write(try! enc.encode(out))
