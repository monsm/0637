import Photos
import SwiftUI

/// The Director photo, before it leaves the phone: one look at the framed
/// shot, then Save to Photos or Share. The PNG is already in Documents; Save
/// copies it into the photo library, Share hands the file to the system.
struct PhotoPreviewSheet: View {
    let photo: DuckController.CapturedPhoto
    @Environment(\.dismiss) private var dismiss
    @State private var shareURL: URL?
    @State private var saved = false
    @State private var saveFailed = false

    var body: some View {
        VStack(spacing: 16) {
            HStack {
                Text(Copy.photoTitle)
                    .font(.system(size: 17, weight: .semibold, design: .rounded))
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("photo-close")
            }
            Image(uiImage: photo.image)
                .resizable()
                .scaledToFit()
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .accessibilityIdentifier("photo-preview")
            HStack(spacing: 12) {
                Button {
                    saveToPhotos()
                } label: {
                    Label(
                        saved ? Copy.saved : (saveFailed ? Copy.saveFailed : Copy.savePhoto),
                        systemImage: saved ? "checkmark.circle.fill" : "square.and.arrow.down"
                    )
                    .font(.system(size: 14, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .disabled(saved)
                .accessibilityIdentifier("photo-save")

                Button {
                    shareURL = photo.url
                    if shareURL == nil {
                        // No file (a write failure): share the image itself.
                        // Save Image is still offered by the system sheet.
                        shareURL = FileManager.default.temporaryDirectory
                            .appendingPathComponent("MicroDuck-photo.png")
                        try? photo.image.pngData()?.write(to: shareURL!, options: .atomic)
                    }
                } label: {
                    Label(Copy.share, systemImage: "square.and.arrow.up")
                        .font(.system(size: 14, weight: .semibold, design: .rounded))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("photo-share")
            }
        }
        .padding(20)
        .preferredColorScheme(.dark)
        .sheet(
            isPresented: Binding(
                get: { shareURL != nil },
                set: { if !$0 { shareURL = nil } }
            )
        ) {
            if let shareURL {
                ShareSheet(items: [shareURL])
            }
        }
    }

    /// The photo library, add-only. A denial is a real answer: the button
    /// says so rather than claiming a save that did not happen.
    private func saveToPhotos() {
        PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
            guard status == .authorized || status == .limited else {
                DispatchQueue.main.async { saveFailed = true }
                return
            }
            PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAsset(from: photo.image)
            } completionHandler: { success, _ in
                DispatchQueue.main.async {
                    saved = success
                    saveFailed = !success
                }
            }
        }
    }
}
