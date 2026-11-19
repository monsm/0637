package ai.duck.microduck

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.media.Image
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import androidx.core.content.ContextCompat
import java.io.File
import java.nio.ByteOrder
import java.util.Locale
import java.util.Random
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sqrt

/**
 * Depth obstacles (#4) and floor anchoring (#20) without ARCore.
 *
 * The ticket offered two routes and asked for a written choice. This is the
 * no-new-dependency route:
 *
 * - **No ARCore / no CameraX Depth.** ARCore Depth means the ARCore Device
 *   API plus Play Services for AR, an APK that grows for a capability CI and
 *   an emulator can never exercise, and a session this app does not otherwise
 *   need. The app already holds CAMERA for Director; the platform's Camera2
 *   depth-output capability exposes the same DEPTH16 frames on phones whose
 *   rear depth camera is visible to third-party apps (`ImageFormat.DEPTH16`,
 *   `REQUEST_AVAILABLE_CAPABILITIES_DEPTH_OUTPUT`, both API 23+; minSdk 29).
 *
 * - **Honest absence.** Most phones do not expose a depth camera this way, a
 *   first launch has not been granted CAMERA yet, and CI has no camera at
 *   all. In every one of those cases nothing here invents a room: no
 *   obstacles.xml is written, no /floor is posted, and the Tools row says
 *   Copy.depthUnsupported when the hardware has no depth sensor.
 *
 * - **Desk / eye-level estimate.** One depth frame is turned into a point
 *   cloud in camera coordinates. A deterministic RANSAC fit finds the
 *   dominant plane; it is accepted as the floor only when it is on the
 *   camera's side, 5 cm..2 m away, and its normal lies mostly across the
 *   optical axis (a floor seen at eye level; a wall with |nz| ≈ 1 is
 *   refused). Up to three point clusters above that plane become
 *   the AABBs written to `filesDir/duck/robot/microduck/obstacles.xml` before
 *   the core is constructed. The file is the same shape iOS writes: a full
 *   `<mujoco>` document whose `<worldbody>` is the fragment J1 merges with
 *   the official scene (a bare `<body>` fragment would not parse as its own
 *   document). The plane's presence is what /floor needs: the detected plane
 *   is the ground the duck already stands on, so MainActivity posts the
 *   contract's zero offset once the intent server is listening, never the
 *   camera's distance to it.
 *
 * Everything except [DepthScanner] is plain Kotlin so the JVM tests can prove
 * the merge, the cluster cap, the XML and the floor fit without a device.
 */

/** One 3-D point in Android camera coordinates: x right, y down, z forward. */
internal data class DepthPoint(val x: Float, val y: Float, val z: Float)

/** An axis-aligned obstacle box in the duck's world: x forward, y left, z up. */
internal data class Aabb(
    val minX: Float,
    val minY: Float,
    val minZ: Float,
    val maxX: Float,
    val maxY: Float,
    val maxZ: Float,
) {
    val centerX get() = (minX + maxX) / 2f
    val centerY get() = (minY + maxY) / 2f
    val centerZ get() = (minZ + maxZ) / 2f
    val halfX get() = (maxX - minX) / 2f
    val halfY get() = (maxY - minY) / 2f
    val halfZ get() = (maxZ - minZ) / 2f

    fun union(other: Aabb) = Aabb(
        min(minX, other.minX),
        min(minY, other.minY),
        min(minZ, other.minZ),
        max(maxX, other.maxX),
        max(maxY, other.maxY),
        max(maxZ, other.maxZ),
    )

    fun expanded(by: Float) = Aabb(
        minX - by,
        minY - by,
        minZ - by,
        maxX + by,
        maxY + by,
        maxZ + by,
    )

    fun intersects(other: Aabb): Boolean =
        minX <= other.maxX && other.minX <= maxX &&
            minY <= other.maxY && other.minY <= maxY &&
            minZ <= other.maxZ && other.minZ <= maxZ
}

/** The plane the frame's dominant surface lies on: n·p + offset = 0. */
internal data class FloorPlane(
    val nx: Float,
    val ny: Float,
    val nz: Float,
    val offset: Float,
) {
    fun height(point: DepthPoint): Float =
        nx * point.x + ny * point.y + nz * point.z + offset
}

/**
 * What one depth frame yielded: the camera's distance to the detected ground
 * plane (validity/diagnostic only, never the /floor z) and up to 3 boxes.
 */
