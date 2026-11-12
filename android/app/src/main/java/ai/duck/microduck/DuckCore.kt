package ai.duck.microduck

import java.io.File

/**
 * Kotlin wrapper over the Rust core. Every call lands on the same C ABI the
 * iOS app uses; the native library is `libduck_mobile_core.so`.
 */
class DuckCore(
    scenePath: String,
    private val policiesPath: String,
) : AutoCloseable {
    private var handle: Long = nativeNew(scenePath, policiesPath)
    private val lock = Any()

    /**
     * The last two ticks' body poses. The loop writes, the renderer reads;
     * the GL thread never touches MuJoCo.
     */
    val poses = PoseHistory()

    val ready: Boolean get() = handle != 0L

    fun paused(): Boolean = synchronized(lock) {
        if (handle != 0L) nativePaused(handle) else false
    }

    fun speed(): Double = synchronized(lock) {
        if (handle != 0L) nativeSpeed(handle) else 1.0
    }

    fun setPaused(value: Boolean) = synchronized(lock) {
        if (handle != 0L) nativeSetPaused(handle, value)
    }

    fun setSpeed(value: Double) = synchronized(lock) {
        if (handle != 0L) nativeSetSpeed(handle, value)
    }

    fun step(ticks: Int = 1) = synchronized(lock) {
        if (handle != 0L) nativeStep(handle, ticks)
    }

    fun seek(backTicks: Int) = synchronized(lock) {
        if (handle != 0L) nativeSeek(handle, backTicks)
    }

    fun fork(label: String? = null) = synchronized(lock) {
        if (handle != 0L) nativeFork(handle, label)
    }

    fun tick(): Int = synchronized(lock) {
        if (handle != 0L) nativeTick(handle) else -1
    }

    fun setTwist(vx: Double, vy: Double, wz: Double) = synchronized(lock) {
        if (handle != 0L) nativeSetTwist(handle, vx, vy, wz)
    }

    fun setHead(neck: Double, head: Double, yaw: Double, roll: Double) = synchronized(lock) {
        if (handle != 0L) nativeSetHead(handle, neck, head, yaw, roll)
    }

    fun setBody(z: Double, roll: Double, pitch: Double) = synchronized(lock) {
        if (handle != 0L) nativeSetBody(handle, z, roll, pitch)
    }

    /** Let go of the body pose; while one is held the controller zeroes the twist. */
    fun releaseBody() = synchronized(lock) {
        if (handle != 0L) nativeReleaseBody(handle)
    }

    /** A hand on the duck: metres from where it was grabbed; [active] false lets go. */
    fun carry(dx: Double, dy: Double, dz: Double, active: Boolean) = synchronized(lock) {
        if (handle != 0L) nativeCarry(handle, dx, dy, dz, active)
    }

    /** Ride the duck: the joyride hold. [hold] true grabs it, false lets go. */
    fun wheee(hold: Boolean) = synchronized(lock) {
        if (handle != 0L) nativeWheee(handle, hold)
    }

    /** Ask for a sound by tag; the voice thread plays it. */
    fun queueSound(tag: String): Int = synchronized(lock) {
        if (handle != 0L) nativeQueueSound(handle, tag) else -1
    }

    /** The next sound the duck wants, or null. The platform owns the speaker. */
    fun nextSound(): String? = synchronized(lock) {
        if (handle != 0L) nativeNextSound(handle) else null
    }

    /** One sound as mono f32 PCM at [voiceRate]. */
    fun renderSound(tag: String): FloatArray = synchronized(lock) {
        if (handle != 0L) nativeRenderSound(handle, tag) ?: FloatArray(0) else FloatArray(0)
    }

    /** The simulation clock's tick, read under the same lock as [tick]. */
    fun tickIndex(): Long = synchronized(lock) {
        if (handle != 0L) nativeTickIndex(handle) else 0L
    }

    /** The oldest checkpoint the core still retains, for the rail's span. */
    fun oldestTick(): Long = synchronized(lock) {
        if (handle != 0L) nativeOldestTick(handle) else 0L
    }

    /** The platform's own measurements, once a second: fps and ticks/s. */
    fun setPerformance(fps: Double, tps: Double) = synchronized(lock) {
        if (handle != 0L) nativeSetPerformance(handle, fps, tps)
    }

    /** The drive mode loaded now: "walk" or "roller". */
    fun mode(): String = synchronized(lock) {
        if (handle != 0L) nativeMode(handle) ?: "walk" else "walk"
    }

    /**
     * The token a client that is not this phone has to send. The app never
     * needs it — it reaches the duck over the loopback address — but a tool
     * on another machine does, and the sheet shows it.
     */
    fun intentToken(): String = synchronized(lock) {
        if (handle != 0L) nativeIntentToken(handle) ?: "" else ""
    }

    /** Bumped when the body model changed (a mode switch). */
    fun modelGeneration(): Long = synchronized(lock) {
        if (handle != 0L) nativeModelGeneration(handle) else 0L
    }

    /** Switch drive bundles; 0 on success. */
    fun setMode(mode: String): Int = synchronized(lock) {
        if (handle != 0L) nativeSetMode(handle, mode) else -1
    }

    /** The real duck's emotion timelines and voices; returns how many loaded. */
    fun loadEmotions(path: String): Int = synchronized(lock) {
        if (handle != 0L) nativeLoadEmotions(handle, path) else -1
    }

    /** The pickup classifier `robotd` scores. Returns 0 when it loaded. */
    fun loadPickup(path: String): Int = synchronized(lock) {
        if (handle != 0L) nativeLoadPickup(handle, path) else -1
    }

    fun skill(name: String): Int = synchronized(lock) {
        if (handle != 0L) nativeSkill(handle, name) else 0
    }

    fun sit(): Int = synchronized(lock) {
        if (handle != 0L) nativeSit(handle) else -1
    }

    fun groundPick(): Int = synchronized(lock) {
        if (handle != 0L) nativeGroundPick(handle) else -1
    }

    fun push(vx: Double, vy: Double, vz: Double): Int = synchronized(lock) {
        if (handle != 0L) nativePush(handle, vx, vy, vz) else -1
    }

    /**
     * The face's flashlight, `robot.flashlight` on a phone: [toggle] flips it,
     * [on] sets it outright, [color] is one of robotd's seven (white, red,
     * green, blue, yellow, cyan, magenta). Returns the new state: 0 off,
     * 1..7 the colour's index plus one, -1 when there is no core.
     */
    fun flashlight(toggle: Boolean, on: Boolean, color: String): Int = synchronized(lock) {
        if (handle != 0L) nativeFlashlight(handle, toggle, on, color) else -1
    }

    fun startIntentServer(port: Int): Int = synchronized(lock) {
        if (handle != 0L) nativeStartIntentServer(handle, port) else -1
    }

    fun reload(directory: String? = null): Int = synchronized(lock) {
        val target = directory ?: policiesPath
        if (handle != 0L) nativeReload(handle, target) else -1
    }

    /**
     * [bringup, fallen, gain, trunk x/y/z, gravity x/y/z, imu_ready, picked_up,
     * p_held, mouth, volts]
     */
    fun status(): DoubleArray = synchronized(lock) {
        if (handle != 0L) nativeStatus(handle) ?: DoubleArray(0) else DoubleArray(0)
    }

    /** 7 trunk + 15 wire joints. */
    fun renderQpos(): DoubleArray = synchronized(lock) {
        if (handle != 0L) nativeRenderQpos(handle) ?: DoubleArray(0) else DoubleArray(0)
    }

    fun bodyCount(): Int = synchronized(lock) {
        if (handle != 0L) nativeBodyCount(handle) else 0
    }

    /** [x, y, z, qw, qx, qy, qz] */
    fun bodyPose(body: Int): DoubleArray? = synchronized(lock) {
        if (handle != 0L) nativeBodyPose(handle, body) else null
    }

    fun geomCount(): Int = synchronized(lock) {
        if (handle != 0L) nativeGeomCount(handle) else 0
    }

    /** [body, type, mesh, group, pos x/y/z, quat w/x/y/z, rgba r/g/b/a] */
    fun geomInfo(geom: Int): DoubleArray? = synchronized(lock) {
        if (handle != 0L) nativeGeomInfo(handle, geom) else null
    }

    fun skillCount(): Int = synchronized(lock) {
        if (handle != 0L) nativeSkillCount(handle) else 0
    }

    fun skillName(index: Int): String? = synchronized(lock) {
        if (handle != 0L) nativeSkillName(handle, index) else null
    }

    fun meshName(mesh: Int): String? = synchronized(lock) {
        if (handle != 0L) nativeMeshName(handle, mesh) else null
    }

    /** Triangle soup (px,py,pz,nx,ny,nz) in MuJoCo's own mesh frame. */
    fun meshSoup(mesh: Int): FloatArray? = synchronized(lock) {
        if (handle != 0L) nativeMeshSoup(handle, mesh) else null
    }

    override fun close() {
        synchronized(lock) {
            if (handle != 0L) {
                nativeFree(handle)
                handle = 0L
            }
        }
    }

    companion object {
        /** The voice's sample rate: the robot synth's native 48 kHz. */
        fun voiceRate(): Int = nativeVoiceRate()

        init {
            // ONNX Runtime is loaded dynamically by the Rust core; preload the
            // bundled lib so a bare dlopen("libonnxruntime.so") finds it.
            runCatching { System.loadLibrary("onnxruntime") }
            System.loadLibrary("duck_mobile_core")
        }

        @JvmStatic private external fun nativeNew(scene: String, policies: String): Long
        @JvmStatic private external fun nativeFree(handle: Long)
        @JvmStatic private external fun nativeTick(handle: Long): Int
        @JvmStatic private external fun nativeTickIndex(handle: Long): Long
        @JvmStatic private external fun nativeOldestTick(handle: Long): Long
        @JvmStatic private external fun nativePaused(handle: Long): Boolean
        @JvmStatic private external fun nativeSpeed(handle: Long): Double
        @JvmStatic private external fun nativeSetPaused(handle: Long, paused: Boolean)
        @JvmStatic private external fun nativeSetSpeed(handle: Long, speed: Double)
        @JvmStatic private external fun nativeStep(handle: Long, ticks: Int)
        @JvmStatic private external fun nativeSeek(handle: Long, backTicks: Int)
        @JvmStatic private external fun nativeFork(handle: Long, label: String?)
        @JvmStatic private external fun nativeSetTwist(handle: Long, vx: Double, vy: Double, wz: Double)
        @JvmStatic private external fun nativeSetHead(handle: Long, neck: Double, head: Double, yaw: Double, roll: Double)
        @JvmStatic private external fun nativeSetBody(handle: Long, z: Double, roll: Double, pitch: Double)
        @JvmStatic private external fun nativeReleaseBody(handle: Long)
        @JvmStatic private external fun nativeCarry(handle: Long, dx: Double, dy: Double, dz: Double, active: Boolean)
        @JvmStatic private external fun nativeWheee(handle: Long, hold: Boolean)
        @JvmStatic private external fun nativeLoadEmotions(handle: Long, path: String): Int
        @JvmStatic private external fun nativeLoadPickup(handle: Long, path: String): Int
        @JvmStatic private external fun nativeSetPerformance(handle: Long, fps: Double, tps: Double)
        @JvmStatic private external fun nativeMode(handle: Long): String?
        @JvmStatic private external fun nativeIntentToken(handle: Long): String?
        @JvmStatic private external fun nativeSetMode(handle: Long, mode: String): Int
        @JvmStatic private external fun nativeModelGeneration(handle: Long): Long
        @JvmStatic private external fun nativeQueueSound(handle: Long, tag: String): Int
        @JvmStatic private external fun nativeNextSound(handle: Long): String?
        @JvmStatic private external fun nativeRenderSound(handle: Long, tag: String): FloatArray?
        @JvmStatic private external fun nativeVoiceRate(): Int
        @JvmStatic private external fun nativeSkill(handle: Long, name: String): Int
        @JvmStatic private external fun nativeSit(handle: Long): Int
        @JvmStatic private external fun nativeGroundPick(handle: Long): Int
        @JvmStatic private external fun nativePush(handle: Long, vx: Double, vy: Double, vz: Double): Int
        @JvmStatic private external fun nativeFlashlight(handle: Long, toggle: Boolean, on: Boolean, color: String): Int
        @JvmStatic private external fun nativeStartIntentServer(handle: Long, port: Int): Int
        @JvmStatic private external fun nativeReload(handle: Long, directory: String): Int
        @JvmStatic private external fun nativeStatus(handle: Long): DoubleArray?
        @JvmStatic private external fun nativeRenderQpos(handle: Long): DoubleArray?
        @JvmStatic private external fun nativeBodyCount(handle: Long): Int
        @JvmStatic private external fun nativeBodyPose(handle: Long, body: Int): DoubleArray?
        @JvmStatic private external fun nativeGeomCount(handle: Long): Int
        @JvmStatic private external fun nativeGeomInfo(handle: Long, geom: Int): DoubleArray?
        @JvmStatic private external fun nativeSkillCount(handle: Long): Int
        @JvmStatic private external fun nativeSkillName(handle: Long, index: Int): String?
        @JvmStatic private external fun nativeMeshName(handle: Long, mesh: Int): String?
        @JvmStatic private external fun nativeMeshSoup(handle: Long, mesh: Int): FloatArray?
    }
}

