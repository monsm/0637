package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The block editor's prop group: five one-tap actions built only from words
 * the core already knows. A prop pick may not introduce a trigger or an
 * action the program cannot run.
 */
class RulePropPicksTest {
    @Test
    fun everyPickUsesAnExistingActionWord() {
        assertEquals(5, RULE_PROP_PICKS.size)
        for (pick in RULE_PROP_PICKS) {
            assertTrue(
                "${pick.id}: ${pick.action} is not a rule action",
                pick.action in RULE_ACTIONS,
            )
            assertTrue("${pick.id}: empty value", pick.value.isNotEmpty())
        }
    }

    @Test
    fun theFivePropsMapToTheRealVerbs() {
        val byId = RULE_PROP_PICKS.associateBy { it.id }
        assertEquals("skill", byId.getValue("ball").action)
        assertEquals("kick_left", byId.getValue("ball").value)
        assertEquals("skill", byId.getValue("hoop").action)
        assertEquals("roulade", byId.getValue("hoop").value)
        assertEquals("skill", byId.getValue("block").action)
        assertEquals("ground_pick", byId.getValue("block").value)
        assertEquals("skill", byId.getValue("cushion").action)
        assertEquals("sit", byId.getValue("cushion").value)
        assertEquals("emotion", byId.getValue("mirror").action)
        assertEquals("curious", byId.getValue("mirror").value)
    }

    @Test
    fun idsAreUniqueAndComplete() {
        assertEquals(
            setOf("ball", "hoop", "block", "cushion", "mirror"),
            RULE_PROP_PICKS.map { it.id }.toSet(),
        )
    }

    @Test
    fun lightRulesTravelBetweenPhones() {
        // rules.rs ACTIONS includes 'light' and the iOS editor offers it; the
        // Android import gate must accept the same program.
        assertTrue("light" in RULE_ACTIONS)
    }
}