internal data class DepthScene(val floorDistance: Float?, val boxes: List<Aabb>)

/**
 * The outcome of one scan, including the two honest "cannot" states.
 *
 * [sensorPresent] is whether the platform advertises a rear-facing DEPTH16
 * output camera at all; a front-only depth camera is [NO_SENSOR] too, because
 * the scan must read the room in front of the person. [reason] is a
 * diagnostic, not user copy. [floorDistance] says a
 * horizontal plane was found and how far the camera is from it; it is not
 * the `/floor` z (see [floorPostBody]). The UI only shows
 * Copy.depthUnsupported for [NO_SENSOR]; a permission or timeout state keeps
 * "—" so the line never claims the hardware is missing when it is not.
 */
internal data class DepthScan(
    val sensorPresent: Boolean,
    val scanned: Boolean,
    val floorDistance: Float?,
    val boxes: List<Aabb>,
    val reason: String,
) {
    companion object {
        const val NO_SENSOR = "no-sensor"
        const val PERMISSION = "permission"
        const val LOOKUP = "lookup"
        const val TIMEOUT = "timeout"
    }
}

/**
 * The platform's DEPTH16 sample layout and the one acceptance policy here.
 *
 * Each pixel is one 16-bit word: the low 13 bits are distance in millimetres,
 * the top 3 bits a confidence bucket (0 = 100 %, 1 = 80 %, 2 = 60 %, ...,
 * 7 = 0 %). 0 mm is the platform's "no reading".
 */
internal object Depth16 {
    /**
     * The only confidence bucket accepted. The platform's own bucket is 20 %
     * coarse, and furniture edges - exactly where depth confidence drops -
     * would otherwise land in the duck's physics scene at a position the
     * sensor itself says it does not trust. A conservative frame with no
     * 100 % samples yields no boxes instead of wrong boxes.
     */
    const val CONFIDENCE = 0

    const val MIN_MILLIMETRES = 100
    const val MAX_MILLIMETRES = 4_000

    /**
     * Distance in millimetres for one raw 16-bit sample, or null when the
     * sample must not become a point. The caller passes the sign-extended
     * `Short` value; masking to 16 bits happens before any shift.
     */
    fun millimetres(raw: Int): Int? {
        val sample = raw and 0xFFFF
        val confidence = sample ushr 13
        if (confidence > CONFIDENCE) return null
        val millimetres = sample and 0x1FFF
        if (millimetres !in MIN_MILLIMETRES..MAX_MILLIMETRES) return null
        return millimetres
    }

    /** The 3-bit confidence bucket of one raw sample, for the scan's log. */
    fun confidence(raw: Int): Int = (raw and 0xFFFF) ushr 13
}

/** Which way a camera looks, as plain Kotlin so the picker is JVM-testable. */
internal enum class LensFacing { BACK, OTHER }

/**
 * The observable facts the picker needs from one Camera2 camera. Produced by
 * [DepthScanner.cameraFacts] and selected by [DepthCamera.select].
 */
internal data class DepthCameraFacts(
    val id: String,
    val lensFacing: LensFacing,
    val depthOutput: Boolean,
    val width: Int,
    val height: Int,
    val fx: Float,
    val fy: Float,
    val cx: Float,
    val cy: Float,
)

internal object DepthCamera {
    /**
     * The scan reads the room in front of the person, so only a rear-facing
     * depth camera is acceptable: a front ToF would scan the room behind the
     * phone and inject furniture into the wrong world. No rear depth camera
     * is an honest "no sensor", never a fallback to the front one. Among
     * several rear depth cameras the largest DEPTH16 output wins.
     */
    fun select(cameras: List<DepthCameraFacts>): DepthCameraFacts? = cameras
        .filter {
            it.depthOutput && it.lensFacing == LensFacing.BACK &&
                it.width > 0 && it.height > 0
        }
        .maxByOrNull { it.width.toLong() * it.height }
}

/**
 * `LENS_INTRINSIC_CALIBRATION` is defined in
 * `SENSOR_INFO_PRE_CORRECTION_ACTIVE_ARRAY_SIZE` pixels, while the DEPTH16
 * ImageReader is created at a usually smaller output size. Projection with
 * unscaled values would send every point in the wrong direction, so x
 * coordinates scale by the width ratio and y by the height ratio. Equal
 * sizes leave the calibration exactly as published.
 */
