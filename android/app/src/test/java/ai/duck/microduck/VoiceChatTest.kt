package ai.duck.microduck

import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.net.URL

/**
 * F3's voice chat: the plain-text model path through an injected fake HTTP
 * (no network, no emulator) and the state machine's three refusals — empty
 * input, an unconfigured model, and a send already in flight.
 */
class VoiceChatTest {

    private val original = AIBrain.http

    private class FakeHttp(
        private val code: Int = 200,
        private val answer: String = """{"choices":[{"message":{"content":"Peep!"}}]}""",
    ) : AIModelHttp {
        var calls = 0
        var url: URL? = null
        var key: String? = null
        var body: String? = null

        override fun post(url: URL, key: String, body: String): AIModelReply {
            calls += 1
            this.url = url
            this.key = key
            this.body = body
            return AIModelReply(code, answer)
        }
    }

    private val config = AIConfig(
        enabled = true,
        endpoint = "http://model.test/v1",
        model = "",
        key = "secret",
    )

    @Before
    fun setUp() {
        AIBrain.http = FakeHttp()
    }

    @After
    fun tearDown() {
        AIBrain.http = original
    }

    @Test
    fun `chat asks the same endpoint for plain text and trims the reply`() {
        val fake = FakeHttp(
            answer = """{"choices":[{"message":{"content":"  The duck peeped once.  "}}]}""",
        )
        AIBrain.http = fake
        val (reply, error) = AIBrain.chat(
            config,
            """{"tick":42,"care":{"hunger":0.2},"life":{"metres":3.5}}""",
            heard = "hello",
            message = "  how are you?  ",
            chinese = false,
        )
        assertNull(error)
        assertEquals("The duck peeped once.", reply)
        assertEquals("http://model.test/v1/chat/completions", fake.url.toString())
        assertEquals("secret", fake.key)
        val body = JSONObject(fake.body!!)
        assertEquals("gpt-4o-mini", body.getString("model"))
        val messages = body.getJSONArray("messages")
        assertEquals(2, messages.length())
        val system = messages.getJSONObject(0).getString("content")
        assertTrue(system.contains("Never invent"))
        assertTrue(system.contains("ONE or TWO short sentences"))
        assertTrue(system.contains("Write in English."))
        val user = messages.getJSONObject(1).getString("content")
        assertTrue(user.contains("The person says: how are you?"))
        // The prompt carries the real state and the last heard words; the
        // model may quote those numbers, and nothing else.
        assertTrue(user.contains("\"tick\":42"))
        assertTrue(user.contains("\"metres\":3.5"))
        assertTrue(user.contains("last_heard"))
        assertTrue(user.contains("hello"))
        assertFalse(body.has("response_format"))
    }

    @Test
    fun `chat asks for Chinese when the shell is in Chinese`() {
        val fake = FakeHttp()
        AIBrain.http = fake
        AIBrain.chat(config, "{}", "", "你好", chinese = true)
        val body = JSONObject(fake.body!!)
        val system = body.getJSONArray("messages").getJSONObject(0).getString("content")
        assertTrue(system.contains("Write in Chinese."))
    }

    @Test
    fun `an HTTP error is an answer, not a crash`() {
        AIBrain.http = FakeHttp(code = 500, answer = """{"error":"nope"}""")
        val (reply, error) = AIBrain.chat(config, "{}", "", "hi", chinese = false)
        assertNull(reply)
        assertEquals("HTTP 500", error)
    }

    @Test
    fun `an empty model answer is an error`() {
        AIBrain.http = FakeHttp(answer = """{"choices":[{"message":{"content":"   "}}]}""")
        val (reply, error) = AIBrain.chat(config, "{}", "", "hi", chinese = false)
        assertNull(reply)
        assertEquals("The model's answer was empty", error)
    }

    @Test
    fun `a blank sentence never reaches the network`() {
        val fake = FakeHttp()
        AIBrain.http = fake
        val (reply, error) = AIBrain.chat(config, "{}", "", "   ", chinese = false)
        assertNull(reply)
        assertEquals("Say something first", error)
        assertEquals(0, fake.calls)
    }

