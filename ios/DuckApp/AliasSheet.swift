import Foundation
import SwiftUI

/// The shell-side memory of the words a person taught the duck.
///
/// `POST /config {"voice_aliases": {...}}` is a whole-table replace, and the
/// core's override is process memory: a relaunch starts with no learned
/// words. Like `WalkPolicyStore`, the phone keeps the table in UserDefaults
/// and hands the same table back once the core is up. The shell never rules
/// on whether a target is legal — that is the core's refusal, and its reason
/// is shown to the person.
struct VoiceAliasStore {
    /// Beside the other shell keepsakes, and the same string Android's
    /// `VoiceAliasStore` uses so the pair is greppable; the stores are
    /// separate (UserDefaults here, SharedPreferences there). Deliberately
    /// not a Copy key: nothing visible is named by it.
    static let defaultsKey = "duck_voice_aliases_v1"

    /// The person's phrase limit, in Unicode scalars — the core's `chars`
    /// (`speech.rs` `MAX_ALIAS_CHARS`), not grapheme clusters: an emoji's
    /// joiners and modifiers are counted too.
    static let maxPhraseLength = 24

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// The whole table, with the keys the core holds (trimmed and
    /// lowercased — a table saved by an older build reads back folded).
    /// Empty means a fresh install or a person who cleared every word;
    /// saving an empty table is still a real request (the core's "forget
    /// all").
    var table: [String: String] {
        guard let data = defaults.data(forKey: Self.defaultsKey),
              let decoded = try? JSONDecoder().decode([String: String].self, from: data)
        else { return [:] }
        return Self.normalized(decoded)
    }

    /// Remember a table for the next launch. Blank phrases and blanks
    /// targets are not words; they are dropped rather than stored as
    /// something the core would refuse.
    func remember(_ table: [String: String]) {
        let clean = Self.normalized(table)
        guard !clean.isEmpty else {
            forget()
            return
        }
        guard let data = try? JSONEncoder().encode(clean) else { return }
        defaults.set(data, forKey: Self.defaultsKey)
    }

    func forget() {
        defaults.removeObject(forKey: Self.defaultsKey)
    }

    /// Hand the stored table to the core. False when there is nothing
    /// stored, so the caller can tell "no request" from "refused". The
    /// closure is injected so a unit test can record the exact table and
    /// prove the replay happens at most once.
    @discardableResult
    func replay(apply: ([String: String]) -> Bool) -> Bool {
        let saved = table
        guard !saved.isEmpty else { return false }
        return apply(saved)
    }

