package ai.duck.microduck

/** D7: a debug-only recomposition counter. The shell body increments it on
 * every composition; one coroutine logs the running total. The smoke reads
 * the log — the number is recorded first, the ceiling is set from real
 * runs, and nothing here ships a fake budget. */
object DuckPerf {
    @Volatile var shellCompositions: Long = 0
}
