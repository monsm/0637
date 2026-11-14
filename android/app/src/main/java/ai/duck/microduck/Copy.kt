package ai.duck.microduck

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.util.Locale
import kotlin.math.roundToInt

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
        English("EN") {
            override val tag get() = "en-US"
        },
        Chinese("中文") {
            override val tag get() = "zh-CN"
        };

        /// The BCP-47 tag the speech recogniser listens in. The core's verb
        /// table accepts both languages, but a zh-CN recogniser transcribes
        /// English as Chinese noise, so the ear must match the screen.
        abstract val tag: String
    }

    // Compose snapshot state, not a plain global: a composable that reads a
    // Copy getter must recompose when the chip flips the language. The old
    // `languageTick` only invalidated Shell, and Compose skipped children
    // whose parameters had not changed — the thumb deck stayed English.
    private var cached by mutableStateOf<Language?>(null)

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
    val waking get() = pick("waking the duck…", "正在唤醒鸭子…")
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
    val held get() = pick("in a hand", "在手里")
    val feed get() = pick("Feed", "喂食")
val awayTitle get() = pick("While you were away", "你不在时")
fun awayRituals(n: Int) = pick("played by itself ×$n", "自己演了 $n 出")
fun awayFalls(n: Int) = pick("fell ×$n", "摔了 $n 次")
fun awayFeeds(n: Int) = pick("ate ×$n", "吃了 $n 次")
fun awayPets(n: Int) = pick("petted ×$n", "被摸 $n 次")
fun awayMoods(n: Int) = pick("$n moods", "$n 种心情")
    // The duck's diary: today's real record, and the paragraph the person's
    // own model writes from it. Without a model the facts stand alone.
    val diary get() = pick("Ducky diary", "鸭子日记")
    val diaryFactsTitle get() = pick("Today, for real", "今天的真实记录")
    val diaryModelTitle get() = pick("Your model wrote", "你的模型写道")
    val diaryBusy get() = pick("Writing…", "正在写…")
    val diaryEmpty get() = pick("Nothing happened yet today.", "今天还什么都没发生。")
    val diaryNeedModel get() = pick(
        "Set a model endpoint in AI companion to hear the day in the duck's own words.",
        "在 AI 伙伴里配置接口，就能听鸭子用自己的话讲这一天。",
    )
    fun diaryGifts(n: Int) = pick("$n gift(s) from neighbours", "收到邻居 $n 份礼物")
