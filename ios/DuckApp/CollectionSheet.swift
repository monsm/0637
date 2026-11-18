import Foundation
import SwiftUI

/// One entry in the collection book: an official emotion or trick, with the
/// core's real lifetime count and the first day the duck really did it.
struct CollectionEntry: Identifiable, Equatable {
    let name: String
    let count: Int
    let firstDay: String

    var id: String { name }

    /// Locked is exactly "never done": the count is the only judge. A known
    /// count with no known day is unlocked, so the two states never blur.
    var isLocked: Bool { count == 0 }

    var countText: String { "\(count)" }

    /// The first-day line: nil when locked (the row shows the locked label
    /// instead), an em dash when the core knows the count but not the day.
    /// Never a guessed date.
    var firstDayText: String? {
        guard !isLocked else { return nil }
        let day = firstDay.trimmingCharacters(in: .whitespacesAndNewlines)
        return day.isEmpty ? "—" : day
    }
}

/// The `GET /collection` document, parsed into display order. Pure, so the
/// tests can pin the em-dash rule, the locked rule and the ordering without
/// a screen or a core.
struct CollectionBook: Equatable {
    let emotions: [CollectionEntry]
    let tricks: [CollectionEntry]
    let moodsUnlocked: Int
    let moodsTotal: Int
    let tricksUnlocked: Int
    let tricksTotal: Int

    static func parse(_ data: Data) -> CollectionBook? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        return parse(json)
    }

    static func parse(_ json: [String: Any]) -> CollectionBook? {
        if let ok = json["ok"] as? Bool, !ok { return nil }
        guard let emotionsRaw = json["emotions"] as? [[String: Any]],
              let skillsRaw = json["skills"] as? [[String: Any]]
        else { return nil }
        let emotions = ordered(emotionsRaw.compactMap(entry))
        let tricks = ordered(skillsRaw.compactMap(entry))
        let moods = json["moods"] as? [String: Any] ?? [:]
        let trickTotals = json["tricks"] as? [String: Any] ?? [:]
        return CollectionBook(
            emotions: emotions,
            tricks: tricks,
            moodsUnlocked: count(moods["unlocked"], fallback: emotions),
            moodsTotal: count(moods["total"], fallback: emotions, total: true),
            tricksUnlocked: count(trickTotals["unlocked"], fallback: tricks),
            tricksTotal: count(trickTotals["total"], fallback: tricks, total: true)
        )
    }

    /// Unlocked first (what the duck has really done), then the bigger
    /// count, then the official name, so ties are deterministic.
    static func ordered(_ entries: [CollectionEntry]) -> [CollectionEntry] {
        entries.sorted { a, b in
            if a.isLocked != b.isLocked { return !a.isLocked }
            if a.count != b.count { return a.count > b.count }
            return a.name < b.name
        }
    }

    private static func entry(_ item: [String: Any]) -> CollectionEntry? {
        guard let rawName = item["name"] as? String else { return nil }
        let name = rawName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return nil }
        let raw = item["count"] as? Int ?? 0
        let day = item["first_day"] as? String ?? ""
        return CollectionEntry(name: name, count: max(0, raw), firstDay: day)
    }

    /// The core's totals are authoritative. Only when the document omits
    /// them does the page fall back to what its own rows actually show.
    private static func count(
        _ value: Any?,
        fallback entries: [CollectionEntry],
        total: Bool = false
    ) -> Int {
        if let number = value as? Int { return max(0, number) }
        return total ? entries.count : entries.filter { !$0.isLocked }.count
    }
}

/// One page's snapshot of the core's vet book. This is view state, not the
/// duck's own state, so it lives beside the page instead of growing the
/// controller's published surface; the fetch still follows the same
/// loopback-first style as the rest of the shell.
final class CollectionStore: ObservableObject {
    @Published private(set) var book: CollectionBook?
    @Published private(set) var busy = false
    @Published private(set) var error = ""

    func load() {
        guard !busy else { return }
        guard let url = URL(string: "http://127.0.0.1:8765/collection") else { return }
        busy = true
        var request = URLRequest(url: url)
        request.timeoutInterval = 2.0
        URLSession.shared.dataTask(with: request) { [weak self] data, response, _ in
            guard let self else { return }
            let parsed = data.flatMap { CollectionBook.parse($0) }
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            DispatchQueue.main.async {
                self.busy = false
                if let parsed {
                    self.book = parsed
                    self.error = ""
                } else if status != 0, !(200..<300).contains(status) {
                    self.error = "HTTP \(status)"
                } else {
                    self.error = "no answer"
                }
            }
        }.resume()
    }
}

