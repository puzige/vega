// Capture-only native driver: activates Vega, reads its layer-0 window bounds
// live, and screenshots that exact rect WITHOUT synthesizing any input.
//
// The R69 acceptance needs this: the whole point of the change is that the
// home composer is usable with no click, so the evidence must not be produced
// by a driver that clicks first.
//
// usage:
//   swiftc -O scripts/native-capture.swift -o /tmp/vcapture
//   /tmp/vcapture <out.png>
import AppKit
import CoreGraphics
import Foundation

func fail(_ msg: String) -> Never {
    FileHandle.standardError.write("native-capture: \(msg)\n".data(using: .utf8)!)
    exit(1)
}

let args = CommandLine.arguments
guard args.count == 2 else { fail("usage: native-capture <out.png>") }
let outPath = args[1]

func frontmostIsVega() -> Bool {
    NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "ai.vega"
}
if !frontmostIsVega() {
    let open = Process()
    open.launchPath = "/usr/bin/open"
    open.arguments = ["-a", "/Applications/Vega.app"]
    try? open.run()
    open.waitUntilExit()
}
var frontmost = false
for _ in 0..<60 {
    if frontmostIsVega() { frontmost = true; break }
    usleep(150_000)
}
guard frontmost else {
    let f = NSWorkspace.shared.frontmostApplication
    fail("Vega did not become frontmost within 9s (frontmost=\(f?.localizedName ?? "?") [\(f?.bundleIdentifier ?? "?")])")
}
usleep(900_000)

guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
    fail("cannot list windows")
}
var origin = CGPoint.zero
var size = CGSize.zero
for w in list {
    guard let owner = w[kCGWindowOwnerName as String] as? String,
          owner.lowercased().contains("vega"),
          (w[kCGWindowLayer as String] as? Int) == 0,
          let b = w[kCGWindowBounds as String] as? [String: Any],
          let x = b["X"] as? Double, let y = b["Y"] as? Double,
          let wd = b["Width"] as? Double, let ht = b["Height"] as? Double
    else { continue }
    origin = CGPoint(x: x, y: y)
    size = CGSize(width: wd, height: ht)
    break
}
guard size.width > 0 else { fail("no layer-0 Vega window found") }
print("window origin=(\(Int(origin.x)),\(Int(origin.y))) size=\(Int(size.width))x\(Int(size.height))")
print("NO INPUT SENT: this capture is the pre-interaction state")

let task = Process()
task.launchPath = "/usr/sbin/screencapture"
task.arguments = [
    "-x",
    "-R", "\(Int(origin.x)),\(Int(origin.y)),\(Int(size.width)),\(Int(size.height))",
    outPath,
]
try? task.run()
task.waitUntilExit()
guard task.terminationStatus == 0 else { fail("screencapture failed") }
print("captured \(outPath) rect=\(Int(size.width))x\(Int(size.height)) (2x -> \(Int(size.width) * 2)x\(Int(size.height) * 2))")
