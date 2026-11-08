// Draws Safelight's icon, the same shapes as docs/brand/logo.svg, as a PNG
// with no transparency, for the app icon and the README.
// Usage: swift scripts/render-icon.swift <out.png> [size]
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let out = CommandLine.arguments[1]
let size = CommandLine.arguments.count > 2 ? Int(CommandLine.arguments[2])! : 1024
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                    space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
// The SVG's coordinates: 1024 units, y down.
let scale = CGFloat(size) / 1024
ctx.translateBy(x: 0, y: CGFloat(size))
ctx.scaleBy(x: scale, y: -scale)

func color(_ hex: UInt32) -> CGColor {
    CGColor(colorSpace: space, components: [CGFloat(hex >> 16 & 255) / 255, CGFloat(hex >> 8 & 255) / 255, CGFloat(hex & 255) / 255, 1])!
}
func fill(_ path: CGPath, _ hex: UInt32) { ctx.addPath(path); ctx.setFillColor(color(hex)); ctx.fillPath() }
func stroke(_ path: CGPath, _ hex: UInt32, _ width: CGFloat) {
    ctx.addPath(path); ctx.setStrokeColor(color(hex)); ctx.setLineWidth(width); ctx.setLineCap(.round); ctx.strokePath()
}
func rounded(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ h: CGFloat, _ r: CGFloat) -> CGPath {
    CGPath(roundedRect: CGRect(x: x, y: y, width: w, height: h), cornerWidth: r, cornerHeight: r, transform: nil)
}
func curve(_ x0: CGFloat, _ y0: CGFloat, _ cx: CGFloat, _ cy: CGFloat, _ x1: CGFloat, _ y1: CGFloat) -> CGPath {
    let p = CGMutablePath(); p.move(to: CGPoint(x: x0, y: y0)); p.addQuadCurve(to: CGPoint(x: x1, y: y1), control: CGPoint(x: cx, y: cy)); return p
}

fill(CGPath(rect: CGRect(x: 0, y: 0, width: 1024, height: 1024), transform: nil), 0x1d1b1b)
// The lamp, centred: it spans y 140 to 652 before the move.
ctx.translateBy(x: 0, y: 116)
// The cord, up to the top edge.
fill(rounded(500, -200, 24, 500, 12), 0x4a4340)
// The shade: the upper half of a circle about (512, 540).
let shade = CGMutablePath()
shade.addArc(center: CGPoint(x: 512, y: 540), radius: 250, startAngle: .pi, endAngle: 2 * .pi, clockwise: false)
shade.closeSubpath()
fill(shade, 0xd6402f)
// A highlight on the shade, upper left: angles measured as the SVG's arc runs.
let highlight = CGMutablePath()
highlight.addArc(center: CGPoint(x: 512, y: 540), radius: 182,
                 startAngle: atan2(482 - 540, 352 - 512) + 2 * .pi, endAngle: atan2(386 - 540, 432 - 512) + 2 * .pi, clockwise: false)
stroke(highlight, 0xe8634c, 24)
// The bulb under the rim: the lower half of a circle about (512, 556).
let bulb = CGMutablePath()
bulb.addArc(center: CGPoint(x: 512, y: 556), radius: 96, startAngle: 0, endAngle: .pi, clockwise: false)
bulb.closeSubpath()
fill(bulb, 0xf6c2b2)
fill(rounded(244, 528, 536, 30, 15), 0x4a4340)


let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, ctx.makeImage()!, nil)
CGImageDestinationFinalize(dest)
print("wrote \(out) at \(size) px")