    /// The one spelling the core keeps: trimmed and lowercased, so "Sit"
    /// and "sit" are one word — the same fold `speech.rs` applies before
    /// its table.
    static func normalizedPhrase(_ phrase: String) -> String {
        phrase.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    /// The phrase's length as the core measures it: the normalized
    /// spelling in Unicode scalars (`phrase.chars().count()` in Rust).
    /// Lowercasing first, because a fold can widen a letter; counting
    /// scalars, not grapheme clusters, so an emoji's joiners are counted.
    static func phraseLength(_ phrase: String) -> Int {
        normalizedPhrase(phrase).unicodeScalars.count
    }

    /// Trim the phrase edges, fold the case, drop blank phrases, drop
    /// phrases past the limit, and drop a blank target. The target itself
    /// is kept verbatim; whether it is legal is the core's ruling, not the
    /// shell's. Two spellings that fold to one phrase keep one target —
    /// the later wins, the same whole-table rule the core applies.
    static func normalized(_ table: [String: String]) -> [String: String] {
        var clean: [String: String] = [:]
        for (phrase, target) in table {
            let key = normalizedPhrase(phrase)
            guard isValidPhrase(key) else { continue }
            let goal = target.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !goal.isEmpty else { continue }
            clean[key] = goal
        }
        return clean
    }

    /// A phrase is a real word and no longer than the limit.
    static func isValidPhrase(_ phrase: String) -> Bool {
        let key = normalizedPhrase(phrase)
        return !key.isEmpty && phraseLength(key) <= maxPhraseLength
    }

    /// The whole-table `/config` body. An empty table still produces a body:
    /// clearing every learned word is a legal save.
    static func configBody(_ table: [String: String]) -> Data? {
        try? JSONSerialization.data(withJSONObject: ["voice_aliases": table])
    }

    enum SaveResult: Equatable {
        case saved
        /// The core's own words where it gave any; a status otherwise.
        case refused(String)
    }

    /// POST the whole table. Success is a 2xx *and* a body that did not
    /// refuse: the intent API's refusal form is HTTP 200 with
    /// `{"ok":false}`. The contract also says an illegal target is a 400,
    /// so the reason is read from the body in either shape. The completion
    /// lands on the main queue.
    func save(
        _ table: [String: String],
        completion: @escaping (SaveResult) -> Void
    ) {
        let clean = Self.normalized(table)
        guard let body = Self.configBody(clean) else {
            completion(.refused("no body"))
            return
        }
        guard let url = URL(string: "http://127.0.0.1:8765/config") else {
            completion(.refused("no core address"))
            return
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 2.0
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = body
        URLSession.shared.dataTask(with: request) { data, response, _ in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            var json: [String: Any]?
            if let data,
               let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                json = parsed
            }
            var refused = false
            var reason: String?
            if let json {
                if let ok = json["ok"] as? Bool, !ok { refused = true }
                reason = (json["error"] as? String)
                    ?? (json["reason"] as? String)
            }
            var raw = ""
            if let data, let text = String(data: data, encoding: .utf8) {
                raw = text.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            let accepted = (200..<300).contains(status) && !refused
            if accepted {
                DispatchQueue.main.async { completion(.saved) }
                return
            }
            if let reason, !reason.isEmpty {
                DispatchQueue.main.async { completion(.refused(reason)) }
            } else if !raw.isEmpty {
                DispatchQueue.main.async { completion(.refused(raw)) }
            } else if status != 0 {
                DispatchQueue.main.async { completion(.refused("HTTP \(status)")) }
            } else {
                // Nothing answered; the sheet says so in the interface's
                // own words rather than inventing a core reason.
                DispatchQueue.main.async { completion(.refused("")) }
            }
        }.resume()
    }
}

/// The words a person may teach: the official thirteen moods, the loop's
/// own three verbs, and the manifest skills the core really loaded.
///
/// Pure, so the picker's contents can be pinned without a screen or a core.
/// `/skills` folds the verbs and the emotions in with the manifest's
/// one-shots, so they are removed here rather than offered twice.
enum AliasTargets {
    /// robotd's thirteen official emotions, in the release's alphabetical
    /// order (`DuckController.emotionNames` is the same set).
    static let emotions = [
        "angry", "curious", "defiant", "devastated", "excited", "impatient",
        "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
    ]

    /// The loop's own verbs; they are always legal targets.
    static let verbs = ["sit", "stand", "ground_pick"]

    static func options(skillNames: [String]) -> [String] {
        // The ticket's order is thirteen moods, the manifest's tricks, then
        // the loop's own verbs. `/skills` mixes all three; the known groups
        // are removed and the manifest's published order is kept, the same
        // order Android's picker shows.
        let known = Set(emotions + verbs)
        var out = emotions
        for name in skillNames where !known.contains(name) && !out.contains(name) {
            out.append(name)
        }
        for name in verbs where !out.contains(name) {
            out.append(name)
        }
        return out
    }
}

/// The "teach it words" page (G2): say this, and the duck does that.
///
/// The table is saved whole — the core's `/config` is a whole-table replace —
/// and kept on this phone so a launch without the core does not forget what
/// the person taught. A refusal shows the core's own reason; nothing here
/// pretends a save landed.
struct AliasSheet: View {
    @ObservedObject var controller: DuckController
    private let store: VoiceAliasStore

    /// The table being edited. The saved table is the core's; this draft
    /// exists until the person presses Save.
    @State private var draft: [String: String]
    @State private var phrase = ""
    @State private var target = "sit"
    @State private var busy = false
    @State private var saved = false
    @State private var error = ""

    init(controller: DuckController, store: VoiceAliasStore = VoiceAliasStore()) {
        _controller = ObservedObject(wrappedValue: controller)
        self.store = store
        _draft = State(initialValue: store.table)
    }

    /// What the core published, split into the three groups the ticket
    /// names. The official list is always offered; if the core is not up
    /// yet its refusal is honest and shown.
    private var options: [String] {
        AliasTargets.options(skillNames: controller.skillNames())
    }

    private var phrases: [String] {
        draft.keys.sorted()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            header
            Text(Copy.aliasLocalOnly)
                .font(.duckCaption)
                .foregroundStyle(.tertiary)
                .accessibilityIdentifier("alias-local-only")
            ScrollView {
                VStack(alignment: .leading, spacing: DuckSpace.lg) {
                    list
                    addRow
                    if !error.isEmpty {
                        Text(error)
                            .font(.duckCaption)
                            .foregroundStyle(.orange)
                            .lineLimit(4)
                            .accessibilityIdentifier("alias-error")
                    }
                    Button {
                        save()
                    } label: {
                        HStack(spacing: DuckSpace.sm) {
                            if busy {
                                ProgressView()
                            }
                            // Android's page uses the same existing Save
                            // word; a new Copy key would break the two
                            // dictionaries' key parity.
                            Text(saved ? Copy.saved : Copy.aiSave)
                                .font(.duckBody.weight(.semibold))
                        }
                        .frame(maxWidth: .infinity, minHeight: 48)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(busy)
                    .accessibilityIdentifier("alias-save")
                }
                .padding(.bottom, DuckSpace.sm)
            }
        }
        .padding(DuckSpace.xl)
        .background(Color.duckStageInstrument)
        .onAppear {
            if !options.contains(target) {
                target = options.first ?? "sit"
            }
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(Copy.aliasTitle)
                .font(.duckTitle.weight(.semibold))
                .accessibilityIdentifier("alias-title")
            Spacer()
            SheetCloseButton(identifier: "alias-close")
        }
    }

    @ViewBuilder private var list: some View {
        if draft.isEmpty {
            Text(Copy.aliasEmpty)
                .font(.duckBody)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, minHeight: 44)
                .accessibilityIdentifier("alias-empty")
        } else {
            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                ForEach(phrases, id: \.self) { phrase in
                    row(phrase, target: draft[phrase] ?? "")
                }
            }
        }
    }

