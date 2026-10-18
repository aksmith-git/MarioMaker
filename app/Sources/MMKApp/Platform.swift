import MetalKit
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// The few AppKit / UIKit differences the shared code needs, in one place. Everything else that
/// differs per platform is behind `#if os(macOS)` / `#if os(iOS)` where it is used.

#if os(macOS)
typealias PlatformView = NSView

/// Hosts an MTKView in SwiftUI on both platforms: conformers write `makeView` / `updateView`
/// once instead of the NSView… and UIView… variants.
protocol MetalViewRepresentable: NSViewRepresentable where NSViewType == MTKView {
    func makeView(context: Context) -> MTKView
    func updateView(_ view: MTKView, context: Context)
}

extension MetalViewRepresentable {
    func makeNSView(context: Context) -> MTKView { makeView(context: context) }
    func updateNSView(_ nsView: MTKView, context: Context) { updateView(nsView, context: context) }
}
#else
typealias PlatformView = UIView

protocol MetalViewRepresentable: UIViewRepresentable where UIViewType == MTKView {
    func makeView(context: Context) -> MTKView
    func updateView(_ view: MTKView, context: Context)
}

extension MetalViewRepresentable {
    func makeUIView(context: Context) -> MTKView { makeView(context: context) }
    func updateUIView(_ uiView: MTKView, context: Context) { updateView(uiView, context: context) }
}
#endif

enum PlatformApp {
    /// Posted when the app stops being the active one (another app in front, Home on iOS).
    static var resignActive: Notification.Name {
        #if os(macOS)
        NSApplication.didResignActiveNotification
        #else
        UIApplication.willResignActiveNotification
        #endif
    }
}

extension View {
    /// The main window's minimum size on macOS; iOS screens fill the display.
    func minimumWindowSize() -> some View {
        #if os(macOS)
        frame(minWidth: 768, minHeight: 432)
        #else
        self
        #endif
    }
}
