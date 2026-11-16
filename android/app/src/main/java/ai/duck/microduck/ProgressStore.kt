package ai.duck.microduck

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.json.JSONObject
import java.io.File
import java.time.LocalDate

/**
 * The growth ledger (Android), modelled line for line on iOS
 * `ProgressKit.swift` (docs/PROGRESS_ANDROID.md is the acceptance spec).
 *
 * Six events, one real-action rule each. The store never invents history:
 * the first read of /state only sets the walk/roll baselines, nothing is
 * back-filled, and an empty ledger is reported as empty. Achievements are
 * one-way (once unlocked they never re-lock), and the last 30 active days
 * are kept while totals live forever.
 *
 * The file format matches iOS's ProgressFile: schemaVersion 1, days,
 * totals, bestWalk, bestStreak, unlocked. A file that does not parse, or
 * carries another schema, starts from zero - honestly retyped rather than
 * guessed.
 */
enum class ProgressEvent(val wire: String) {
    Pet("pet"),
    Feed("feed"),
    Trick("trick"),
    Walk("walk"),
    Photo("photo"),
    Peer("peer"),
}

/** One cell of the last-seven-days strip. weekday: 0 = Sunday … 6 = Saturday. */
data class GrowthDayTile(
    val id: String,
    val weekday: Int,
    val active: Boolean,
    val isToday: Boolean,
)

/** Everything the panel renders; pure values, safe to publish. */
data class ProgressSnapshot(
    val todayAmounts: Map<String, Int> = emptyMap(),
    val streak: Int = 0,
    val bestStreak: Int = 0,
    val tasksDoneToday: Int = 0,
    val week: List<GrowthDayTile> = emptyList(),
    val unlocked: Map<String, String> = emptyMap(),
    /** True once any real event has ever been recorded (totals are never pruned). */
    val hasAnyRecord: Boolean = false,
    val latestUnlock: String? = null,
)

/** The facts an achievement is judged against, at the moment of recording. */
data class GrowthFacts(
    val totals: Map<String, Int>,
    val bestWalk: Int,
    val bestStreak: Int,
    val isLateNight: Boolean,
) {
    fun total(event: ProgressEvent): Int = totals[event.wire] ?: 0
}

/**
 * The ten achievements, same ids and conditions as iOS. Names and details
 * come from Copy at render time so the wall follows the language switch.
 */
object GrowthAchievements {
    data class Entry(val id: String, val met: (GrowthFacts) -> Boolean)

    val catalog: List<Entry> = listOf(
        Entry("first_trick") { it.total(ProgressEvent.Trick) >= 1 },
        Entry("first_feed") { it.total(ProgressEvent.Feed) >= 1 },
        Entry("first_postcard") { it.total(ProgressEvent.Photo) >= 1 },
        Entry("first_gift") { it.total(ProgressEvent.Peer) >= 1 },
        Entry("streak_3") { it.bestStreak >= 3 },
        Entry("streak_7") { it.bestStreak >= 7 },
        Entry("walk_100") { it.total(ProgressEvent.Walk) >= 100 },
        Entry("pet_100") { it.total(ProgressEvent.Pet) >= 100 },
        Entry("late_night") { it.isLateNight },
        Entry("walk_30_single") { it.bestWalk >= 30 },
    )

    fun name(id: String): String = when (id) {
        "first_trick" -> Copy.achFirstTrickName
        "first_feed" -> Copy.achFirstFeedName
        "first_postcard" -> Copy.achFirstPostcardName
        "first_gift" -> Copy.achFirstGiftName
        "streak_3" -> Copy.achStreak3Name
        "streak_7" -> Copy.achStreak7Name
        "walk_100" -> Copy.achWalk100Name
        "pet_100" -> Copy.achPet100Name
        "late_night" -> Copy.achLateNightName
        "walk_30_single" -> Copy.achWalk30Name
        else -> id
    }

