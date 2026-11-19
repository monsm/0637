import Photos
import SwiftUI

/// One highlight tile's words, and one row of them, Identifiable so the
/// ForEaches stay plain (tuple key paths do not resolve as row ids).
private struct VetbookCell: Identifiable {
    var id: String { label }
    let value: String
    let label: String
}

private struct VetbookRow: Identifiable {
    let id: Int
    let cells: [VetbookCell]
}

/// The vet book as one shareable card: the duck's real numbers, drawn on
/// this phone (boss ruling 2026-11-17 — the card is generated locally and
/// never posted anywhere; Share hands the PNG to the system sheet).
///
/// Data comes from the core's own `GET /export/vetbook.json` (core
/// `export.rs`): `{ format, build.fingerprint, care.summary(), life,
/// highlights{…} }`. The mapping is pure and unit-tested
/// (DuckTests/VetbookCardTests.swift); the view only renders what the
/// parser kept.

/// The card's numbers, parsed from the core's document. A field the
/// document does not carry stays nil — an absent fact must not become a 0
/// on the card (the zero-fabrication rule REPORT.md lives by).
struct VetbookCardModel: Equatable {
    var metres: Double?
    var rolls: Int?
    var pickups: Int?
    var falls: Int?
    var pets: Int?
    var fed: Int?
    var claps: Int?
    var whistles: Int?
    /// `life.powered_s`, in hours.
    var poweredHours: Double?
    /// `care.summary()`'s three numbers as the percentages the instrument
    /// sheet and the core's own Markdown page show.
    var fullness: Int?
    var mood: Int?
    var bond: Int?
    /// The build's provenance hash, cut to the first eight digits.
    var fingerprint: String?

    /// A life with nothing in it — every counter at zero and no walk —
    /// shows the honest "no records yet" line instead of a grid of zeroes
    /// that would claim things happened. Half a centimetre of drift is not
    /// a walk either.
    var isEmpty: Bool {
        let counters = [rolls, pickups, falls, pets, fed, claps, whistles]
        let anyCount = counters.contains { ($0 ?? 0) > 0 }
        return !anyCount && (metres ?? 0) < 0.05
    }
}

/// The one place the wire document becomes the card's model. Pure: the
/// same JSON gives the same card, and nothing here touches a view or a
/// controller, so the tests can pin the mapping without a core running.
enum VetbookCardParser {
    static func parse(_ json: [String: Any]) -> VetbookCardModel {
        var model = VetbookCardModel()
        if let highlights = json["highlights"] as? [String: Any] {
            model.metres = highlights["metres"] as? Double
            model.rolls = highlights["rolls"] as? Int
            model.pickups = highlights["pickups"] as? Int
            model.falls = highlights["falls"] as? Int
            model.pets = highlights["pets"] as? Int
            model.fed = highlights["fed"] as? Int
            model.claps = highlights["claps"] as? Int
            model.whistles = highlights["whistles"] as? Int
        }
        if let life = json["life"] as? [String: Any],
           let powered = life["powered_s"] as? Double {
            model.poweredHours = powered / 3600.0
        }
        if let care = json["care"] as? [String: Any] {
            // The same three percentages the core's own Markdown page
            // derives: fullness not hunger, mood rescaled from −1…1, bond
            // as-is. Clamped, so a bad reading cannot print 140%.
            if let hunger = care["hunger"] as? Double {
                model.fullness = Self.percent((1.0 - hunger) * 100.0)
            }
            if let mood = care["mood"] as? Double {
                model.mood = Self.percent(((mood + 1.0) / 2.0) * 100.0)
            }
            if let bond = care["bond"] as? Double {
                model.bond = Self.percent(bond * 100.0)
            }
        }
        if let build = json["build"] as? [String: Any] {
            let raw = build["fingerprint"] as? String ?? ""
            if !raw.isEmpty {
                model.fingerprint = String(raw.prefix(8))
            }
        }
        return model
    }

    private static func percent(_ value: Double) -> Int {
        Int(min(max(value, 0), 100).rounded())
    }
}

