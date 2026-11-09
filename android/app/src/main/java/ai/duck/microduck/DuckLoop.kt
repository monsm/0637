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

    fun start() {
        scheduler.scheduleAtFixedRate(
            {
                try {
                    // Keepalive: the core has robotd's 500 ms twist deadman.
                    core.setTwist(input.vx, 0.0, input.wz)
                    // Always tick at least once: the core drains the intent
                    // queue and publishes state on every call, so a paused duck
                    // can still receive the request that unpauses it. When
                    // paused the core does not advance the simulation.
                    if (core.paused()) {
                        remainder = 0.0
                        core.tick()
                    } else {
                        remainder += core.speed()
                        var whole = remainder.toInt()
                        remainder -= whole
                        if (whole < 1) whole = 1
                        while (whole-- > 0) {
                            core.tick()
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