internal object DepthIntrinsics {
    fun scaled(
        fx: Float,
        fy: Float,
        cx: Float,
        cy: Float,
        preWidth: Int,
        preHeight: Int,
        width: Int,
        height: Int,
    ): FloatArray {
        if (preWidth <= 0 || preHeight <= 0 || width <= 0 || height <= 0) {
            return floatArrayOf(fx, fy, cx, cy)
        }
        if (preWidth == width && preHeight == height) {
            return floatArrayOf(fx, fy, cx, cy)
        }
        val sx = width.toFloat() / preWidth
        val sy = height.toFloat() / preHeight
        return floatArrayOf(fx * sx, fy * sy, cx * sx, cy * sy)
    }
}

/** The last scan of this launch, read by the Tools row and MainActivity. */
internal object DepthStatus {
    @Volatile
    var last: DepthScan? = null

    /**
     * Whether this launch's boxes reached the disk: true when written, false
     * when staging failed or was degraded, null when there were no boxes to
     * stage. Tools reads it so the row never counts furniture the core did
     * not load.
     */
    @Volatile
    var obstaclesWritten: Boolean? = null
}

/**
 * The `/floor` body for one scan, or null when no horizontal plane was seen.
 *
 * The coordinator pinned the core's semantics: z is the ground plane's offset
 * from the scene's default ground, the core accepts only -0.1..0.3 m and
 * moves both the plane and the duck. The plane this scan found IS the ground
 * the duck already stands on in its own MuJoCo world, so the honest offset is
 * exactly 0.0. A non-zero value would mean anchoring the duck into the real
 * room first, which is render-layer work this batch does not do; the camera's
 * height above the plane is not that number and would be refused.
 */
internal fun floorPostBody(scan: DepthScan?): String? {
    if (scan == null || !scan.scanned || scan.floorDistance == null) return null
    return """{"z":0.0}"""
}

internal object DepthGeometry {
    /** The contract says at most three boxes. */
    const val MAX_BOXES = 3

    private const val CELL = 0.15f
    private const val MIN_POINTS = 24
    private const val MIN_PLANE_POINTS = 30
    private const val MIN_INLIERS = 30
    private const val TOLERANCE = 0.02f
    private const val RANSAC_TRIES = 96

    private const val MIN_FLOOR = 0.05f
    private const val MAX_FLOOR = 2.0f
    private const val MIN_HEIGHT = 0.06f
    private const val MAX_HEIGHT = 1.5f
    private const val MIN_FORWARD = 0.10f
    private const val MAX_RANGE = 3.0f

    private const val MIN_HALF = 0.01f
    private const val SEED = 0x0D0C2026L

    /**
     * Fit the dominant plane to the cloud. Deterministic on purpose: the same
     * frame must give the same room on every run (and the JVM test can pin
     * it). See the class comment for why the plane must sit below an upright
     * camera to count as the floor.
     */
    fun fitFloor(points: List<DepthPoint>): FloorPlane? {
        if (points.size < MIN_PLANE_POINTS) return null
        val random = Random(SEED)
        var best: FloorPlane? = null
        var bestInliers = 0
        repeat(RANSAC_TRIES) {
            val a = points[random.nextInt(points.size)]
            val b = points[random.nextInt(points.size)]
            val c = points[random.nextInt(points.size)]
            if (a === b || b === c || a === c) return@repeat
            val ux = b.x - a.x
            val uy = b.y - a.y
            val uz = b.z - a.z
            val vx = c.x - a.x
            val vy = c.y - a.y
            val vz = c.z - a.z
            var nx = uy * vz - uz * vy
            var ny = uz * vx - ux * vz
            var nz = ux * vy - uy * vx
            val length = sqrt(nx * nx + ny * ny + nz * nz)
            if (length < 1e-5f) return@repeat
            nx /= length
            ny /= length
            nz /= length
            val offset = -(nx * a.x + ny * a.y + nz * a.z)
            var inliers = 0
            for (point in points) {
                if (abs(nx * point.x + ny * point.y + nz * point.z + offset) <= TOLERANCE) {
                    inliers++
                }
            }
            if (inliers > bestInliers) {
                bestInliers = inliers
                best = FloorPlane(nx, ny, nz, offset)
            }
        }
        val seed = best ?: return null
        // A plane that explains less than an eighth of the cloud is not the
        // room's floor; it is an edge, a hand, or noise.
        if (bestInliers < MIN_INLIERS || bestInliers * 8 < points.size) return null
        val refined = refine(points, seed) ?: return null
        var nx = refined.nx
        var ny = refined.ny
        var nz = refined.nz
        var offset = refined.offset
        // The camera sits on the positive side: offset > 0 means n points
        // from the plane back toward the lens, i.e. "up" in duck world.
        if (offset < 0f) {
            nx = -nx
            ny = -ny
            nz = -nz
            offset = -offset
        }
        if (offset < MIN_FLOOR || offset > MAX_FLOOR) return null
        // Eye-level estimate, independent of sensor mounting: with the phone
        // held roughly upright, the floor's normal lies mostly across the
        // optical axis. A wall in front of the lens has |nz| ≈ 1 and is
        // refused; a phone pointed straight down is also refused rather than
        // guessing a floor the ticket's desk/eye-level path cannot place.
        // (SENSOR_ORIENTATION can rotate the camera's x/y axes, so checking
        // a fixed y sign would reject perfectly good floors on many phones.)
        if (abs(nz) > 0.75f) return null
        return FloorPlane(nx, ny, nz, offset)
    }

