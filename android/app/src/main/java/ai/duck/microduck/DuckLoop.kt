package ai.duck.microduck

import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/**
 * The 50 Hz control loop.
 *
 * It used to run from `onDrawFrame`, so the duck's control rate was the
 * display rate: a slow frame dropped ticks on a robot that is supposed to run
 * at exactly 50 Hz, and the real duck's loop does not care how fast the
 * camera renders. A dedicated scheduler keeps the tick at 20 ms; the renderer
 * only reads the latest state.
 */
class DuckLoop(private val core: DuckCore, private val input: DuckInput) {
    private val scheduler = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "duck-50hz").apply {
            priority = Thread.NORM_PRIORITY + 1
        }
    }

    private var remainder = 0.0
    /** Wall time of the last performance report, once a second. */
    private var lastReport = 0L

    fun start() {
        scheduler.scheduleAtFixedRate(
            {
                try {
                    val before = core.tickIndex()
                    // Keepalive: the core has robotd's 500 ms twist deadman.
                    core.setTwist(input.vx, 0.0, input.wz)
                    // Always tick at least once: the core drains the intent
                    // queue and publishes state on every call, so a paused duck
                    // can still receive the request that unpauses it. When
                    // paused the core does not advance the simulation.
                    var stepped = false
                    if (core.paused()) {
                        remainder = 0.0
                        if (core.tick() == 0) stepped = true
                    } else {
                        remainder += core.speed()
                        var whole = remainder.toInt()
                        remainder -= whole
                        // Zero is a real answer at 0.5x: the next frame runs
                        // the tick, and intents drain on it.
                        while (whole-- > 0) {
                            if (core.tick() == 0) stepped = true
                        }
                    }
                    // One snapshot per frame, so the renderer can blend the
                    // last two ticks instead of showing 20 ms steps.
                    val after = core.tickIndex()
                    if (stepped) core.poses.capture(core)
                    // The simulation clock's advance, not the number of
                    // scheduler firings: at 0.5x it is 25, at 2x it is 100,
                    // and a paused no-op is zero.
                    input.ticks.addAndGet((after - before).toInt().coerceAtLeast(0))
                    val now = System.nanoTime()
                    if (lastReport == 0L) lastReport = now
                    val elapsed = (now - lastReport) / 1_000_000_000.0
                    if (elapsed >= 1.0) {
                        lastReport = now
                        val frames = input.frames.getAndSet(0)
                        val ticks = input.ticks.getAndSet(0)
                        // A mode switch holds the core lock across a full
                        // model reload, and a long GC pause does the same:
                        // neither window is a frame rate. Discard it rather
                        // than publish a false low followed by a catch-up.
                        if (elapsed < 3.0) {
                            core.setPerformance(frames / elapsed, ticks / elapsed)
                        }
                    }
                } catch (_: Throwable) {
                    // A tick that throws must not stop the schedule; the next
                    // one retries.
                }
            },
            0,
            20,
            TimeUnit.MILLISECONDS,
        )
    }

    fun stop() {
        scheduler.shutdownNow()
    }
}