    fun detail(id: String): String = when (id) {
        "first_trick" -> Copy.achFirstTrickDetail
        "first_feed" -> Copy.achFirstFeedDetail
        "first_postcard" -> Copy.achFirstPostcardDetail
        "first_gift" -> Copy.achFirstGiftDetail
        "streak_3" -> Copy.achStreak3Detail
        "streak_7" -> Copy.achStreak7Detail
        "walk_100" -> Copy.achWalk100Detail
        "pet_100" -> Copy.achPet100Detail
        "late_night" -> Copy.achLateNightDetail
        "walk_30_single" -> Copy.achWalk30Detail
        else -> ""
    }
}

/**
 * The ledger. All mutation happens on the caller's thread under one lock;
 * snapshots are published through a StateFlow so Compose re-reads them.
 *
 * `file == null` (or an unwritable path) keeps the facts in memory: the
 * panel still works for this run and the next boot honestly starts over.
 */
class ProgressStore(
    private val file: File?,
    private val clock: () -> LocalDate = { LocalDate.now() },
    private val hourOfDay: () -> Int = { java.time.LocalTime.now().hour },
) {
    private val _snapshot = MutableStateFlow(ProgressSnapshot())
    val snapshot: StateFlow<ProgressSnapshot> = _snapshot

    private var days: MutableMap<String, MutableMap<String, Int>> = mutableMapOf()
    private var totals: MutableMap<String, Int> = mutableMapOf()
    private var bestWalk = 0
    private var bestStreak = 0
    private var unlocked: MutableMap<String, String> = mutableMapOf()
    private var latestUnlock: String? = null

    init {
        load()
        _snapshot.value = makeSnapshot()
    }

    /**
     * Record one real event. metric is the amount (metres for walk, 1 for
     * everything else); non-positive amounts are ignored, like iOS.
     */
    @Synchronized
    fun record(event: ProgressEvent, metric: Int = 1) {
        if (metric <= 0) return
        val today = clock()
        val key = today.toString()
        val day = days.getOrPut(key) { mutableMapOf() }
        day[event.wire] = (day[event.wire] ?: 0) + metric
        totals[event.wire] = (totals[event.wire] ?: 0) + metric
        if (event == ProgressEvent.Walk && metric > bestWalk) bestWalk = metric
        val streakNow = streak(today)
        if (streakNow > bestStreak) bestStreak = streakNow
        val facts = GrowthFacts(
            totals = totals.toMap(),
            bestWalk = bestWalk,
            bestStreak = bestStreak,
            isLateNight = hourOfDay() in 0..5,
        )
        for (entry in GrowthAchievements.catalog) {
            if (unlocked[entry.id] == null && entry.met(facts)) {
                unlocked[entry.id] = key
                latestUnlock = entry.id
            }
        }
        prune(today)
        save()
        _snapshot.value = makeSnapshot()
    }

    /** Recompute the snapshot (crossing midnight, panel re-opened). No disk write. */
    @Synchronized
    fun refresh() {
        _snapshot.value = makeSnapshot()
    }

    // ---- snapshot ----

    private fun makeSnapshot(): ProgressSnapshot {
        val today = clock()
        val key = today.toString()
        val day = days[key] ?: emptyMap()
        val amounts = ProgressEvent.entries.associate { it.wire to (day[it.wire] ?: 0) }
        return ProgressSnapshot(
            todayAmounts = amounts,
            streak = streak(today),
            bestStreak = bestStreak,
            tasksDoneToday = GrowthTasks.catalog.count { (amounts[it.event.wire] ?: 0) >= it.goal },
            week = weekTiles(today),
            unlocked = unlocked.toMap(),
            hasAnyRecord = totals.isNotEmpty(),
            latestUnlock = latestUnlock,
        )
    }

    /** Today counts if active; otherwise yesterday counts (today is not over). */
    private fun streak(asOf: LocalDate): Int {
        var day = asOf
        if (days[day.toString()] == null) day = day.minusDays(1)
        var count = 0
        while (count < 400 && days[day.toString()] != null) {
            count += 1
            day = day.minusDays(1)
        }
        return count
    }

    /** Last seven days, oldest first, including today. */
    private fun weekTiles(today: LocalDate): List<GrowthDayTile> {
        val tiles = mutableListOf<GrowthDayTile>()
        for (offset in 6 downTo 0) {
            val date = today.minusDays(offset.toLong())
            val key = date.toString()
            // java.time: Monday=1..Sunday=7; the tiles use Sunday=0..Saturday=6.
            val weekday = date.dayOfWeek.value % 7
            tiles += GrowthDayTile(
                id = key,
                weekday = weekday,
                active = days[key] != null,
                isToday = offset == 0,
            )
        }
        return tiles
    }

    /** Keep the newest 30 active days; totals, bests and unlocks never prune. */
    private fun prune(today: LocalDate) {
        if (days.size <= 30) return
        val kept = days.keys.sorted().takeLast(30).associateWith { days.getValue(it) }
        days = kept.toMutableMap()
    }

    // ---- disk ----

    private fun load() {
        val f = file ?: return
        if (!f.exists()) return
        try {
            val root = JSONObject(f.readText())
            if (root.optInt("schemaVersion", 0) != 1) return
            root.optJSONObject("days")?.let { dayMap ->
                for (key in dayMap.keys()) {
                    val record = dayMap.optJSONObject(key) ?: continue
                    val amounts = mutableMapOf<String, Int>()
                    for (wire in record.keys()) {
                        amounts[wire] = record.optInt(wire, 0)
                    }
                    days[key] = amounts
                }
            }
            root.optJSONObject("totals")?.let { totalMap ->
                for (wire in totalMap.keys()) {
                    totals[wire] = totalMap.optInt(wire, 0)
                }
            }
            bestWalk = root.optInt("bestWalk", 0)
            bestStreak = root.optInt("bestStreak", 0)
            root.optJSONObject("unlocked")?.let { unlockedMap ->
                for (id in unlockedMap.keys()) {
                    unlocked[id] = unlockedMap.optString(id, "")
                }
            }
        } catch (_: Throwable) {
            // Corrupt or foreign file: start from zero, never guess a history.
            days = mutableMapOf()
            totals = mutableMapOf()
            bestWalk = 0
            bestStreak = 0
            unlocked = mutableMapOf()
            latestUnlock = null
        }
    }

    private fun save() {
        val f = file ?: return
        try {
            f.parentFile?.mkdirs()
            val root = JSONObject()
            root.put("schemaVersion", 1)
            val dayMap = JSONObject()
            for ((key, amounts) in days) {
                val record = JSONObject()
                for ((wire, amount) in amounts) record.put(wire, amount)
                dayMap.put(key, record)
            }
            root.put("days", dayMap)
            root.put("totals", JSONObject(totals as Map<*, *>))
            root.put("bestWalk", bestWalk)
            root.put("bestStreak", bestStreak)
            root.put("unlocked", JSONObject(unlocked as Map<*, *>))
            val tmp = File(f.path + ".tmp")
            tmp.writeText(root.toString())
            if (!tmp.renameTo(f)) {
                f.delete()
                tmp.renameTo(f)
            }
        } catch (_: Throwable) {
            // A failed write loses this increment only; memory still has it.
        }
    }
}

/** The three daily tasks, same ids/goals as iOS's DailyTask.catalog. */
data class GrowthTask(val event: ProgressEvent, val goal: Int)

object GrowthTasks {
    val catalog = listOf(
        GrowthTask(ProgressEvent.Pet, 3),
        GrowthTask(ProgressEvent.Feed, 1),
        GrowthTask(ProgressEvent.Walk, 10),
    )
}

/**
 * Process-wide instance, initialized once from the activity. Recording
 * before init is a no-op rather than a crash: the shell never records
 * before the app is up.
 */
object DuckProgress {
    @Volatile
    private var instance: ProgressStore? = null

    fun init(file: File?) {
        if (instance == null) instance = ProgressStore(file)
    }

    fun record(event: ProgressEvent, metric: Int = 1) {
        instance?.record(event, metric)
    }

    fun refresh() {
        instance?.refresh()
    }

    val snapshot: StateFlow<ProgressSnapshot>
        get() = instance?.snapshot ?: fallback

    private val fallback = MutableStateFlow(ProgressSnapshot())
}
