package ai.duck.microduck

import android.content.Context
import java.util.Locale

/**
 * Every word the shell says, in both languages.
 *
 * The duck is the same in either; only the labels change. Chinese is the
 * default when the system language is Chinese, English otherwise, and the
 * chip in the top bar switches it.
 */
object Copy {
    private const val PREFS = "duck"
    private const val KEY = "language"

    enum class Language(val label: String) {
        English("EN"),
        Chinese("中文"),
    }

    private var cached: Language? = null

    fun load(context: Context) {
        val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY, null)
        cached = saved?.let { value -> Language.entries.firstOrNull { it.name == value } }
            ?: if (Locale.getDefault().language.startsWith("zh")) Language.Chinese else Language.English
    }

    fun save(context: Context, language: Language) {
        cached = language
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY, language.name).apply()
    }

    var language: Language
        get() = cached ?: if (Locale.getDefault().language.startsWith("zh")) Language.Chinese else Language.English
        set(value) { cached = value }

    private fun pick(en: String, zh: String) = if (language == Language.Chinese) zh else en

    // Live view
    val name get() = "MicroDuck"
    val fell get() = pick("it fell", "摔倒了")
    val waking get() = pick("waking up…", "正在唤醒…")
    val tapHint get() = pick("tap ahead to walk · long-press the duck", "点前方走路 · 长按鸭子")
    val live get() = pick("Live", "实时")
    val director get() = pick("Director", "取景")
    val time get() = pick("Time", "时间")
    val skills get() = pick("Skills", "技能")
    // Skill ring section headings: actions and robotd emotions.
    val actions get() = pick("Actions", "动作")
    val emotions get() = pick("Emotions", "情绪")
    val sit get() = pick("sit", "坐下")
    val pickUp get() = pick("pick", "抓取")
    val grab get() = pick("Hold", "拎起")
    val putDown get() = pick("Drop", "放下")
    val held get() = pick("in a hand · drag to carry", "在手里 · 拖动搬运")
    val quack get() = pick("Quack", "叫一声")
    val drive get() = pick("drive", "驱动")
    fun stick(mode: String): String = when (mode) {
        "head" -> pick("look", "看")
        "body" -> pick("pose", "姿态")
        else -> pick("drive", "驱动")
    }
    val walk get() = pick("walk", "走")
    val roller get() = pick("roller", "轮")
    val viewfinder get() = pick("VIEWFINDER", "取景器")
    val couldNotStart get() = pick("MicroDuck could not start", "MicroDuck 无法启动")
    // First-launch hint: shown once above the bottom bar, then never again.
    val dragHint get() = pick("Drag to drive · long-press the duck for skills", "拖动开车 · 长按鸭子出技能")

    // Instrument sheet
    val thisDuck get() = pick("This duck", "这只鸭子")
    val timeMachine get() = pick("Time machine", "时间机器")
    val freeze get() = pick("Freeze", "冻结")
    val resume get() = pick("Resume", "继续")
    val step get() = pick("Step", "单步")
    val rewind get() = pick("Rewind", "倒回")
    val fork get() = pick("Fork", "分叉")
    val api get() = "AI API :8765 · MCP /mcp"
    val microphone get() = pick("microphone", "麦克风")
    val denied get() = pick("denied", "未授权")
    // Spoken commands: the platform recognizer as a second, opt-in channel.
    val voiceCommands get() = pick("Voice commands", "听懂中文")
    val listening get() = pick("listening…", "聆听中…")
    val unavailable get() = pick("unavailable", "不可用")
    val behavior get() = pick("behaviour", "此刻")
    val trickBook get() = pick("Trick book", "技巧图鉴")

    // The face: the status LED, the four-light battery gauge and the
    // flashlight, all published on /state.leds.
    val face get() = pick("Face", "脸")
    val flashlight get() = pick("Flash", "手电")
    val flashlightRow get() = pick("Flashlight", "手电筒")

    /** The status LED's verdict, in the shell's language. */
    fun faceStatus(status: String): String = when (status) {
        "Starting" -> pick("Starting", "启动中")
        "Healthy" -> pick("Healthy", "正常")
        "Degraded" -> pick("Degraded", "降级")
        "Fault" -> pick("Fault", "故障")
        else -> status
    }

    /** The flashlight's colour, in the shell's language. */
    fun faceColor(color: String): String = when (color) {
        "white" -> pick("white", "白")
        "red" -> pick("red", "红")
        "green" -> pick("green", "绿")
        "blue" -> pick("blue", "蓝")
        "yellow" -> pick("yellow", "黄")
        "cyan" -> pick("cyan", "青")
        "magenta" -> pick("magenta", "品红")
        else -> color
    }

    fun bringup(code: Int): String = when (code) {
        0 -> pick("Limp", "软瘫")
        1 -> pick("Homing", "归位中")
        2 -> pick("Ready", "就绪")
        else -> "?"
    }
}

/** A skill's display name; the wire name stays the protocol's. */
fun skillLabel(name: String): String {
    if (Copy.language != Copy.Language.Chinese) return name
    return when (name) {
        "stand" -> "站起"
        "roulade" -> "翻滚"
        "kick_left" -> "左踢"
        "kick_right" -> "右踢"
        "crouch" -> "蹲下"
        "sit" -> "坐下"
        "ground_pick" -> "贴地抓取"
        // The 13 official robotd emotions.
        "angry" -> "生气"
        "curious" -> "好奇"
        "defiant" -> "不服"
        "devastated" -> "崩溃"
        "excited" -> "兴奋"
        "impatient" -> "不耐烦"
        "laugh" -> "大笑"
        "mmh" -> "嗯哼"
        "mock" -> "嘲弄"
        "no" -> "摇头"
        "sad" -> "难过"
        "yes" -> "点头"
        "yes_fast" -> "快速点头"
        else -> name
    }
}
