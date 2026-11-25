package ai.duck.microduck

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Props on the duck's floor: a ball it can kick, blocks it can pick, a
 * cushion it can sit on, a hoop it can roll through, and a mirror it can
 * look into.
 *
 * Every prop answers with the duck's own body — the kick is the official
 * ball_kick policy, the pick the ground_pick scheduler, the sit the loop's
 * own sit, the roll the official roulade. Nothing here invents a verb; the
 * prop is a target, not an animation. The duck also reaches for props on its
 * own: the idle look-around's real head command lights the prop it is
 * looking at, a kick/pick/sit/roll it actually started (from the ring, a
 * block rule, a neighbour or its own AI) moves the matching prop, and its
 * own emotions reach them — a curious duck's mirror glints, an excited one
 * twirls the hoop, a laughing or sad one warms the cushion. A sleeping duck
 * answers nothing — the core owns sleep.
 *
 * The layer takes no touches: the stage gesture reads taps against the same
 * zones ([StagePropsLayout.hit]), so the first screen's seven-clickable
 * budget is untouched and TalkBack still hears one stage element.
 */

/** The props' real cues, from /state: the idle glance, the counters, and
 * the emotion timeline the duck is in right now. */
data class PropCues(
    val idleYaw: Double = 0.0,
    val kickLeft: Int = 0,
    val kickRight: Int = 0,
    val groundPicks: Int = 0,
    val sitDowns: Int = 0,
    val rolls: Int = 0,
    val emotion: String = "",
)

/**
 * Read the cues the props answer to: the idle look-around's own head command
 * (yaw is the third item, the upstream `[neck, head, yaw, roll]` order) while
 * it has the head, the vet book's real counters, and the emotion timeline
 * (`/state.emotion.name`, "" when none plays). Every value only changes when
 * the duck really did something — the prop is the consequence, never the
 * claim.
 */
fun parsePropCues(text: String): PropCues? = try {
    val root = JSONObject(text)
    val life = root.optJSONObject("life")
    val skills = life?.optJSONObject("skills")
    val idle = root.optJSONObject("idle")
    val takeover = idle?.optDouble("takeover", 0.0) ?: 0.0
    val head = idle?.optJSONArray("head")
    PropCues(
        idleYaw = if (takeover > 0.1 && head != null && head.length() >= 3) {
            head.optDouble(2, 0.0)
        } else {
            0.0
        },
        kickLeft = skills?.optInt("kick_left", 0) ?: 0,
        kickRight = skills?.optInt("kick_right", 0) ?: 0,
        groundPicks = life?.optInt("ground_picks", 0) ?: 0,
        sitDowns = life?.optInt("sit_downs", 0) ?: 0,
        rolls = skills?.optInt("roulade", 0) ?: 0,
        emotion = root.optJSONObject("emotion")?.optString("name", "") ?: "",
    )
} catch (_: Throwable) {
    null
}

enum class StageProp { Ball, Hoop, Block, Mirror, Cushion }

/**
 * Where a tap lands among the props. The zones sit beside and under the duck
 * body (x 0.25–0.75, y 0.33–0.68), so the body keeps its own taps and the
 * props keep the floor. Two rows per side: the upper row is the ball/block,
 * the lower row the hoop/mirror.
 */
object StagePropsLayout {
    fun hit(x: Float, y: Float): StageProp? = when {
        x < 0.25f && y >= 0.50f && y < 0.70f -> StageProp.Ball
        x < 0.25f && y >= 0.70f -> StageProp.Hoop
        x > 0.75f && y >= 0.50f && y < 0.70f -> StageProp.Block
        x > 0.75f && y >= 0.70f -> StageProp.Mirror
        x > 0.34f && x < 0.66f && y >= 0.70f -> StageProp.Cushion
        else -> null
    }
}

/**
 * A prop was tapped: the duck does its real verb toward it. The prop's
 * response comes from the vet book's counter (or the emotion timeline) a
 * beat later, so the screen cannot claim a move the core refused. Returns the
 * skill name only when the core accepted it — a refusal leaves the top bar
 * unchanged. (The C entries answer differently: `skill` is 1 started / 0
 * refused, while `ground_pick` is 0 done / -1 refused.)
 */
