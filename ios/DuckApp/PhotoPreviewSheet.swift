import Photos
import SwiftUI

/// The Director photo, before it leaves the phone: one look at the framed
/// shot, then Save to Photos or Share. Save and Share hand the system the
/// composed postcard (frame + caption band + date), never the bare frame.
/// Keep asks the shelf to store the same composed card beside the untouched
/// frame. The raw PNG is a cache file; dismissing deletes it unless kept.
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
                    // 货架里存成片（字幕 + 日期），原帧由货架另存一份并在元数据里区分。
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
                    sharePostcard()
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
            // A preview is not a keepsake: an un-kept shot's raw PNG (a cache
            // file) goes with it. Keep has already copied the composed card
            // into Postcards; Share hands over its own temporary composed PNG.
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
                falls: events.filter(LifeEvent.isFall).count,
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

    /// 分享成片：字幕、日期、品牌标都烙进 PNG，离开 App 也还在。
    private func sharePostcard() {
        let postcard = PostcardCompose.make(from: photo.image, caption: caption, date: Date())
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("MicroDuck-postcard.png")
        if let data = postcard.pngData(),
           (try? data.write(to: url, options: .atomic)) != nil
        {
            shareURL = url
            return
        }
        // 成片写不出来时退回原帧（旧行为）：宁可分享一张裸图，也别让分享按钮失声。
        shareURL = photo.url
        if shareURL == nil {
            let fallback = FileManager.default.temporaryDirectory
                .appendingPathComponent("MicroDuck-photo.png")
            if let data = photo.image.pngData(),
               (try? data.write(to: fallback, options: .atomic)) != nil
            {
                shareURL = fallback
            }
        }
    }

    /// The photo library, add-only. A denial is a real answer: the button
    /// says so rather than claiming a save that did not happen.
    private func saveToPhotos() {
        // 相册里放成片：原帧 + 字幕带 + 日期，而不是一张裸渲染帧。
        let postcard = PostcardCompose.make(from: photo.image, caption: caption, date: Date())
        PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
            guard status == .authorized || status == .limited else {
                DispatchQueue.main.async { saveFailed = true }
                return
            }
            PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAsset(from: postcard)
            } completionHandler: { success, _ in
                DispatchQueue.main.async {
                    saved = success
                    saveFailed = !success
                }
            }
        }
    }
}
