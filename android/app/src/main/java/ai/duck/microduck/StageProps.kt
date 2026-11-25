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
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * Props on the duck's floor: a ball it can kick, a block it can pick, a
 * cushion it can sit on.
 *
 * Every prop answers with the duck's own body — the kick is the official
 * ball_kick policy, the pick the ground_pick scheduler, the sit the loop's
 * own sit. Nothing here invents a verb; the prop is a target, not an
 * animation. The duck also reaches for props on its own: the idle
 * look-around's real head command (`/state.idle.head`, the glance the core
 * already produces while standing) lights the prop it is looking at, and a
 * kick, pick or sit it actually started (from the ring, a block rule, a
 * neighbour or its own AI) moves the matching prop. A sleeping duck answers
 * nothing — the core owns sleep.
 *
 * The layer takes no touches: the stage gesture reads taps against the same
 * zones ([StagePropsLayout.hit]), so the first screen's seven-clickable
 * budget is untouched and TalkBack still hears one stage element.
 */

/** The props' real cues, from /state: the idle glance and the vet counters. */
data class PropCues(
    val idleYaw: Double = 0.0,
    val kickLeft: Int = 0,
    val kickRight: Int = 0,
    val groundPicks: Int = 0,
    val sitDowns: Int = 0,
)

/**
 * Read the cues the props answer to: the idle look-around's own head command
 * (yaw is the third item, the upstream `[neck, head, yaw, roll]` order) while
 * it has the head, and the vet book's real counters. A counter only ever grows
 * when the duck really moved — the prop is the consequence, never the claim.
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
    )
} catch (_: Throwable) {
    null
}

enum class StageProp { Ball, Block, Cushion }

/**
 * Where a tap lands among the props. The zones sit beside and under the duck
 * body (x 0.25–0.75, y 0.33–0.68), so the body keeps its own taps and the
 * props keep the floor.
 */
object StagePropsLayout {
    fun hit(x: Float, y: Float): StageProp? = when {
        x < 0.25f && y > 0.58f -> StageProp.Ball
        x > 0.75f && y > 0.58f -> StageProp.Block
        x > 0.36f && x < 0.64f && y > 0.70f -> StageProp.Cushion
        else -> null
    }
}

/**
 * A prop was tapped: the duck does its real verb toward it. The prop's
 * response comes from the vet book's counter a beat later, so the screen
 * cannot claim a move the core refused. Returns the skill name only when the
 * core accepted it — a refusal leaves the top bar unchanged. (The C entries
 * answer differently: `skill` is 1 started / 0 refused, while `ground_pick`
 * is 0 done / -1 refused.)
 */
fun reactProp(prop: StageProp, core: DuckCore): String = when (prop) {
    StageProp.Ball -> if (core.skill("kick_left") != 0) "kick_left" else ""
    StageProp.Block -> if (core.groundPick() == 0) "ground_pick" else ""
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
    }

    val spec = if (reduce) tween<Float>(durationMillis = 0) else DuckMotion.slow()
    val flight by animateFloatAsState(ballFlight, spec, label = "prop-ball")
    val lift by animateFloatAsState(if (carried) 1f else 0f, spec, label = "prop-block")
    val glow by animateFloatAsState(if (warm) 1f else 0f, spec, label = "prop-cushion")
    val look by animateFloatAsState(
        when {
            lookedAtYaw > 0.25 -> 1f
            lookedAtYaw < -0.25 -> -1f
            else -> 0f
        },
        spec,
        label = "prop-look",
    )

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

        // The ball, left of the duck: a kick sends it that way and it fades.
        val ball = Offset(
            x = 0.16f * w + flight * w * 0.16f,
            y = 0.68f * h - kotlin.math.abs(flight) * h * 0.09f,
        )
        val ballGlow = look < -0.5f || flight != 0f
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

        // The block, right of the duck: a pick lifts it off the floor.
        val block = Offset(0.86f * w, 0.68f * h - lift * h * 0.06f)
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

        // The cushion, under the duck: a sit warms it.
        val cushion = Offset(0.5f * w, 0.75f * h)
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
        if (glow > 0.01f) {
            drawRoundRect(
                color = DuckTheme.AccentAction.copy(alpha = 0.35f * glow),
                topLeft = cushion - Offset(37 * unit, 8 * unit),
                size = Size(74 * unit, 16 * unit),
                cornerRadius = CornerRadius(8 * unit),
            )
        }
    }
}