    /// One learned pair. The target is the official label, not the wire
    /// spelling; an unknown manifest skill keeps its real name.
    private func row(_ phrase: String, target: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: DuckSpace.sm) {
            Text(phrase)
                .font(.duckBody.weight(.medium))
                .foregroundStyle(.white)
                .lineLimit(1)
            Image(systemName: "arrow.right")
                .font(DuckIcon.xs)
                .foregroundStyle(.tertiary)
            Text(SkillLabel.of(target))
                .font(.duckBody)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .minimumScaleFactor(0.8)
            Spacer(minLength: DuckSpace.sm)
            Button {
                draft.removeValue(forKey: phrase)
                saved = false
                error = ""
            } label: {
                Image(systemName: "trash")
                    .font(DuckIcon.xs)
                    .foregroundStyle(.secondary)
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            // The dictionaries already own the one word for this action;
            // a new Copy key would break key parity with Android.
            .accessibilityLabel(Copy.deleteRule)
            .accessibilityIdentifier("alias-delete-\(phrase)")
        }
        .frame(minHeight: 44)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("alias-row-\(phrase)")
    }

    private var addRow: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            Text(Copy.aliasHint)
                .font(.duckCaption.weight(.semibold))
                .foregroundStyle(.secondary)
            TextField(Copy.aliasHint, text: $phrase)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .accessibilityIdentifier("alias-phrase")
            if VoiceAliasStore.phraseLength(phrase) > VoiceAliasStore.maxPhraseLength {
                Text(Copy.aliasTooLong)
                    .font(.duckCaption)
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("alias-too-long")
            }
            Text(Copy.aliasDoes)
                .font(.duckCaption.weight(.semibold))
                .foregroundStyle(.secondary)
            Picker(Copy.aliasDoes, selection: $target) {
                ForEach(options, id: \.self) { name in
                    Text(SkillLabel.of(name)).tag(name)
                }
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("alias-target")
            Button {
                add()
            } label: {
                Text(Copy.aliasAdd)
                    .font(.duckBody.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.bordered)
            .disabled(!VoiceAliasStore.isValidPhrase(phrase))
            .accessibilityIdentifier("alias-add")
        }
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    private func add() {
        let key = VoiceAliasStore.normalizedPhrase(phrase)
        guard VoiceAliasStore.isValidPhrase(key),
              options.contains(target)
        else { return }
        // One word, one target: a spelling that only differs in case
        // replaces the row it names — the list shows the updated pair, the
        // same later-wins the core's own table applies.
        draft[key] = target
        phrase = ""
        saved = false
        error = ""
    }

    private func save() {
        guard !busy else { return }
        busy = true
        saved = false
        error = ""
        let table = VoiceAliasStore.normalized(draft)
        store.save(table) { result in
            busy = false
            switch result {
            case .saved:
                // The core took the whole table; this phone keeps it for
                // the next launch's replay.
                store.remember(table)
                draft = table
                saved = true
            case .refused(let reason):
                error = reason.isEmpty ? Copy.aiOffline : reason
            }
        }
    }
}