    /** Least-squares normal of the seed's inliers, via power iteration. */
    private fun refine(points: List<DepthPoint>, seed: FloorPlane): FloorPlane? {
        val inliers = points.filter { abs(seed.height(it)) <= TOLERANCE * 2f }
        if (inliers.size < MIN_INLIERS) return null
        var cx = 0.0
        var cy = 0.0
        var cz = 0.0
        for (point in inliers) {
            cx += point.x
            cy += point.y
            cz += point.z
        }
        val n = inliers.size.toDouble()
        cx /= n
        cy /= n
        cz /= n
        var xx = 0.0
        var xy = 0.0
        var xz = 0.0
        var yy = 0.0
        var yz = 0.0
        var zz = 0.0
        for (point in inliers) {
            val dx = (point.x - cx).toDouble()
            val dy = (point.y - cy).toDouble()
            val dz = (point.z - cz).toDouble()
            xx += dx * dx
            xy += dx * dy
            xz += dx * dz
            yy += dy * dy
            yz += dy * dz
            zz += dz * dz
        }
        val trace = xx + yy + zz
        var vx = seed.nx.toDouble()
        var vy = seed.ny.toDouble()
        var vz = seed.nz.toDouble()
        repeat(24) {
            // M = trace·I - C: its largest eigenvector is C's smallest, the
            // plane normal. Starting at the RANSAC normal keeps the sign.
            val mx = (trace - xx) * vx - xy * vy - xz * vz
            val my = -xy * vx + (trace - yy) * vy - yz * vz
            val mz = -xz * vx - yz * vy + (trace - zz) * vz
            val length = sqrt(mx * mx + my * my + mz * mz)
            if (length < 1e-9) return@repeat
            vx = mx / length
            vy = my / length
            vz = mz / length
        }
        val offset = -(vx * cx + vy * cy + vz * cz)
        return FloorPlane(vx.toFloat(), vy.toFloat(), vz.toFloat(), offset.toFloat())
    }

    /**
     * Floor + obstacles for one frame. Points are camera-frame; the result is
     * duck world: x forward (away from the hand), y left, z above the floor.
     */
    fun scene(points: List<DepthPoint>, maxBoxes: Int = MAX_BOXES): DepthScene {
        val floor = fitFloor(points) ?: return DepthScene(null, emptyList())
        val nx = floor.nx
        val ny = floor.ny
        val nz = floor.nz

        // The camera's optical axis projected onto the floor: the duck faces
        // the same way the phone does. If the phone looks straight down there
        // is no meaningful forward, so use the camera's y axis as a fallback.
        var fx = -nx * nz
        var fy = -ny * nz
        var fz = 1f - nz * nz
        var flen = sqrt(fx * fx + fy * fy + fz * fz)
        if (flen < 0.1f) {
            fx = -nx * ny
            fy = 1f - ny * ny
            fz = -nz * ny
            flen = sqrt(fx * fx + fy * fy + fz * fz)
            if (flen < 1e-3f) return DepthScene(floor.offset, emptyList())
        }
        fx /= flen
        fy /= flen
        fz /= flen
        // right = forward × up, so world y (left) is the negative projection.
        var rx = fy * nz - fz * ny
        var ry = fz * nx - fx * nz
        var rz = fx * ny - fy * nx
        val rlen = sqrt(rx * rx + ry * ry + rz * rz)
        if (rlen < 1e-3f) return DepthScene(floor.offset, emptyList())
        rx /= rlen
        ry /= rlen
        rz /= rlen

        val above = ArrayList<DepthPoint>()
        for (point in points) {
            val height = nx * point.x + ny * point.y + nz * point.z + floor.offset
            if (height < MIN_HEIGHT || height > MAX_HEIGHT) continue
            val forward = point.x * fx + point.y * fy + point.z * fz
            val right = point.x * rx + point.y * ry + point.z * rz
            if (forward < MIN_FORWARD || forward > MAX_RANGE || abs(right) > MAX_RANGE) continue
            above.add(DepthPoint(forward, -right, height))
        }
        if (above.isEmpty()) return DepthScene(floor.offset, emptyList())
        val clusters = cluster(above, CELL, MIN_POINTS, maxBoxes * 2)
        val merged = mergeBoxes(clusters, 0f).take(maxBoxes)
        return DepthScene(floor.offset, merged)
    }

