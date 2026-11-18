import Foundation
import XCTest
@testable import Duck

/// The voice chat's pure half (F2): plain text in, plain text out. The
/// network and the speaker are closures, so these tests answer with canned
/// bodies and record what would have been spoken — no server, no audio.
final class AIBrainChatTests: XCTestCase {

    /// A transport that answers from fields the test sets, and counts how
    /// many requests actually left. No URLSession, no server.
    private final class FakeTransport: ChatTransport {
        var data: Data?
        var response: URLResponse?
        var error: Error?
        private(set) var sent = 0

        func send(
            _ request: URLRequest,
            completion: @escaping (Data?, URLResponse?, Error?) -> Void
        ) {
            sent += 1
            completion(data, response, error)
        }
    }

    private func makeConfig(
        enabled: Bool = true,
        endpoint: String = "http://example.test/v1"
    ) -> AIConfig {
        var config = AIConfig()
        config.enabled = enabled
        config.endpoint = endpoint
        config.model = "test-model"
        return config
    }

    func testPlainTextReplyIsTrimmed() {
        XCTAssertEqual(AIBrain.parseChatReply("  hello  "), "hello")
        XCTAssertEqual(AIBrain.parseChatReply("one line\n"), "one line")
    }

    func testEmptyReplyIsNotAnAnswer() {
        XCTAssertNil(AIBrain.parseChatReply(nil))
        XCTAssertNil(AIBrain.parseChatReply(""))
        XCTAssertNil(AIBrain.parseChatReply(" \n\t "))
    }

    func testReplyReadsTheOpenAIAndOllamaShapes() {
        let openAI = #"{"choices":[{"message":{"content":"Hi there"}}]}"#
        XCTAssertEqual(AIBrain.chatReply(from: Data(openAI.utf8)), "Hi there")
        let ollama = #"{"message":{"content":"Quack"}}"#
        XCTAssertEqual(AIBrain.chatReply(from: Data(ollama.utf8)), "Quack")
        let empty = #"{"choices":[{"message":{"content":"   "}}]}"#
        XCTAssertNil(AIBrain.chatReply(from: Data(empty.utf8)))
        XCTAssertNil(AIBrain.chatReply(from: Data("not json".utf8)))
        XCTAssertNil(AIBrain.chatReply(from: nil))
    }

    func testMissingEndpointIsNoModel() {
        let transport = FakeTransport()
        let brain = ChatBrain(transport: transport)
        let done = expectation(description: "no model")
        brain.ask(
            config: makeConfig(endpoint: ""),
            question: "hello",
            state: [:],
            language: .english
        ) { reply, error in
            XCTAssertNil(reply)
            XCTAssertEqual(error, Copy.chatNoModel)
            done.fulfill()
        }
        wait(for: [done], timeout: 1)
        XCTAssertEqual(
            transport.sent,
            0,
            "no request may leave without a configured endpoint"
        )
    }

    func testEmptyQuestionNeverReachesTheModel() {
        let transport = FakeTransport()
        let brain = ChatBrain(transport: transport)
        brain.ask(
            config: makeConfig(),
            question: "   \n ",
            state: [:],
            language: .english
        ) { _, _ in
            XCTFail("an empty question is not a turn")
        }
        XCTAssertEqual(transport.sent, 0)
    }

    func testDisabledCompanionIsNoModel() {
        let transport = FakeTransport()
        let brain = ChatBrain(transport: transport)
        let done = expectation(description: "disabled")
        brain.ask(
            config: makeConfig(enabled: false),
            question: "hi",
            state: [:],
            language: .english
        ) { reply, error in
            XCTAssertNil(reply)
            XCTAssertEqual(error, Copy.chatNoModel)
            done.fulfill()
        }
        wait(for: [done], timeout: 1)
        XCTAssertEqual(
            transport.sent,
            0,
            "the endpoint must not be contacted while the toggle is off"
        )
    }

    func testUsableEndpointIsHttpOrHttpsOnly() {
        XCTAssertNotNil(AIBrain.usableEndpoint("http://127.0.0.1:11434/v1"))
        XCTAssertNotNil(AIBrain.usableEndpoint("https://api.example.test/v1"))
        XCTAssertNotNil(AIBrain.usableEndpoint("  https://api.example.test/v1  "))
        XCTAssertNil(AIBrain.usableEndpoint(""))
        XCTAssertNil(AIBrain.usableEndpoint("not a url"))
        XCTAssertNil(AIBrain.usableEndpoint("example.test/v1"))
        XCTAssertNil(AIBrain.usableEndpoint("ftp://example.test/v1"))
    }

