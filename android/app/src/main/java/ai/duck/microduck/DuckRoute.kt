package ai.duck.microduck

/**
 * One route table for the shell (B7, docs/UIUX_REFACTOR_2026-11-15.md §5).
 *
 * The shell used to carry a dozen independent `show*` booleans, the drawer
 * its own `Destination` enum and the instrument four more booleans. Three
 * sheet trees meant three chances to disagree — two sheets could stack, the
 * back key had to guess, and process death lost the lot. This enum is the
 * single vocabulary; [ShellOverlayReducer] is the only place that changes it.
 *
 * Overlay and sheet are deliberately two fields: the skill ring can be up
 * while a sheet is open (it floats above the chrome), and collapsing them
 * into one enum would let the ring and the drawer cancel each other.
 *
 * The case set must stay a subset of the registered list in
 * scripts/check-routes.py; every case carries a `route-` tag so per-route
 * tests can address the destination surface.
 */
enum class DuckRoute(val id: String) {
    Instrument("instrument"),
    Rules("rules"),
    Peers("peers"),
    AI("ai"),
    Dance("dance"),
    Progress("progress"),
    More("more"),
    Diary("diary"),
    Postcards("postcards"),
    Dex("dex"),
    Modes("modes"),
    Big("big"),
    PlayMore("play-more"),
    DuckPanel("duck-panel"),
    Tools("tools");

    /** The tag a per-route test looks for on the destination surface. */
    val tag: String get() = "route-$id"

    companion object {
        fun fromId(id: String?): DuckRoute? = entries.firstOrNull { it.id == id }
    }
}

enum class ShellOverlay { None, Ring }

/**
 * The whole navigation state, small enough to save. Only the sheet id is
 * persisted: the photo payload (iOS) and every controller value stay out of
 * the Saver, so process death restores the destination, never a stale image
 * or a stale core snapshot.
 */
data class ShellRouteState(
    val sheet: DuckRoute? = null,
    val overlay: ShellOverlay = ShellOverlay.None,
) {
    val layerCount: Int
        get() = (if (sheet != null) 1 else 0) + (if (overlay != ShellOverlay.None) 1 else 0)

    fun savedId(): String = sheet?.id ?: ""

    companion object {
        fun fromSavedId(id: String?): ShellRouteState =
            ShellRouteState(sheet = DuckRoute.fromId(id))
    }
}

sealed interface ShellRouteAction {
    data class Open(val route: DuckRoute) : ShellRouteAction
    data object CloseSheet : ShellRouteAction
    data object OpenRing : ShellRouteAction
    data object CloseRing : ShellRouteAction
    data object Back : ShellRouteAction
}

/**
 * The pure reducer. It is the one place route transitions happen, which is
 * why it can be unit-tested on the JVM without an emulator: open replaces the
 * sheet (one sheet at a time, no stacking), close clears the sheet, ring and
 * sheet are independent, and back closes the topmost layer — ring first so a
 * system back never leaves the app with the ring up (NV-4).
 */
object ShellOverlayReducer {
    fun reduce(state: ShellRouteState, action: ShellRouteAction): ShellRouteState = when (action) {
        is ShellRouteAction.Open -> state.copy(sheet = action.route)
        ShellRouteAction.CloseSheet -> state.copy(sheet = null)
        ShellRouteAction.OpenRing -> state.copy(overlay = ShellOverlay.Ring)
        ShellRouteAction.CloseRing -> state.copy(overlay = ShellOverlay.None)
        ShellRouteAction.Back -> when {
            state.overlay != ShellOverlay.None -> state.copy(overlay = ShellOverlay.None)
            state.sheet != null -> state.copy(sheet = null)
            else -> state
        }
    }
}