    /**
     * Voxelize the floor plane, take the connected components with at least
     * [minPoints] samples, and return their bounding boxes, largest first.
     */
    fun cluster(
        points: List<DepthPoint>,
        cell: Float,
        minPoints: Int,
        maxBoxes: Int,
    ): List<Aabb> {
        if (points.isEmpty()) return emptyList()
        val cells = HashMap<Long, MutableList<DepthPoint>>()
        for (point in points) {
            val cx = floor(point.x / cell).toInt()
            val cy = floor(point.y / cell).toInt()
            cells.getOrPut(cellKey(cx, cy)) { ArrayList() }.add(point)
        }
        val seen = HashSet<Long>()
        val found = ArrayList<Pair<Int, Aabb>>()
        for (start in cells.keys) {
            if (!seen.add(start)) continue
            val queue = ArrayDeque<Long>()
            queue.add(start)
            var count = 0
            var box: Aabb? = null
            while (queue.isNotEmpty()) {
                val key = queue.removeFirst()
                for (point in cells[key].orEmpty()) {
                    count++
                    val one = Aabb(point.x, point.y, point.z, point.x, point.y, point.z)
                    box = box?.union(one) ?: one
                }
                val cx = (key shr 32).toInt()
                val cy = key.toInt()
                for (dx in -1..1) {
                    for (dy in -1..1) {
                        if (dx == 0 && dy == 0) continue
                        val neighbour = cellKey(cx + dx, cy + dy)
                        if (cells.containsKey(neighbour) && seen.add(neighbour)) {
                            queue.add(neighbour)
                        }
                    }
                }
            }
            val value = box ?: continue
            if (count >= minPoints) found.add(count to value)
        }
        found.sortWith(
            compareByDescending<Pair<Int, Aabb>> { it.first }
                .thenBy { it.second.minX }
                .thenBy { it.second.minY },
        )
        return found.take(maxBoxes).map { it.second }
    }

    /**
     * Merge boxes that overlap (or come within [tolerance]) until no pair is
     * left, then sort deterministically. Adjacent voxels from one chair can
     * produce two boxes; the duck should see one obstacle.
     */
    fun mergeBoxes(boxes: List<Aabb>, tolerance: Float = 0f): List<Aabb> {
        val open = ArrayList(boxes)
        var changed = true
        while (changed) {
            changed = false
            outer@ for (i in open.indices) {
                for (j in i + 1 until open.size) {
                    if (open[i].expanded(tolerance).intersects(open[j])) {
                        open[i] = open[i].union(open[j])
                        open.removeAt(j)
                        changed = true
                        break@outer
                    }
                }
            }
        }
        open.sortWith(
            compareBy<Aabb> { it.minX }.thenBy { it.minY }.thenBy { it.minZ },
        )
        return open
    }