val quack get() = pick("Quack", "叫一声")
    val drive get() = pick("drive", "驱动")
    fun stick(mode: String): String = when (mode) {
        "head" -> pick("look", "看")
        "body" -> pick("pose", "姿态")
        "path" -> pathMode
        else -> pick("drive", "驱动")
    }
    val walk get() = pick("walk", "走")
    val roller get() = pick("roller", "轮")
    // Path mode: draw the walk on the stage, then let the duck follow it.
    val pathMode get() = pick("Path", "画路走")
    val pathGo get() = pick("Walk it", "走这条路")
    val pathClear get() = pick("Clear", "清掉")
    val pathCancel get() = pick("Stop", "不走了")
    fun pathStep(step: Int, total: Int) = pick("Step $step of $total", "第 $step 步，共 $total 步")
    val pathDone get() = pick("Walked the whole path", "走完全程啦")
    val viewfinder get() = pick("VIEWFINDER", "取景器")
    val couldNotStart get() = pick("MicroDuck could not start", "MicroDuck 无法启动")
    /** Startup failure in words a person can act on. The raw text is a log line. */
    fun humanError(raw: String): String {
        val lower = raw.lowercase()
        if (lower.contains("bundle") || lower.contains("resource") ||
            lower.contains("no such file") || lower.contains("not found")
        ) {
            return pick(
                "The app's files are incomplete. Reinstall the app.",
                "应用资源不完整，请重新安装",
            )
        }
        if (lower.contains("metal") || lower.contains("render") || lower.contains("shader") ||
            lower.contains("gpu") || lower.contains("opengl")
        ) {
            return pick(
                "The picture could not start on this phone. Open the app again.",
                "画面没能启动，请重新打开 App",
            )
        }
        return pick(
            "The duck could not wake up. Open the app again.",
            "鸭子没能醒来，请重新打开 App",
        )
    }
    val retry get() = pick("Try again", "重试")
    // First-launch hint: shown once above the bottom bar, then never again.
    val firstLaunchHint get() = pick(
        "Drag to walk · long-press for skills · tap the duck to pet",
        "拖动走路 · 长按出技能 · 点鸭子摸摸",
    )

    // Instrument sheet
    val thisDuck get() = pick("This duck", "这只鸭子")
    // The one More drawer (B4): four groups, at most four rows each.
    val more get() = pick("More", "更多")
    val groupPlay get() = pick("Play", "玩")
    val groupTogether get() = pick("Together", "一起")
    val groupRecord get() = pick("Record", "记录")
    val dailyQuests get() = pick("Daily quests", "每日任务")
    val stickModes get() = pick("Controls", "操控方式")
    val pathRowNote get() = pick("Draw a route, then walk it", "画一条路，让它走")
    val blocksRowNote get() = pick("When this, then that", "当……就……")
    val danceRowNote get() = pick("Record a move, then replay it", "录下来，再重演")
    val peersRowNote get() = pick("Chorus · gifts · nearby", "合唱 · 互送礼物 · 附近")
    val questsRowNote get() = pick("Three small goals a day", "每天三个小目标")
    val dexRowNote get() = pick("Moods you have met", "见过多少种心情")
    val languageRow get() = pick("Language", "语言")
    val nightRowNote get() = pick("Let it sleep", "让鸭子睡下")
    val morningRowNote get() = pick("Wake the duck", "叫醒鸭子")
    val tiltLook get() = pick("Tilt to look", "倾斜看")
    // Keys the iOS dictionary already had: the instrument, the face and the
    // phone's own sensors. Values copied verbatim so the two sides agree.
    val battery get() = pick("pack", "电池")
    val branches get() = pick("branches", "分叉数")
    val bringup get() = pick("bring-up", "启动阶段")
    val camera get() = pick("camera", "相机")
    val cameraUnavailable get() = pick("NO CAMERA", "无相机")
    val danceSaved get() = pick("Saved on this phone", "已存在这台手机")
    val danceStepPet get() = pick("Pet", "摸摸")
    val depth get() = pick("depth", "深度")
    val fallen get() = pick("fallen", "已摔倒")
    val gain get() = pick("gain", "增益")
    val gauge get() = pick("gauge", "电量灯")
    val gravityZ get() = pick("gravity z", "重力 z")
    val heard get() = pick("heard", "听到")
    val imu get() = pick("IMU warm", "IMU 预热")
    val imuWarming get() = pick("IMU warming", "IMU 预热中")
    val ledStatus get() = pick("status", "状态灯")
    val menu get() = pick("Menu", "菜单")
    val mouth get() = pick("beak", "嘴")
    val no get() = pick("no", "否")
    val progress get() = pick("Daily", "每日")
    val provenance get() = pick("Provenance", "出处")
    val reload get() = pick("Reload AI", "重载 AI")
    val sensorNote get() = pick(
        "None of these is part of the duck's 61-D observation. The IMU is a shove, the rest is the room.",
        "这些都不在鸭子的 61 维观测里。IMU 是推力，其余是房间。",
    )
    val speechOff get() = pick("off", "未开启")
    val speechOnDeviceMissing get() = pick("no local model", "无本地模型")
    val state get() = pick("State", "状态")
    val tiltLookOn get() = pick("Tilt to look: on", "倾斜看：开")
    val trunkZ get() = pick("trunk z", "躯干高度")
    val voiceNote get() = pick(
        "Chinese speech is recognised on this phone and handed to the duck as text. The audio never leaves the phone.",
        "中文语音在本机识别成文字再交给鸭子，音频不会离开手机。",
    )
    val wakeFailed get() = pick("did not wake up", "没能醒来")
    val whatPhoneSenses get() = pick("What the phone senses", "手机感知到什么")
    val yes get() = pick("yes", "是")

    /** The battery gauge's own words, the same five the iOS face shows. */
    fun faceGauge(raw: String): String = when (raw) {
        "Unknown" -> pick("unknown", "未知")
        "Critical" -> pick("critical", "耗尽")
        "Low" -> pick("low", "偏低")
        "One" -> pick("one light", "一格")
        "Two" -> pick("two lights", "两格")
        "Three" -> pick("three lights", "三格")
        else -> raw
    }

    /** The idle look-around, said in words with the percentage the core publishes. */
    fun lookingAround(percent: Int): String = pick("looking around $percent%", "四处张望 $percent%")

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
    val voiceCommands get() = pick("Voice commands", "语音指令")
    val voiceEar get() = pick("Understand Chinese", "听懂中文")
    val speechListening get() = pick("listening", "聆听中")
    val speechUnavailable get() = pick("unavailable", "不可用")
    val behavior get() = pick("behaviour", "此刻")

    /**
     * The core's state machine, said in words a person would use.
     *
     * `/state.label` is the protocol's `char label[64]` copied verbatim:
     * `held`, `limp_fall`, `mode: walk`, `policy reloaded`, sometimes a
     * refused skill's error. None of that belongs on the stage. The
     * instrument keeps the raw string; the first screen reads this.
     */
    fun behaviourOf(raw: String): String = when (raw) {
        "held" -> pick("in a hand", "被拎着")
        "limp", "limp_fall" -> pick("fell over", "摔倒了")
        "homing" -> pick("steadying itself", "正在站稳")
        "mode: walk" -> pick("walk", "散步中")
        "mode: roller" -> pick("rolling", "滚轮中")
        "policy reloaded" -> pick("just woke up", "刚睡醒")
        "idle" -> pick("hanging out", "待着")
        "walk" -> pick("walk", "散步中")
        "stand" -> pick("stand", "站着")
        "sit" -> pick("sit", "坐着")
        "ready" -> pick("on its feet", "站稳了")
        "rest" -> pick("dozing", "打盹中")
        "off" -> pick("resting", "歇着")
        "pose" -> pick("posing", "摆姿势")
        "shutdown" -> pick("lying down", "趴下了")
        // A refused skill can write a whole error into the label. The label
        // is not a debug surface; anything unknown is a duck being a duck.
        else -> pick("hanging out", "待着")
    }
    // The companion layer: three numbers and a nap. "Fullness" is shown so a
    // full bar means fed, not starving.
    val care get() = pick("Companion", "陪伴")
    val fullness get() = pick("fullness", "饱腹")
    val mood get() = pick("mood", "心情")
    val bond get() = pick("bond", "亲密")
    val dozing get() = pick("dozing", "打盹中")
    fun careLine(hunger: Double, mood: Double, bond: Double, asleep: Boolean): String {
        // Same rounding as iOS: percentages are read, and 67.4999… must not
        // read one point lower on one platform than the other.
        val full = ((1.0 - hunger) * 100).roundToInt()
        val moodPercent = (((mood + 1) / 2) * 100).roundToInt()
        if (asleep) return "$dozing · $fullness $full%"
        if (hunger > 0.65) return pick("hungry · fullness $full%", "饿了 · 饱腹 $full%")
        if (mood < -0.2) return pick(
            "a little sad · bond ${(bond * 100).toInt()}%",
            "有点难过 · 亲密 ${(bond * 100).toInt()}%",
        )
        return pick("content · mood $moodPercent%", "心情不错 · 心情 $moodPercent%")
    }
    /**
     * The first screen's one-line mood: how it feels, with no numbers. The
     * percentages are readings for the instrument, not something a pet wears
     * on its face.
     */
    fun moodLine(hunger: Double, mood: Double, asleep: Boolean): String {
        if (asleep) return pick("fast asleep", "睡着了")
        if (hunger > 0.65) return pick("a little hungry", "有点饿了")
        if (mood < -0.2) return pick("a little sad", "有点难过")
        return pick("in a good mood", "心情不错")
    }
    // The chosen night: the child puts the duck to sleep on purpose, the row
    // says how long it has been quiet, and morning wakes it.
    val goodNight get() = pick("Good night", "晚安")
    val goodMorning get() = pick("Good morning", "早安")
    fun nightQuiet(minutes: Int) = pick("Quiet for $minutes min", "已经安静 $minutes 分钟")
    fun goodMorningLine(minutes: Int) = pick(
        "Good morning! Quiet for $minutes min",
        "早安！昨晚安静了 $minutes 分钟",
    )
    val trickBook get() = pick("Tricks", "把戏")
    /** The label for the API token: what a tool on another machine has to
     *  send, so the duck's own history cannot be rewritten from the network. */
    val apiToken get() = pick("API token (for other machines)", "API 令牌（给别的机器）")
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
        val postcardKeep get() = pick("Keep", "收藏")
        val postcardQuiet get() = pick("A quiet night.", "安静的一夜。")
    val postcardShelf get() = pick("Postcards", "明信片")
    val postcardSaved get() = pick("Kept to the shelf", "已收藏")
    val postcardNone get() = pick("No postcards yet", "还没有明信片")
    fun postcardMetres(m: Double) = pick("walked ${"%.1f".format(m)} m", "走了 ${"%.1f".format(m)} 米")
    // Mood dex.
    val dex get() = pick("Mood dex", "情绪图鉴")
    val dexMoves get() = pick("Moves", "招式")
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
        val ruleWhen get() = pick("When", "当")
    val ruleThen get() = pick("then", "就")
    val addRule get() = pick("Add", "添加")
    val ruleChain get() = pick("then also", "然后再")
    val ruleChainAdd get() = pick("Add step", "加一步")
    val ruleLimit get() = pick("Full (8/8)", "已满（8/8）")
    val deleteRule get() = pick("Delete", "删除")
    // Sharing a program between phones: the share sheet carries it as text,
    // the paste chip reads a program back.
    val rulesShare get() = pick("Share program", "分享程序")
    val rulesPaste get() = pick("Paste program", "粘贴程序")
    val rulesPasteFailed get() = pick(
        "That text is not a duck program",
        "这段文字不是鸭子程序",
    )
    fun rulesPasted(count: Int) = pick(
        "Imported $count block(s)",
        "导入了 $count 条积木",
    )
    /** The core refused the program: the editor puts the old one back. */
    val rulesRefusedText get() = pick(
        "The duck would not take that program, so nothing changed",
        "鸭子没有接受这份程序，所以什么都没变",
    )
    /** The core's own program has not been read yet, so there is nothing to merge onto. */
    val rulesNotReady get() = pick(
        "The duck has not finished starting; try again in a moment",
        "鸭子还没启动完，等一下再试",
    )
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
    // Gifts between neighbours: a treat for the tummy, a cheer for the mood,
    // and the one-line pill the receiver shows.
    val giftTreat get() = pick("Treat a neighbour", "给邻居一颗")
    val giftEmotion get() = pick("Cheer a neighbour", "逗邻居笑")
    fun giftReceivedTreat(name: String) = pick("$name sent a treat", "$name 送来一颗零食")
    fun giftReceivedEmotion(name: String) = pick("$name cheered you up", "$name 逗你笑了")
    /// The mood arrived while the duck was busy: it was sent, it was not played.
    fun giftMissedEmotion(name: String) = pick("$name sent a cheer", "$name 送来一份心意")
    // The room's chorus: every duck performs the same moves together.
    val chorus get() = pick("Chorus", "合唱")
    fun chorusLine(peers: Int) = pick(
        "Singing together with $peers duck(s)",
        "和 $peers 只鸭子一起唱",
    )
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
    val flashlight get() = pick("Flashlight", "手电筒")
        val flashlightOn get() = pick("on", "开")
    val flashlightOff get() = pick("off", "关")

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

    fun bringupText(code: Int): String = when (code) {
        0 -> pick("Limp", "软瘫")
        1 -> pick("Homing", "归位中")
        2 -> pick("Ready", "就绪")
        else -> "?"
    }
}

/** A skill's display name; the wire name stays the protocol's. */
fun skillLabel(name: String): String {
    // English keeps the protocol word, but a wire name is `kick_left`, not a
    // label: the underscore is a protocol detail, not something a person
    // reads on the first screen.
    if (Copy.language != Copy.Language.Chinese) return name.replace('_', ' ')
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
