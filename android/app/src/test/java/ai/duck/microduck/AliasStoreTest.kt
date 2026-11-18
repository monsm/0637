package ai.duck.microduck

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 教它说话's pure half (G3): the whole table encodes/decodes, a phrase is
 * at most 24 字 counted by code point, a target must be one of the names the
 * picker really offers, and the startup replay waits for the core, posts the
 * stored table exactly once, and reports a refusal instead of pretending.
 */
class AliasStoreTest {

    private class MapPrefs : StringPrefs {
        val values = mutableMapOf<String, String>()

        override fun get(key: String): String? = values[key]

        override fun put(key: String, value: String?) {
            if (value == null) values.remove(key) else values[key] = value
        }
    }

    private class FakeClient(
        var accepted: Boolean = true,
        var reason: String? = null,
    ) : AliasCoreClient {
        val calls = mutableListOf<Map<String, String>>()

        override fun save(table: Map<String, String>): AliasSaveResult {
            calls.add(table)
            return AliasSaveResult(accepted, reason)
        }
    }

    @Test
    fun `the whole table round-trips through prefs and the wire`() {
        val prefs = MapPrefs()
        val table = mapOf("转圈圈" to "roulade", "点头" to "yes")
        VoiceAliasStore.save(prefs, table)
        assertEquals(table, VoiceAliasStore.load(prefs))

        // The config body is the wrapped shape G1 reads.
        val wire = JSONObject(aliasConfigBody(table)).optJSONObject("voice_aliases")!!
        assertEquals("roulade", wire.optString("转圈圈"))
        assertEquals("yes", wire.optString("点头"))
        // Both the bare stored table and the wrapped document parse back.
        assertEquals(table, parseAliasTable(encodeAliasTable(table)))
        assertEquals(table, parseAliasTable(JSONObject(aliasConfigBody(table)).toString()))
    }

    @Test
    fun `an empty table forgets the key`() {
        val prefs = MapPrefs()
        VoiceAliasStore.save(prefs, mapOf("转圈圈" to "roulade"))
        VoiceAliasStore.save(prefs, emptyMap())
        assertNull(prefs.get(VOICE_ALIASES_KEY))
        assertTrue(VoiceAliasStore.load(prefs).isEmpty())

        // No saved table means nothing to replay, and no core is asked.
        val client = FakeClient()
        var checks = 0
        assertFalse(
            replayVoiceAliases(
                prefs = prefs,
                client = client,
                ready = { checks += 1; true },
                sleep = {},
            )
        )
        assertEquals(0, checks)
        assertTrue(client.calls.isEmpty())
    }

    @Test
    fun `a phrase is at most 24 characters, counted in unicode scalars`() {
        assertFalse(aliasPhraseTooLong("转".repeat(24)))
        assertTrue(aliasPhraseTooLong("转".repeat(25)))
        assertTrue(aliasPhraseValid("say hi"))
        assertTrue(aliasPhraseValid("  转圈圈  "))
        assertEquals("转圈圈", normalizeAliasPhrase("  转圈圈  "))
        assertFalse(aliasPhraseValid(""))
        assertFalse(aliasPhraseValid("   "))
        // One 🐤 is one scalar (two UTF-16 units); 24 of them still fit.
        assertFalse(aliasPhraseTooLong("🐤".repeat(24)))
        assertTrue(aliasPhraseTooLong("🐤".repeat(25)))
        // A joined family emoji is one grapheme but seven scalars (four
        // people and three joiners): the core counts scalars, so three
        // fit and four refuse.
        val family = "👨‍👩‍👧‍👦"
        assertFalse(aliasPhraseTooLong(family.repeat(3)))
        assertTrue(aliasPhraseTooLong(family.repeat(4)))
        assertFalse(aliasPhraseValid(family.repeat(4)))
    }

    @Test
    fun `spellings that differ only in case are one phrase`() {
        assertEquals("sit", normalizeAliasPhrase("  Sit  "))
        assertTrue(aliasPhraseValid("SIT"))
        var table = aliasWith(emptyMap(), "Sit", "roulade")
        assertEquals(mapOf("sit" to "roulade"), table)
        table = aliasWith(table, "SIT", "stand")
        assertEquals(mapOf("sit" to "stand"), table)
        // A table saved by an older build keeps its mixed-case keys on
        // disk; loading folds them, so the replay hands the core the keys
        // it holds itself.
        val prefs = MapPrefs()
        VoiceAliasStore.save(prefs, mapOf("Sit" to "roulade"))
        assertEquals(mapOf("sit" to "roulade"), VoiceAliasStore.load(prefs))
    }

