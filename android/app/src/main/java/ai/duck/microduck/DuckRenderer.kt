package ai.duck.microduck

import android.content.Context
import android.opengl.GLES30
import android.opengl.GLSurfaceView
import android.opengl.Matrix
import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.sin

/**
 * OpenGL ES 3 renderer for the official meshes. It reads body poses from the
 * core each frame; physics is the 50 Hz tick in [DuckRenderer.onDrawFrame].
 */
class DuckRenderer(
    private val context: Context,
    private val core: DuckCore,
    private val input: DuckInput,
) : GLSurfaceView.Renderer {

    private data class Mesh(val buffer: FloatBuffer, val vertices: Int)
    private data class Instance(
        val mesh: Mesh,
        val body: Int,
        val local: FloatArray,
        val color: FloatArray,
        /// The lower beak: the one geom the shell animates itself. The
        /// `jaw` mesh shares its body with the upper beak, the eyes and the
        /// bottom shell — the hinge rotation is composed BEFORE the geom's
        /// own local, in mesh space, so only this geom moves.
        val isJaw: Boolean = false,
        val jawPivotZ: Float = 0f,
        val jawPivotY: Float = 0f,
    )

    private var program = 0
    private var uMvp = 0
    private var uModel = 0
    private var uColor = 0
    private var uLight = 0
    private var uChecker = 0
    private var uMaterial = 0
    private var uStage = 0
    private var uHalo = 0
    private var uDaylight = 0
    private var uShadow = 0
    private var aPos = 0
    private var aNormal = 0
    private val meshes = mutableMapOf<Int, Mesh>()
    /** Mesh-space bounds per mesh id, from the soup's positions only: the
     *  jaw's hinge is the rear-top corner of its own bounds (the iOS renderer
     *  derives the same pivot the same way). */
    private val meshBounds = mutableMapOf<Int, FloatArray>()
    private val instances = mutableListOf<Instance>()
    private var floor: Mesh? = null
    /// 全屏背景板：盖住 NDC 的大三角，顶点就是 NDC 坐标，MVP 用单位阵。
    private var backdrop: Mesh? = null
    private var aspect = 1f
    /// The body model this renderer was built for; a mode switch changes it.
    private var builtGeneration = -1L
    private val yaw = Math.PI.toFloat()
    private val pitch = 0.32f
    private val distance = 0.85f

    private var viewportWidth = 0
    private var viewportHeight = 0
    /// The beak the renderer draws, smoothed toward the core's mouth value
    /// with the same 90 ms time constant the iOS shell uses for a quack —
    /// a step open would snap, this sways. The core clamps it shut while the
    /// duck is asleep, so a reading duck's beak shuts the moment it dozes.
    private var visualMouth = 0f
    /// Set by the Director shutter: the next finished frame is read back and
    /// handed over instead of only being presented.
    @Volatile private var captureCallback: ((android.graphics.Bitmap?) -> Unit)? = null

    /** One frame, read from the GL surface. The callback runs on the GL thread. */
    fun captureFrame(callback: (android.graphics.Bitmap?) -> Unit) {
        captureCallback = callback
    }

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        GLES30.glClearColor(0.055f, 0.075f, 0.10f, 1f)
        GLES30.glEnable(GLES30.GL_DEPTH_TEST)
        program = buildProgram()
        uMvp = GLES30.glGetUniformLocation(program, "uMvp")
        uModel = GLES30.glGetUniformLocation(program, "uModel")
        uColor = GLES30.glGetUniformLocation(program, "uColor")
        uLight = GLES30.glGetUniformLocation(program, "uLight")
        uChecker = GLES30.glGetUniformLocation(program, "uChecker")
        uMaterial = GLES30.glGetUniformLocation(program, "uMaterial")
        uStage = GLES30.glGetUniformLocation(program, "uStage")
        uHalo = GLES30.glGetUniformLocation(program, "uHalo")
        uDaylight = GLES30.glGetUniformLocation(program, "uDaylight")
        uShadow = GLES30.glGetUniformLocation(program, "uShadow")
        aPos = GLES30.glGetAttribLocation(program, "aPos")
        aNormal = GLES30.glGetAttribLocation(program, "aNormal")
        buildScene()
        // Record the model this scene was built from, so the first frame does
        // not rebuild it straight away.
        builtGeneration = core.modelGeneration()
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
        GLES30.glViewport(0, 0, width, height)
        viewportWidth = width
        viewportHeight = height
        aspect = width.toFloat() / height.coerceAtLeast(1).toFloat()
    }

    override fun onDrawFrame(gl: GL10?) {
        // A different model means different meshes: rebuild before drawing.
        val generation = core.modelGeneration()
        if (generation != builtGeneration) {
            if (builtGeneration >= 0) {
                instances.clear()
                meshes.clear()
                meshBounds.clear()
                floor = null
                backdrop = null
            }
            buildScene()
            builtGeneration = generation
        }
        // The 50 Hz loop runs on DuckLoop's own scheduler; this only draws.
        input.frames.incrementAndGet()
        val status = core.status()
        // The beak follows the core's mouth (an intent write, the idle yawn
        // included), smoothed with the quack's own 90 ms time constant.
        val mouthTarget = if (status.size > 12) status[12].toFloat().coerceIn(0f, 1f) else 0f
        visualMouth += (mouthTarget - visualMouth) * (1f - exp(-0.02f / 0.09f))
        // The camera follows the interpolated trunk, not the status sample: a
        // camera on a slower clock than the duck makes the room judder.
        val trunkPose = core.poses.interpolated(1)
        val trunk = if (trunkPose != null && trunkPose.size >= 3) {
            floatArrayOf(trunkPose[0].toFloat(), trunkPose[1].toFloat(), trunkPose[2].toFloat())
        } else if (status.size >= 9) {
            floatArrayOf(status[3].toFloat(), status[4].toFloat(), status[5].toFloat())
        } else {
            floatArrayOf(0f, 0f, 0.12f)
        }

        // The room's hour, fixed for this frame. The clear, the backdrop and
        // every lit surface read the same sample, so they cannot disagree.
        val hour = Daylight.localHour()
        val day = Daylight.at(hour)
        val instrument = input.instrumentOpen

        // Director mode: transparent and black, so the camera behind shows
        // through and the studio colour does not veil it. Instrument is the
        // lab: cold and dark behind the sheet (STAGE_LOOK §3). Live is the
        // room: a warm brown horizon, dimmed and tinted by the hour.
        if (input.cameraActive) {
            GLES30.glClearColor(0f, 0f, 0f, 0f)
        } else if (instrument) {
            GLES30.glClearColor(0.055f, 0.075f, 0.10f, 1f)
        } else {
            val room = Daylight.roomClear(hour)
            GLES30.glClearColor(room[0], room[1], room[2], 1f)
        }
        GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT or GLES30.GL_DEPTH_BUFFER_BIT)
        GLES30.glUseProgram(program)

        val projection = FloatArray(16)
        val view = FloatArray(16)
        val viewProjection = FloatArray(16)
        val cosPitch = cos(pitch.toDouble()).toFloat()
        // A portrait window is narrower than it is tall; pull the eye back a
        // little so the duck does not fill the frame edge to edge. The same
        // 0.85 base distance, scaled the way the iOS renderer does.
        val cameraDistance = if (aspect < 1f) distance * 1.15f else distance
        val eye = floatArrayOf(
            trunk[0] + cameraDistance * cosPitch * sin(yaw.toDouble()).toFloat(),
            trunk[1] + cameraDistance * cosPitch * cos(yaw.toDouble()).toFloat(),
            trunk[2] + cameraDistance * sin(pitch.toDouble()).toFloat(),
        )
        Matrix.perspectiveM(projection, 0, 45f, aspect, 0.01f, 100f)
        Matrix.setLookAtM(
            view, 0,
            eye[0], eye[1], eye[2],
            trunk[0], trunk[1], trunk[2],
            0f, 0f, 1f,
        )
        Matrix.multiplyMM(viewProjection, 0, projection, 0, view, 0)

        // A headlight, like MuJoCo's own viewer: the light sits at the camera,
        // so whatever the camera sees is lit. The shader negates the stored
        // vector, so this one points the way the light travels: from the
        // camera towards the duck (iOS Renderer.swift's `focus - eye`). The
        // old `eye - trunk` here pointed the other way, so the key term was
        // zero on every camera-facing surface and only ambient lit the duck.
        val headlight = floatArrayOf(
            trunk[0] - eye[0],
            trunk[1] - eye[1],
            trunk[2] - eye[2],
        )
        val lightLength = kotlin.math.sqrt(
            headlight[0] * headlight[0] + headlight[1] * headlight[1] + headlight[2] * headlight[2]
        ).coerceAtLeast(1e-6f)
        headlight[0] /= lightLength
        headlight[1] /= lightLength
        headlight[2] /= lightLength

        // The room's light, once per frame, shared by the backdrop, the floor
        // and every duck part. The halo is the trunk projected into NDC: the
        // warm glow sits behind the duck wherever the camera puts it. The
        // instrument is a lab, not a room: its light is neutral at any hour.
        // The room's lamp: the shell's scale on the exposure, clamped so the
        // room can dim but never go black. The duck's senses never see it.
        val lamp = input.ambientScale.coerceIn(0.55f, 1.15f)
        val daylight = if (instrument) {
            floatArrayOf(1f, 1f, 1f, 1f)
        } else {
            floatArrayOf(day.tint[0], day.tint[1], day.tint[2], day.exposure * lamp)
        }
        val shadow = floatArrayOf(trunk[0], trunk[1], 0.62f, 0f)
        val clip = FloatArray(4)
        Matrix.multiplyMV(
            clip, 0, viewProjection, 0,
            floatArrayOf(trunk[0], trunk[1], trunk[2], 1f), 0,
        )
        val halo = if (kotlin.math.abs(clip[3]) > 1e-4f) {
            floatArrayOf(clip[0] / clip[3], clip[1] / clip[3], 0.8f, aspect)
        } else {
            floatArrayOf(0f, 0.1f, 0.8f, aspect)
        }

        // The studio floor would hide the room (and the duck's feet) over a
        // camera image. Physics ground stays at z=0 either way. The backdrop
        // sits at the far plane and is drawn first, so floor and duck win
        // against it; the instrument keeps its own cold void and no backdrop.
        if (!input.cameraActive) {
            if (!instrument) {
                backdrop?.let {
                    draw(
                        it, identity(), floatArrayOf(1f, 1f, 1f, 1f), headlight,
                        look = 3, viewProjection = viewProjection,
                        halo = halo, daylight = daylight, shadow = shadow,
                    )
                }
            }
            floor?.let {
                // Live: warm stone #38322C (STAGE_LOOK §2.1), no grid.
                // Instrument: the cold checkerboard the lab has always had.
                draw(
                    it, identity(), floatArrayOf(0.220f, 0.196f, 0.173f, 1f), headlight,
                    look = if (instrument) 2 else 1, viewProjection = viewProjection,
                    halo = halo, daylight = daylight, shadow = shadow,
                )
            }
        }

        for (instance in instances) {
            // Interpolated between the last two ticks: MuJoCo runs at 50 Hz, a
            // display at 60–120 Hz, and the live pose would move in steps.
            val pose = core.poses.interpolated(instance.body)
                ?: core.bodyPose(instance.body)
                ?: continue
            if (pose.size < 7) continue

            val body = identity()
            Matrix.translateM(body, 0, pose[0].toFloat(), pose[1].toFloat(), pose[2].toFloat())
            val rotation = FloatArray(16)
            quaternionMatrix(
                pose[3].toFloat(), pose[4].toFloat(), pose[5].toFloat(), pose[6].toFloat(),
                rotation,
            )
            val world = FloatArray(16)
            Matrix.multiplyMM(world, 0, body, 0, rotation, 0)
            var model = FloatArray(16)
            Matrix.multiplyMM(model, 0, world, 0, instance.local, 0)
            if (instance.isJaw && visualMouth > 0.01f) {
                // Hinge on the geom-local X axis through the rear-top corner
                // of the mesh bounds; rotation composed BEFORE the geom's own
                // local, in mesh space, so nothing else in the head moves.
                val jaw = identity()
                Matrix.translateM(jaw, 0, 0f, instance.jawPivotY, instance.jawPivotZ)
                // Kotlin's Matrix.rotateM takes DEGREES; 0.30 rad ≈ 17.19°.
                Matrix.rotateM(jaw, 0, visualMouth * 0.30f * 57.29578f, 1f, 0f, 0f)
                Matrix.translateM(jaw, 0, 0f, -instance.jawPivotY, -instance.jawPivotZ)
                val withJaw = FloatArray(16)
                Matrix.multiplyMM(withJaw, 0, model, 0, jaw, 0)
                model = withJaw
            }
            draw(
                instance.mesh, model, instance.color, headlight,
                look = 0, viewProjection = viewProjection,
                halo = halo, daylight = daylight, shadow = shadow,
            )
        }
        // The Director photo is the frame the user is looking at, read back
        // from the same default framebuffer. In Director the clear is
        // transparent, so the result is the duck with an alpha channel; the
        // activity composites it over the camera still.
        captureCallback?.let { callback ->
            captureCallback = null
            GLES30.glFinish()
            callback(readFrame())
        }
    }

    private fun readFrame(): android.graphics.Bitmap? {
        val width = viewportWidth
        val height = viewportHeight
        if (width <= 0 || height <= 0) return null
        val pixels = ByteBuffer.allocateDirect(width * height * 4)
            .order(ByteOrder.nativeOrder())
        GLES30.glReadPixels(
            0, 0, width, height,
            GLES30.GL_RGBA, GLES30.GL_UNSIGNED_BYTE, pixels,
        )
        pixels.rewind()
        val raw = android.graphics.Bitmap.createBitmap(
            width, height, android.graphics.Bitmap.Config.ARGB_8888,
        )
        raw.copyPixelsFromBuffer(pixels)
        // GL's origin is bottom-left; a bitmap's is top-left.
        val flip = android.graphics.Matrix().apply { postScale(1f, -1f) }
        val upright = android.graphics.Bitmap.createBitmap(
            raw, 0, 0, width, height, flip, true,
        )
        if (upright !== raw) raw.recycle()
        return upright
    }

    private fun buildScene() {
        instances.clear()
        val count = core.geomCount()
        for (geom in 0 until count) {
            val info = core.geomInfo(geom) ?: continue
            if (info.size < 15) continue
            val body = info[0].toInt()
            val type = info[1].toInt()
            val meshId = info[2].toInt()
            val group = info[3].toInt()
            if (group != 2 || type != 7 || meshId < 0) continue
            val mesh = loadMesh(meshId) ?: continue

            val local = identity()
            Matrix.translateM(local, 0, info[4].toFloat(), info[5].toFloat(), info[6].toFloat())
            val rotation = FloatArray(16)
            quaternionMatrix(
                info[7].toFloat(), info[8].toFloat(), info[9].toFloat(), info[10].toFloat(),
                rotation,
            )
            val transform = FloatArray(16)
            Matrix.multiplyMM(transform, 0, local, 0, rotation, 0)
            val alpha = if (info[14] > 0) info[14].toFloat() else 1f
            // The visible lower beak is the geom whose MESH is `jaw` — the
            // `jaw_soft` mesh is the servo rack inside the head. The hinge is
            // the geom-local X axis through the rear-top corner of the mesh's
            // own bounds; a POSITIVE angle swings the front tip down (the
            // same 0.30 rad ≈ 17 mm gape the iOS renderer verified).
            val bounds = meshBounds[meshId]
            val isJaw = core.meshName(meshId) == "jaw" && bounds != null
            instances.add(
                Instance(
                    mesh,
                    body,
                    transform,
                    floatArrayOf(info[11].toFloat(), info[12].toFloat(), info[13].toFloat(), alpha),
                    isJaw = isJaw,
                    jawPivotZ = bounds?.get(3) ?: 0f,
                    jawPivotY = bounds?.get(4) ?: 0f,
                )
            )
        }
        floor = buildFloor()
        backdrop = buildBackdrop()
        Log.i("DuckRenderer", "visual geoms: ${instances.size}")
    }

    /** MuJoCo's own mesh data: the STL files are in different coordinates. */
    private fun loadMesh(meshId: Int): Mesh? {
        meshes[meshId]?.let { return it }
        val floats = core.meshSoup(meshId) ?: return null
        if (floats.isEmpty()) return null
        val buffer = ByteBuffer.allocateDirect(floats.size * 4)
            .order(ByteOrder.nativeOrder())
            .asFloatBuffer()
        buffer.put(floats).position(0)
        val mesh = Mesh(buffer, floats.size / 6)
        meshes[meshId] = mesh
        // Position + normal per vertex: bounds come from the positions only.
        var lowX = Float.MAX_VALUE; var lowY = Float.MAX_VALUE; var lowZ = Float.MAX_VALUE
        var highX = -Float.MAX_VALUE; var highY = -Float.MAX_VALUE; var highZ = -Float.MAX_VALUE
        var index = 0
        while (index + 2 < floats.size) {
            val x = floats[index]; val y = floats[index + 1]; val z = floats[index + 2]
            if (x < lowX) lowX = x; if (y < lowY) lowY = y; if (z < lowZ) lowZ = z
            if (x > highX) highX = x; if (y > highY) highY = y; if (z > highZ) highZ = z
            index += 6
        }
        meshBounds[meshId] = floatArrayOf(lowX, lowY, lowZ, highX, highY, highZ)
        return mesh
    }

    private fun buildFloor(): Mesh {
        val extent = 4f
        val z = -0.002f
        val floats = floatArrayOf(
            -extent, -extent, z, 0f, 0f, 1f,
            extent, -extent, z, 0f, 0f, 1f,
            extent, extent, z, 0f, 0f, 1f,
            -extent, -extent, z, 0f, 0f, 1f,
            extent, extent, z, 0f, 0f, 1f,
            -extent, extent, z, 0f, 0f, 1f,
        )
        val buffer = ByteBuffer.allocateDirect(floats.size * 4)
            .order(ByteOrder.nativeOrder())
            .asFloatBuffer()
        buffer.put(floats).position(0)
        return Mesh(buffer, floats.size / 6)
    }

    /**
     * The room's wall: a triangle that covers NDC, vertices given in NDC, so
     * the vertex shader's MVP is the identity and depth 0.9995 keeps every
     * floor and duck triangle in front of it. The fragment shader paints the
     * vertical gradient and the warm halo (STAGE_LOOK §2.2).
     */
    private fun buildBackdrop(): Mesh {
        val z = 0.9995f
        val floats = floatArrayOf(
            -1f, -1f, z, 0f, 0f, 1f,
            3f, -1f, z, 0f, 0f, 1f,
            -1f, 3f, z, 0f, 0f, 1f,
        )
        val buffer = ByteBuffer.allocateDirect(floats.size * 4)
            .order(ByteOrder.nativeOrder())
            .asFloatBuffer()
        buffer.put(floats).position(0)
        return Mesh(buffer, floats.size / 6)
    }

    /**
     * look: 0 = duck parts; 1 = Live warm-stone floor; 2 = Instrument cold
     * checkerboard; 3 = the full-screen backdrop. The backdrop's vertices are
     * already NDC, so its MVP is the identity while everything else goes
     * through the camera.
     */
    private fun draw(
        mesh: Mesh,
        model: FloatArray,
        color: FloatArray,
        light: FloatArray,
        look: Int,
        viewProjection: FloatArray,
        halo: FloatArray,
        daylight: FloatArray,
        shadow: FloatArray,
    ) {
        val mvp = FloatArray(16)
        if (look == 3) {
            System.arraycopy(model, 0, mvp, 0, 16)
        } else {
            Matrix.multiplyMM(mvp, 0, viewProjection, 0, model, 0)
        }
        GLES30.glUniformMatrix4fv(uMvp, 1, false, mvp, 0)
        GLES30.glUniformMatrix4fv(uModel, 1, false, model, 0)
        GLES30.glUniform4fv(uColor, 1, color, 0)
        GLES30.glUniform3f(uLight, light[0], light[1], light[2])
        // The floor flag and the checker flag are separate: both floor looks
        // get the contact shadow and the horizon fog; only the instrument
        // draws the 0.2 m grid.
        GLES30.glUniform1f(uChecker, if (look == 1 || look == 2) 1f else 0f)
        // The duck is painted plastic over a few metal parts; the floor is
        // matte. Live stone is a little rougher than the lab's grid.
        val material = when (look) {
            1 -> floatArrayOf(0f, 0.92f)
            2 -> floatArrayOf(0f, 0.85f)
            3 -> floatArrayOf(0f, 1f)
            else -> floatArrayOf(0.05f, 0.42f)
        }
        GLES30.glUniform2f(uMaterial, material[0], material[1])
        GLES30.glUniform4f(
            uStage,
            if (look == 2) 1f else 0f,
            if (look == 3) 1f else 0f,
            0f, 0f,
        )
        GLES30.glUniform4fv(uHalo, 1, halo, 0)
        GLES30.glUniform4fv(uDaylight, 1, daylight, 0)
        GLES30.glUniform4fv(uShadow, 1, shadow, 0)

        mesh.buffer.position(0)
        GLES30.glEnableVertexAttribArray(aPos)
        GLES30.glVertexAttribPointer(aPos, 3, GLES30.GL_FLOAT, false, 24, mesh.buffer)
        mesh.buffer.position(3)
        GLES30.glEnableVertexAttribArray(aNormal)
        GLES30.glVertexAttribPointer(aNormal, 3, GLES30.GL_FLOAT, false, 24, mesh.buffer)
        GLES30.glDrawArrays(GLES30.GL_TRIANGLES, 0, mesh.vertices)
    }

    private fun buildProgram(): Int {
        val vertex = compile(GLES30.GL_VERTEX_SHADER, VERTEX_SHADER)
        val fragment = compile(GLES30.GL_FRAGMENT_SHADER, FRAGMENT_SHADER)
        val program = GLES30.glCreateProgram()
        GLES30.glAttachShader(program, vertex)
        GLES30.glAttachShader(program, fragment)
        GLES30.glLinkProgram(program)
        val linked = IntArray(1)
        GLES30.glGetProgramiv(program, GLES30.GL_LINK_STATUS, linked, 0)
        if (linked[0] == 0) {
            Log.e("DuckRenderer", "link: ${GLES30.glGetProgramInfoLog(program)}")
        }
        GLES30.glDeleteShader(vertex)
        GLES30.glDeleteShader(fragment)
        return program
    }

    private fun compile(type: Int, source: String): Int {
        val shader = GLES30.glCreateShader(type)
        GLES30.glShaderSource(shader, source)
        GLES30.glCompileShader(shader)
        val compiled = IntArray(1)
        GLES30.glGetShaderiv(shader, GLES30.GL_COMPILE_STATUS, compiled, 0)
        if (compiled[0] == 0) {
            Log.e("DuckRenderer", "compile: ${GLES30.glGetShaderInfoLog(shader)}")
        }
        return shader
    }

    private fun identity(): FloatArray = FloatArray(16).also { Matrix.setIdentityM(it, 0) }

    private fun quaternionMatrix(w: Float, x: Float, y: Float, z: Float, out: FloatArray) {
        val xx = x * x
        val yy = y * y
        val zz = z * z
        val xy = x * y
        val xz = x * z
        val yz = y * z
        val wx = w * x
        val wy = w * y
        val wz = w * z
        out[0] = 1 - 2 * (yy + zz); out[1] = 2 * (xy + wz); out[2] = 2 * (xz - wy); out[3] = 0f
        out[4] = 2 * (xy - wz); out[5] = 1 - 2 * (xx + zz); out[6] = 2 * (yz + wx); out[7] = 0f
        out[8] = 2 * (xz + wy); out[9] = 2 * (yz - wx); out[10] = 1 - 2 * (xx + yy); out[11] = 0f
        out[12] = 0f; out[13] = 0f; out[14] = 0f; out[15] = 1f
    }

    private object STLLoader {
        fun parse(data: ByteArray): FloatArray? {
            if (data.size >= 84) {
                val count = ByteBuffer.wrap(data, 80, 4)
                    .order(ByteOrder.LITTLE_ENDIAN)
                    .int
                if (count > 0 && 84 + count * 50 == data.size) {
                    return binary(data, count)
                }
            }
            return ascii(data)
        }

        private fun binary(data: ByteArray, count: Int): FloatArray {
            val output = FloatArray(count * 18)
            val buffer = ByteBuffer.wrap(data).order(ByteOrder.LITTLE_ENDIAN)
            var out = 0
            for (triangle in 0 until count) {
                val base = 84 + triangle * 50 + 12
                val points = FloatArray(9)
                for (vertex in 0 until 3) {
                    for (axis in 0 until 3) {
                        points[vertex * 3 + axis] = buffer.getFloat(base + vertex * 12 + axis * 4)
                    }
                }
                val normal = normal(points, 0)
                for (vertex in 0 until 3) {
                    output[out++] = points[vertex * 3]
                    output[out++] = points[vertex * 3 + 1]
                    output[out++] = points[vertex * 3 + 2]
                    output[out++] = normal[0]
                    output[out++] = normal[1]
                    output[out++] = normal[2]
                }
            }
            return output
        }

        private fun ascii(data: ByteArray): FloatArray? {
            val text = String(data, Charsets.US_ASCII)
            val points = ArrayList<Float>(9)
            val output = ArrayList<Float>()
            for (line in text.lineSequence()) {
                val parts = line.trim().split(Regex("\\s+"))
                if (parts.size >= 4 && parts[0] == "vertex") {
                    points.add(parts[1].toFloatOrNull() ?: continue)
                    points.add(parts[2].toFloatOrNull() ?: continue)
                    points.add(parts[3].toFloatOrNull() ?: continue)
                    if (points.size == 9) {
                        val normal = normal(points.toFloatArray(), 0)
                        var index = 0
                        while (index < 9) {
                            output.add(points[index])
                            output.add(points[index + 1])
                            output.add(points[index + 2])
                            output.add(normal[0])
                            output.add(normal[1])
                            output.add(normal[2])
                            index += 3
                        }
                        points.clear()
                    }
                }
            }
            return if (output.isEmpty()) null else output.toFloatArray()
        }

        private fun normal(points: FloatArray, offset: Int): FloatArray {
            val ax = points[offset + 3] - points[offset]
            val ay = points[offset + 4] - points[offset + 1]
            val az = points[offset + 5] - points[offset + 2]
            val bx = points[offset + 6] - points[offset]
            val by = points[offset + 7] - points[offset + 1]
            val bz = points[offset + 8] - points[offset + 2]
            var nx = ay * bz - az * by
            var ny = az * bx - ax * bz
            var nz = ax * by - ay * bx
            val length = Math.sqrt((nx * nx + ny * ny + nz * nz).toDouble()).toFloat()
            if (length > 0f) {
                nx /= length
                ny /= length
                nz /= length
            } else {
                nx = 0f
                ny = 0f
                nz = 1f
            }
            return floatArrayOf(nx, ny, nz)
        }
    }

    private companion object {
        val VERTEX_SHADER = """
            #version 300 es
            precision highp float;
            in vec3 aPos;
            in vec3 aNormal;
            uniform mat4 uMvp;
            uniform mat4 uModel;
            out vec3 vNormal;
            out vec3 vWorld;
            out vec2 vNdc;
            void main() {
                gl_Position = uMvp * vec4(aPos, 1.0);
                vNormal = normalize(mat3(uModel[0].xyz, uModel[1].xyz, uModel[2].xyz) * aNormal);
                vWorld = (uModel * vec4(aPos, 1.0)).xyz;
                vNdc = gl_Position.xy / gl_Position.w;
            }
        """.trimIndent()

        val FRAGMENT_SHADER = """
            #version 300 es
            precision mediump float;
            in vec3 vNormal;
            in vec3 vWorld;
            in vec2 vNdc;
            uniform vec4 uColor;
            uniform vec3 uLight;
            uniform float uChecker;
            uniform vec2 uMaterial;
            // x: Instrument checkerboard; y: full-screen backdrop.
            uniform vec4 uStage;
            // xy: the duck's NDC; z: glow radius; w: aspect.
            uniform vec4 uHalo;
            // rgb: the hour's tint; w: exposure (never below 0.80).
            uniform vec4 uDaylight;
            // xy: trunk world xy (contact-shadow centre); z: strength.
            uniform vec4 uShadow;
            out vec4 fragColor;

            // A cheap analytic environment: sky above, bounce below. Gives a
            // dielectric something to reflect and a metal something to mirror.
            vec3 environment(vec3 direction) {
                float up = clamp(direction.z * 0.5 + 0.5, 0.0, 1.0);
                vec3 sky = mix(vec3(0.16, 0.19, 0.24), vec3(0.55, 0.65, 0.80), up);
                vec3 ground = vec3(0.10, 0.11, 0.13);
                float horizon = smoothstep(-0.15, 0.15, direction.z);
                return mix(ground, sky, horizon);
            }

            float distributionGGX(vec3 n, vec3 h, float roughness) {
                float a = roughness * roughness;
                float a2 = a * a;
                float ndoth = max(dot(n, h), 0.0);
                float denom = ndoth * ndoth * (a2 - 1.0) + 1.0;
                return a2 / max(3.14159265 * denom * denom, 1e-5);
            }

            float geometrySchlick(float ndotv, float roughness) {
                float k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
                return ndotv / max(ndotv * (1.0 - k) + k, 1e-5);
            }

            vec3 fresnelSchlick(float cosTheta, vec3 f0) {
                return f0 + (1.0 - f0) * pow(1.0 - cosTheta, 5.0);
            }

            void main() {
                // The room's wall (STAGE_LOOK §2.2): a vertical gradient from
                // warm-brown horizon to cold-blue-black zenith, plus a warm
                // halo behind the duck. Screen-space NDC, so it holds in both
                // orientations and at any camera distance.
                if (uStage.y > 0.5) {
                    float t = vNdc.y * 0.5 + 0.5;
                    vec3 horizon = vec3(0.102, 0.090, 0.078);  // #1A1714
                    vec3 zenith = vec3(0.047, 0.055, 0.071);   // #0C0E12
                    // The gradient starts at the floor's far edge (the same
                    // screen row on both aspects), so the fogged floor and
                    // the wall meet in the same horizon colour: one
                    // continuous room from the status bar down.
                    vec3 bg = mix(horizon, zenith, smoothstep(0.78, 1.12, t));
                    float dx = (vNdc.x - uHalo.x) * uHalo.w;
                    float dy = vNdc.y - uHalo.y;
                    float glow = exp(-(dx * dx + dy * dy)
                        / max(uHalo.z * uHalo.z, 1e-5) * 2.2);
                    bg += vec3(1.0, 0.851, 0.659) * glow * 0.08;
                    bg *= uDaylight.rgb * uDaylight.w;
                    bg = clamp(bg, 0.0, 4.0);
                    bg = mix(bg, bg / (bg + vec3(0.55)) * 1.55,
                        smoothstep(0.72, 1.6, max(max(bg.r, bg.g), bg.b)));
                    fragColor = vec4(clamp(bg, 0.0, 1.0), 1.0);
                    return;
                }

                vec3 n = normalize(vNormal);
                vec3 v = normalize(-vWorld);
                vec3 l = normalize(-uLight);
                vec3 h = normalize(l + v);

                float metalness = clamp(uMaterial.x, 0.0, 1.0);
                float roughness = clamp(uMaterial.y, 0.04, 1.0);

                // The floor's albedo: Live passes warm stone; Instrument's
                // cold checkerboard is generated here, and only there
                // (STAGE_LOOK §3: the grid is the dashboard's language).
                vec3 base = uColor.rgb;
                if (uChecker > 0.5 && uStage.x > 0.5) {
                    vec2 cell = floor(vWorld.xy / 0.2);
                    float odd = mod(cell.x + cell.y, 2.0);
                    base = mix(vec3(0.20, 0.30, 0.40), vec3(0.10, 0.20, 0.30), odd);
                }

                vec3 f0 = mix(vec3(0.04), base, metalness);
                vec3 diffuseAlbedo = base * (1.0 - metalness);

                float ndotl = max(dot(n, l), 0.0);
                float ndotv = max(dot(n, v), 1e-4);

                float d = distributionGGX(n, h, roughness);
                float g = geometrySchlick(ndotv, roughness) * geometrySchlick(ndotl, roughness);
                vec3 f = fresnelSchlick(max(dot(h, v), 0.0), f0);
                vec3 specular = (d * g) * f / max(4.0 * ndotv * ndotl, 1e-4);

                vec3 ambientDir = reflect(-v, n);
                vec3 ambient = environment(ambientDir) * mix(0.35, 1.0, metalness);
                float rim = pow(1.0 - ndotv, 3.0);

                // The hour tints the room; the duck is an object under a lamp,
                // so its key light stays neutral and the white shell keeps its
                // authored colour at any hour (mirrors Shaders.metal's split:
                // the floor takes 60% of the tint, the duck takes none).
                float isFloor = uChecker > 0.5 ? 1.0 : 0.0;
                vec3 key = mix(vec3(1.0), uDaylight.rgb, isFloor * 0.6);
                vec3 color = diffuseAlbedo * (0.45 + 0.75 * ndotl) * key
                           + specular * ndotl * key
                           + ambient * diffuseAlbedo * 0.85 * mix(vec3(1.0), key, 0.5)
                           + ambient * f0 * (0.15 + 0.85 * rim);

                // The floor's three steps: own ambient floor, then the
                // contact shadow that follows the duck, then a faint fog that
                // melts the 8 m plane into the wall.
                if (uChecker > 0.5) {
                    color = max(color, base * 0.55);
                    float radial = length(vWorld.xy - uShadow.xy);
                    float shadow = smoothstep(0.02, 0.55, radial);
                    color *= mix(1.0 - uShadow.z, 1.0, shadow);
                    // The floor's far edge fades fully into the wall colour;
                    // the backdrop above it starts at that same colour, so
                    // the 8 m plane leaves no hard horizon line.
                    float fog = smoothstep(0.9, 4.0, vWorld.y);
                    color = mix(color, vec3(0.102, 0.090, 0.078) * uDaylight.rgb, fog);
                }

                // The room takes the night's full exposure; the duck, lit by
                // the headlight, only loses a fifth of it. Same split as the
                // Metal shader.
                color *= mix(1.0, uDaylight.w, isFloor * 0.8 + 0.2);
                color = clamp(color, 0.0, 4.0);
                color = mix(
                    color,
                    color / (color + vec3(0.55)) * 1.55,
                    smoothstep(0.72, 1.6, max(max(color.r, color.g), color.b))
                );
                color = clamp(color, 0.0, 1.0);
                fragColor = vec4(color, uColor.a);
            }
        """.trimIndent()
    }
}
