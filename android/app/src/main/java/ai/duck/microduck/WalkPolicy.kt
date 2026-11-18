package ai.duck.microduck

import android.content.Context
import org.json.JSONObject

/**
 * The walk slot the shell last handed the core, kept across process death.
 *
 * The core's `POST /config {"walk_policy": name}` is a process global: the
 * official robotd writes its `[policy] walk` override into robotd.toml and a
 * load survives a reboot, while the phone's core has no config file. The
 * shell is the layer that outlives the core, so the name lives in the same
 * "duck" preferences as the companion state and is replayed once the core
 * is ready.
 *
 * `"none"` is not stored and never replayed: walk is the one slot the
 * official config refuses to switch off (`robotd/src/main.rs`'
 * `loadPolicy`, "the walking policy cannot be switched off"), and the core
 * now answers that request with 400. A caller asking for "none" is read as
 * "forget the saved override", which is the shell-side clear.
 *
 * The key string is the same one iOS's `WalkPolicyStore.defaultsKey` uses,
 * so the pair is greppable; the stores themselves are separate
 * (SharedPreferences and UserDefaults).
 */
internal const val WALK_POLICY_KEY = "duck_walk_policy_v1"

/** The official sentinel: the walk slot cannot be switched off. */
internal fun isWalkNone(name: String): Boolean =
    name.equals("none", ignoreCase = true)

/**
 * What the shell may store: a real file name. `"none"` and blank mean
 * "clear the override", so they are not values to remember.
 */
internal fun normalizeWalkPolicy(name: String?): String? =
    name?.takeIf { it.isNotBlank() && !isWalkNone(it) }

/**
 * The slice of SharedPreferences this store needs. The JVM unit tests use a
 * map-backed implementation; the app uses the real preferences.
 */
internal interface StringPrefs {
    fun get(key: String): String?
    fun put(key: String, value: String?)
}

private class SharedStringPrefs(context: Context) : StringPrefs {
    private val prefs = context.getSharedPreferences("duck", Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)

    override fun put(key: String, value: String?) {
        val edit = prefs.edit()
        if (value == null) edit.remove(key) else edit.putString(key, value)
        edit.apply()
    }
}

internal object WalkPolicyStore {
    fun prefs(context: Context): StringPrefs = SharedStringPrefs(context)

    fun load(prefs: StringPrefs): String? = prefs.get(WALK_POLICY_KEY)

    /**
     * Remember a real file name exactly; null, blank or `"none"` forgets the
     * override. The last is the official rule, not a shell invention: walk
     * cannot be switched off.
     */
    fun save(prefs: StringPrefs, name: String?) {
        prefs.put(WALK_POLICY_KEY, normalizeWalkPolicy(name))
    }

    /**
     * What the startup replay should send, or null when there is nothing to
     * send. An archive from a build that stored `"none"` normalizes to null
     * here, so it cannot reach the core's 400.
     */
    fun replayTarget(saved: String?): String? = normalizeWalkPolicy(saved)
}

/**
 * Whether a replayed name has to be followed by a reload to take effect.
 * `/config` only writes the core's process-global override, which the set
 * resolution reads on its next load. Normalized targets never carry
 * `"none"`; the guard stays so a stray sentinel can never trigger a reload.
 */
internal fun walkPolicyNeedsReload(target: String): Boolean =
    target.isNotBlank() && !isWalkNone(target)

/**
 * Remember the walk policy for the next launch and set it now. A shell-side
 * caller that picks a different walk file goes through here; the core's
 * process-global override alone would be gone after a restart. When a live
 * core is passed, the override is followed by the reload it needs to land.
 *
 * A request for `"none"` is the official "cannot be switched off" case: the
 * shell cannot store or send it (the core answers 400), so it means "forget
 * the saved walk override" and succeeds without touching a running core.
 */
internal fun setWalkPolicy(context: Context, name: String?, core: DuckCore? = null): Boolean {
    val prefs = WalkPolicyStore.prefs(context)
    val target = normalizeWalkPolicy(name)
    if (target == null) {
        WalkPolicyStore.save(prefs, null)
        return true
    }
    return try {
        val accepted = postJson(
            "/config",
            JSONObject().put("walk_policy", target).toString(),
        )
        if (accepted) WalkPolicyStore.save(prefs, target)
        if (!accepted || core == null || !walkPolicyNeedsReload(target)) {
            accepted
        } else {
            core.reload() == 0
        }
    } catch (_: Throwable) {
        false
    }
}

/**
 * Hand a fresh core the walk slot it had last launch, the way robotd reads
 * its robotd.toml. Waits until /state is published (the tick that drains the
 * queue) and gives up after about ten seconds rather than pretending. The
 * override only lands on a reload, so a real name gets one; the core resolves
 * it exactly as it would at boot and drops a file that is no longer there.
 * A refusal is logged and returned, not retried: the core's answer is final.
 *
 * A `"none"` left behind by an older build is normalized away and deleted
 * here — never posted, never reloaded, because the core now refuses it.
 */
internal fun replayWalkPolicy(prefs: StringPrefs, core: DuckCore? = null): Boolean {
    val stored = WalkPolicyStore.load(prefs)
    val target = WalkPolicyStore.replayTarget(stored) ?: run {
        if (stored != null) WalkPolicyStore.save(prefs, null)
        return false
    }
    for (attempt in 0 until 40) {
        try {
            if (fetchRawState() == null) throw IllegalStateException("state not ready")
            val body = JSONObject().put("walk_policy", target).toString()
            if (!postJson("/config", body)) {
                android.util.Log.w("duck-policy", "the core refused the saved walk policy $target")
                return false
            }
            android.util.Log.i("duck-policy", "replayed walk policy $target")
            // The core resolved its walk slot inside duck_core_new; /config
            // writes the override, and only a reload reads it back.
            if (core != null && walkPolicyNeedsReload(target)) {
                val loaded = core.reload()
                android.util.Log.i("duck-policy", "walk policy $target reload -> $loaded")
                return loaded == 0
            }
            return true
        } catch (_: Throwable) {
            try {
                Thread.sleep(250)
            } catch (_: InterruptedException) {
                return false
            }
        }
    }
    android.util.Log.w("duck-policy", "walk policy $target was not replayed: core not ready")
    return false
}

internal fun replayWalkPolicy(context: Context, core: DuckCore? = null): Boolean =
    replayWalkPolicy(WalkPolicyStore.prefs(context), core)
