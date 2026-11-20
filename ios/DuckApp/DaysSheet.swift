import Foundation
import SwiftUI

/// One "label: value" line of a day card or the share document.
struct DaysLine: Equatable, Identifiable {
    let label: String
    let value: String
    var id: String { label }
}

/// One finished day of the duck's life, exactly as `GET /days` publishes it.
/// The contract is "missing key = 0": an older save may not carry every
/// counter, and the board reads that as a real zero, not a gap to fill in.
struct DaysDay: Equatable {
    let day: String
    let metres: Double
    let poweredS: Double
    let rolls: Int
    let pets: Int
    let pickups: Int
    let falls: Int
    let fed: Int
    let claps: Int
    let whistles: Int
    let groundPicks: Int
    let sitDowns: Int

    /// Parse one day row. The day string is kept verbatim (edges trimmed,
    /// the same fold the Android page uses) — the board never invents,
    /// renames or normalizes a date the core did not send.
    static func parse(_ json: [String: Any]) -> DaysDay? {
        guard let raw = json["day"] as? String else { return nil }
        let day = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !day.isEmpty else { return nil }
        return totals(json, day: day)
    }

    /// The week object carries counters but no date of its own; it parses
    /// through the same key list, with an empty day that is never shown or
    /// shared as a date.
    static func totals(_ json: [String: Any]) -> DaysDay {
        totals(json, day: "")
    }

    private static func totals(_ json: [String: Any], day: String) -> DaysDay {
        DaysDay(
            day: day,
            metres: json["metres"] as? Double ?? 0,
            poweredS: json["powered_s"] as? Double ?? 0,
            rolls: json["rolls"] as? Int ?? 0,
            pets: json["pets"] as? Int ?? 0,
            pickups: json["pickups"] as? Int ?? 0,
            falls: json["falls"] as? Int ?? 0,
            fed: json["fed"] as? Int ?? 0,
            claps: json["claps"] as? Int ?? 0,
            whistles: json["whistles"] as? Int ?? 0,
            groundPicks: json["ground_picks"] as? Int ?? 0,
            sitDowns: json["sit_downs"] as? Int ?? 0
        )
    }

    /// The nine counters, in the one order every rendering uses: the day
    /// rows, the week card and the share document (Android's `daysLines`
    /// has the same order).
    var lines: [DaysLine] {
        [
            DaysLine(label: Copy.daysPowered, value: Self.powered(poweredS)),
            DaysLine(label: Copy.daysMetres, value: Self.metres(metres)),
            DaysLine(label: Copy.daysRolls, value: "\(rolls)"),
            DaysLine(label: Copy.daysPets, value: "\(pets)"),
            DaysLine(label: Copy.daysPickups, value: "\(pickups)"),
            DaysLine(label: Copy.daysFalls, value: "\(falls)"),
            DaysLine(label: Copy.daysFed, value: "\(fed)"),
            DaysLine(label: Copy.daysClaps, value: "\(claps)"),
            DaysLine(label: Copy.daysWhistles, value: "\(whistles)"),
        ]
    }

    /// All nine counters a day row shows, in the same order as the week
    /// card, the share document and Android's `daysLines` (陪伴、米、
    /// 翻滚、被摸、被抱起、摔倒、被喂、拍手、口哨). Built from `lines` so the
    /// row cannot drift from the other two renderings.
    var summary: String {
        lines.map { "\($0.label) \($0.value)" }.joined(separator: " · ")
    }

    static func powered(_ seconds: Double) -> String {
        let hours = seconds / 3600.0
        return String(format: "%.1f h", hours)
    }

    static func metres(_ metres: Double) -> String {
        String(format: "%.1f m", metres)
    }
}

