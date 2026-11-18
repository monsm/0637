package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The walk slot's shell-side memory: the core's override is process memory,
 * so the name that must survive a restart is written here. These tests pin
 * the preference round-trip and the replay decision. `"none"` is the one
 * value the official walk slot refuses ("the walking policy cannot be
 * switched off"), and the core now answers it with 400; the shell therefore
 * never stores or replays it, and an archive from an older build is deleted
 * rather than sent.
 */
class WalkPolicyTest {

    private class MapPrefs : StringPrefs {
        val values = mutableMapOf<String, String>()

        override fun get(key: String): String? = values[key]

        override fun put(key: String, value: String?) {
            if (value == null) values.remove(key) else values[key] = value
        }
    }

    @Test
    fun `a saved walk policy round-trips exactly`() {
        val prefs = MapPrefs()
        WalkPolicyStore.save(prefs, "velstand.onnx")
        assertEquals("velstand.onnx", WalkPolicyStore.load(prefs))
        assertEquals(
            "velstand.onnx",
            WalkPolicyStore.replayTarget(WalkPolicyStore.load(prefs)),
        )
    }

    @Test
    fun `none is not stored, it clears the saved override`() {
        val prefs = MapPrefs()
        WalkPolicyStore.save(prefs, "velstand.onnx")
        WalkPolicyStore.save(prefs, "none")
        assertNull("the core refuses walk=none, so it must not be kept", WalkPolicyStore.load(prefs))
        assertNull(WalkPolicyStore.replayTarget("none"))
    }

    @Test
    fun `the sentinel is recognised case-insensitively`() {
        assertTrue(isWalkNone("none"))
        assertTrue(isWalkNone("NoNe"))
        assertNull(normalizeWalkPolicy("NONE"))
        // A real name is not mangled on the way in.
        assertEquals("velstand.onnx", normalizeWalkPolicy("velstand.onnx"))
    }

    @Test
    fun `a legacy stored none is deleted instead of replayed`() {
        val prefs = MapPrefs()
        // What a build from before the core's 400 would have left behind.
        prefs.values[WALK_POLICY_KEY] = "none"
        assertFalse(replayWalkPolicy(prefs))
        assertNull(prefs.get(WALK_POLICY_KEY))
    }

    @Test
    fun `null, blank or none forgets the override`() {
        val prefs = MapPrefs()
        WalkPolicyStore.save(prefs, "a.onnx")
        WalkPolicyStore.save(prefs, null)
        assertNull(WalkPolicyStore.load(prefs))

        WalkPolicyStore.save(prefs, "a.onnx")
        WalkPolicyStore.save(prefs, "   ")
        assertNull(WalkPolicyStore.load(prefs))

        WalkPolicyStore.save(prefs, "a.onnx")
        WalkPolicyStore.save(prefs, "NONE")
        assertNull(WalkPolicyStore.load(prefs))
    }

    @Test
    fun `nothing saved means nothing to replay`() {
        assertNull(WalkPolicyStore.replayTarget(null))
        assertNull(WalkPolicyStore.replayTarget(""))
        assertNull(WalkPolicyStore.replayTarget("  "))
    }

    @Test
    fun `a stored name needs a reload to land, the none sentinel does not`() {
        assertTrue(walkPolicyNeedsReload("velstand.onnx"))
        assertFalse(walkPolicyNeedsReload("none"))
        assertFalse(walkPolicyNeedsReload("NONE"))
        assertFalse(walkPolicyNeedsReload(""))
    }
}
