import XCTest
@testable import Duck

/// The learned words' shell half (G2): the table's storage encoding, the
/// 24-scalar rule, the whole-table `/config` body, and the one replay the
/// launch performs. The core round trip needs the dylib and belongs to the
/// simulator's CI run.
final class AliasTests: XCTestCase {

    private func freshDefaults() -> UserDefaults {
        let suite = "duck-voice-alias-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suite)
        }
        return defaults
    }

    func testTableSurvivesANewStore() {
        let defaults = freshDefaults()
        let table = ["转圈圈": "roulade", "坐下": "sit"]
        VoiceAliasStore(defaults: defaults).remember(table)
        XCTAssertEqual(
            VoiceAliasStore(defaults: defaults).table,
            table,
            "a taught table must survive the store object that wrote it"
        )
    }

    func testStoredTableIsJSONEncodedAndDecodable() throws {
        let defaults = freshDefaults()
        let table = ["转圈圈": "roulade"]
        VoiceAliasStore(defaults: defaults).remember(table)
        let data = try XCTUnwrap(defaults.data(forKey: VoiceAliasStore.defaultsKey))
        let decoded = try JSONDecoder().decode([String: String].self, from: data)
        XCTAssertEqual(decoded, table)
        XCTAssertNil(
            defaults.dictionary(forKey: VoiceAliasStore.defaultsKey),
            "the table is JSON data, not a property-list dictionary"
        )
    }

    func testConfigBodyIsTheWholeTableReplace() throws {
        let table = ["转圈圈": "roulade", "坐下": "sit"]
        let body = try XCTUnwrap(VoiceAliasStore.configBody(table))
        let object = try JSONSerialization.jsonObject(with: body)
        let json = try XCTUnwrap(object as? [String: Any])
        let aliases = try XCTUnwrap(json["voice_aliases"] as? [String: Any])
        XCTAssertEqual(aliases.count, table.count)
        for (phrase, target) in table {
            XCTAssertEqual(aliases[phrase] as? String, target)
        }
        XCTAssertEqual(json.count, 1, "the body carries only the whole table")
    }

    func testAnEmptyTableIsStillAClearRequest() throws {
        let body = try XCTUnwrap(VoiceAliasStore.configBody([:]))
        let object = try JSONSerialization.jsonObject(with: body)
        let json = try XCTUnwrap(object as? [String: Any])
        let aliases = try XCTUnwrap(json["voice_aliases"] as? [String: Any])
        XCTAssertTrue(aliases.isEmpty, "clearing every word is a legal save")
    }

    func testReplayCallsTheInjectedClosureOnceWithTheExactTable() {
        let defaults = freshDefaults()
        let store = VoiceAliasStore(defaults: defaults)
        let table = ["转圈圈": "roulade", "坐下": "sit"]
        store.remember(table)
        var calls: [[String: String]] = []
        let accepted = store.replay { handed in
            calls.append(handed)
            return true
        }
        XCTAssertTrue(accepted)
        XCTAssertEqual(calls, [table], "one replay, the stored table verbatim")
    }

    func testNothingStoredMeansNoReplayCall() {
        let store = VoiceAliasStore(defaults: freshDefaults())
        var calls = 0
        let accepted = store.replay { _ in
            calls += 1
            return true
        }
        XCTAssertFalse(accepted, "a fresh install has no words to hand back")
        XCTAssertEqual(calls, 0, "a fresh install must not post an empty table")
    }

    func testRefusalIsReportedAndNotRetried() {
        let defaults = freshDefaults()
        let store = VoiceAliasStore(defaults: defaults)
        store.remember(["转圈圈": "roulade"])
        var calls = 0
        let accepted = store.replay { _ in
            calls += 1
            // The core may refuse a target (a manifest skill went away);
            // it is still exactly one hand-back, not a retry loop here.
            return false
        }
        XCTAssertFalse(accepted)
        XCTAssertEqual(calls, 1)
    }

    func testBlankTableIsNotRemembered() {
        let defaults = freshDefaults()
        let store = VoiceAliasStore(defaults: defaults)
        store.remember([:])
        XCTAssertTrue(store.table.isEmpty)
        XCTAssertFalse(store.replay { _ in true })
    }

    func testPhraseLengthBoundary() {
        XCTAssertTrue(
            VoiceAliasStore.isValidPhrase(String(repeating: "字", count: 24)),
            "24 characters is the limit"
        )
        XCTAssertFalse(
            VoiceAliasStore.isValidPhrase(String(repeating: "字", count: 25)),
            "25 characters is too long"
        )
        XCTAssertTrue(VoiceAliasStore.isValidPhrase("  转圈圈  "), "edges are trimmed")
        XCTAssertFalse(VoiceAliasStore.isValidPhrase("   "))
        XCTAssertFalse(VoiceAliasStore.isValidPhrase(""))
        // One 🐤 is one scalar (two UTF-16 units), so 24 of them still fit.
        XCTAssertTrue(VoiceAliasStore.isValidPhrase(String(repeating: "🐤", count: 24)))
        XCTAssertFalse(VoiceAliasStore.isValidPhrase(String(repeating: "🐤", count: 25)))
        // A joined family emoji is one grapheme but seven scalars (four
        // people and three joiners): the core counts scalars, so three
        // fit and four refuse.
        let family = "👨\u{200D}👩\u{200D}👧\u{200D}👦"
        XCTAssertTrue(
            VoiceAliasStore.isValidPhrase(String(repeating: family, count: 3)),
            "21 scalars fit the limit"
        )
        XCTAssertFalse(
            VoiceAliasStore.isValidPhrase(String(repeating: family, count: 4)),
            "28 scalars in four graphemes is past the limit"
        )
    }

    func testSpellingsThatDifferOnlyInCaseAreOneWord() {
        XCTAssertEqual(VoiceAliasStore.normalizedPhrase("  Sit  "), "sit")
        XCTAssertTrue(
            VoiceAliasStore.isValidPhrase("SIT"),
            "the case folds before anything else is judged"
        )
        // A table saved by an older build keeps its mixed-case keys on
        // disk; reading folds them, so the replay hands the core the keys
        // it holds itself.
        let defaults = freshDefaults()
        let store = VoiceAliasStore(defaults: defaults)
        store.remember(["Sit": "roulade"])
        XCTAssertEqual(
            store.table,
            ["sit": "roulade"],
            "the stored spelling reads back folded, like the core's table"
        )
        // Three spellings collapse to one word; one of the targets wins,
        // the same later-wins whole-table rule the core applies.
        let folded = VoiceAliasStore.normalized(["Sit": "roulade", "SIT": "stand", "sit": "yes"])
        XCTAssertEqual(folded.count, 1, "the three spellings are one word")
        XCTAssertNotNil(folded["sit"], "the folded key is the one the core keeps")
    }

    func testNormalizationDropsWhatTheCoreWouldRefuse() {
        let dirty: [String: String] = [
            "  转圈圈  ": " roulade ",
            "  ": "sit",
            String(repeating: "字", count: 25): "sit",
            "坐下": "  ",
        ]
        XCTAssertEqual(
            VoiceAliasStore.normalized(dirty),
            ["转圈圈": "roulade"],
            "blank and over-long phrases and blank targets never reach the core"
        )
    }

    func testPickerOffersThirteenMoodsThreeVerbsAndTheLoadedManifest() {
        let names = AliasTargets.emotions
            + ["sit", "stand", "ground_pick", "roulade", "kick_left", "roulade"]
        let options = AliasTargets.options(skillNames: names)
        XCTAssertEqual(options.count, 13 + 3 + 2)
        XCTAssertEqual(Array(options.prefix(13)), AliasTargets.emotions)
        XCTAssertEqual(
            Array(options.dropFirst(13).prefix(2)),
            ["roulade", "kick_left"],
            "manifest skills follow the official names in the core's published order"
        )
        XCTAssertEqual(
            Array(options.suffix(3)),
            AliasTargets.verbs,
            "the loop's own verbs close the list, as the ticket orders it"
        )
        XCTAssertEqual(options.filter { $0 == "sit" }.count, 1)
        XCTAssertEqual(options.filter { $0 == "roulade" }.count, 1)
    }

    func testPickerStillOffersTheVerbsWithNoCoreAnswer() {
        let options = AliasTargets.options(skillNames: [])
        XCTAssertEqual(options, AliasTargets.emotions + AliasTargets.verbs)
        XCTAssertEqual(AliasTargets.emotions.count, 13)
    }
}
