import XCTest
@testable import Duck

/// The game cards' quest note: a pure read of the real board. The mapping is
/// the core's own blueprint ids (`quests.rs` BLUEPRINTS); a game with no such
/// quest on the board says nothing, and a done quest says so in its own words.
final class QuestNoteTests: XCTestCase {
    private func card(
        _ id: String, _ title: String, _ progress: Double = 0, _ done: Bool = false
    ) -> DuckController.QuestCard {
        DuckController.QuestCard(id: id, title: title, progress: progress, done: done)
    }

    func testEachGameNamesItsOwnQuest() {
        let board = [
            card("skills", "做 2 个动作", 0.5),
            card("moods", "做 2 个表情"),
            card("clap", "拍手两次"),
            card("walk", "一起走 2 米", 1, true),
        ]
        XCTAssertEqual(gameQuestNote(board, game: "kick-rally"), Copy.questNotePrefix + "做 2 个动作")
        XCTAssertEqual(gameQuestNote(board, game: "mood-charade"), Copy.questNotePrefix + "做 2 个表情")
        XCTAssertEqual(gameQuestNote(board, game: "sound-simon"), Copy.questNotePrefix + "拍手两次")
        XCTAssertEqual(gameQuestNote(board, game: "red-light"), Copy.questNoteDonePrefix + "一起走 2 米")
    }

    func testAnAbsentQuestOrAnUnmappedGameSaysNothing() {
        XCTAssertEqual(gameQuestNote([], game: "kick-rally"), "")
        let board = [card("skills", "做 2 个动作")]
        XCTAssertEqual(gameQuestNote(board, game: "mood-charade"), "")
        XCTAssertEqual(gameQuestNote(board, game: "ledger"), "")
        XCTAssertEqual(gameQuestNote(board, game: "kick-rally-extra"), "")
    }

    func testEveryMappedIdIsACoreBlueprintId() {
        let blueprintIds: Set<String> = [
            "walk", "roll", "pet", "hold", "feed", "clap",
            "whistle", "moods", "skills", "peck", "bond",
        ]
        for game in ["kick-rally", "mood-charade", "sound-simon", "red-light"] {
            let id = gameQuestID(game)
            XCTAssertNotNil(id)
            XCTAssertTrue(blueprintIds.contains(id ?? ""), "\(game) must map to a blueprint id")
        }
    }

    func testTheNoteFollowsTheBoardAsItChanges() {
        let before = [card("skills", "做 2 个动作", 0.5)]
        let after = [card("skills", "做 2 个动作", 1, true)]
        XCTAssertEqual(gameQuestNote(before, game: "kick-rally"), Copy.questNotePrefix + "做 2 个动作")
        XCTAssertEqual(gameQuestNote(after, game: "kick-rally"), Copy.questNoteDonePrefix + "做 2 个动作")
    }
}