    func testChatRulesRequireTheCompanionOnAndARealEndpoint() {
        XCTAssertTrue(ChatRules.canAsk(
            enabled: true,
            endpoint: "http://example.test/v1"
        ))
        XCTAssertFalse(ChatRules.canAsk(
            enabled: false,
            endpoint: "http://example.test/v1"
        ))
        XCTAssertFalse(ChatRules.canAsk(enabled: true, endpoint: "nonsense"))
    }

    func testComposerClearsTheDraftOnlyWhenItSends() {
        var composer = ChatComposer(draft: "  hello  ")
        XCTAssertEqual(composer.take(), "hello")
        XCTAssertEqual(composer.draft, "")
        XCTAssertNil(composer.take(), "a cleared field has nothing to send")
        composer.draft = "   "
        XCTAssertNil(composer.take(), "whitespace is not a question")
        XCTAssertEqual(composer.draft, "   ")
    }

    func testReplyIsSpokenThroughTheInjectedVoice() {
        let payload = #"{"choices":[{"message":{"content":"Hello!"}}]}"#
        let transport = FakeTransport()
        transport.data = Data(payload.utf8)
        var spoken: [String] = []
        let brain = ChatBrain(
            transport: transport,
            speak: { spoken.append($0) }
        )
        let done = expectation(description: "reply")
        brain.ask(
            config: makeConfig(),
            question: "hi",
            state: ["tick": 42],
            language: .english
        ) { reply, error in
            XCTAssertEqual(reply, "Hello!")
            XCTAssertNil(error)
            XCTAssertEqual(spoken, ["Hello!"])
            done.fulfill()
        }
        wait(for: [done], timeout: 1)
    }

    func testHTTPFailureIsAnErrorAndSpeaksNothing() {
        let transport = FakeTransport()
        transport.response = HTTPURLResponse(
            url: URL(string: "http://example.test")!,
            statusCode: 500,
            httpVersion: nil,
            headerFields: nil
        )
        var spoken: [String] = []
        let brain = ChatBrain(
            transport: transport,
            speak: { spoken.append($0) }
        )
        let done = expectation(description: "http failure")
        brain.ask(
            config: makeConfig(),
            question: "hi",
            state: [:],
            language: .chinese
        ) { reply, error in
            XCTAssertNil(reply)
            XCTAssertEqual(error, "HTTP 500")
            XCTAssertTrue(spoken.isEmpty)
            done.fulfill()
        }
        wait(for: [done], timeout: 1)
    }

    func testEmptyModelAnswerIsAnErrorAndSpeaksNothing() {
        let transport = FakeTransport()
        transport.data = Data(#"{"choices":[]}"#.utf8)
        var spoken: [String] = []
        let brain = ChatBrain(
            transport: transport,
            speak: { spoken.append($0) }
        )
        let done = expectation(description: "empty answer")
        brain.ask(
            config: makeConfig(),
            question: "hi",
            state: [:],
            language: .english
        ) { reply, error in
            XCTAssertNil(reply)
            XCTAssertNotNil(error)
            XCTAssertTrue(spoken.isEmpty)
            done.fulfill()
        }
        wait(for: [done], timeout: 1)
    }

    func testChatRequestCarriesTheRealLastHeard() throws {
        let request = try XCTUnwrap(AIBrain.chatRequest(
            config: makeConfig(),
            question: "hi",
            state: [
                "tick": 7,
                "last_heard": ["text": "hello duck"],
                "carrying": true,
                "powered_off": false,
            ],
            language: .english
        ))
        let body = try XCTUnwrap(request.httpBody)
        let text = String(decoding: body, as: UTF8.self)
        XCTAssertTrue(text.contains("last_heard"))
        XCTAssertTrue(text.contains("hello duck"))
        XCTAssertTrue(text.contains("carrying"))
        XCTAssertTrue(text.contains("powered_off"))
    }

    func testChatRequestUsesTheConfiguredEndpointAndKey() throws {
        var config = makeConfig(endpoint: "http://example.test/v1")
        config.key = "secret"
        let request = try XCTUnwrap(AIBrain.chatRequest(
            config: config,
            question: "hello",
            state: ["tick": 7, "emotion": ["name": "curious"]],
            language: .chinese
        ))
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(
            request.url?.absoluteString,
            "http://example.test/v1/chat/completions"
        )
        XCTAssertEqual(
            request.value(forHTTPHeaderField: "Authorization"),
            "Bearer secret"
        )
        let body = try XCTUnwrap(request.httpBody)
        let text = String(decoding: body, as: UTF8.self)
        XCTAssertTrue(text.contains("Chinese"))
        XCTAssertTrue(text.contains("Never invent"))
        XCTAssertTrue(text.contains("tick"))
    }
}
