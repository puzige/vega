// Render the canonical SVG using macOS AppKit, preserving transparent margins.
import AppKit

func render() throws {
    guard CommandLine.arguments.count == 3 else {
        throw NSError(domain: "VegaIcon", code: 1, userInfo: [NSLocalizedDescriptionKey:
            "usage: swift render-icon.swift <source.svg> <output.png>"])
    }
    let source = URL(fileURLWithPath: CommandLine.arguments[1])
    guard let image = NSImage(contentsOf: source),
          let bitmap = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: 1024, pixelsHigh: 1024,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
            isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
          let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
        throw NSError(domain: "VegaIcon", code: 2, userInfo: [NSLocalizedDescriptionKey:
            "AppKit could not load the SVG or create an RGBA bitmap"])
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    context.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: 1024, height: 1024),
               from: .zero, operation: .copy, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    // Fail closed if a system SVG renderer ever flattens or drops the artwork.
    guard bitmap.colorAt(x: 0, y: 0)?.alphaComponent == 0,
          bitmap.colorAt(x: 512, y: 512)?.alphaComponent == 1,
          let png = bitmap.representation(using: .png, properties: [:]) else {
        throw NSError(domain: "VegaIcon", code: 3, userInfo: [NSLocalizedDescriptionKey:
            "SVG rasterization did not preserve the transparent canvas and opaque tile"])
    }
    try png.write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
}

do {
    try render()
} catch {
    FileHandle.standardError.write(Data("icon render error: \(error.localizedDescription)\n".utf8))
    exit(1)
}