/// The card itself: the duck's name, the eight highlights, the care
/// percentages, the hours together, and the build's first eight digits —
/// in the shelf's own visual language (dark surface, rounded figures,
/// warm white on muted labels, the theme's one accent left alone). The
/// same view is shown in the sheet and rendered into the shared PNG, so
/// what a person sees is exactly what leaves the phone.
struct VetbookCardContent: View {
    let model: VetbookCardModel
    /// The design width; the sheet shows it at the screen's width, the PNG
    /// renders at this full width, scaled 3×.
    var width: CGFloat = 390

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack(alignment: .firstTextBaseline) {
                Text(Copy.name)
                    .font(.duckDisplay.weight(.semibold))
                    .foregroundStyle(Color.duckTextPrimary)
                Spacer()
                Text(Copy.lifeRecord)
                    .font(.duckLabel)
                    .foregroundStyle(Color.duckTextSecondary)
            }
            if model.isEmpty {
                Text(Copy.vetbookCardNone)
                    .font(.duckLabel)
                    .foregroundStyle(Color.duckTextSecondary)
                    .frame(maxWidth: .infinity, minHeight: 96, alignment: .center)
            } else {
                highlightGrid
                careRow
                if let hours = model.poweredHours {
                    HStack {
                        Text(Copy.vetbookCardPowered)
                            .font(.duckCaption)
                            .foregroundStyle(Color.duckTextSecondary)
                        Spacer()
                        Text(String(format: "%.2f h", hours))
                            .font(.duckData)
                            .foregroundStyle(Color.duckTextPrimary)
                    }
                    .padding(.vertical, DuckSpace.sm)
                }
            }
            if let fingerprint = model.fingerprint {
                Text("build \(fingerprint)")
                    .font(.duckMicro)
                    .foregroundStyle(Color.duckTextTertiary)
            }
        }
        .padding(DuckSpace.xl)
        .frame(width: width, alignment: .leading)
        .background(Color.duckBgSurface, in: RoundedRectangle(cornerRadius: DuckRadius.panel))
    }

    /// The eight highlights, in the life record's own order, four to a
    /// row. Not a LazyVGrid on purpose: the shared PNG is rendered from
    /// this view with ImageRenderer, which draws no lazy containers — the
    /// rows here are plain HStacks of fixed-width tiles, so the preview
    /// and the PNG cannot disagree.
    private var highlightGrid: some View {
        VStack(spacing: DuckSpace.sm) {
            ForEach(highlightRows) { row in
                HStack(spacing: DuckSpace.sm) {
                    ForEach(row.cells) { item in
                        cell(item.value, item.label, width: highlightCellWidth)
                    }
                }
            }
        }
    }

    private var highlightCellWidth: CGFloat {
        // The card's own 20 pt padding on each side and three 10 pt gaps
        // between four tiles.
        (width - 70) / 4
    }

    /// A tile only exists for a number the document actually carried; a
    /// real zero is a fact and shows as 0.
    private var highlightRows: [VetbookRow] {
        var cells: [VetbookCell] = []
        if let metres = model.metres {
            cells.append(VetbookCell(value: String(format: "%.1f m", metres), label: Copy.walked))
        }
        if let rolls = model.rolls { cells.append(VetbookCell(value: "\(rolls)", label: Copy.rolls)) }
        if let pickups = model.pickups { cells.append(VetbookCell(value: "\(pickups)", label: Copy.pickups)) }
        if let falls = model.falls { cells.append(VetbookCell(value: "\(falls)", label: Copy.vetbookFalls)) }
        if let pets = model.pets { cells.append(VetbookCell(value: "\(pets)", label: Copy.vetbookPets)) }
        if let fed = model.fed { cells.append(VetbookCell(value: "\(fed)", label: Copy.vetbookFed)) }
        if let claps = model.claps { cells.append(VetbookCell(value: "\(claps)", label: Copy.vetbookClaps)) }
        if let whistles = model.whistles { cells.append(VetbookCell(value: "\(whistles)", label: Copy.vetbookWhistles)) }
        var rows: [VetbookRow] = []
        var index = 0
        while index < cells.count {
            let end = min(index + 4, cells.count)
            rows.append(VetbookRow(id: rows.count, cells: Array(cells[index..<end])))
            index = end
        }
        return rows
    }

    @ViewBuilder private var careRow: some View {
        if model.fullness != nil || model.mood != nil || model.bond != nil {
            HStack(spacing: DuckSpace.sm) {
                if let fullness = model.fullness {
                    cell("\(fullness)%", Copy.fullness, width: (width - 60) / 3)
                }
                if let mood = model.mood {
                    cell("\(mood)%", Copy.mood, width: (width - 60) / 3)
                }
                if let bond = model.bond {
                    cell("\(bond)%", Copy.bond, width: (width - 60) / 3)
                }
            }
        }
    }

    /// The lifeStat tile from the instrument sheet, at card scale.
    private func cell(_ value: String, _ label: String, width: CGFloat) -> some View {
        VStack(spacing: DuckSpace.xxs) {
            Text(value)
                .font(.duckTitle.weight(.semibold))
                .foregroundStyle(Color.duckTextPrimary)
            Text(label)
                .font(.duckCaption)
                .foregroundStyle(Color.duckTextSecondary)
        }
        .frame(width: width)
        .padding(.vertical, DuckSpace.sm)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: DuckRadius.thumb))
    }
}

