import Photos
import SwiftUI

/// The Director photo, before it leaves the phone: one look at the framed
/// shot, then Save to Photos or Share. The PNG is a cache file; Save copies
/// it into the photo library, Keep into Documents/Postcards, Share hands the
/// file to the system, and dismissing deletes it.
struct PhotoPreviewSheet: View {
    let photo: DuckController.CapturedPhoto
    @ObservedObject var shelf: PostcardShelf
    @Environment(\.dismiss) private var dismiss
    @State private var shareURL: URL?
    @State private var saved = false
    @State private var saveFailed = false
    @State private var kept = false
    /// One honest line from the deltas the duck actually logged.
    @State private var caption = ""

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
            if !caption.isEmpty {
                Text(caption)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.white.opacity(0.75))
                    .lineLimit(2)
                    .accessibilityIdentifier("postcard-caption")
            }
            HStack(spacing: 12) {
                Button {
                    kept = shelf.keep(image: photo.image, caption: caption)
                } label: {
                    Label(
                        kept ? Copy.postcardSaved : Copy.postcardKeep,
                        systemImage: kept ? "checkmark.circle.fill" : "square.on.square"
                    )
                    .font(.system(size: 14, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .disabled(kept)
                .accessibilityIdentifier("photo-keep")

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
        .onDisappear {
            // A preview is not a keepsake: an un-kept shot's PNG (a cache
            // file) goes with it. Keep copied it into Postcards already;
            // Share has handed the file to the system by now.
            if !kept, let url = photo.url {
                try? FileManager.default.removeItem(at: url)
            }
        }
        .task {
            // The away recap's own counters, read once for this caption.
            guard let url = URL(string: "http://127.0.0.1:8765/state"),
                  let data = try? Data(contentsOf: url),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else {
                caption = Copy.postcardQuiet
                return
            }
            let events = (json["life"] as? [String: Any])?["events"] as? [String] ?? []
            let metres = (json["life"] as? [String: Any])?["metres"] as? Double ?? 0
            caption = postcardCaption(
                rituals: events.filter { $0 == "idle_ritual" }.count,
                falls: events.filter { $0.hasPrefix("fall") }.count,
                feeds: events.filter { $0 == "feed" }.count,
                pets: events.filter { $0 == "pet" }.count,
                moods: events.filter { $0.hasPrefix("emotion:") }.count,
                metres: metres
            )
        }
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

    /// One deterministic line from the duck's real event deltas. Empty
    /// becomes a quiet line, never an invented one.
    private func postcardCaption(
        rituals: Int, falls: Int, feeds: Int, pets: Int, moods: Int, metres: Double
    ) -> String {
        var parts: [String] = []
        if pets > 0 { parts.append(Copy.awayPets(pets)) }
        if feeds > 0 { parts.append(Copy.awayFeeds(feeds)) }
        if rituals > 0 { parts.append(Copy.awayRituals(rituals)) }
        if falls > 0 { parts.append(Copy.awayFalls(falls)) }
        if moods > 0 { parts.append(Copy.awayMoods(moods)) }
        if metres >= 0.5 { parts.append(Copy.postcardMetres(metres)) }
        return parts.isEmpty ? Copy.postcardQuiet : parts.joined(separator: " · ")
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