    @Test
    fun `a target must be in the picker's real list`() {
        val options = aliasOptions(
            listOf("curious", "yes"),
            listOf("roulade", "kick_left"),
        )
        assertEquals(
            listOf("curious", "yes", "roulade", "kick_left", "sit", "stand", "ground_pick"),
            options,
        )
        assertTrue(aliasTargetValid("roulade", options))
        assertTrue(aliasTargetValid("sit", options))
        assertTrue(aliasTargetValid("ground_pick", options))
        assertFalse(aliasTargetValid("fly", options))
        assertFalse(aliasTargetValid("", options))
    }

    @Test
    fun `the catalog always offers thirteen moods plus the published tricks`() {
        val collection = CollectionUi(
            emotions = listOf(CollectionEntry("laugh", 1, "")),
            skills = listOf(CollectionEntry("roulade", 0, "")),
        )
        val fromBook = aliasCatalog(collection, listOf("ignored", "yes"))
        // The thirteen are the ticket's picker, even when a partially loaded
        // core published fewer mood rows; the core still judges each target.
        assertEquals(ALIAS_OFFICIAL_MOODS, fromBook.emotions)
        assertEquals(13, fromBook.emotions.size)
        assertEquals(listOf("roulade"), fromBook.skills)

        // /skills mixes moods, tricks and loop verbs; the split keeps them apart.
        val fromSkills = aliasCatalog(
            null,
            listOf("curious", "roulade", "kick_left", "sit", "stand", "ground_pick"),
        )
        assertEquals(ALIAS_OFFICIAL_MOODS, fromSkills.emotions)
        assertEquals(listOf("roulade", "kick_left"), fromSkills.skills)
    }

    @Test
    fun `adding the same phrase replaces its target`() {
        var table = aliasWith(emptyMap(), " 转圈圈 ", "roulade")
        assertEquals(mapOf("转圈圈" to "roulade"), table)
        table = aliasWith(table, "转圈圈", "sit")
        assertEquals(mapOf("转圈圈" to "sit"), table)
        assertTrue(aliasWithout(table, "转圈圈").isEmpty())
    }

    @Test
    fun `a stored table with a wrong shape is dropped, not rendered or replayed`() {
        val tooLong = "x".repeat(25)
        val parsed = parseAliasTable(
            """{"ok":true,"":"roulade","$tooLong":"sit","转圈圈":42,"点头":"yes"}"""
        )
        assertEquals(mapOf("点头" to "yes"), parsed)
        assertTrue(parseAliasTable("not json").isEmpty())
        assertTrue(parseAliasTable(null).isEmpty())
    }

    @Test
    fun `replay waits for the core and posts the stored table exactly once`() {
        val prefs = MapPrefs()
        val table = mapOf("转圈圈" to "roulade")
        VoiceAliasStore.save(prefs, table)
        var checks = 0
        val client = FakeClient()
        val ok = replayVoiceAliases(
            prefs = prefs,
            client = client,
            ready = { checks += 1; checks >= 3 },
            sleep = {},
        )
        assertTrue(ok)
        assertEquals(3, checks)
        assertEquals(listOf(table), client.calls)
    }

    @Test
    fun `a core that never answers gets no post and no lie`() {
        val prefs = MapPrefs()
        VoiceAliasStore.save(prefs, mapOf("转圈圈" to "roulade"))
        val client = FakeClient()
        var checks = 0
        assertFalse(
            replayVoiceAliases(
                prefs = prefs,
                client = client,
                ready = { checks += 1; false },
                sleep = {},
            )
        )
        assertEquals(40, checks)
        assertTrue(client.calls.isEmpty())
    }

    @Test
    fun `a refused table is reported and not retried`() {
        val prefs = MapPrefs()
        VoiceAliasStore.save(prefs, mapOf("转圈圈" to "roulade"))
        val client = FakeClient(accepted = false, reason = "no such target")
        assertFalse(
            replayVoiceAliases(
                prefs = prefs,
                client = client,
                ready = { true },
                sleep = {},
            )
        )
        assertEquals(1, client.calls.size)
    }
}
