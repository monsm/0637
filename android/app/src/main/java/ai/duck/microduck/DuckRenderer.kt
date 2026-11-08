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
    )

    private var program = 0
    private var uMvp = 0
    private var uModel = 0
    private var uColor = 0
    private var aPos = 0
    private var aNormal = 0
    private val meshes = mutableMapOf<String, Mesh>()
    private val instances = mutableListOf<Instance>()
    private var floor: Mesh? = null
    private var aspect = 1f
    private var lastTick = 0L
    private val yaw = Math.PI.toFloat()
    private val pitch = 0.32f
    private val distance = 0.85f

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        GLES30.glClearColor(0.055f, 0.075f, 0.10f, 1f)
        GLES30.glEnable(GLES30.GL_DEPTH_TEST)
        program = buildProgram()
        uMvp = GLES30.glGetUniformLocation(program, "uMvp")
        uModel = GLES30.glGetUniformLocation(program, "uModel")
        uColor = GLES30.glGetUniformLocation(program, "uColor")
        aPos = GLES30.glGetAttribLocation(program, "aPos")
        aNormal = GLES30.glGetAttribLocation(program, "aNormal")
        buildScene()
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
        GLES30.glViewport(0, 0, width, height)
        aspect = width.toFloat() / height.coerceAtLeast(1).toFloat()
    }

    override fun onDrawFrame(gl: GL10?) {
        val now = System.nanoTime()
        if (now - lastTick >= 20_000_000L) {
            // Keepalive: the core has robotd's 500 ms twist deadman.
            core.setTwist(input.vx, 0.0, input.wz)
            core.tick()
            lastTick = now
        }

        val status = core.status()
        val trunk = if (status.size >= 9) {
            floatArrayOf(status[3].toFloat(), status[4].toFloat(), status[5].toFloat())
        } else {
            floatArrayOf(0f, 0f, 0.12f)
        }

        GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT or GLES30.GL_DEPTH_BUFFER_BIT)
        GLES30.glUseProgram(program)

        val projection = FloatArray(16)
        val view = FloatArray(16)
        val viewProjection = FloatArray(16)
        val cosPitch = cos(pitch.toDouble()).toFloat()
        val eye = floatArrayOf(
            trunk[0] + distance * cosPitch * sin(yaw.toDouble()).toFloat(),
            trunk[1] + distance * cosPitch * cos(yaw.toDouble()).toFloat(),
            trunk[2] + distance * sin(pitch.toDouble()).toFloat(),
        )
        Matrix.perspectiveM(projection, 0, 45f, aspect, 0.01f, 100f)
        Matrix.setLookAtM(
            view, 0,
            eye[0], eye[1], eye[2],
            trunk[0], trunk[1], trunk[2],
            0f, 0f, 1f,
        )
        Matrix.multiplyMM(viewProjection, 0, projection, 0, view, 0)

        floor?.let {
            draw(it, identity(), floatArrayOf(0.14f, 0.15f, 0.18f, 1f), viewProjection)
        }

        for (instance in instances) {
            val pose = core.bodyPose(instance.body) ?: continue
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
            val model = FloatArray(16)
            Matrix.multiplyMM(model, 0, world, 0, instance.local, 0)
            draw(instance.mesh, model, instance.color, viewProjection)
        }
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
            val name = core.meshName(meshId) ?: continue
            val mesh = loadMesh(name) ?: continue

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
            instances.add(
                Instance(
                    mesh,
                    body,
                    transform,
                    floatArrayOf(info[11].toFloat(), info[12].toFloat(), info[13].toFloat(), alpha),
                )
            )
        }
        floor = buildFloor()
        Log.i("DuckRenderer", "visual geoms: ${instances.size}")
    }

    private fun loadMesh(name: String): Mesh? {
        meshes[name]?.let { return it }
        val bytes = runCatching {
            context.assets.open("robot/microduck/assets/$name.stl").use { it.readBytes() }
        }.getOrNull() ?: return null
        val floats = STLLoader.parse(bytes) ?: return null
        val buffer = ByteBuffer.allocateDirect(floats.size * 4)
            .order(ByteOrder.nativeOrder())
            .asFloatBuffer()
        buffer.put(floats).position(0)
        val mesh = Mesh(buffer, floats.size / 6)
        meshes[name] = mesh
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

    private fun draw(
        mesh: Mesh,
        model: FloatArray,
        color: FloatArray,
        viewProjection: FloatArray,
    ) {
        val mvp = FloatArray(16)
        Matrix.multiplyMM(mvp, 0, viewProjection, 0, model, 0)
        GLES30.glUniformMatrix4fv(uMvp, 1, false, mvp, 0)
        GLES30.glUniformMatrix4fv(uModel, 1, false, model, 0)
        GLES30.glUniform4fv(uColor, 1, color, 0)

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
            void main() {
                gl_Position = uMvp * vec4(aPos, 1.0);
                vNormal = normalize(mat3(uModel[0].xyz, uModel[1].xyz, uModel[2].xyz) * aNormal);
            }
        """.trimIndent()

        val FRAGMENT_SHADER = """
            #version 300 es
            precision mediump float;
            in vec3 vNormal;
            uniform vec4 uColor;
            out vec4 fragColor;
            void main() {
                vec3 normal = normalize(vNormal);
                vec3 light = normalize(vec3(-0.45, -0.65, 1.0));
                float diffuse = max(dot(normal, light), 0.0);
                fragColor = vec4(uColor.rgb * (0.30 + 0.70 * diffuse), uColor.a);
            }
        """.trimIndent()
    }
}