fun reactProp(prop: StageProp, core: DuckCore): String = when (prop) {
    StageProp.Ball -> if (core.skill("kick_left") != 0) "kick_left" else ""
    StageProp.Hoop -> if (core.skill("roulade") != 0) "roulade" else ""
    StageProp.Block -> if (core.groundPick() == 0) "ground_pick" else ""
    StageProp.Mirror -> if (core.skill("curious") != 0) "curious" else ""
    StageProp.Cushion -> if (core.skill("sit") != 0) "sit" else ""
}

@Composable
fun StagePropsStage(
    cues: PropCues,
    asleep: Boolean,
    modifier: Modifier = Modifier,
) {
    val reduce = readReduceMotion(LocalView.current)
    var ballFlight by remember { mutableStateOf(0f) }
    var carried by remember { mutableStateOf(false) }
    var warm by remember { mutableStateOf(false) }
    var hoopSpin by remember { mutableStateOf(0f) }
    var lookedAtYaw by remember { mutableStateOf(0.0) }
    var last by remember { mutableStateOf(PropCues()) }

    LaunchedEffect(cues, asleep) {
        val before = last
        last = cues
        if (asleep) {
            lookedAtYaw = 0.0
            return@LaunchedEffect
        }
        if (cues.idleYaw != before.idleYaw) lookedAtYaw = cues.idleYaw
        if (cues.kickLeft > before.kickLeft) {
            ballFlight = -1f
            launch { delay(800); if (ballFlight == -1f) ballFlight = 0f }
        }
        if (cues.kickRight > before.kickRight) {
            ballFlight = 1f
            launch { delay(800); if (ballFlight == 1f) ballFlight = 0f }
        }
        if (cues.groundPicks > before.groundPicks) {
            carried = true
            launch { delay(1000); carried = false }
        }
        if (cues.sitDowns > before.sitDowns) {
            warm = true
            launch { delay(2000); warm = false }
        }
        if (cues.rolls > before.rolls) {
            hoopSpin = 1f
            launch { delay(1200); hoopSpin = 0f }
        }
        // The duck's own excitement reaches the hoop even when the emotion
        // came from a rule, an achievement or a neighbour.
        if (cues.emotion == "excited" && before.emotion != "excited") {
            hoopSpin = 1f
            launch { delay(1200); hoopSpin = 0f }
        }
    }

    val spec = if (reduce) tween<Float>(durationMillis = 0) else DuckMotion.slow()
    val flight by animateFloatAsState(ballFlight, spec, label = "prop-ball")
    val lift by animateFloatAsState(if (carried) 1f else 0f, spec, label = "prop-block")
    val spin by animateFloatAsState(hoopSpin, spec, label = "prop-hoop")
    val warmScale by animateFloatAsState(if (warm) 1f else 0f, spec, label = "prop-cushion")
    val look by animateFloatAsState(
        when {
            lookedAtYaw > 0.25 -> 1f
            lookedAtYaw < -0.25 -> -1f
            else -> 0f
        },
        spec,
        label = "prop-look",
    )

    // The duck's own emotion, from /state: the same cues the animation reads.
    val emotion = if (asleep) "" else cues.emotion
    val cushionWarm = warmScale > 0.01f || emotion == "laugh" || emotion == "sad"

    Canvas(modifier) {
        val w = size.width
        val h = size.height
        val unit = 1.dp.toPx()

        fun halo(center: Offset, radius: Float) {
            drawCircle(
                brush = Brush.radialGradient(
                    listOf(DuckTheme.AccentAction.copy(alpha = 0.28f), Color.Transparent),
                    center = center,
                    radius = radius,
                ),
                radius = radius,
                center = center,
            )
        }

        // The ball, upper left: a kick sends it that way and it fades.
        val ball = Offset(
            x = 0.14f * w + flight * w * 0.16f,
            y = 0.62f * h - kotlin.math.abs(flight) * h * 0.09f,
        )
        val ballGlow = look < -0.5f || flight != 0f || emotion == "excited"
        if (ballGlow) halo(ball, 26 * unit)
        drawCircle(
            brush = Brush.radialGradient(
                listOf(DuckTheme.AccentAction, DuckTheme.AccentAction.copy(alpha = 0.55f)),
                center = ball - Offset(4 * unit, 5 * unit),
                radius = 18 * unit,
            ),
            radius = 15 * unit,
            center = ball,
            alpha = 1f - kotlin.math.abs(flight) * 0.4f,
        )
        drawCircle(
            color = Color.White.copy(alpha = 0.65f),
            radius = 15 * unit,
            center = ball,
            style = Stroke(width = 1.6f * unit),
            alpha = 1f - kotlin.math.abs(flight) * 0.4f,
        )
        drawLine(
            color = Color.White.copy(alpha = 0.5f),
            start = ball - Offset(11 * unit, 4 * unit),
            end = ball + Offset(11 * unit, -4 * unit),
            strokeWidth = 1.8f * unit,
            alpha = 1f - kotlin.math.abs(flight) * 0.4f,
        )

        // The hoop, lower left: a brass ring that twirls on a real roll.
        val hoop = Offset(0.14f * w, 0.76f * h)
        val hoopGlow = look < -0.5f || spin > 0.01f || emotion == "excited"
        if (hoopGlow) halo(hoop, 24 * unit)
        rotate(degrees = spin * 140f, pivot = hoop) {
            drawCircle(
                brush = Brush.linearGradient(
                    listOf(DuckTheme.Brass, DuckTheme.BrassDeep),
                    start = hoop - Offset(15 * unit, 15 * unit),
                    end = hoop + Offset(15 * unit, 15 * unit),
                ),
                radius = 14 * unit,
                center = hoop,
                style = Stroke(width = 3 * unit),
            )
        }

        // The block, upper right: a pick lifts it off the floor.
        val block = Offset(0.86f * w, 0.62f * h - lift * h * 0.06f)
        val blockGlow = look > 0.5f || carried
        if (blockGlow) halo(block, 22 * unit)
        drawRoundRect(
            brush = Brush.verticalGradient(
                listOf(DuckTheme.Brass, DuckTheme.BrassDeep),
                startY = block.y - 12 * unit,
                endY = block.y + 12 * unit,
            ),
            topLeft = block - Offset(12 * unit, 12 * unit),
            size = Size(24 * unit, 24 * unit),
            cornerRadius = CornerRadius(4 * unit),
            alpha = if (carried) 0.35f else 1f,
        )
        drawCircle(
            color = Color.Black.copy(alpha = 0.35f),
            radius = 3 * unit,
            center = block,
            alpha = if (carried) 0.2f else 1f,
        )

        // The mirror, lower right: a small disc that glints for a curious duck.
        val mirror = Offset(0.86f * w, 0.76f * h)
        val mirrorGlow = look > 0.5f || emotion == "curious"
        if (mirrorGlow) halo(mirror, 22 * unit)
        drawCircle(
            brush = Brush.radialGradient(
                listOf(Color.White.copy(alpha = 0.92f), DuckTheme.Ceramic.copy(alpha = 0.55f)),
                center = mirror - Offset(3 * unit, 4 * unit),
                radius = 16 * unit,
            ),
            radius = 13 * unit,
            center = mirror,
        )
        drawCircle(
            color = DuckTheme.Brass,
            radius = 13 * unit,
            center = mirror,
            style = Stroke(width = 2 * unit),
        )
        drawLine(
            color = Color.White.copy(alpha = 0.75f),
            start = mirror - Offset(6 * unit, 6 * unit),
            end = mirror + Offset(1 * unit, -1 * unit),
            strokeWidth = 1.6f * unit,
        )

        // The cushion, centre bottom: a sit, or the duck's own laugh or
        // sadness, warms it.
        val cushion = Offset(0.5f * w, 0.76f * h)
        drawRoundRect(
            brush = Brush.verticalGradient(
                listOf(
                    DuckTheme.Ceramic.copy(alpha = 0.9f),
                    DuckTheme.Ceramic.copy(alpha = 0.55f),
                ),
                startY = cushion.y - 8 * unit,
                endY = cushion.y + 8 * unit,
            ),
            topLeft = cushion - Offset(37 * unit, 8 * unit),
            size = Size(74 * unit, 16 * unit),
            cornerRadius = CornerRadius(8 * unit),
        )
        if (cushionWarm) {
            drawRoundRect(
                color = DuckTheme.AccentAction.copy(alpha = 0.35f),
                topLeft = cushion - Offset(37 * unit, 8 * unit),
                size = Size(74 * unit, 16 * unit),
                cornerRadius = CornerRadius(8 * unit),
            )
        }
    }
}
