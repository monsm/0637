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

/// One badge in the collection book: a real threshold over the core's own
/// counters. The thresholds are the ticket's; the numbers are the book's.
/// Nothing here guesses a count, a date or an unlock.
enum CollectionBadgeKind: String, CaseIterable {
    case firstMood
    case allMoods
    case firstTrick
    case allTricks
    case moods10
    case moods100
    case moods1000
    case tricks10
    case tricks100

    /// The official release's thirteen moods — the core's `MOODS_TOTAL`.
    /// "All 13 moods" must mean all thirteen, not "all the rows this page
    /// happened to receive": a build that loaded five must not wear it.
    static let officialMoods = 13

    var icon: String {
        switch self {
        case .firstMood: return "heart.fill"
        case .allMoods: return "heart.circle.fill"
        case .firstTrick: return "graduationcap.fill"
        case .allTricks: return "rosette"
        case .moods10: return "hand.thumbsup.fill"
        case .moods100: return "flame.fill"
        case .moods1000: return "crown.fill"
        case .tricks10: return "star.fill"
        case .tricks100: return "trophy.fill"
        }
    }

    /// The cross-shell id the accessibility tags use. Android's shelf
    /// carries the same kebab-case ids, so a test reads both phones alike.
    var id: String {
        switch self {
        case .firstMood: return "first-mood"
        case .allMoods: return "all-moods"
        case .firstTrick: return "first-trick"
        case .allTricks: return "all-tricks"
        case .moods10: return "moods-10"
        case .moods100: return "moods-100"
        case .moods1000: return "moods-1000"
        case .tricks10: return "tricks-10"
        case .tricks100: return "tricks-100"
        }
    }

    var title: String {
        switch self {
        case .firstMood: return Copy.badgeFirstMood
        case .allMoods: return Copy.badgeAllMoods
        case .firstTrick: return Copy.badgeFirstTrick
        case .allTricks: return Copy.badgeAllTricks
        case .moods10: return Copy.badgeMoods10
        case .moods100: return Copy.badgeMoods100
        case .moods1000: return Copy.badgeMoods1000
        case .tricks10: return Copy.badgeTricks10
        case .tricks100: return Copy.badgeTricks100
        }
    }

    /// Which half of the book this badge counts; the condition line uses
    /// the collection's own noun so it follows the interface language.
    var isTrick: Bool {
        switch self {
        case .firstTrick, .allTricks, .tricks10, .tricks100: return true
        default: return false
        }
    }

    /// The "all" badges read goal/goal; the firsts and milestones read
    /// "≥ goal". Both spell out the real unlock condition.
    var isAll: Bool {
        switch self {
        case .allMoods, .allTricks: return true
        default: return false
        }
    }
}

/// One evaluated badge. `progress` is the book's real number; the display
/// caps it at the goal, so 1001 moods still reads 1000/1000.
struct CollectionBadge: Identifiable, Equatable {
    let kind: CollectionBadgeKind
    let unlocked: Bool
    let progress: Int
    let goal: Int

    var id: String { kind.id }
    var title: String { kind.title }
    var icon: String { kind.icon }

    var progressText: String {
        "\(min(max(progress, 0), goal))/\(goal)"
    }

    /// The unlock condition, on the row's second line. The nouns come from
    /// the same two book headers the page already shows.
    var conditionText: String {
        let noun = kind.isTrick ? Copy.collectionTricks : Copy.collectionMoods
        return kind.isAll ? "\(noun) \(goal)/\(goal)" : "\(noun) ≥ \(goal)"
    }
}

