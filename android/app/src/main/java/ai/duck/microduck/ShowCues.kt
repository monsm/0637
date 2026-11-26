package ai.duck.microduck

/**
 * The one-duck show: three real beats at its own props, in the order a child
 * would meet them. Each is a standing verb the duck already has, so a
 * refused beat is simply skipped and the next still runs.
 */
internal val SHOW_BEATS = listOf("roulade", "curious", "kick_left")

/**
 * A show waits for this much derived idle (care-seconds, scaled like the
 * doze clock). Long enough that it is a reward for being left alone.
 */
internal const val SHOW_IDLE_SECONDS = 240.0

/**
 * Whether the show is due: the self-play switch is on, this long idle has
 * not had its show yet, and the duck has really been alone that long.
 */
internal fun showDue(idleS: Double, enabled: Boolean, done: Boolean): Boolean =
    enabled && !done && idleS >= SHOW_IDLE_SECONDS
