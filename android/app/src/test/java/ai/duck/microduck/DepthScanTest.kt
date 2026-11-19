package ai.duck.microduck

import java.io.ByteArrayInputStream
import java.io.File
import java.nio.file.Files
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * JVM tests for the J3 depth half. The Camera2 runtime needs a phone; these
 * pin the parts that must be right on every phone: the AABB merge, the
 * three-box cap, the MJCF document handed to the core, and the floor fit
 * that refuses to call a wall a floor.
 */
class DepthScanTest {

    @Test
    fun `overlapping boxes merge until no pair is left`() {
        val a = Aabb(0f, 0f, 0f, 0.4f, 0.4f, 0.4f)
        val b = Aabb(0.3f, 0f, 0f, 0.6f, 0.4f, 0.4f)
        val c = Aabb(0.55f, 0.1f, 0f, 1.0f, 0.5f, 0.4f)
        val d = Aabb(2f, 0f, 0f, 2.5f, 0.4f, 0.4f)
        val merged = DepthGeometry.mergeBoxes(listOf(d, c, a, b))
        assertEquals(2, merged.size)
        // Sorted by minX: the chain collapses to its union, the lone box stays.
        assertEquals(Aabb(0f, 0f, 0f, 1.0f, 0.5f, 0.4f), merged[0])
        assertEquals(d, merged[1])
    }

    @Test
    fun `tolerance bridges a gap - without it neighbors stay apart`() {
        val a = Aabb(0f, 0f, 0f, 0.4f, 0.4f, 0.4f)
        val b = Aabb(0.41f, 0f, 0f, 0.8f, 0.4f, 0.4f)
        assertEquals(2, DepthGeometry.mergeBoxes(listOf(a, b)).size)
        assertEquals(1, DepthGeometry.mergeBoxes(listOf(a, b), tolerance = 0.02f).size)
    }

    @Test
    fun `cluster keeps the three largest components and drops noise`() {
        val points = ArrayList<DepthPoint>()
        for (cluster in 0 until 4) {
            for (index in 0 until 40) {
                points.add(
                    DepthPoint(
                        cluster * 1.0f + (index % 5) * 0.02f,
                        (index / 5) * 0.02f,
                        0.2f + (index % 3) * 0.02f,
                    ),
                )
            }
        }
        // One lone voxel: below minPoints, never a box.
        points.add(DepthPoint(-1f, -1f, 0.2f))
        val boxes = DepthGeometry.cluster(points, 0.15f, 20, DepthGeometry.MAX_BOXES)
        assertEquals(3, boxes.size)
        assertEquals(DepthGeometry.MAX_BOXES, boxes.size)
    }

    @Test
    fun `scene finds a floor and one obstacle above it`() {
        val points = ArrayList<DepthPoint>()
        // The floor: camera y points down, so the desk top sits at y = 0.8 m.
        for (x in -20..20) {
            for (z in 3..20) {
                points.add(DepthPoint(x * 0.05f, 0.8f, z * 0.1f))
            }
        }
        // A box on the desk: 0.5..0.7 m forward, 0..0.48 m tall.
        for (i in 0..4) {
            for (j in 0..4) {
                for (k in 0..4) {
                    points.add(DepthPoint(0.5f + i * 0.05f, 0.8f - j * 0.12f, 1.2f + k * 0.05f))
                }
            }
        }
        val scene = DepthGeometry.scene(points)
        assertNotNull(scene.floorDistance)
        assertEquals(0.8f, scene.floorDistance!!, 0.03f)
        assertEquals(1, scene.boxes.size)
        val box = scene.boxes[0]
        assertTrue("box should start at the desk surface, was ${box.minZ}", box.minZ >= 0f)
        assertTrue("box should stay below 0.7 m, was ${box.maxZ}", box.maxZ <= 0.7f)
        // Forward is the camera's optical axis: the obstacle sits 1.3 m away
        // and 0.6 m to the right, which is -0.6 on the duck's left-handed y.
        assertEquals(1.3f, box.centerX, 0.08f)
        assertEquals(-0.6f, box.centerY, 0.08f)
    }

