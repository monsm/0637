package ai.duck.microduck

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the pure route reducer (B7). The meeting picked this shape
 * deliberately: on the blind-change machines (Windows, no Android SDK) a
 * plain Kotlin unit test is the only navigation check that runs in minutes.
 *
 * These tests pin the four invariant families the sheet trees never had:
 * one sheet at a time, ring above sheet, back closes the top layer, and the
 * saved state is just the destination id.
 */
class DuckRouteTest {

    private fun open(state: ShellRouteState, route: DuckRoute) =
        ShellOverlayReducer.reduce(state, ShellRouteAction.Open(route))

    private fun back(state: ShellRouteState) =
        ShellOverlayReducer.reduce(state, ShellRouteAction.Back)

    @Test
    fun `opening a route replaces the current sheet instead of stacking`() {
        var state = ShellRouteState()
        state = open(state, DuckRoute.More)
        state = open(state, DuckRoute.Rules)
        assertEquals(DuckRoute.Rules, state.sheet)
        assertEquals(1, state.layerCount)
    }

    @Test
    fun `closing clears only the sheet and leaves the overlay alone`() {
        val ring = ShellRouteState(overlay = ShellOverlay.Ring)
        val withSheet = open(ring, DuckRoute.Diary)
        val closed = ShellOverlayReducer.reduce(withSheet, ShellRouteAction.CloseSheet)
        assertNull(closed.sheet)
        assertEquals(ShellOverlay.Ring, closed.overlay)
    }

    @Test
    fun `the skill ring and a sheet coexist - they are two fields, not one enum`() {
        var state = open(ShellRouteState(), DuckRoute.Instrument)
        state = ShellOverlayReducer.reduce(state, ShellRouteAction.OpenRing)
        assertEquals(DuckRoute.Instrument, state.sheet)
        assertEquals(ShellOverlay.Ring, state.overlay)
        assertEquals(2, state.layerCount)
    }

    @Test
    fun `back closes the ring first, then the sheet, then does nothing`() {
        var state = ShellRouteState(overlay = ShellOverlay.Ring, sheet = DuckRoute.Peers)

        state = back(state)
        assertEquals(ShellOverlay.None, state.overlay)
        assertEquals(DuckRoute.Peers, state.sheet)

        state = back(state)
        assertNull(state.sheet)

        val terminal = back(state)
        assertEquals(state, terminal)
    }

    @Test
    fun `the saver stores only the destination id and restores it`() {
        val state = ShellRouteState(sheet = DuckRoute.Postcards)
        val saved = state.savedId()
        assertEquals("postcards", saved)
        assertEquals(state, ShellRouteState.fromSavedId(saved))
    }

    @Test
    fun `an unknown saved id restores to no sheet, never to a crash`() {
        assertEquals(ShellRouteState(), ShellRouteState.fromSavedId("a-route-from-a-future-build"))
        assertEquals(ShellRouteState(), ShellRouteState.fromSavedId(null))
        assertEquals(ShellRouteState(), ShellRouteState.fromSavedId(""))
    }

    @Test
    fun `every route id is unique and every tag carries the route- prefix`() {
        val ids = DuckRoute.entries.map { it.id }
        assertEquals(ids.size, ids.toSet().size)
        for (route in DuckRoute.entries) {
            assertTrue("route ${route.id} lacks the route- tag", route.tag.startsWith("route-"))
        }
    }

    @Test
    fun `the registered case set matches the shell's route table`() {
        val expected = setOf(
            "instrument", "rules", "peers", "ai", "dance", "progress",
            "more", "diary", "postcards", "dex", "modes", "big",
            "play-more", "duck-panel", "tools",
        )
        assertEquals(expected, DuckRoute.entries.map { it.id }.toSet())
    }
}
