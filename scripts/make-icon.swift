// Draws the app's placeholder icon (original art: plain shapes, no text) as a 1024×1024 PNG.
//   swift scripts/make-icon.swift out.png [--ios]
// Used by scripts/make-app.sh, which scales it into an .icns. --ios draws the same art full-bleed
// and opaque (iOS rounds the corners itself), for scripts/make-ios-app.sh.
import CoreGraphics
import Foundation
import ImageIO

let size = 1024
let args = CommandLine.arguments.dropFirst()
let ios = args.contains("--ios")
let out = args.first { !$0.hasPrefix("--") } ?? "icon_1024.png"
let space = CGColorSpace(name: CGColorSpace.sRGB)!
guard let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                          space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
else { fatalError("no context") }

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat(hex >> 16 & 0xFF) / 255, green: CGFloat(hex >> 8 & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255, alpha: a)
}

// macOS icon grid: 824-px rounded square centred in the 1024 canvas, with a soft shadow. iOS:
// the square scaled to fill the canvas, no corners or shadow.
let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
let shape = ios ? CGPath(rect: tile, transform: nil) : CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)
if ios {
    let k = CGFloat(size) / tile.width
    ctx.scaleBy(x: k, y: k)
    ctx.translateBy(x: -tile.minX, y: -tile.minY)
}
ctx.saveGState()
if !ios { ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: rgb(0x000000, 0.35)) }
ctx.addPath(shape)
ctx.setFillColor(rgb(0x2A7DE1))
ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addPath(shape)
ctx.clip()
// sky gradient
let sky = CGGradient(colorsSpace: space, colors: [rgb(0x7FD0FF), rgb(0x2A6FDB)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(sky, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])
// ground band
ctx.setFillColor(rgb(0x3E9B4F))
ctx.fill(CGRect(x: 100, y: 100, width: 824, height: 150))
ctx.setFillColor(rgb(0x2F7A3D))
ctx.fill(CGRect(x: 100, y: 236, width: 824, height: 14))

// a staircase of bevelled blocks
func block(_ x: CGFloat, _ y: CGFloat, _ s: CGFloat) {
    let r = CGRect(x: x, y: y, width: s, height: s)
    ctx.setFillColor(rgb(0x9C4A1A))
    ctx.fill(r)
    ctx.setFillColor(rgb(0xF29A3A))
    ctx.fill(r.insetBy(dx: s * 0.07, dy: s * 0.07))
    ctx.setFillColor(rgb(0xFFC77A))
    ctx.fill(CGRect(x: x + s * 0.07, y: y + s * 0.78, width: s * 0.86, height: s * 0.15))
    ctx.setFillColor(rgb(0xD0732A))
    ctx.fill(CGRect(x: x + s * 0.07, y: y + s * 0.07, width: s * 0.86, height: s * 0.12))
}
let s: CGFloat = 150
for col in 0..<3 {
    for row in 0...col {
        block(190 + CGFloat(col) * s, 250 + CGFloat(row) * s, s)
    }
}

// play symbol, top right
let play = CGMutablePath()
play.move(to: CGPoint(x: 690, y: 840))
play.addLine(to: CGPoint(x: 690, y: 620))
play.addLine(to: CGPoint(x: 868, y: 730))
play.closeSubpath()
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -6), blur: 12, color: rgb(0x0A2A66, 0.45))
ctx.beginTransparencyLayer(auxiliaryInfo: nil) // one shadow for fill + stroke
ctx.setLineJoin(.round)
ctx.setLineWidth(46)
ctx.setStrokeColor(rgb(0xFFFFFF))
ctx.setFillColor(rgb(0xFFFFFF))
ctx.addPath(play)
ctx.drawPath(using: .fillStroke)
ctx.endTransparencyLayer()
ctx.restoreGState()
ctx.restoreGState()

guard let img = ctx.makeImage(),
      let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL, "public.png" as CFString, 1, nil)
else { fatalError("cannot write \(out)") }
CGImageDestinationAddImage(dest, img, nil)
guard CGImageDestinationFinalize(dest) else { fatalError("cannot write \(out)") }
