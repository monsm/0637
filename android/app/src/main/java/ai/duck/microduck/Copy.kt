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
    val tapHint get() = pick(
        "drag to walk · long-press for skills · tap the duck to pet",
        "拖动走路 · 长按出技能 · 点鸭子摸摸",
    )
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
    val feed get() = pick("Feed", "喂食")
val awayTitle get() = pick("While you were away", "你不在时")
fun awayRituals(n: Int) = pick("played by itself ×$n", "自己演了 $n 出")
fun awayFalls(n: Int) = pick("fell ×$n", "摔了 $n 次")
fun awayFeeds(n: Int) = pick("ate ×$n", "吃了 $n 次")
fun awayPets(n: Int) = pick("petted ×$n", "被摸 $n 次")
fun awayMoods(n: Int) = pick("$n moods", "$n 种心情")
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
    val dragHint get() = pick(
        "Drag to walk · long-press for skills · tap the duck to pet",
        "拖动走路 · 长按出技能 · 点鸭子摸摸",
    )

    // Instrument sheet
    val thisDuck get() = pick("This duck", "这只鸭子")
    val close get() = pick("Close", "关闭")
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
    // The companion layer: three numbers and a nap. "Fullness" is shown so a
    // full bar means fed, not starving.
    val care get() = pick("Companion", "陪伴")
    val fullness get() = pick("fullness", "饱腹")
    val mood get() = pick("mood", "心情")
    val bond get() = pick("bond", "亲密")
    val dozing get() = pick("dozing", "打盹中")
    fun careLine(hunger: Double, mood: Double, bond: Double, asleep: Boolean): String {
        val full = ((1.0 - hunger) * 100).toInt()
        val moodPercent = (((mood + 1) / 2) * 100).toInt()
        if (asleep) return "$dozing · $fullness $full%"
        if (hunger > 0.65) return pick("hungry · fullness $full%", "饿了 · 饱腹 $full%")
        if (mood < -0.2) return pick(
            "a little sad · bond ${(bond * 100).toInt()}%",
            "有点难过 · 亲密 ${(bond * 100).toInt()}%",
        )
        return pick("content · mood $moodPercent%", "心情不错 · 心情 $moodPercent%")
    }
    val trickBook get() = pick("Trick book", "技巧图鉴")
    // Daily quests.
    val quests get() = pick("Today's quests", "今日小挑战")
    val questDone get() = pick("Done", "完成")
    fun questStreak(days: Int) = pick("$days-day streak", "连续 $days 天")
    val questAllDone get() = pick("All done — the duck is proud of you!", "全部完成，鸭子为你骄傲！")
    // Dance Studio.
    val dance get() = pick("Dance Studio", "编舞铺")
    val danceHint get() = pick(
        "Press Record, then do anything: a skill, a quack, a treat. Press Play and the duck repeats it.",
        "按录制，然后做动作：技能、叫声、喂食。按播放，鸭子照着重演。",
    )
    val danceEmpty get() = pick("No steps yet", "还没有动作")
    val danceRecord get() = pick("Record", "录制")
    val danceStopRecord get() = pick("Stop recording", "停止录制")
    val dancePlay get() = pick("Play", "播放")
    val danceStop get() = pick("Stop", "停止")
    val danceTry get() = pick("Try these while recording:", "录制时点这些：")
    val danceStepSound get() = pick("Quack", "叫一声")
    // Morning postcard.
    val postcard get() = pick("Morning postcard", "早安明信片")
    val postcardKeep get() = pick("Keep", "收藏")
    val postcardShare get() = pick("Share", "分享")
    val postcardQuiet get() = pick("A quiet night.", "安静的一夜。")
    val postcardShelf get() = pick("Postcards", "明信片")
    val postcardSaved get() = pick("Kept to the shelf", "已收藏")
    val postcardNone get() = pick("No postcards yet", "还没有明信片")
    fun postcardMetres(m: Double) = pick("walked ${"%.1f".format(m)} m", "走了 ${"%.1f".format(m)} 米")
    // Mood dex.
    val dex get() = pick("Mood dex", "情绪图鉴")
    val dexMoves get() = pick("Moves", "动作")
    val dexMoods get() = pick("Moods", "情绪")
    val dexNever get() = pick("not yet", "还没见过")
    val dexFirst get() = pick("first", "初次")
    // The vet book, and the file it exports.
    val lifeRecord get() = pick("Life record", "终身记录")
    val walked get() = pick("walked", "走过")
    val rolls get() = pick("rolls", "翻滚")
    val pickups get() = pick("picked up", "被抱起")
    val exportVetbook get() = pick("Export vet book", "导出记录")
    val shareVetbook get() = pick("Share vet book", "分享记录")
    // The Director photo.
    val photoTitle get() = pick("Your duck, in your room", "你的鸭子，在你房间")
    val photoTake get() = pick("Take photo", "拍照")
    val photoShort get() = pick("Photo", "拍照")
    val savePhoto get() = pick("Save", "保存")
    val saved get() = pick("Saved", "已保存")
    val saveFailed get() = pick("No photo access", "无相册权限")
    val share get() = pick("Share", "分享")
    fun heardSound(kind: String): String = when (kind) {
        "clap" -> pick("heard a clap!", "听到拍手！")
        "whistle" -> pick("heard a whistle!", "听到口哨！")
        else -> kind
    }
    // The block rules a child programs.
    val blocks get() = pick("Blocks", "积木规则")
    val blocksShort get() = pick("Blocks", "积木")
    val ruleWhen get() = pick("When", "当")
    val ruleThen get() = pick("then", "就")
    val addRule get() = pick("Add", "添加")
    val ruleChain get() = pick("then also", "然后再")
    val ruleChainAdd get() = pick("Add step", "加一步")
    val ruleLimit get() = pick("Full (8/8)", "已满（8/8）")
    val deleteRule get() = pick("Delete", "删除")
    val rulesEmpty get() = pick(
        "No rules yet. Build one: when I clap, roll over.",
        "还没有规则。拼一个：拍手，就翻滚。",
    )
    fun ruleTrigger(trigger: String): String = when (trigger) {
        "clap" -> pick("clap", "拍手")
        "whistle" -> pick("whistle", "吹口哨")
        "pet" -> pick("pet me", "摸摸我")
        else -> trigger
    }
    fun ruleKind(action: String): String = when (action) {
        "skill" -> pick("move", "动作")
        "emotion" -> pick("mood", "心情")
        "sound" -> pick("sound", "声音")
        else -> action
    }
    // Other ducks on the same Wi-Fi.
    val peers get() = pick("Neighbours", "邻居")
    val peerSayHi get() = pick("Say hi", "打招呼")
    val peerSayAll get() = pick("Say hi to everyone", "向大家问好")
    val peerFind get() = pick("Find ducks on this Wi-Fi", "在 Wi-Fi 里找鸭子")
    val peersEmpty get() = pick("Looking for ducks on this Wi-Fi…", "正在同一 Wi-Fi 里找鸭子…")
    val peerRepliedLabel get() = pick("answered", "已回应")
    val peerNew get() = pick("new", "新朋友")
    fun peerGreeted(name: String) = pick("$name says hi!", "$name 向你问好！")
    fun peerReplied(name: String) = pick("$name answered!", "$name 回应了！")
    // Bring your own model.
    val ai get() = pick("AI companion", "AI 伙伴")
    val aiOn get() = pick("watching", "观察中")
    val aiOff get() = pick("off", "未开启")
    val aiThinking get() = pick("thinking…", "思考中…")
    val aiOffline get() = pick("no answer", "没有回应")
    val aiNeedEndpoint get() = pick("Set an endpoint first", "请先填写接口地址")
    val aiEndpoint get() = pick("Endpoint", "接口地址")
    val aiModel get() = pick("Model", "模型")
    val aiKey get() = pick("API key", "密钥")
    val aiInterval get() = pick("Every (seconds)", "每隔（秒）")
    val aiPersona get() = pick("Persona", "性格")
    val aiThinkNow get() = pick("Think now", "现在想")
    val aiSave get() = pick("Save", "保存")
    val aiHint get() = pick(
        "Any OpenAI-compatible endpoint, including a local Ollama or LM Studio server.",
        "任何兼容 OpenAI 的接口，包括本机的 Ollama 或 LM Studio。",
    )
    val aiNote get() = pick(
        "The duck's state and the last words it heard go to the endpoint you choose. Your key stays on this phone.",
        "鸭子的状态和它最后听到的话会发给你填写的接口。密钥只保存在这台手机上。",
    )
    val showMore get() = pick("More", "更多")
    val showLess get() = pick("Less", "收起")

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
