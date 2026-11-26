package ai.duck.microduck

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The rules share/paste round-trip, with the timer's `period_s` kept. The
 * wire field name is the core's own (rules.rs Rule), so the version tag and
 * document shape stay exactly as they were.
 */
class RulesShareTest {

    private val timer = RuleUi("timer", "skill", "roulade", emptyList(), periodS = 60)

    @Test
    fun `a timer block survives share and parse with its period`() {
        val parsed = parseSharedRules(rulesShareText(listOf(timer)))
        assertEquals(listOf(timer), parsed)
    }

    @Test
    fun `the share text keeps the core field name and version`() {
        val text = rulesShareText(listOf(timer))
        val doc = JSONObject(text)
        assertEquals(1, doc.getInt("microduck-rules"))
        val item = doc.getJSONArray("rules").getJSONObject(0)
        assertEquals("timer", item.getString("trigger"))
        assertEquals(60, item.getInt("period_s"))
    }

    @Test
    fun `a bare array of timer rules parses`() {
        val text = """[{"trigger":"timer","action":"sound","value":"chirp","period_s":120}]"""
        assertEquals(listOf(RuleUi("timer", "sound", "chirp", emptyList(), 120)), parseSharedRules(text))
    }

    @Test
    fun `a timer outside 5 to 3600 seconds is skipped and counted`() {
        val short = """[{"trigger":"timer","action":"skill","value":"roulade","period_s":2}]"""
        val shortReport = parseSharedRulesImport(short)
        assertNotNull(shortReport)
        assertTrue(shortReport!!.rules.isEmpty())
        assertEquals(1, shortReport.skippedTimers)
        assertNull(parseSharedRules(short))

        val long = """[{"trigger":"timer","action":"skill","value":"roulade","period_s":4000}]"""
        assertEquals(1, parseSharedRulesImport(long)!!.skippedTimers)
    }

    @Test
    fun `a timer without period_s is skipped instead of poisoning the program`() {
        val text = """[{"trigger":"timer","action":"skill","value":"roulade"}]"""
        val report = parseSharedRulesImport(text)!!
        assertTrue(report.rules.isEmpty())
        assertEquals(1, report.skippedTimers)
    }

    @Test
    fun `valid blocks around a bad timer still import`() {
        val text = """
            [
              {"trigger":"clap","action":"sound","value":"chirp"},
              {"trigger":"timer","action":"skill","value":"roulade","period_s":1},
              {"trigger":"timer","action":"skill","value":"kick_left","period_s":30}
            ]
        """.trimIndent()
        val report = parseSharedRulesImport(text)!!
        assertEquals(2, report.rules.size)
        assertEquals(1, report.skippedTimers)
        assertEquals(30, report.rules[1].periodS)
    }

    @Test
    fun `the core's rules answer round-trips its timer period`() {
        val text = """
            {"ok":true,"rules":{"rules":[
              {"trigger":"timer","action":"skill","value":"roulade","period_s":300},
              {"trigger":"clap","action":"sound","value":"chirp"}
            ]}}
        """.trimIndent()
        val rules = parseRules(text)
        assertEquals(300, rules!![0].periodS)
        assertNull(rules[1].periodS)
        // Share then parse keeps the same program.
        assertEquals(rules, parseSharedRules(rulesShareText(rules)))
    }

    @Test
    fun `two timers that differ only in period are not duplicates`() {
        val current = listOf(timer)
        val imported = listOf(timer.copy(periodS = 120))
        val merged = mergeRules(current, imported)
        assertEquals(2, merged.size)
        assertEquals(setOf(60, 120), merged.mapNotNull { it.periodS }.toSet())
    }

    @Test
    fun `an identical timer is not duplicated`() {
        assertEquals(1, mergeRules(listOf(timer), listOf(timer.copy())).size)
    }

    @Test
    fun `a shared program may carry the lamp step`() {
        // rules.rs accepts `light` and both editors offer it; the importer
        // used to drop it on the stale belief that neither editor did, so a
        // program authored on one phone lost its lamp step on the other.
        val text = """[{"trigger":"clap","action":"light","value":"yellow"}]"""
        val imported = parseSharedRules(text)
        assertEquals(1, imported?.size)
        assertEquals("light", imported?.first()?.action)
        assertEquals("yellow", imported?.first()?.value)
    }
}