    @Test
    fun `a wall-facing cloud is not a floor`() {
        // A plane at z = 1 m: its normal lies along the optical axis, so the
        // upright-camera check must refuse it instead of anchoring the duck
        // to a wall.
        val points = ArrayList<DepthPoint>()
        for (x in -20..20) {
            for (y in -20..20) {
                points.add(DepthPoint(x * 0.02f, y * 0.02f, 1.0f))
            }
        }
        assertNull(DepthGeometry.fitFloor(points))
        assertNull(DepthGeometry.scene(points).floorDistance)
    }

    @Test
    fun `obstacles xml is one MJCF document with worldbody geoms`() {
        val xml = DepthGeometry.obstaclesXml(
            listOf(Aabb(0.1f, 0.2f, 0.0f, 0.3f, 0.4f, 0.5f)),
        )
        assertTrue(xml.startsWith("<?xml version=\"1.0\"?>"))
        assertTrue(xml.contains("""<mujoco model="duck-depth-obstacles">"""))
        assertTrue(xml.contains("<worldbody>"))
        assertTrue(
            xml.contains(
                """<geom name="depth_obstacle_0" type="box" """ +
                    """size="0.1000 0.1000 0.2500" """ +
                    """pos="0.2000 0.3000 0.2500" """ +
                    """rgba="0.55 0.60 0.62 0.35" contype="1" conaffinity="1"/>""",
            ),
        )
        // The whole document parses as XML; J1 merges its worldbody.
        DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(
            ByteArrayInputStream(xml.toByteArray()),
        )
        // K2 #5: the header says what actually happened - one frame at
        // launch, not a one-second scan.
        assertTrue(xml.contains("one depth frame captured at launch"))
        assertTrue(!xml.contains("one-second"))
    }

    @Test
    fun `floor intent is zero only after a scan saw the plane`() {
        // Nothing ran, the hardware is absent, permission was refused, or the
        // scan saw no horizontal plane: all of those post nothing.
        assertNull(floorPostBody(null))
        assertNull(
            floorPostBody(
                DepthScan(
                    sensorPresent = false,
                    scanned = false,
                    floorDistance = null,
                    boxes = emptyList(),
                    reason = DepthScan.NO_SENSOR,
                ),
            ),
        )
        assertNull(
            floorPostBody(
                DepthScan(
                    sensorPresent = true,
                    scanned = false,
                    floorDistance = null,
                    boxes = emptyList(),
                    reason = DepthScan.PERMISSION,
                ),
            ),
        )
        assertNull(
            floorPostBody(
                DepthScan(
                    sensorPresent = true,
                    scanned = true,
                    floorDistance = null,
                    boxes = emptyList(),
                    reason = "",
                ),
            ),
        )
        // A plane was found: the detected plane is the duck's own ground, so
        // the offset from the scene's default ground is exactly zero.
        assertEquals(
            """{"z":0.0}""",
            floorPostBody(
                DepthScan(
                    sensorPresent = true,
                    scanned = true,
                    floorDistance = 0.82f,
                    boxes = listOf(Aabb(0f, 0f, 0f, 0.2f, 0.2f, 0.4f)),
                    reason = "",
                ),
            ),
        )
    }

    @Test
    fun `xml never writes more than three geoms`() {
        val boxes = (0 until 6).map { index ->
            Aabb(index * 1f, 0f, 0f, index + 0.4f, 0.3f, 0.3f)
        }
        val xml = DepthGeometry.obstaclesXml(boxes)
        assertEquals(3, Regex("<geom ").findAll(xml).count())
        assertEquals("", DepthGeometry.obstaclesXml(emptyList()))
    }