/// The collection page (F2): every official emotion and trick in the core's
/// vet book, its real count, and the first day it happened. Locked rows
/// stay visible and can still be practised — the tap is the same path the
/// skill ring uses, so the core keeps the final say on what runs.
struct CollectionSheet: View {
    /// Kept for the practice tap: the skill ring's own trigger.
    @ObservedObject var controller: DuckController
    @StateObject private var store = CollectionStore()

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if let book = store.book {
                        totals(book)
                        section(Copy.collectionMoods, tag: "moods", entries: book.emotions)
                        section(Copy.collectionTricks, tag: "tricks", entries: book.tricks)
                    } else if store.busy {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                            .padding(.top, 24)
                    } else {
                        Button(Copy.retry) {
                            store.load()
                        }
                        .buttonStyle(.bordered)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .accessibilityIdentifier("collection-retry")
                        if !store.error.isEmpty {
                            Text(store.error)
                                .font(.system(.caption2, design: .monospaced))
                                .foregroundStyle(.tertiary)
                                .accessibilityIdentifier("collection-error")
                        }
                    }
                }
                .padding(.bottom, 8)
            }
        }
        .padding(20)
        .onAppear { store.load() }
    }

    private var header: some View {
        HStack {
            Text(Copy.collectionTitle)
                .font(.system(.title3, design: .rounded, weight: .semibold))
                .accessibilityIdentifier("collection-title")
            Spacer()
            Button {
                store.load()
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(.system(.caption, design: .rounded, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: 48, height: 48)
                    .background(Color.white.opacity(0.10), in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.retry)
            .accessibilityIdentifier("collection-refresh")
            SheetCloseButton(identifier: "collection-close")
        }
    }

    private func totals(_ book: CollectionBook) -> some View {
        HStack(spacing: 8) {
            total(
                "\(Copy.collectionMoods) \(book.moodsUnlocked)/\(book.moodsTotal)",
                id: "collection-moods-total"
            )
            total(
                "\(Copy.collectionTricks) \(book.tricksUnlocked)/\(book.tricksTotal)",
                id: "collection-tricks-total"
            )
            Spacer(minLength: 0)
        }
    }

    private func total(_ text: String, id: String) -> some View {
        Text(text)
            .font(.system(.caption, design: .rounded, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(Color.white.opacity(0.08), in: Capsule())
            .accessibilityIdentifier(id)
    }

    private func section(
        _ title: String,
        tag: String,
        entries: [CollectionEntry]
    ) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.system(.caption, design: .rounded, weight: .semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, 4)
            ForEach(entries) { entry in
                Button {
                    // "Go practice": the skill ring's own trigger, so the
                    // duck really performs it (or the core honestly refuses).
                    controller.trigger(entry.name)
                    // The core counts the moment it runs; refresh after a
                    // beat so the row's count and lock state catch up, the
                    // same 1.2 s the Android page waits.
                    let store = self.store
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                        store.load()
                    }
                } label: {
                    row(entry)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("collection-\(tag)-\(entry.name)")
            }
        }
    }

    private func row(_ entry: CollectionEntry) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(SkillLabel.of(entry.name))
                    .font(.system(.subheadline, design: .rounded, weight: .medium))
                    .foregroundStyle(Color.white.opacity(entry.isLocked ? 0.4 : 1))
                Text(firstDayLine(entry))
                    .font(.system(.caption2, design: .rounded))
                    .foregroundStyle(.tertiary)
            }
            Spacer(minLength: 8)
            Text(entry.countText)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(.secondary)
        }
        .frame(minHeight: 44)
        .opacity(entry.isLocked ? 0.55 : 1)
        .contentShape(Rectangle())
    }

    /// A locked row says it was never tried; an unlocked row says when it
    /// was first unlocked, with an em dash when the core does not know.
    private func firstDayLine(_ entry: CollectionEntry) -> String {
        guard let day = entry.firstDayText else { return Copy.collectionLocked }
        return "\(Copy.collectionFirst) \(day)"
    }
}
