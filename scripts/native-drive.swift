// Native acceptance driver for the Vega app.
//
// Why this exists: shell round-trips between "activate" and "click" let the
// terminal window steal focus back, so clicks land on the wrong app. This does
// activate -> wait-until-frontmost -> click -> capture in ONE process, and
// reads the window origin live so window moves cannot silently invalidate
// hard-coded screen coordinates. See the "原生验收的操作方法" section in
// AGENTS.md for the four other traps this avoids.
//
// usage:
//   swiftc -O scripts/native-drive.swift -o /tmp/vdrive
//   /tmp/vdrive <out.png> <relX> <relY> [<relX> <relY> ...]
//
//   rel* are window-relative LOGICAL points (the same units as GPUI layout).
//   Each point is clicked in order with a short settle between clicks, then
//   the window is captured with an explicit -R rect, so the PNG is exactly
//   2x the window size with no shadow padding: pixel = 2 * window-relative.
import AppKit
import CoreGraphics
import Foundation

func fail(_ msg: String) -> Never {
    FileHandle.standardError.write("native-drive: \(msg)\n".data(using: .utf8)!)
    exit(1)
}

let args = CommandLine.arguments
guard args.count >= 4, (args.count - 2) % 2 == 0 else {
    fail("usage: native-drive <out.png> <relX> <relY> [<relX> <relY> ...]")
}
let outPath = args[1]
var points: [(Double, Double)] = []
var i = 2
while i + 1 < args.count {
    guard let x = Double(args[i]), let y = Double(args[i + 1]) else { fail("bad coords") }
    points.append((x, y))
    i += 2
}

// --- 1. find the running Vega instance ------------------------------------
let apps = NSRunningApplication.runningApplications(withBundleIdentifier: "ai.vega")
guard let vega = apps.first else { fail("Vega (ai.vega) is not running") }
_ = vega

// --- 2. activate and wait until it really is frontmost --------------------
// NSRunningApplication.activate() is refused here (the caller lacks the
// privilege to raise another app), so shell out to `open -a`, which the
// desktop session honours, then poll the workspace until it takes effect.
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
usleep(700_000)

// --- 3. read its layer-0 window bounds ------------------------------------
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

// --- 4. click each point --------------------------------------------------
let src = CGEventSource(stateID: .hidSystemState)
for (rx, ry) in points {
    let pt = CGPoint(x: origin.x + rx, y: origin.y + ry)
    guard let move = CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: pt, mouseButton: .left),
          let down = CGEvent(mouseEventSource: src, mouseType: .leftMouseDown, mouseCursorPosition: pt, mouseButton: .left),
          let up = CGEvent(mouseEventSource: src, mouseType: .leftMouseUp, mouseCursorPosition: pt, mouseButton: .left)
    else { fail("cannot build mouse events") }
    move.post(tap: .cghidEventTap)
    usleep(150_000)
    down.post(tap: .cghidEventTap)
    usleep(80_000)
    up.post(tap: .cghidEventTap)
    print("clicked window-rel (\(Int(rx)),\(Int(ry))) -> screen (\(Int(pt.x)),\(Int(pt.y)))")
    usleep(900_000)
}

// --- 5. capture that window ----------------------------------------------
// Use an explicit -R rect: a window-id capture silently pads the image with
// the window's drop shadow, and that padding changes with the window's
// shadow state (measured 34 to 56 logical px), which makes pixel->point
// mapping unreliable. A -R capture of the window's own bounds is exactly 2x
// with no padding.
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
