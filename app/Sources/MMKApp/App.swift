import AppKit
import SwiftUI

@main
struct MMKApp: App {
    init() {
        // Running as a plain executable (swift run) rather than a bundle: become a regular,
        // focused app so the window appears in front and receives keyboard input.
        NSApplication.shared.setActivationPolicy(.regular)
        DispatchQueue.main.async { NSApplication.shared.activate(ignoringOtherApps: true) }
    }

    var body: some Scene {
        WindowGroup("Engine") {
            GameView()
                .frame(minWidth: 768, minHeight: 432)
        }
    }
}

struct GameView: View {
    @State private var error: String?

    var body: some View {
        ZStack {
            MetalView(error: $error)
            if let error {
                Text(error)
                    .font(.system(.body, design: .monospaced))
                    .padding()
                    .background(.black.opacity(0.75))
                    .foregroundStyle(.white)
            }
        }
    }
}