    /**
     * The staged `obstacles.xml`: the full MJCF document iOS writes too, with
     * the obstacle boxes as worldbody geoms. Absolute metres in the duck's
     * frame; the furniture is the room at launch, not a moving model.
     */
    fun obstaclesXml(boxes: List<Aabb>): String {
        val kept = boxes.take(MAX_BOXES)
        if (kept.isEmpty()) return ""
        return buildString {
            append("<?xml version=\"1.0\"?>\n")
            append("<mujoco model=\"duck-depth-obstacles\">\n")
            append("    <!-- Generated from one depth frame captured at launch\n")
            append("         (Camera2 DEPTH16). Absolute metres in the duck's MuJoCo\n")
            append("         frame; the furniture does not follow the room after launch. -->\n")
            append("    <worldbody>\n")
            for ((index, box) in kept.withIndex()) {
                append("        <geom name=\"depth_obstacle_$index\" type=\"box\" ")
                append(
                    "size=\"${format(max(box.halfX, MIN_HALF))} " +
                        "${format(max(box.halfY, MIN_HALF))} " +
                        "${format(max(box.halfZ, MIN_HALF))}\" ",
                )
                append(
                    "pos=\"${format(box.centerX)} ${format(box.centerY)} " +
                        "${format(box.centerZ)}\" ",
                )
                append("rgba=\"0.55 0.60 0.62 0.35\" contype=\"1\" conaffinity=\"1\"/>\n")
            }
            append("    </worldbody>\n")
            append("</mujoco>\n")
        }
    }

    private fun cellKey(cx: Int, cy: Int): Long =
        (cx.toLong() shl 32) xor (cy.toLong() and 0xffffffffL)

    /** Locale-independent MJCF numbers; -0.0000 is normalized to 0.0000. */
    private fun format(value: Float): String =
        String.format(Locale.US, "%.4f", if (value == 0f) 0f else value)
}

/** What [stageObstacles] did with this launch's obstacle file. */
internal enum class ObstacleStage {
    /** The scan's boxes are on disk for the core to compile. */
    WRITTEN,

    /** Nothing needed staging and no stale file is left behind. */
    ABSENT,

    /** The boxes could not be written, or a stale file would not delete. */
    DEGRADED,
}

/**
 * Write this scan's obstacles or remove a stale file, and never throw: a
 * full or read-only `filesDir` must not stop the duck from starting. The
 * caller logs [DEGRADED] and still constructs the core, without this scan's
 * furniture (or, after a failed delete, possibly with an older file).
 * Returns the failure itself only when the file operation threw.
 */
internal fun stageObstacles(file: File, scan: DepthScan): Result<ObstacleStage> = runCatching {
    if (scan.scanned && scan.boxes.isNotEmpty()) {
        try {
            file.writeText(DepthGeometry.obstaclesXml(scan.boxes))
            ObstacleStage.WRITTEN
        } catch (failure: Throwable) {
            // Best effort: a stale file from another room must not survive
            // a failed write and become the core's invented furniture.
            runCatching { file.delete() }
            throw failure
        }
    } else {
        // A file left by another room would be an invented room; a delete
        // that reports false but leaves the path in place degrades instead
        // of throwing.
        val removed = file.delete()
        if (removed || !file.exists()) ObstacleStage.ABSENT else ObstacleStage.DEGRADED
    }
}

/**
 * The one-shot Camera2 depth reader. Every failure is a [DepthScan] value,
 * never an exception: no camera service, no depth-capable back camera, no
 * CAMERA permission (a first launch has not asked yet), the camera busy, or
 * no frame within the launch budget all end in an honest state.
 */
internal class DepthScanner(private val context: Context) {

