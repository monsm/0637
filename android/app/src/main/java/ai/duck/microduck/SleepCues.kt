package ai.duck.microduck

/**
 * The last little sounds before the duck dozes (`care.rs` DOZE_AFTER_S=720):
 * a settling coo at a derived idle of 600 s, a quieter chirp at 690 s. A
 * sound never touches care, so neither can postpone the doze it announces.
 */
internal const val SLEEP_CUE_COO_S = 600.0
internal const val SLEEP_CUE_CHIRP_S = 690.0

/**
 * Which settling sound the idle clock calls for now, if any. If the clock
 * jumps past both marks while the app was away, the later sound is the
 * honest one; the moment for the earlier one has passed.
 */
internal fun sleepCue(idleS: Double, cooed: Boolean, chirped: Boolean): String? = when {
    idleS >= SLEEP_CUE_CHIRP_S && !chirped -> "chirp"
    idleS >= SLEEP_CUE_COO_S && !cooed -> "coo"
    else -> null
}
