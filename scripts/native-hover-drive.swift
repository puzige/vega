// Move-only native driver: activate Vega, MOVE the pointer to a window-relative
// point (no click — a click on a control would activate it, and for a
// hover-only state there is nothing to click anyway), settle, then capture the
// window with an exact -R rect.
//
// Why a separate driver from native-drive.swift: hover states cannot be
// verified by clicking. `native-drive.swift` clicks; this one only moves.
// See the "验证 hover 类改动不能用点击" section in AGENTS.md.
//
// The pointer is moved in two steps (a point outside the target first, then the
// target) because GPUI repaints on hover *transitions*: a single move straight
// onto the target may not register as an entry.
//
// usage:
//   swiftc -O scripts/native-hover-drive.swift -o /tmp/vmove
//   /tmp/vmove <out.png> <relX> <relY>
//
//   rel* are window-relative LOGICAL points. The PNG is exactly 2x the window
//   size (no shadow padding): pixel = 2 * window-relative.
import AppKit
import CoreGraphics
import Foundation

func fail(_ m: String) -> Never {
    FileHandle.standardError.write("vmove: \(m)\n".data(using: .utf8)!); exit(1)
}
let a = CommandLine.arguments
guard a.count == 4, let rx = Double(a[2]), let ry = Double(a[3]) else { fail("usage: vmove <out.png> <relX> <relY>") }

func frontmostIsVega() -> Bool {
    NSWorkspace.shared.frontmostApplication?.bundleIdentifier == "ai.vega"
}
if !frontmostIsVega() {
    let p = Process(); p.launchPath = "/usr/bin/open"
    p.arguments = ["-a", "/Applications/Vega.app"]; try? p.run(); p.waitUntilExit()
}
var frontmost = false
for _ in 0..<60 { if frontmostIsVega() { frontmost = true; break }; usleep(150_000) }
guard frontmost else { fail("Vega did not become frontmost") }
usleep(700_000)

guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { fail("cannot list windows") }
var origin = CGPoint.zero, size = CGSize.zero
for w in list {
    guard let owner = w[kCGWindowOwnerName as String] as? String, owner.lowercased().contains("vega"),
          (w[kCGWindowLayer as String] as? Int) == 0,
          let b = w[kCGWindowBounds as String] as? [String: Any],
          let x = b["X"] as? Double, let y = b["Y"] as? Double,
          let wd = b["Width"] as? Double, let ht = b["Height"] as? Double else { continue }
    origin = CGPoint(x: x, y: y); size = CGSize(width: wd, height: ht); break
}
guard size.width > 0 else { fail("no layer-0 Vega window") }
print("window origin=(\(Int(origin.x)),\(Int(origin.y))) size=\(Int(size.width))x\(Int(size.height))")

let src = CGEventSource(stateID: .hidSystemState)
// Move in two steps so GPUI sees a real transition into the target.
let start = CGPoint(x: origin.x + 10, y: origin.y + size.height - 10)
let target = CGPoint(x: origin.x + rx, y: origin.y + ry)
if let m0 = CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: start, mouseButton: .left) {
    m0.post(tap: .cghidEventTap); usleep(250_000)
}
if let m1 = CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: target, mouseButton: .left) {
    m1.post(tap: .cghidEventTap)
}
usleep(700_000)
print("moved to window-rel (\(Int(rx)),\(Int(ry))) -> screen (\(Int(target.x)),\(Int(target.y)))")

let task = Process(); task.launchPath = "/usr/sbin/screencapture"
task.arguments = ["-x", "-R", "\(Int(origin.x)),\(Int(origin.y)),\(Int(size.width)),\(Int(size.height))", a[1]]
try? task.run(); task.waitUntilExit()
guard task.terminationStatus == 0 else { fail("screencapture failed") }
print("captured \(a[1])")