/** Staging the APK assets into a real directory MuJoCo can read. */
object AssetStager {
    fun stage(context: android.content.Context): File {
        val root = File(context.filesDir, "duck")
        val marker = File(root, "robot/microduck/scene_backlash.xml")
        val pickup = File(root, "models/pickup_detector.onnx")
        // The emotion timelines are part of the marker: an install upgraded
        // from a build without them would otherwise keep the old filesDir
        // and silently lose all 13 skills.
        val emotions = File(root, "emotions/curious.json")
        if (marker.exists() && pickup.exists() && emotions.exists()) return root
        copy(context, "robot", File(root, "robot"))
        copy(context, "policies", File(root, "policies"))
        // The pickup classifier ships beside the policies but is not one.
        if (!context.assets.list("models").isNullOrEmpty()) {
            copy(context, "models", File(root, "models"))
        }
        // `robotd/emotions/`: thirteen keyframe timelines and their voices.
        if (!context.assets.list("emotions").isNullOrEmpty()) {
            copy(context, "emotions", File(root, "emotions"))
        }
        return root
    }

    private fun copy(context: android.content.Context, assetPath: String, target: File) {
        val children = context.assets.list(assetPath).orEmpty()
        if (children.isEmpty()) {
            target.parentFile?.mkdirs()
            context.assets.open(assetPath).use { input ->
                target.outputStream().use { output -> input.copyTo(output) }
            }
            return
        }
        target.mkdirs()
        for (child in children) {
            copy(context, "$assetPath/$child", File(target, child))
        }
    }
}

/** The joystick's latest command, read by the GL tick. */
class DuckInput {
    @Volatile var vx: Double = 0.0
    @Volatile var wz: Double = 0.0
    /** Director mode with a live camera: the renderer goes transparent. */
    @Volatile var cameraActive: Boolean = false
    /**
     * The instrument sheet is up: the stage is the lab, and the floor draws
     * the cold checkerboard there (STAGE_LOOK §3). Presentation only — no
     * frame or command here reaches the duck's senses.
     */
    @Volatile var instrumentOpen: Boolean = false
    // Performance counters, reset once a second. The loop counts achieved
    // simulation ticks, the renderer counts frames on its own thread: an
    // atomic getAndSet is needed or a frame drawn between the read and the
    // reset is lost.
    val ticks = java.util.concurrent.atomic.AtomicInteger(0)
    val frames = java.util.concurrent.atomic.AtomicInteger(0)
}
