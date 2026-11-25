package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The floor props' pure half: the zones a tap is read against and the cues
 * the props answer to. The animation itself is a UI concern; the contract
 * worth testing is that a body tap never becomes a prop tap, that every prop
 * zone is reachable, and that the cues are read from the duck's real fields.
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
        assertEquals(StageProp.Ball, StagePropsLayout.hit(0.10f, 0.60f))
        assertEquals(StageProp.Hoop, StagePropsLayout.hit(0.10f, 0.80f))
        assertEquals(StageProp.Block, StagePropsLayout.hit(0.90f, 0.60f))
        assertEquals(StageProp.Mirror, StagePropsLayout.hit(0.90f, 0.80f))
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
              "emotion": {"name": "curious", "elapsed_s": 0.4, "duration_s": 2.0},
              "life": {
                "skills": {"kick_left": 3, "kick_right": 1, "roulade": 2},
                "ground_picks": 2,
                "sit_downs": 4
              }
            }
        """.trimIndent()
        val cues = parsePropCues(state)!!
        assertEquals(0.31, cues.idleYaw, 1e-9)
        assertEquals(3, cues.kickLeft)
        assertEquals(1, cues.kickRight)
        assertEquals(2, cues.rolls)
        assertEquals(2, cues.groundPicks)
        assertEquals(4, cues.sitDowns)
        assertEquals("curious", cues.emotion)
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
        assertEquals(0, cues.rolls)
        assertEquals("", cues.emotion)
        assertEquals(false, cues.paused)
        assertEquals(0.0, cues.idleS, 1e-9)
        assertNull(parsePropCues("not json"))
    }

    @Test
    fun theSelfPlaySwitchHasItsOwnZoneAboveTheProps() {
        assertTrue(StagePropsLayout.hitToggle(0.14f, 0.38f))
        // The prop rows and the duck's body never toggle the switch.
        assertFalse(StagePropsLayout.hitToggle(0.14f, 0.60f))
        assertFalse(StagePropsLayout.hitToggle(0.50f, 0.40f))
        // The zones do not overlap: a tap resolves to exactly one target.
        assertNull(StagePropsLayout.hit(0.14f, 0.38f))
    }

    @Test
    fun selfPlayNeedsEveryRealGuard() {
        val idle = PropCues(idleTakeover = 0.4, idleS = 20.0, tick = 5000)
        assertTrue(selfPlayDue(idle, asleep = false, enabled = true, lastTick = 0))
        // The switch off is the first guard.
        assertFalse(selfPlayDue(idle, asleep = false, enabled = false, lastTick = 0))
        // Sleeping, frozen, carried, in an emotion: none may move.
        assertFalse(selfPlayDue(idle, asleep = true, enabled = true, lastTick = 0))
        assertFalse(selfPlayDue(idle.copy(paused = true), false, true, 0))
        assertFalse(selfPlayDue(idle.copy(carrying = true), false, true, 0))
        assertFalse(selfPlayDue(idle.copy(emotion = "laugh"), false, true, 0))
        // Not looking around on its own / not left alone long enough.
        assertFalse(selfPlayDue(idle.copy(idleTakeover = 0.0), false, true, 0))
        assertFalse(selfPlayDue(idle.copy(idleS = 5.0), false, true, 0))
        // The cooldown: a beat cannot repeat inside 1250 ticks.
        assertFalse(
            selfPlayDue(idle, false, true, lastTick = idle.tick - 1249)
        )
        assertTrue(
            selfPlayDue(idle, false, true, lastTick = idle.tick - 1250)
        )
    }

    @Test
    fun selfPlayPicksAreStablePerBeatAndDuck() {
        // The same duck in the same beat keeps its choice; the next beat can
        // differ; each choice lands on a real prop.
        val a = selfPlayProp("duck-a", 3000)
        assertEquals(a, selfPlayProp("duck-a", 3000))
        assertEquals(a, selfPlayProp("duck-a", 3499))
        val picks = (0..20).map { selfPlayProp("duck-a", 1250L * it) }.toSet()
        assertTrue(picks.size > 1)
        assertTrue(picks.all { it in StageProp.entries })
        // Two ducks are not forced to the same prop.
        assertNotEquals(selfPlayProp("duck-a", 1250), selfPlayProp("duck-b", 1250))
    }
}