    @Test
    fun `no endpoint is refused before any network call`() {
        val fake = FakeHttp()
        AIBrain.http = fake
        val (reply, error) = AIBrain.chat(AIConfig(), "{}", "", "hi", chinese = false)
        assertNull(reply)
        assertEquals("Set a model endpoint first", error)
        assertEquals(0, fake.calls)
    }

    @Test
    fun `the machine refuses empty input, no model and a send in flight`() {
        assertEquals(ChatVerdict.Blank, ChatMachine.ready(config, "   ", sending = false))
        assertEquals(
            ChatVerdict.NoModel,
            ChatMachine.ready(config.copy(endpoint = ""), "hello", sending = false),
        )
        assertEquals(ChatVerdict.Busy, ChatMachine.ready(config, "hello", sending = true))
        val ready = ChatMachine.ready(config, "  hello  ", sending = false)
        assertTrue(ready is ChatVerdict.Ready)
        assertEquals("hello", (ready as ChatVerdict.Ready).prompt)
    }

    @Test
    fun `configured means the switch is on and the endpoint is a real http(s) url`() {
        assertTrue(ChatMachine.configured(config))
        assertTrue(ChatMachine.configured(config.copy(endpoint = "https://model.test/v1")))
        // The AI switch is part of "configured", same as the iOS ticket.
        assertFalse(ChatMachine.configured(config.copy(enabled = false)))
        // A half-typed address, another scheme, or no endpoint is no model.
        assertFalse(ChatMachine.configured(config.copy(endpoint = "")))
        assertFalse(ChatMachine.configured(config.copy(endpoint = "model.test/v1")))
        assertFalse(ChatMachine.configured(config.copy(endpoint = "ftp://model.test/v1")))
        assertFalse(ChatMachine.configured(config.copy(endpoint = "http://")))
    }

    @Test
    fun `the manifest declares the text to speech service query`() {
        // targetSdk 30+: TextToSpeech resolves its engine through package
        // visibility. Without the TTS_SERVICE <intent> in <queries> the
        // lookup is filtered and ChatVoice.ready can stay false forever.
        val manifest = listOf(
            File("src/main/AndroidManifest.xml"),
            File("app/src/main/AndroidManifest.xml"),
            File("android/app/src/main/AndroidManifest.xml"),
        ).firstOrNull { it.exists() }
            ?: error("AndroidManifest.xml not found from ${File(".").absolutePath}")
        val text = manifest.readText()
        assertTrue(
            "manifest must declare android.intent.action.TTS_SERVICE; looked in ${manifest.absolutePath}",
            text.contains("android.intent.action.TTS_SERVICE"),
        )
        // The recognition service query must survive the edit.
        assertTrue(
            "manifest must keep the RecognitionService query",
            text.contains("android.speech.RecognitionService"),
        )
    }

    @Test
    fun `sent appends the person's line and spends the draft`() {
        val before = ChatState(draft = "hi", error = "old")
        val after = ChatMachine.sent(before, "hi")
        assertEquals(listOf(ChatLine(false, "hi")), after.lines)
        assertEquals("", after.draft)
        assertTrue(after.sending)
        assertNull(after.error)
    }

    @Test
    fun `delivered appends the duck's line only on a real answer`() {
        val sent = ChatMachine.sent(ChatState(), "hi")
        val answered = ChatMachine.delivered(sent, "  peep  ", null)
        assertEquals(listOf(ChatLine(false, "hi"), ChatLine(true, "peep")), answered.lines)
        assertFalse(answered.sending)
        assertNull(answered.error)

        val failed = ChatMachine.delivered(sent, null, "HTTP 500")
        assertEquals(listOf(ChatLine(false, "hi")), failed.lines)
        assertFalse(failed.sending)
        assertEquals("HTTP 500", failed.error)

        val empty = ChatMachine.delivered(sent, "   ", null)
        assertNotNull(empty.error)
        assertEquals(1, empty.lines.size)
    }
}