/// The parsed `GET /days` document: the finished days (the core sends only
/// days that really completed), today's running row, and the week's true
/// totals. `today == nil` means the core sent no today — the board shows
/// no today row rather than fabricating one. `week == nil` means the core
/// sent no aggregate — no card, and the shell never re-sums the days (the
/// same rule the parallel Android page follows: the core is the source).
struct DaysBoard: Equatable {
    let days: [DaysDay]
    let today: DaysDay?
    let week: DaysDay?

    /// Parse the whole document. Broken JSON or a shape without a `days`
    /// array returns nil — the sheet then shows nothing rather than a
    /// guessed board.
    static func parse(_ data: Data) -> DaysBoard? {
        guard let json = try? JSONSerialization.jsonObject(with: data),
              let root = json as? [String: Any]
        else { return nil }
        return parse(root)
    }

    static func parse(_ root: [String: Any]) -> DaysBoard? {
        // The intent API's refusal form is HTTP 200 with ok:false; that is
        // not a board the sheet may draw.
        if let ok = root["ok"] as? Bool, !ok { return nil }
        guard let rows = root["days"] as? [[String: Any]] else { return nil }
        let days = rows.compactMap { DaysDay.parse($0) }
        let today = (root["today"] as? [String: Any]).flatMap { DaysDay.parse($0) }
        let week = (root["week"] as? [String: Any]).map { DaysDay.totals($0) }
        return DaysBoard(days: days, today: today, week: week)
    }

    /// The board is empty when the core sent no finished day and no today.
    var isEmpty: Bool { days.isEmpty && today == nil }

    /// The whole board as one bilingual plain-text document, one line per
    /// fact with the English and the Chinese words side by side
    /// (`Metres: 12.3 m / 米: 12.3 m`). The same shape as Android's
    /// `daysShareText`: the dictionary renders the document twice — once in
    /// each language — and the two renders are zipped, so no label is
    /// duplicated by hand and the dates stay the core's own strings.
    /// Nothing here leaves the phone; the ShareSheet carries it.
    func shareText() -> String {
        let saved = Copy.language
        defer { Copy.language = saved }
        Copy.language = .english
        let english = shareLines()
        Copy.language = .chinese
        let chinese = shareLines()
        var out: [String] = []
        for (en, zh) in zip(english, chinese) {
            out.append(en == zh ? en : "\(en) / \(zh)")
        }
        return out.joined(separator: "\n")
    }

    /// One render of the document in the current interface language. The
    /// two renders always align line for line: only labels change language,
    /// values and dates do not.
    private func shareLines() -> [String] {
        var out: [String] = []
        if let week {
            out.append(Copy.daysWeek)
            for line in week.lines {
                out.append("\(line.label): \(line.value)")
            }
        }
        if let today {
            out.append("\(Copy.daysToday) (\(today.day))")
            for line in today.lines {
                out.append("\(line.label): \(line.value)")
            }
        }
        for day in days {
            out.append(day.day)
            for line in day.lines {
                out.append("\(line.label): \(line.value)")
            }
        }
        return out
    }
}