/// The nine badges, computed only from `GET /collection`. Pure, so the
/// thresholds and the locked/unlocked edges are unit-tested without a core.
enum CollectionBadges {
    static func evaluate(book: CollectionBook) -> [CollectionBadge] {
        // One row per name, as the core publishes. A repeated row must not
        // count twice or let a badge unlock on its own repetition.
        let moods = distinct(book.emotions)
        let tricks = distinct(book.tricks)
        let moodsTried = moods.filter { !$0.isLocked }.count
        let tricksTried = tricks.filter { !$0.isLocked }.count
        let moodPlays = moods.reduce(0) { $0 + max(0, $1.count) }
        let trickPlays = tricks.reduce(0) { $0 + max(0, $1.count) }
        let moodGoal = CollectionBadgeKind.officialMoods
        // An empty trick list is not a graduation: "all of {}" is true and
        // would lie, so the goal is the rows the manifest really published.
        let trickGoal = tricks.count

        return [
            CollectionBadge(
                kind: .firstMood,
                unlocked: moodsTried > 0,
                progress: moodsTried > 0 ? 1 : 0,
                goal: 1
            ),
            CollectionBadge(
                kind: .allMoods,
                unlocked: moodsTried >= moodGoal,
                progress: moodsTried,
                goal: moodGoal
            ),
            CollectionBadge(
                kind: .firstTrick,
                unlocked: tricksTried > 0,
                progress: tricksTried > 0 ? 1 : 0,
                goal: 1
            ),
            CollectionBadge(
                kind: .allTricks,
                unlocked: trickGoal > 0 && tricksTried == trickGoal,
                progress: tricksTried,
                goal: trickGoal
            ),
            CollectionBadge(
                kind: .moods10,
                unlocked: moodPlays >= 10,
                progress: moodPlays,
                goal: 10
            ),
            CollectionBadge(
                kind: .moods100,
                unlocked: moodPlays >= 100,
                progress: moodPlays,
                goal: 100
            ),
            CollectionBadge(
                kind: .moods1000,
                unlocked: moodPlays >= 1000,
                progress: moodPlays,
                goal: 1000
            ),
            CollectionBadge(
                kind: .tricks10,
                unlocked: trickPlays >= 10,
                progress: trickPlays,
                goal: 10
            ),
            CollectionBadge(
                kind: .tricks100,
                unlocked: trickPlays >= 100,
                progress: trickPlays,
                goal: 100
            ),
        ]
    }

