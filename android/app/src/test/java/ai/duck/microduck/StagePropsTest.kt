package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The floor props' pure half: the zones a tap is read against and the cues
 * the props answer to. The animation itself is a UI concern; the contract
 * worth testing is that a body tap never becomes a prop tap, and that a
 * counter is only ever read as an increment.
 */
class StagePropsTest {
    @Test
    fun bodyZoneNeverHitsAProp() {
        // The duck's body keeps its own taps: x 0.25–0.75, y 0.33–0.68.
        for (x in listOf(0.26f, 0.50f, 0.74f)) {
            for (y in listOf(0.34f, 0.50f, 0.67f)) {
                assertNull("body zone ($x, $y) must not hit a prop", StagePropsLayout.hit(x, y))
            }
        }
    }

    @Test
    fun floorZonesHitTheirProps() {
        assertEquals(StageProp.Ball, StagePropsLayout.hit(0.10f, 0.70f))
        assertEquals(StageProp.Block, StagePropsLayout.hit(0.90f, 0.70f))
        assertEquals(StageProp.Cushion, StagePropsLayout.hit(0.50f, 0.80f))
        // Above the floor line on the same sides is still the duck's stage,
        // not a prop.
        assertNull(StagePropsLayout.hit(0.10f, 0.40f))
        assertNull(StagePropsLayout.hit(0.90f, 0.40f))
    }

    @Test
    fun cuesReadTheRealStateFields() {
        val state = """
            {
              "idle": {"takeover": 0.42, "head": [0.0, 0.0, 0.31, 0.0]},
              "life": {
                "skills": {"kick_left": 3, "kick_right": 1},
                "ground_picks": 2,
                "sit_downs": 4
              }
            }
        """.trimIndent()
        val cues = parsePropCues(state)!!
        assertEquals(0.31, cues.idleYaw, 1e-9)
        assertEquals(3, cues.kickLeft)
        assertEquals(1, cues.kickRight)
        assertEquals(2, cues.groundPicks)
        assertEquals(4, cues.sitDowns)
    }

    @Test
    fun anIdleHeadWithoutTakeoverIsNotAGlance() {
        // The head command is published every tick; only while the idle
        // look-around owns the head is it the duck looking on its own.
        val state = """{"idle": {"takeover": 0.0, "head": [0.0, 0.0, 0.9, 0.0]}}"""
        assertEquals(0.0, parsePropCues(state)!!.idleYaw, 1e-9)
    }

    @Test
    fun missingFieldsReadAsZeroNotAClaim() {
        val cues = parsePropCues("""{"life": {}}""")!!
        assertEquals(0, cues.kickLeft)
        assertEquals(0, cues.groundPicks)
        assertEquals(0, cues.sitDowns)
        assertNull(parsePropCues("not json"))
    }
}