/// 「日子」(K2, 2026-11-19): the parent's board of the duck's real days.
///
/// A tool-page row opens it as a local sheet — the route table is pinned by
/// scripts/check-routes.py, so this is the same local-`.sheet` pattern the
/// duck panel's words page uses, not a new route case. Every number on the
/// page is the core's own from `GET /days`; an empty record says so and
/// shows no zeros. The share button hands the whole board, as plain text,
/// to the existing ShareSheet — nothing leaves the phone on its own.
struct DaysSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    /// The parsed board. Nil before the core answers; `failed` says the
    /// answer never came or was unreadable, so the sheet never mistakes an
    /// offline core for a duck with no days.
    @State private var board: DaysBoard?
    @State private var failed = false
    @State private var shareOpen = false
    /// The document is built once at tap time (shareText flips Copy.language
    /// for the two renders); a rebuild on every view refresh would re-flip a
    /// global and write defaults in the middle of rendering.
    @State private var shareText = ""

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            header
            if let board {
                content(board)
            } else if failed {
                Text(Copy.aiOffline)
                    .font(.duckBody)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .accessibilityIdentifier("days-offline")
            } else {
                ProgressView()
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
        }
        .padding(DuckSpace.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.duckStageInstrument)
        .onAppear(perform: fetch)
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(Copy.daysTitle)
                .font(.duckTitle.weight(.semibold))
                .foregroundStyle(.white)
                .accessibilityIdentifier("days-title")
                .duckSheetFirstFocus()
            Spacer()
            SheetCloseButton(identifier: "days-close")
        }
    }

    @ViewBuilder private func content(_ board: DaysBoard) -> some View {
        if board.isEmpty {
            empty
        } else {
            ScrollView {
                VStack(alignment: .leading, spacing: DuckSpace.md) {
                    if let week = board.week {
                        weekCard(week)
                    }
                    if let today = board.today {
                        dayRow(today, title: Copy.daysToday)
                    }
                    ForEach(board.days, id: \.day) { day in
                        dayRow(day, title: day.day)
                    }
                    Button {
                        shareText = board.shareText()
                        shareOpen = true
                    } label: {
                        Label(Copy.daysShare, systemImage: "square.and.arrow.up")
                            .font(.duckBody.weight(.semibold))
                            .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.borderedProminent)
                    .accessibilityIdentifier("days-share")
                }
                .padding(.bottom, DuckSpace.sm)
            }
            .sheet(isPresented: $shareOpen) {
                ShareSheet(items: [shareText])
            }
        }
    }

    /// The week's true totals, one card: 陪伴小时、米数、翻滚、被摸、被抱起、
    /// 摔倒、被喂、拍手、口哨 — the counters the core sums itself.
    private func weekCard(_ week: DaysDay) -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            Text(Copy.daysWeek)
                .font(.duckBody.weight(.semibold))
                .foregroundStyle(.white)
                .accessibilityIdentifier("days-week")
            ForEach(week.lines) { line in
                if typeSize.duckIsAccessibility {
                    VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                        Text(line.label)
                            .font(.duckCaption)
                            .foregroundStyle(.secondary)
                        Text(line.value)
                            .font(.duckCaption.weight(.semibold))
                            .foregroundStyle(.white)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    HStack {
                        Text(line.label)
                            .font(.duckCaption)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text(line.value)
                            .font(.duckCaption.weight(.semibold))
                            .foregroundStyle(.white)
                    }
                }
            }
        }
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    /// One day: the date (or the Today word for the core's running row) and
    /// all nine counters, the same set and order as the week card and the
    /// share document (and Android's day rows). A row exists only when the
    /// core sent it; the shell never lays out a zero day.
    private func dayRow(_ day: DaysDay, title: String) -> some View {
        VStack(alignment: .leading, spacing: DuckSpace.xxs) {
            Text(title)
                .font(.duckBody.weight(.medium))
                .foregroundStyle(.white)
            Text(day.summary)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, DuckSpace.xs)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("days-row-\(day.day)")
    }

    /// No finished day and no today: one true sentence, one real way out.
    private var empty: some View {
        DuckEmptyState(
            title: Copy.daysEmpty,
            actionTitle: Copy.emptyPlayAction,
            action: { dismiss() },
            identifier: "days-empty-state"
        )
    }

    /// Read the board from the core's own local API. The sheet refreshes on
    /// open. A fetch that fails or answers unreadably says only "no answer"
    /// — never the empty sentence, which would be a claim about the duck.
    private func fetch() {
        guard let url = URL(string: "http://127.0.0.1:8765/days") else {
            failed = true
            return
        }
        URLSession.shared.dataTask(with: url) { data, _, _ in
            let parsed = data.flatMap { DaysBoard.parse($0) }
            DispatchQueue.main.async {
                if let parsed {
                    board = parsed
                    failed = false
                } else {
                    failed = true
                }
            }
        }.resume()
    }
}