    private static func distinct(_ entries: [CollectionEntry]) -> [CollectionEntry] {
        var seen: Set<String> = []
        var out: [CollectionEntry] = []
        for entry in entries {
            if seen.insert(entry.name).inserted {
                out.append(entry)
            }
        }
        return out
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
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: DuckSpace.lg) {
                    if let book = store.book {
                        totals(book)
                        if book.emotions.isEmpty && book.tricks.isEmpty {
                            DuckEmptyState(
                                title: Copy.collectionEmpty,
                                actionTitle: Copy.emptyCollectionAction,
                                action: {
                                    controller.trigger("curious")
                                    let store = self.store
                                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                                        store.load()
                                    }
                                },
                                identifier: "collection-empty-state"
                            )
                        } else {
                            badgeSection(book)
                            section(Copy.collectionMoods, tag: "moods", entries: book.emotions)
                            section(Copy.collectionTricks, tag: "tricks", entries: book.tricks)
                        }
                    } else if store.busy {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                            .padding(.top, DuckSpace.xxl)
                    } else {
                        Button(Copy.retry) {
                            store.load()
                        }
                        .buttonStyle(.bordered)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .accessibilityIdentifier("collection-retry")
                        if !store.error.isEmpty {
                            Text(store.error)
                                .font(.duckCaption)
                                .foregroundStyle(.tertiary)
                                .accessibilityIdentifier("collection-error")
                        }
                    }
                }
                .padding(.bottom, DuckSpace.sm)
            }
        }
        .padding(DuckSpace.xl)
        .onAppear { store.load() }
    }

    private var header: some View {
        HStack {
            Text(Copy.collectionTitle)
                .font(.duckTitle.weight(.semibold))
                .accessibilityIdentifier("collection-title")
                .duckSheetFirstFocus()
            Spacer()
            Button {
                store.load()
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(DuckIcon.xs)
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

    @ViewBuilder private func totals(_ book: CollectionBook) -> some View {
        if typeSize.duckIsAccessibility {
            VStack(alignment: .leading, spacing: DuckSpace.xs) {
                total(
                    "\(Copy.collectionMoods) \(book.moodsUnlocked)/\(book.moodsTotal)",
                    id: "collection-moods-total"
                )
                total(
                    "\(Copy.collectionTricks) \(book.tricksUnlocked)/\(book.tricksTotal)",
                    id: "collection-tricks-total"
                )
            }
        } else {
            HStack(spacing: DuckSpace.sm) {
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
    }

    private func total(_ text: String, id: String) -> some View {
        Text(text)
            .font(.duckCaption.weight(.semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, DuckSpace.sm)
            .padding(.vertical, DuckSpace.xs)
            .background(Color.white.opacity(0.08), in: Capsule())
            .accessibilityIdentifier(id)
    }

    /// The badges, before the rows they are measured from. Every number is
    /// the core's; a locked badge is dimmed and shows exactly how far away
    /// its threshold still is.
    private func badgeSection(_ book: CollectionBook) -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.xxs) {
            Text(Copy.badgeSection)
                .font(.duckCaption.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, DuckSpace.xs)
            ForEach(CollectionBadges.evaluate(book: book)) { badge in
                badgeRow(badge)
            }
        }
        .accessibilityIdentifier("collection-section-badges")
    }

    @ViewBuilder private func badgeRow(_ badge: CollectionBadge) -> some View {
        if typeSize.duckIsAccessibility {
            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                Text(badge.title)
                    .font(.duckBody.weight(.medium))
                    .foregroundStyle(Color.white.opacity(badge.unlocked ? 1 : 0.4))
                    .fixedSize(horizontal: false, vertical: true)
                Text(badge.conditionText)
                    .font(.duckCaption)
                    .foregroundStyle(.tertiary)
                Text(badge.progressText)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .frame(minHeight: 44)
            .opacity(badge.unlocked ? 1 : 0.55)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("collection-badge-\(badge.id)")
        } else {
            HStack(alignment: .firstTextBaseline, spacing: DuckSpace.sm) {
                Image(systemName: badge.icon)
                    .font(DuckIcon.sm)
                    .foregroundStyle(
                        badge.unlocked ? Color.duckData : Color.white.opacity(0.4)
                    )
                    .frame(width: 22)
                VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                    Text(badge.title)
                        .font(.duckBody.weight(.medium))
                        .foregroundStyle(Color.white.opacity(badge.unlocked ? 1 : 0.4))
                    // The unlock condition is the row's second line, in the
                    // collection's own nouns: "Moods 7/13" is the state, and
                    // "Moods 13/13" (or "≥ 10") is the threshold.
                    Text(badge.conditionText)
                        .font(.duckCaption)
                        .foregroundStyle(.tertiary)
                }
                Spacer(minLength: DuckSpace.sm)
                Text(badge.progressText)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            }
            .frame(minHeight: 44)
            .opacity(badge.unlocked ? 1 : 0.55)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("collection-badge-\(badge.id)")
        }
    }

    private func section(
        _ title: String,
        tag: String,
        entries: [CollectionEntry]
    ) -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.xxs) {
            Text(title)
                .font(.duckCaption.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, DuckSpace.xs)
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

    @ViewBuilder private func row(_ entry: CollectionEntry) -> some View {
        if typeSize.duckIsAccessibility {
            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                Text(SkillLabel.of(entry.name))
                    .font(.duckBody.weight(.medium))
                    .foregroundStyle(Color.white.opacity(entry.isLocked ? 0.4 : 1))
                Text(firstDayLine(entry))
                    .font(.duckCaption)
                    .foregroundStyle(.tertiary)
                Text(entry.countText)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .frame(minHeight: 44)
            .opacity(entry.isLocked ? 0.55 : 1)
            .contentShape(Rectangle())
        } else {
            HStack(alignment: .firstTextBaseline, spacing: DuckSpace.sm) {
                VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                    Text(SkillLabel.of(entry.name))
                        .font(.duckBody.weight(.medium))
                        .foregroundStyle(Color.white.opacity(entry.isLocked ? 0.4 : 1))
                    Text(firstDayLine(entry))
                        .font(.duckCaption)
                        .foregroundStyle(.tertiary)
                }
                Spacer(minLength: DuckSpace.sm)
                Text(entry.countText)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            }
            .frame(minHeight: 44)
            .opacity(entry.isLocked ? 0.55 : 1)
            .contentShape(Rectangle())
        }
    }

    /// A locked row says it was never tried; an unlocked row says when it
    /// was first unlocked, with an em dash when the core does not know.
    private func firstDayLine(_ entry: CollectionEntry) -> String {
        guard let day = entry.firstDayText else { return Copy.collectionLocked }
        return "\(Copy.collectionFirst) \(day)"
    }
}