    @Test
    fun `xml prints a negative zero as zero`() {
        val xml = DepthGeometry.obstaclesXml(
            listOf(Aabb(-0.1f, -0.1f, 0f, 0.1f, 0.1f, 0.2f)),
        )
        assertTrue(xml.contains("""pos="0.0000 0.0000 0.1000""""))
    }

    // K2 #1: DEPTH16 packs 13 distance bits under 3 confidence bits. The
    // policy keeps only the 100 % bucket (confidence 0); lower buckets are
    // dropped rather than placed as furniture.
    @Test
    fun `depth16 splits thirteen distance bits from three confidence bits`() {
        // 800 mm at 100 % confidence: the top three bits are zero.
        assertEquals(800, Depth16.millimetres(0x0320))
        // 800 mm at 80 % confidence is 0x2000 or 0x0320 = 0x2320: dropped.
        assertNull(Depth16.millimetres(0x2320))
        // 0x4320 is confidence 2 with 4896 mm of distance: dropped too.
        assertNull(Depth16.millimetres(0x4320))
        // The bucket itself, as the scan's histogram log reports it.
        assertEquals(0, Depth16.confidence(0x0320))
        assertEquals(1, Depth16.confidence(0x2320))
        assertEquals(7, Depth16.confidence(0xFFFF))
    }

    @Test
    fun `depth16 rejects zero, out-of-range and low-confidence samples`() {
        // The room budget is 0.1..4 m.
        assertEquals(100, Depth16.millimetres(100))
        assertEquals(4_000, Depth16.millimetres(4_000))
        assertNull(Depth16.millimetres(99))
        assertNull(Depth16.millimetres(4_001))
        // The 13-bit maximum is 8191 mm, far past the 4 m budget.
        assertNull(Depth16.millimetres(0x1FFF))
        // 0 mm is the platform's "no reading". DEPTH16 is integral, so the
        // float pipeline's NaN cannot occur at this layer; zero is the
        // no-sample spelling and is filtered here.
        assertNull(Depth16.millimetres(0))
        // Low 13 bits all ones under the 0 % confidence bucket: dropped.
        assertNull(Depth16.millimetres(0xFFFF))
        // A plausible distance under the 0 % bucket is still dropped.
        assertNull(Depth16.millimetres((7 shl 13) or 800))
    }

    private fun camera(
        id: String,
        facing: LensFacing,
        depthOutput: Boolean = true,
        width: Int = 640,
        height: Int = 480,
    ) = DepthCameraFacts(
        id = id,
        lensFacing = facing,
        depthOutput = depthOutput,
        width = width,
        height = height,
        fx = 500f,
        fy = 500f,
        cx = 320f,
        cy = 240f,
    )

    // K2 #2: the scan must read the room in front of the person. A front ToF
    // would scan the room behind the phone.
    @Test
    fun `a front-only depth camera is no sensor`() {
        assertNull(
            DepthCamera.select(
                listOf(
                    camera("front-tof", LensFacing.OTHER),
                    camera("rear-plain", LensFacing.BACK, depthOutput = false),
                ),
            ),
        )
    }

    @Test
    fun `both ends depth pick the rear camera even if the front is larger`() {
        val front = camera("front-tof", LensFacing.OTHER, width = 1280, height = 960)
        val back = camera("rear-tof", LensFacing.BACK)
        assertEquals("rear-tof", DepthCamera.select(listOf(front, back))?.id)
    }

    @Test
    fun `no depth-capable camera is no sensor`() {
        assertNull(
            DepthCamera.select(
                listOf(
                    camera("rear-plain", LensFacing.BACK, depthOutput = false),
                    camera("front-plain", LensFacing.OTHER, depthOutput = false),
                ),
            ),
        )
        assertNull(DepthCamera.select(emptyList()))
    }

    // K2 #3: calibration intrinsics live in PRE_CORRECTION_ACTIVE_ARRAY_SIZE
    // pixels; the DEPTH16 stream is usually smaller.
    @Test
    fun `intrinsics scale from the pre-correction array to half-size depth`() {
        val scaled = DepthIntrinsics.scaled(
            fx = 1000f,
            fy = 1000f,
            cx = 640f,
            cy = 480f,
            preWidth = 1280,
            preHeight = 960,
            width = 640,
            height = 480,
        )
        assertArrayEquals(floatArrayOf(500f, 500f, 320f, 240f), scaled, 1e-3f)
    }

    @Test
    fun `intrinsics are untouched when the depth size equals the array`() {
        val same = DepthIntrinsics.scaled(
            900f, 910f, 600f, 450f,
            preWidth = 1280, preHeight = 960,
            width = 1280, height = 960,
        )
        assertArrayEquals(floatArrayOf(900f, 910f, 600f, 450f), same, 0f)
    }

    @Test
    fun `a non-uniform aspect scales cx and cy by their own ratios`() {
        val scaled = DepthIntrinsics.scaled(
            fx = 1000f,
            fy = 1000f,
            cx = 640f,
            cy = 480f,
            preWidth = 1280,
            preHeight = 960,
            width = 640,
            height = 720,
        )
        // sx = 0.5, sy = 0.75: fx/cx follow width, fy/cy follow height.
        assertArrayEquals(floatArrayOf(500f, 750f, 320f, 360f), scaled, 1e-3f)
    }

    @Test
    fun `missing pre-correction size leaves intrinsics untouched`() {
        val fallback = DepthIntrinsics.scaled(
            1000f, 1000f, 640f, 480f,
            preWidth = 0, preHeight = 0,
            width = 640, height = 480,
        )
        assertArrayEquals(floatArrayOf(1000f, 1000f, 640f, 480f), fallback, 0f)
    }

    // K2 #4: a read-only or full filesDir must neither throw nor stop the
    // core; stageObstacles reports what happened and the caller continues.
    private val scannedWithBox = DepthScan(
        sensorPresent = true,
        scanned = true,
        floorDistance = 0.8f,
        boxes = listOf(Aabb(0.1f, 0.1f, 0f, 0.2f, 0.2f, 0.3f)),
        reason = "",
    )
    private val scannedEmpty = DepthScan(
        sensorPresent = true,
        scanned = true,
        floorDistance = null,
        boxes = emptyList(),
        reason = "",
    )

    @Test
    fun `staging writes boxes and removes the file when there are none`() {
        val dir = Files.createTempDirectory("duck-obstacles").toFile()
        try {
            val file = File(dir, "obstacles.xml")
            assertEquals(ObstacleStage.WRITTEN, stageObstacles(file, scannedWithBox).getOrThrow())
            assertTrue(file.readText().contains("depth_obstacle_0"))
            assertEquals(ObstacleStage.ABSENT, stageObstacles(file, scannedEmpty).getOrThrow())
            assertTrue(!file.exists())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun `a write failure is returned, not thrown`() {
        val dir = Files.createTempDirectory("duck-obstacles").toFile()
        try {
            // A regular file where the directory should be: writeText throws.
            val blocker = File(dir, "blocked")
            blocker.writeText("not a directory")
            val result = stageObstacles(File(blocker, "obstacles.xml"), scannedWithBox)
            assertTrue(result.isFailure)
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun `an undeletable stale file degrades but does not throw`() {
        val dir = Files.createTempDirectory("duck-obstacles").toFile()
        try {
            // A non-empty directory cannot be deleted by File.delete: the
            // stale path survives, but the result is a value, not an
            // exception, so the launch continues.
            val stale = File(dir, "obstacles.xml")
            stale.mkdirs()
            File(stale, "child").writeText("stale")
            assertEquals(ObstacleStage.DEGRADED, stageObstacles(stale, scannedEmpty).getOrThrow())
        } finally {
            dir.deleteRecursively()
        }
    }
}