package ai.duck.microduck

import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.nio.file.Files
import java.time.LocalDate

/**
 * JVM tests for the Android growth ledger (docs/PROGRESS_ANDROID.md).
 *
 * The acceptance spec's hard rules are pinned here: an empty ledger stays
 * empty, only real records unlock anything, streaks count today-or-yesterday,
 * 30 days prune while totals and unlocks never do, and a corrupt or foreign
 * file starts from zero instead of inventing a history.
 */
class ProgressStoreTest {

    private lateinit var dir: File
    private lateinit var file: File
    private var today: LocalDate = LocalDate.of(2026, 11, 16) // Monday
    private var hour: Int = 12

    private fun store() = ProgressStore(file, clock = { today }, hourOfDay = { hour })

    @Before
    fun setUp() {
        dir = Files.createTempDirectory("duck-progress-test").toFile()
        file = File(dir, "progress.json")
    }

    @After
    fun tearDown() {
        dir.deleteRecursively()
    }

    @Test
    fun `a fresh ledger is honestly empty`() {
        val snapshot = store().snapshot.value
        assertEquals(0, snapshot.streak)
        assertEquals(0, snapshot.bestStreak)
        assertEquals(0, snapshot.tasksDoneToday)
        assertFalse(snapshot.hasAnyRecord)
        assertTrue(snapshot.unlocked.isEmpty())
        assertEquals(7, snapshot.week.size)
        assertTrue(snapshot.week.none { it.active })
        assertTrue(snapshot.week.last().isToday)
        assertNull(snapshot.latestUnlock)
    }

    @Test
    fun `one real event counts and only the matching achievement unlocks`() {
        val s = store()
        s.record(ProgressEvent.Pet)
        val snapshot = s.snapshot.value
        assertEquals(1, snapshot.todayAmounts["pet"])
        assertTrue(snapshot.hasAnyRecord)
        assertNull(snapshot.latestUnlock)
        assertFalse(snapshot.unlocked.containsKey("first_feed"))
        assertFalse(snapshot.unlocked.containsKey("first_trick"))
    }

    @Test
    fun `feeding unlocks the first-meal badge with the real day key`() {
        val s = store()
        s.record(ProgressEvent.Feed)
        assertEquals(snapshotDay(s), s.snapshot.value.unlocked["first_feed"])
    }

    private fun snapshotDay(s: ProgressStore) = s.snapshot.value.week.last().id

    @Test
    fun `a three-day streak counts, unlocks, and survives an inactive today`() {
        val s = store()
        s.record(ProgressEvent.Pet)
        today = today.plusDays(1)
        s.record(ProgressEvent.Pet)
        today = today.plusDays(1)
        s.record(ProgressEvent.Pet)
        assertEquals(3, s.snapshot.value.streak)
        assertEquals(3, s.snapshot.value.bestStreak)
        assertTrue(s.snapshot.value.unlocked.containsKey("streak_3"))

        // Tomorrow has not happened yet; the streak is not broken by silence.
        today = today.plusDays(1)
        s.refresh()
        assertEquals(3, s.snapshot.value.streak)
    }

    @Test
    fun `thirty active days prune the oldest, totals and unlocks survive`() {
        val s = store()
        val first = today
        repeat(31) {
            s.record(ProgressEvent.Pet)
            today = today.plusDays(1)
        }
        val root = JSONObject(file.readText())
        val days = root.getJSONObject("days")
        assertEquals(30, days.length())
        assertFalse(days.has(first.toString()))
        assertEquals(31, root.getJSONObject("totals").getInt("pet"))
        assertTrue(s.snapshot.value.unlocked.containsKey("pet_100").not())
    }

    @Test
    fun `walking keeps the best single walk and the lifetime total`() {
        val s = store()
        s.record(ProgressEvent.Walk, 12)
        s.record(ProgressEvent.Walk, 35)
        assertTrue(s.snapshot.value.unlocked.containsKey("walk_30_single"))
        assertFalse(s.snapshot.value.unlocked.containsKey("walk_100"))
        s.record(ProgressEvent.Walk, 60)
        assertTrue(s.snapshot.value.unlocked.containsKey("walk_100"))
    }

    @Test
    fun `a hundred pets unlock the hundred-pets badge`() {
        val s = store()
        repeat(100) { s.record(ProgressEvent.Pet) }
        assertEquals(100, s.snapshot.value.todayAmounts["pet"])
        assertTrue(s.snapshot.value.unlocked.containsKey("pet_100"))
    }

    @Test
    fun `a neighbour gift unlocks only when it is recorded as applied`() {
        val s = store()
        s.record(ProgressEvent.Peer)
        assertTrue(s.snapshot.value.unlocked.containsKey("first_gift"))
        assertEquals(1, s.snapshot.value.todayAmounts["peer"])
    }

    @Test
    fun `a late-night event unlocks the late-night badge`() {
        hour = 3
        val s = store()
        s.record(ProgressEvent.Trick)
        assertTrue(s.snapshot.value.unlocked.containsKey("late_night"))
    }

    @Test
    fun `zero and negative amounts are ignored`() {
        val s = store()
        s.record(ProgressEvent.Walk, 0)
        s.record(ProgressEvent.Pet, -3)
        assertFalse(s.snapshot.value.hasAnyRecord)
        assertEquals(0, s.snapshot.value.todayAmounts["pet"])
    }

    @Test
    fun `a corrupt file starts from zero instead of guessing`() {
        file.writeText("not json at all")
        val snapshot = store().snapshot.value
        assertFalse(snapshot.hasAnyRecord)
        assertTrue(snapshot.unlocked.isEmpty())
    }

    @Test
    fun `a foreign schema starts from zero`() {
        file.writeText("""{"schemaVersion":2,"totals":{"pet":9}}""")
        val snapshot = store().snapshot.value
        assertFalse(snapshot.hasAnyRecord)
    }

    @Test
    fun `reloading keeps totals and unlocks, never re-locks`() {
        val first = store()
        first.record(ProgressEvent.Feed)
        first.record(ProgressEvent.Pet)
        val day = first.snapshot.value.week.last().id

        val reloaded = store()
        val snapshot = reloaded.snapshot.value
        assertEquals(1, snapshot.todayAmounts["feed"])
        assertEquals(1, snapshot.todayAmounts["pet"])
        assertEquals(day, snapshot.unlocked["first_feed"])
        assertTrue(snapshot.hasAnyRecord)
    }

    @Test
    fun `the week strip runs oldest to newest with today last`() {
        val s = store()
        val week = s.snapshot.value.week
        assertEquals(7, week.size)
        assertEquals(today.toString(), week.last().id)
        assertEquals(today.minusDays(6).toString(), week.first().id)
        // 2026-11-16 is a Monday: Sunday=0 … Saturday=6 → Monday is 1.
        assertEquals(1, week.last().weekday)
    }
}