    fun scan(openWaitMs: Long = 900, frameWaitMs: Long = 900): DepthScan {
        val manager = context.getSystemService(Context.CAMERA_SERVICE) as? CameraManager
            ?: return DepthScan(false, false, null, emptyList(), DepthScan.LOOKUP)
        val cameras = try {
            manager.cameraIdList.mapNotNull { id -> cameraFacts(manager, id) }
        } catch (_: Throwable) {
            return DepthScan(false, false, null, emptyList(), DepthScan.LOOKUP)
        }
        // Only a rear-facing depth camera reads the room the duck shares
        // with the person; a front one is an honest NO_SENSOR.
        val candidate = DepthCamera.select(cameras)
            ?: return DepthScan(false, false, null, emptyList(), DepthScan.NO_SENSOR)
        if (ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.CAMERA,
            ) != PackageManager.PERMISSION_GRANTED
        ) {
            return DepthScan(true, false, null, emptyList(), DepthScan.PERMISSION)
        }
        val frame = try {
            capture(manager, candidate, openWaitMs, frameWaitMs)
        } catch (_: Throwable) {
            null
        } ?: return DepthScan(true, false, null, emptyList(), DepthScan.TIMEOUT)
        val scene = DepthGeometry.scene(frame)
        return DepthScan(
            sensorPresent = true,
            scanned = true,
            floorDistance = scene.floorDistance,
            boxes = scene.boxes,
            reason = "",
        )
    }

    /**
     * Everything the picker may use about one camera, including its facing.
     * A camera whose characteristics cannot be read is absent; one without
     * a usable DEPTH16 size is present but not depth-capable.
     */
    private fun cameraFacts(manager: CameraManager, id: String): DepthCameraFacts? {
        val characteristics = runCatching { manager.getCameraCharacteristics(id) }
            .getOrNull() ?: return null
        val facing = runCatching {
            characteristics.get(CameraCharacteristics.LENS_FACING)
        }.getOrNull()
        val lensFacing = if (facing == CameraCharacteristics.LENS_FACING_BACK) {
            LensFacing.BACK
        } else {
            LensFacing.OTHER
        }
        val capabilities = runCatching {
            characteristics.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES)
        }.getOrNull() ?: return null
        if (!capabilities.contains(
                CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_DEPTH_OUTPUT,
            )
        ) {
            return DepthCameraFacts(id, lensFacing, false, 0, 0, 0f, 0f, 0f, 0f)
        }
        val map = runCatching {
            characteristics.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
        }.getOrNull()
        val sizes = map?.let {
            runCatching { it.getOutputSizes(ImageFormat.DEPTH16) }.getOrNull()
        }
        val size = sizes
            ?.filter { it.width > 0 && it.height > 0 && it.width.toLong() * it.height <= 1_000_000L }
            ?.maxByOrNull { it.width.toLong() * it.height }
            ?: return DepthCameraFacts(id, lensFacing, true, 0, 0, 0f, 0f, 0f, 0f)
        val intrinsics = intrinsics(characteristics, size.width, size.height)
        return DepthCameraFacts(
            id = id,
            lensFacing = lensFacing,
            depthOutput = true,
            width = size.width,
            height = size.height,
            fx = intrinsics[0],
            fy = intrinsics[1],
            cx = intrinsics[2],
            cy = intrinsics[3],
        )
    }

    /**
     * fx, fy, cx, cy in pixels, scaled to the depth output. When the camera
     * publishes `LENS_INTRINSIC_CALIBRATION` it is exact, but only after
     * mapping from the pre-correction active array to the DEPTH16 size.
     * Without that array the calibration's pixel frame is unknown, so the
     * estimate below (focal length over physical sensor, then a 60°
     * horizontal FOV) is safer than using the raw numbers at the wrong size.
     */
    private fun intrinsics(
        characteristics: CameraCharacteristics,
        width: Int,
        height: Int,
    ): FloatArray {
        val calibration = runCatching {
            characteristics.get(CameraCharacteristics.LENS_INTRINSIC_CALIBRATION)
        }.getOrNull()
        if (calibration != null && calibration.size >= 4 &&
            calibration[0] > 0f && calibration[1] > 0f
        ) {
            val active = runCatching {
                characteristics.get(
                    CameraCharacteristics.SENSOR_INFO_PRE_CORRECTION_ACTIVE_ARRAY_SIZE,
                )
            }.getOrNull()
            if (active != null && active.width() > 0 && active.height() > 0) {
                return DepthIntrinsics.scaled(
                    calibration[0],
                    calibration[1],
                    calibration[2],
                    calibration[3],
                    active.width(),
                    active.height(),
                    width,
                    height,
                )
            }
        }
        val focal = runCatching {
            characteristics.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS)
        }.getOrNull()?.firstOrNull()
        val physical = runCatching {
            characteristics.get(CameraCharacteristics.SENSOR_INFO_PHYSICAL_SIZE)
        }.getOrNull()
        if (focal != null && physical != null && physical.width > 0f && physical.height > 0f) {
            return floatArrayOf(
                focal / physical.width * width,
                focal / physical.height * height,
                width / 2f,
                height / 2f,
            )
        }
        val focalPx = width / (2f * 0.57735f) // tan(30°), a 60° horizontal FOV
        return floatArrayOf(focalPx, focalPx, width / 2f, height / 2f)
    }

    /** Open, configure, wait for one DEPTH16 frame, and always close. */
    private fun capture(
        manager: CameraManager,
        candidate: DepthCameraFacts,
        openWaitMs: Long,
        frameWaitMs: Long,
    ): List<DepthPoint>? {
        val thread = HandlerThread("duck-depth")
        thread.start()
        val handler = Handler(thread.looper)
        val reader = try {
            ImageReader.newInstance(candidate.width, candidate.height, ImageFormat.DEPTH16, 2)
        } catch (_: Throwable) {
            thread.quitSafely()
            return null
        }
        val device = AtomicReference<CameraDevice?>()
        val session = AtomicReference<CameraCaptureSession?>()
        val imageRef = AtomicReference<Image?>()
        val got = CountDownLatch(1)
        try {
            reader.setOnImageAvailableListener({ source ->
                val image = source.acquireLatestImage() ?: return@setOnImageAvailableListener
                if (got.count > 0L) {
                    imageRef.set(image)
                    got.countDown()
                } else {
                    image.close()
                }
            }, handler)
            val opened = CountDownLatch(1)
            try {
                manager.openCamera(candidate.id, object : CameraDevice.StateCallback() {
                    override fun onOpened(camera: CameraDevice) {
                        device.set(camera)
                        try {
                            camera.createCaptureSession(
                                listOf(reader.surface),
                                object : CameraCaptureSession.StateCallback() {
                                    override fun onConfigured(configured: CameraCaptureSession) {
                                        session.set(configured)
                                        try {
                                            val request = camera
                                                .createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                                                .apply { addTarget(reader.surface) }
                                                .build()
                                            configured.setRepeatingRequest(request, null, handler)
                                        } catch (_: Throwable) {
                                            got.countDown()
                                        }
                                    }

                                    override fun onConfigureFailed(failed: CameraCaptureSession) {
                                        got.countDown()
                                    }
                                },
                                handler,
                            )
                        } catch (_: Throwable) {
                            got.countDown()
                        }
                        opened.countDown()
                    }

                    override fun onDisconnected(camera: CameraDevice) {
                        camera.close()
                        got.countDown()
                    }

                    override fun onError(camera: CameraDevice, error: Int) {
                        camera.close()
                        got.countDown()
                    }
                }, handler)
            } catch (_: Throwable) {
                return null
            }
            if (!opened.await(openWaitMs, TimeUnit.MILLISECONDS)) return null
            if (!got.await(frameWaitMs, TimeUnit.MILLISECONDS)) return null
            val image = imageRef.getAndSet(null) ?: return null
            return try {
                pointsFromImage(image, candidate)
            } finally {
                image.close()
            }
        } finally {
            runCatching { session.get()?.close() }
            runCatching { device.get()?.close() }
            runCatching { reader.close() }
            imageRef.getAndSet(null)?.let { runCatching { it.close() } }
            thread.quitSafely()
            runCatching { thread.join(300) }
        }
    }

    private fun pointsFromImage(image: Image, candidate: DepthCameraFacts): List<DepthPoint> {
        val plane = image.planes.firstOrNull() ?: return emptyList()
        // Android image buffers are native-endian. DEPTH16 packs distance
        // and confidence into one 16-bit word: the low 13 bits are
        // millimetres, the top 3 are the confidence bucket, and 0 mm means
        // "no reading". [Depth16.millimetres] applies the acceptance policy.
        val buffer = plane.buffer.duplicate().order(ByteOrder.nativeOrder())
        val width = candidate.width
        val height = candidate.height
        val rowStride = plane.rowStride
        val pixelStride = plane.pixelStride
        val area = width.toLong() * height
        val step = max(1, sqrt(area / 6_000.0).toInt())
        val points = ArrayList<DepthPoint>((area / (step * step)).toInt().coerceAtLeast(16))
        val confidenceCounts = IntArray(8)
        var row = step / 2
        while (row < height) {
            var column = step / 2
            while (column < width) {
                val index = row * rowStride + column * pixelStride
                if (index + 1 < buffer.limit()) {
                    val raw = buffer.getShort(index).toInt()
                    confidenceCounts[Depth16.confidence(raw)]++
                    val millimetres = Depth16.millimetres(raw)
                    if (millimetres != null) {
                        val depth = millimetres / 1000f
                        points.add(
                            DepthPoint(
                                (column - candidate.cx) * depth / candidate.fx,
                                (row - candidate.cy) * depth / candidate.fy,
                                depth,
                            ),
                        )
                    }
                }
                column += step
            }
            row += step
        }
        // One line per launch, sampled: the confidence buckets the frame
        // actually carried (0 = 100 %, 7 = 0 %) and the points this policy
        // kept. A real-device scan that yields no boxes should be read
        // against this line before the geometry is suspected.
        android.util.Log.i(
            "duck",
            "depth: confidence ${confidenceCounts.joinToString("/")} (0=100%..7=0%), " +
                "${points.size} points kept",
        )
        return points
    }
}