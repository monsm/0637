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
    private var uLight = 0
    private var uChecker = 0
    private var uMaterial = 0
    private var aPos = 0
    private var aNormal = 0
    private val meshes = mutableMapOf<Int, Mesh>()
    private val instances = mutableListOf<Instance>()
    private var floor: Mesh? = null
    private var aspect = 1f
    /// The body model this renderer was built for; a mode switch changes it.
    private var builtGeneration = -1L
    private val yaw = Math.PI.toFloat()
    private val pitch = 0.32f
    private val distance = 0.85f

    private var viewportWidth = 0
    private var viewportHeight = 0
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
                floor = null
            }
            buildScene()
            builtGeneration = generation
        }
        // The 50 Hz loop runs on DuckLoop's own scheduler; this only draws.
        input.frames.incrementAndGet()
        val status = core.status()
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

        // Director mode: transparent and black, so the camera behind shows
        // through and the studio colour does not veil it.
        if (input.cameraActive) {
            GLES30.glClearColor(0f, 0f, 0f, 0f)
        } else {
            GLES30.glClearColor(0.055f, 0.075f, 0.10f, 1f)
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
        // so whatever the camera sees is lit. A fixed world light left the
        // front of the duck almost black.
        val headlight = floatArrayOf(
            eye[0] - trunk[0],
            eye[1] - trunk[1],
            eye[2] - trunk[2],
        )
        val lightLength = kotlin.math.sqrt(
            headlight[0] * headlight[0] + headlight[1] * headlight[1] + headlight[2] * headlight[2]
        ).coerceAtLeast(1e-6f)
        headlight[0] /= lightLength
        headlight[1] /= lightLength
        headlight[2] /= lightLength

        // The studio floor would hide the room (and the duck's feet) over a
        // camera image. Physics ground stays at z=0 either way.
        if (!input.cameraActive) {
            floor?.let {
                draw(it, identity(), floatArrayOf(0.14f, 0.15f, 0.18f, 1f), headlight, 1f, viewProjection)
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
            val model = FloatArray(16)
            Matrix.multiplyMM(model, 0, world, 0, instance.local, 0)
            draw(instance.mesh, model, instance.color, headlight, 0f, viewProjection)
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
        light: FloatArray,
        checker: Float,
        viewProjection: FloatArray,
    ) {
        val mvp = FloatArray(16)
        Matrix.multiplyMM(mvp, 0, viewProjection, 0, model, 0)
        GLES30.glUniformMatrix4fv(uMvp, 1, false, mvp, 0)
        GLES30.glUniformMatrix4fv(uModel, 1, false, model, 0)
        GLES30.glUniform4fv(uColor, 1, color, 0)
        GLES30.glUniform3f(uLight, light[0], light[1], light[2])
        GLES30.glUniform1f(uChecker, checker)
        // The duck is painted plastic over a few metal parts; the floor is matte.
        if (checker > 0.5f) {
            GLES30.glUniform2f(uMaterial, 0f, 0.85f)
        } else {
            GLES30.glUniform2f(uMaterial, 0.05f, 0.42f)
        }

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
            void main() {
                gl_Position = uMvp * vec4(aPos, 1.0);
                vNormal = normalize(mat3(uModel[0].xyz, uModel[1].xyz, uModel[2].xyz) * aNormal);
                vWorld = (uModel * vec4(aPos, 1.0)).xyz;
            }
        """.trimIndent()

        val FRAGMENT_SHADER = """
            #version 300 es
            precision mediump float;
            in vec3 vNormal;
            in vec3 vWorld;
            uniform vec4 uColor;
            uniform vec3 uLight;
            uniform float uChecker;
            uniform vec2 uMaterial;
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
                vec3 n = normalize(vNormal);
                vec3 v = normalize(-vWorld);
                vec3 l = normalize(-uLight);
                vec3 h = normalize(l + v);

                float metalness = clamp(uMaterial.x, 0.0, 1.0);
                float roughness = clamp(uMaterial.y, 0.04, 1.0);

                vec3 base = uColor.rgb;
                if (uChecker > 0.5) {
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

                // The duck is a painted toy: most of what you see is diffuse.
                vec3 color = diffuseAlbedo * (0.45 + 0.75 * ndotl)
                           + specular * ndotl
                           + ambient * diffuseAlbedo * 0.85
                           + ambient * f0 * (0.15 + 0.85 * rim);

                if (uChecker > 0.5) {
                    float radial = length(vWorld.xy);
                    float shadow = smoothstep(0.02, 0.55, radial);
                    color *= mix(0.55, 1.0, shadow);
                    // A ground plane seen from a shallow angle gets almost no
                    // key light; give it its own ambient floor.
                    color = max(color, base * 0.55);
                }

                // Keep the albedo: only highlights get a soft knee, or the
                // duck's orange washes out to beige.
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