/// The card, before it leaves the phone: one fetch of the core's own
/// export route, the rendered card, and two actions — Share and Save —
/// both handing over the very PNG the preview shows. Share reuses the
/// shell's `ShareSheet`; Save reuses the photo preview's add-only photo
/// library path. No new mechanism: the fetch is the same local
/// `Data(contentsOf:)` read `vetbookFile()` and the photo preview use.
struct VetbookCardSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var model: VetbookCardModel?
    /// The core is not answering — a real state, said in words.
    @State private var failed = false
    @State private var shareURL: URL?
    @State private var saved = false
    @State private var saveFailed = false

    var body: some View {
        VStack(spacing: DuckSpace.lg) {
            HStack {
                Text(Copy.vetbookCardShare)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("vetbook-card-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(DuckIcon.xs)
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("vetbook-card-close")
            }
            if let model {
                GeometryReader { geometry in
                    ScrollView {
                        VetbookCardContent(
                            model: model,
                            width: min(390, max(280, geometry.size.width))
                        )
                    }
                }
                HStack(spacing: DuckSpace.md) {
                    Button {
                        saveCard()
                    } label: {
                        Label(
                            saved ? Copy.saved : (saveFailed ? Copy.saveFailed : Copy.savePhoto),
                            systemImage: saved ? "checkmark.circle.fill" : "square.and.arrow.down"
                        )
                        .font(.duckLabel.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.bordered)
                    .disabled(saved)
                    .accessibilityIdentifier("vetbook-card-save")

                    Button {
                        shareCard()
                    } label: {
                        Label(Copy.share, systemImage: "square.and.arrow.up")
                            .font(.duckLabel.weight(.semibold))
                            .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("vetbook-card-share")
                }
            } else if failed {
                Text(Copy.rulesNotReady)
                    .font(.duckLabel)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 96, alignment: .center)
            } else {
                Text("…")
                    .font(.duckLabel)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 96, alignment: .center)
            }
        }
        .padding(DuckSpace.xl)
        .task {
            load()
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

    /// The core's own document, read the way `vetbookFile()` and the photo
    /// preview read their routes: one synchronous local GET.
    private func load() {
        guard let url = URL(string: "http://127.0.0.1:8765/export/vetbook.json"),
              let data = try? Data(contentsOf: url),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            failed = true
            return
        }
        model = VetbookCardParser.parse(json)
    }

    /// The card as a 3× PNG at the design width — the same view the sheet
    /// previews, so the shared image is the seen image. ImageRenderer is
    /// main-actor isolated, so the render and both callers are too.
    @MainActor private func renderCard() -> UIImage? {
        guard let model else { return nil }
        let renderer = ImageRenderer(content: VetbookCardContent(model: model, width: 390))
        renderer.scale = 3
        return renderer.uiImage
    }

    @MainActor private func shareCard() {
        guard let image = renderCard() else { return }
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("MicroDuck-vetbook-card.png")
        guard let data = image.pngData(),
              (try? data.write(to: url, options: .atomic)) != nil else { return }
        shareURL = url
    }

    /// The photo preview's own save path: add-only permission, then the
    /// asset request. A denial says so rather than claiming a save.
    @MainActor private func saveCard() {
        guard let image = renderCard() else { return }
        PHPhotoLibrary.requestAuthorization(for: .addOnly) { status in
            guard status == .authorized || status == .limited else {
                DispatchQueue.main.async { saveFailed = true }
                return
            }
            PHPhotoLibrary.shared().performChanges {
                PHAssetChangeRequest.creationRequestForAsset(from: image)
            } completionHandler: { success, _ in
                DispatchQueue.main.async {
                    saved = success
                    saveFailed = !success
                }
            }
        }
    }
}
