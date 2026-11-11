import SwiftUI
import UIKit

/// The system share sheet, for a file the app already wrote. SwiftUI's
/// ShareLink would rebuild the file on every render; this presents exactly the
/// URL the button produced, once.
struct ShareSheet: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
