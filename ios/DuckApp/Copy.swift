import Foundation

/// Every word the shell says, in both languages.
///
/// The duck is the same in either; only the labels change. Chinese is the
/// default when the system language is Chinese, English otherwise, and the
/// instrument sheet can switch it at any time.
enum Copy {
    enum Language: String, CaseIterable {
        case english = "EN"
        case chinese = "中文"
    }

    static var language: Language {
        get {
            if let saved = UserDefaults.standard.string(forKey: "duck.language"),
               let value = Language(rawValue: saved) {
                return value
            }
            let preferred = Locale.preferredLanguages.first ?? "en"
            return preferred.hasPrefix("zh") ? .chinese : .english
        }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "duck.language") }
    }

    private static func pick(_ en: String, _ zh: String) -> String {
        language == .chinese ? zh : en
    }

    // Live view
    static var name: String { "MicroDuck" }
    static var fell: String { pick("it fell", "摔倒了") }
    static var waking: String { pick("waking the duck…", "正在叫醒鸭子…") }
    /// 核没起来时状态行就显示这句；下面的卡片负责解释原因。
    static var wakeFailed: String { pick("did not wake up", "没能醒来") }
    static var tapHint: String {
        // The gesture is a drag from wherever the thumb lands; "tap ahead"
        // described a walk-to-point the app never had.
        pick("drag to walk · long-press for skills · tap the duck to pet",
             "拖动走路 · 长按出技能 · 点鸭子摸摸")
    }
    /// Shown once, above the bottom bar, then never again. Distinct from the
    /// persistent `tapHint`: this one teaches the two gestures on first launch.
    static var firstLaunchHint: String {
        pick(
            "Drag to walk · long-press for skills · tap the duck to pet",
            "拖动走路 · 长按出技能 · 点鸭子摸摸"
        )
    }
    static var live: String { pick("Live", "实时") }
    static var director: String { pick("Director", "取景") }
    static var time: String { pick("Time", "时间") }
    static var skills: String { pick("Skills", "技能") }
    /// The skill picker's two groups: the duck's moves and its moods.
    static var actions: String { pick("Actions", "动作") }
    static var emotions: String { pick("Emotions", "情绪") }
    static var close: String { pick("Close", "关闭") }
    static var couldNotStart: String { pick("MicroDuck could not start", "MicroDuck 无法启动") }
    /// The failure card's one action. Android's core loader can retry; the
    /// key lives on both sides so the dictionaries stay aligned.
    static var retry: String { pick("Try again", "重试") }
    /// 启动失败翻成人话。日志里那句 "Bundle resources not found" 和 Metal
    /// 报错是给工程师看的，不是给站在屏幕前的人看的。
    static func humanError(raw: String) -> String {
        let lower = raw.lowercased()
        if lower.contains("bundle") || lower.contains("resource")
            || lower.contains("no such file") || lower.contains("not found") {
            return pick(
                "The app's files are incomplete. Reinstall the app.",
                "应用资源不完整，请重新安装"
            )
        }
        if lower.contains("metal") || lower.contains("render") || lower.contains("shader")
            || lower.contains("gpu") {
            return pick(
                "The picture could not start on this phone. Open the app again.",
                "画面没能启动，请重新打开 App"
            )
        }
        return pick(
            "The duck could not wake up. Open the app again.",
            "鸭子没能醒来，请重新打开 App"
        )
    }
    static var viewfinder: String { pick("VIEWFINDER", "取景器") }
    static var sit: String { pick("sit", "坐下") }
    static var pickUp: String { pick("pick", "抓取") }
    static var grab: String { pick("Hold", "拎起") }
    static var putDown: String { pick("Drop", "放下") }
    static var held: String { pick("in a hand", "在手里") }
    static var quack: String { pick("Quack", "叫一声") }

    // Instrument sheet
    static var thisDuck: String { pick("This duck", "这只鸭子") }
    static var timeMachine: String { pick("Time machine", "时间机器") }
    static var freeze: String { pick("Freeze", "冻结") }
    static var resume: String { pick("Resume", "继续") }
    static var step: String { pick("Step", "单步") }
    static var rewind: String { pick("Rewind", "倒回") }
    static var fork: String { pick("Fork", "分叉") }
    static var state: String { pick("State", "状态") }
    static var bringup: String { pick("bring-up", "启动阶段") }
    static var gain: String { pick("gain", "增益") }
    static var fallen: String { pick("fallen", "已摔倒") }
    static var trunkZ: String { pick("trunk z", "躯干高度") }
    static var gravityZ: String { pick("gravity z", "重力 z") }
    static var imu: String { pick("IMU warm", "IMU 预热") }
    static var drive: String { pick("drive", "驱动") }
    static func stick(_ mode: String) -> String {
        switch mode {
        case "head": return pick("look", "看")
        case "body": return pick("pose", "姿态")
        case "path": return pathMode
        default: return pick("drive", "驱动")
        }
    }
    static var walk: String { pick("walk", "走") }
    static var roller: String { pick("roller", "轮") }
    static var mouth: String { pick("beak", "嘴") }
    static var battery: String { pick("pack", "电池") }
    static var branches: String { pick("branches", "分叉数") }
    // The face: the virtual status LED, the four-light battery gauge and the
    // flashlight, as /state.leds publishes them. The wire names (Starting,
    // Two, white…) stay the protocol's; only these labels translate.
    static var face: String { pick("Face", "脸") }
    static var ledStatus: String { pick("status", "状态灯") }
    static var gauge: String { pick("gauge", "电量灯") }
    static var flashlight: String { pick("flashlight", "手电筒") }
    static var flashlightOn: String { pick("on", "开") }
    static var flashlightOff: String { pick("off", "关") }
    /// The Director shutter's short name, mirrored by Android's photoShort.
    static var photoShort: String { pick("Photo", "拍照") }
    /// The core has not finished starting, so there is nothing to merge onto.
    static var rulesNotReady: String {
        pick(
            "The duck has not finished starting; try again in a moment",
            "鸭子还没启动完，等一下再试"
        )
    }
    /// The IMU warming-up state, beside the warm reading.
    static var imuWarming: String { pick("IMU warming", "IMU 预热中") }
    /// The idle look-around, said in words with the core's own percentage.
    static func lookingAround(_ percent: Int) -> String {
        String(format: pick("looking around %d%%", "四处张望 %d%%"), percent)
    }
    /// The flashlight's colour, in the shell's language.
    static func faceColor(_ raw: String) -> String {
        switch raw {
        case "white": return pick("white", "白")
        case "red": return pick("red", "红")
        case "green": return pick("green", "绿")
        case "blue": return pick("blue", "蓝")
        case "yellow": return pick("yellow", "黄")
        case "cyan": return pick("cyan", "青")
        case "magenta": return pick("magenta", "品红")
        default: return raw
        }
    }
    /// The status LED's verdict. An unknown wire name passes through, the
    /// way a user-manifest skill's name does.
    static func faceStatus(_ raw: String) -> String {
        switch raw {
        case "Starting": return pick("starting", "启动中")
        case "Healthy": return pick("healthy", "健康")
        case "Degraded": return pick("degraded", "降级")
        case "Fault": return pick("fault", "故障")
        default: return raw
        }
    }
    /// The battery gauge's level: the same five words the upstream LED
    /// order implies, from empty to full.
    static func faceGauge(_ raw: String) -> String {
        switch raw {
        case "Unknown": return pick("unknown", "未知")
        case "Critical": return pick("critical", "耗尽")
        case "Low": return pick("low", "偏低")
        case "One": return pick("one light", "一格")
        case "Two": return pick("two lights", "两格")
        case "Three": return pick("three lights", "三格")
        default: return raw
        }
    }
    static var yes: String { pick("yes", "是") }
    static var no: String { pick("no", "否") }
    static var whatPhoneSenses: String { pick("What the phone senses", "手机感知到什么") }
    static var sensorNote: String {
        pick(
            "None of these is part of the duck's 61-D observation. The IMU is a shove, the rest is the room.",
            "这些都不在鸭子的 61 维观测里。IMU 是推力，其余是房间。"
        )
    }
    static var camera: String { pick("camera", "相机") }
    static var microphone: String { pick("microphone", "麦克风") }
    /// A refused permission is a real state: "—" would claim no sensor exists.
    static var denied: String { pick("denied", "未授权") }
    /// The duck's ear. The words a person says are the duck's own input, so
    /// this has its own section rather than living with the room's sensors.
    static var voiceCommands: String { pick("Voice commands", "语音指令") }
    /// The toggle's own label: the duck understands spoken Chinese.
    static var voiceEar: String { pick("Understand Chinese", "听懂中文") }
    /// What the ear is doing, in the row's value column.
    static var speechOff: String { pick("off", "未开启") }
    static var speechListening: String { pick("listening", "聆听中") }
    /// No recogniser here: the simulator, or a locale this device does not
    /// know.
    static var speechUnavailable: String { pick("unavailable", "不可用") }
    /// The recogniser works, but only in the cloud: the on-device model is
    /// not installed, and the audio would have to leave the phone.
    static var speechOnDeviceMissing: String { pick("no local model", "无本地模型") }
    /// The last thing the duck heard.
    static var heard: String { pick("heard", "听到") }
    /// The privacy note under the toggle: the audio stays on the phone and
    /// only the words travel.
    static var voiceNote: String {
        pick(
            "Chinese speech is recognised on this phone and handed to the duck as text. The audio never leaves the phone.",
            "中文语音在本机识别成文字再交给鸭子，音频不会离开手机。"
        )
    }
    static var behavior: String { pick("behaviour", "此刻") }

    /// 核的状态机，翻成一个人会说的话。
    ///
    /// `controller.label` 是协议里的 `char label[64]` 原文：`held`、
    /// `limp_fall`、`mode: walk`、`policy reloaded`，有时还留着一条被拒绝
    /// 技能的错误。这些都不是该摆到舞台上的句子。仪表盘留给想知道原话的
    /// 人，首屏读这一层。
    ///
    /// 英文里 `walk` 和 `stand` 照旧：它们本来就是英文，而且 UI 测试盯着
    /// 这一行找这两个词。
    static func behaviourOf(raw: String) -> String {
        switch raw {
        case "held": return pick("in a hand", "被拎着")
        case "limp", "limp_fall": return pick("fell over", "摔倒了")
        case "homing": return pick("steadying itself", "正在站稳")
        case "mode: walk": return pick("walk", "散步中")
        case "mode: roller": return pick("rolling", "滚轮中")
        case "policy reloaded": return pick("just woke up", "刚睡醒")
        case "idle": return pick("hanging out", "待着")
        // 步态的动词：和手柄给驾驶员看的字一样。
        case "walk": return pick("walk", "散步中")
        case "stand": return pick("stand", "站着")
        case "sit": return pick("sit", "坐着")
        case "ready": return pick("on its feet", "站稳了")
        case "rest": return pick("dozing", "打盹中")
        case "off": return pick("resting", "歇着")
        case "pose": return pick("posing", "摆姿势")
        case "shutdown": return pick("lying down", "趴下了")
        // 被拒绝的技能会把整条错误写进 label。label 不是调试的地方，
        // 认不出来的一律当成一只正在做鸭子的鸭子。
        default: return pick("hanging out", "待着")
        }
    }
    // The companion layer: three numbers and a nap. "Fullness" is shown
    // because a full bar should mean "fed", not "starving".
    static var care: String { pick("Companion", "陪伴") }
    static var fullness: String { pick("fullness", "饱腹") }
    static var mood: String { pick("mood", "心情") }
    static var bond: String { pick("bond", "亲密") }
    static var dozing: String { pick("dozing", "打盹中") }
    static func careLine(hunger: Double, mood: Double, bond: Double, asleep: Bool) -> String {
        let full = Int(((1.0 - hunger) * 100).rounded())
        let moodPercent = Int((((mood + 1) / 2) * 100).rounded())
        if asleep { return "\(dozing) · \(fullness) \(full)%" }
        if hunger > 0.65 { return pick("hungry · fullness \(full)%", "饿了 · 饱腹 \(full)%") }
        if mood < -0.2 { return pick("a little sad · bond \(Int(bond * 100))%", "有点难过 · 亲密 \(Int(bond * 100))%") }
        return pick("content · mood \(moodPercent)%", "心情不错 · 心情 \(moodPercent)%")
    }
    /// 首屏心情胶囊的一行话：只说感觉，不带数字。百分比是仪表盘上的读数，
    /// 不是宠物该挂在脸上的东西。
    static func moodLine(hunger: Double, mood: Double, asleep: Bool) -> String {
        if asleep { return pick("fast asleep", "睡着了") }
        if hunger > 0.65 { return pick("a little hungry", "有点饿了") }
        if mood < -0.2 { return pick("a little sad", "有点难过") }
        return pick("in a good mood", "心情不错")
    }
    // Sleep mode: the chosen night, and the morning that wakes it. The quiet
    // numbers are the core's own simulated seconds; the shells only divide
    // them into minutes for the person reading.
    static var goodNight: String { pick("Good night", "晚安") }
    static var goodMorning: String { pick("Good morning", "早安") }
    static func nightQuiet(_ minutes: Int) -> String {
        String(format: pick("Quiet for %d min", "已经安静 %d 分钟"), minutes)
    }
    static func goodMorningLine(_ minutes: Int) -> String {
        String(format: pick(
            "Good morning! Quiet for %d min",
            "早安！昨晚安静了 %d 分钟"
        ), minutes)
    }
    static var menu: String { pick("Menu", "菜单") }
    // The one "More" drawer (B4): four groups, at most four rows each.
    static var more: String { pick("More", "更多") }
    static var groupPlay: String { pick("Play", "玩") }
    static var groupTogether: String { pick("Together", "一起") }
    static var groupRecord: String { pick("Record", "记录") }
    static var dailyQuests: String { pick("Daily quests", "每日任务") }
    static var stickModes: String { pick("Controls", "操控方式") }
    static var pathRowNote: String { pick("Draw a route, then walk it", "画一条路，让它走") }
    static var blocksRowNote: String { pick("When this, then that", "当……就……") }
    static var danceRowNote: String { pick("Record a move, then replay it", "录下来，再重演") }
    /// The neighbours row carries its own actions in the subtitle; the
    /// chorus and the gifts are actions inside that page, not pages.
    static var peersRowNote: String { pick("Chorus · gifts · nearby", "合唱 · 互送礼物 · 附近") }
    static var questsRowNote: String { pick("Three small goals a day", "每天三个小目标") }
    static var dexRowNote: String { pick("Moods you have met", "见过多少种心情") }
    static var languageRow: String { pick("Language", "语言") }
    static var nightRowNote: String { pick("Let it sleep", "让鸭子睡下") }
    static var morningRowNote: String { pick("Wake the duck", "叫醒鸭子") }
    static var cameraUnavailable: String { pick("NO CAMERA", "无相机") }
    static var feed: String { pick("Feed", "喂食") }
    /// 每日任务与成就墙（成长系统）的入口。
    static var progress: String { pick("Daily", "每日") }
    static var awayTitle: String { pick("While you were away", "你不在时") }
    static func awayRituals(_ n: Int) -> String {
        String(format: pick("played by itself ×%d", "自己演了 %d 出"), n)
    }
    static func awayFalls(_ n: Int) -> String {
        String(format: pick("fell ×%d", "摔了 %d 次"), n)
    }
    static func awayFeeds(_ n: Int) -> String {
        String(format: pick("ate ×%d", "吃了 %d 次"), n)
    }
    static func awayPets(_ n: Int) -> String {
        String(format: pick("petted ×%d", "被摸 %d 次"), n)
    }
    static func awayMoods(_ n: Int) -> String {
        String(format: pick("%d moods", "%d 种心情"), n)
    }
    static var tiltLook: String { pick("Tilt to look", "倾斜看") }
    static var tiltLookOn: String { pick("Tilt to look: on", "倾斜看：开") }
    static var trickBook: String { pick("Tricks", "把戏") }
    // The vet book, and the file it exports.
    static var lifeRecord: String { pick("Life record", "终身记录") }
    // Dance Studio: record actions and replay them.
    static var dance: String { pick("Dance Studio", "编舞铺") }
    static var danceHint: String {
        pick("Press Record, then do anything: a skill, a quack, a treat. Press Play and the duck repeats it.",
             "按录制，然后做动作：技能、叫声、喂食。按播放，鸭子照着重演。")
    }
    static var danceEmpty: String { pick("No steps yet", "还没有动作") }
    static var danceRecord: String { pick("Record", "录制") }
    static var danceStopRecord: String { pick("Stop recording", "停止录制") }
    static var dancePlay: String { pick("Play", "播放") }
    static var danceStop: String { pick("Stop", "停止") }
    static var danceSaved: String { pick("Saved on this phone", "已存在这台手机") }
    static var danceTry: String { pick("Try these while recording:", "录制时点这些：") }
    static var danceStepPet: String { pick("Pet", "摸摸") }
    static var danceStepSound: String { pick("Quack", "叫一声") }
    // Path Painter: draw a route on the stage and the duck walks it.
    static var pathMode: String { pick("Path", "画路走") }
    static var pathGo: String { pick("Walk it", "走这条路") }
    static var pathClear: String { pick("Clear", "清掉") }
    static var pathCancel: String { pick("Stop", "不走了") }
    /// The waypoint the duck is walking, one-based for the child.
    static func pathStep(_ step: Int, _ total: Int) -> String {
        String(format: pick("Step %d of %d", "第 %d 步，共 %d 步"), step, total)
    }
    static var pathDone: String { pick("Walked the whole path", "走完全程啦") }
    // Morning postcard.
    static var postcardKeep: String { pick("Keep", "收藏") }
    static var postcardQuiet: String { pick("A quiet night.", "安静的一夜。") }
    static var postcardShelf: String { pick("Postcards", "明信片") }
    static var postcardSaved: String { pick("Kept to the shelf", "已收藏") }
    static var postcardNone: String { pick("No postcards yet", "还没有明信片") }
    static func postcardMetres(_ m: Double) -> String {
        pick(String(format: "walked %.1f m", m), String(format: "走了 %.1f 米", m))
    }
    // Mood dex.
    static var dex: String { pick("Mood dex", "情绪图鉴") }
    static var dexMoves: String { pick("Moves", "招式") }
    static var dexMoods: String { pick("Moods", "情绪") }
    static var dexNever: String { pick("not yet", "还没见过") }
    static var dexFirst: String { pick("first", "初次") }
    // Daily quests.
    static var quests: String { pick("Today's quests", "今日小挑战") }
    static var questDone: String { pick("Done", "完成") }
    static func questStreak(_ days: Int) -> String {
        pick("\(days)-day streak", "连续 \(days) 天")
    }
    static var questAllDone: String {
        pick("All done — the duck is proud of you!", "全部完成，鸭子为你骄傲！")
    }
    static var walked: String { pick("walked", "走过") }
    static var rolls: String { pick("rolls", "翻滚") }
    static var pickups: String { pick("picked up", "被抱起") }
    static var exportVetbook: String { pick("Export vet book", "导出记录") }
    static var shareVetbook: String { pick("Share vet book", "分享记录") }
    // The Director photo.
    static var photoTitle: String { pick("Your duck, in your room", "你的鸭子，在你房间") }
    static var photoTake: String { pick("Take photo", "拍照") }
    static var savePhoto: String { pick("Save", "保存") }
    static var saved: String { pick("Saved", "已保存") }
    static var saveFailed: String { pick("No photo access", "无相册权限") }
    static var share: String { pick("Share", "分享") }
    // What the room said, from the microphone channel.
    static func heardSound(_ kind: String) -> String {
        switch kind {
        case "clap": return pick("heard a clap!", "听到拍手！")
        case "whistle": return pick("heard a whistle!", "听到口哨！")
        default: return kind
        }
    }
    // The block rules a child programs.
    static var blocks: String { pick("Blocks", "积木规则") }
    static var ruleWhen: String { pick("When", "当") }
    static var ruleThen: String { pick("then", "就") }
    static var addRule: String { pick("Add", "添加") }
    static var ruleChain: String { pick("then also", "然后再") }
    static var ruleChainAdd: String { pick("Add step", "加一步") }
    static var ruleLimit: String { pick("Full (8/8)", "已满（8/8）") }
    static var deleteRule: String { pick("Delete", "删除") }
    // Sharing a program between phones: the share sheet carries it as text,
    // the paste chip reads a program back.
    static var rulesShare: String { pick("Share program", "分享程序") }
    static var rulesPaste: String { pick("Paste program", "粘贴程序") }
    static var rulesPasteFailed: String { pick("That text is not a duck program", "这段文字不是鸭子程序") }
    static func rulesPasted(_ count: Int) -> String {
        String(format: pick("Imported %d block(s)", "导入了 %d 条积木"), count)
    }
    // The duck's diary: today's real record, and the paragraph the person's
    // own model writes from it. Without a model the facts stand alone.
    static var diary: String { pick("Ducky diary", "鸭子日记") }
    static var diaryFactsTitle: String { pick("Today, for real", "今天的真实记录") }
    static var diaryModelTitle: String { pick("Your model wrote", "你的模型写道") }
    static var diaryBusy: String { pick("Writing…", "正在写…") }
    static var diaryEmpty: String { pick("Nothing happened yet today.", "今天还什么都没发生。") }
    static var diaryNeedModel: String {
        pick(
            "Set a model endpoint in AI companion to hear the day in the duck's own words.",
            "在 AI 伙伴里配置接口，就能听鸭子用自己的话讲这一天。"
        )
    }
    static func diaryGifts(_ n: Int) -> String {
        String(format: pick("%d gift(s) from neighbours", "收到邻居 %d 份礼物"), n)
    }
    // The speak button on the diary: the system voice reading the real facts
    // (or the model's paragraph) aloud. Honest about the stand-in: this is
    // the phone speaking FOR the duck, not the duck's own formant voice.
    static var speak: String { pick("Let it speak", "让它说") }
    static var stopSpeaking: String { pick("Stop", "停下") }
    static var speakNote: String {
        pick(
            "The system voice speaks for it — not the duck's own synth.",
            "系统声音替它说——不是鸭子自己的合成嗓。"
        )
    }
    // The palm's heartbeat: the duck's care numbers as a haptic rhythm.
    // A reading of the real state, not a new fact; off until asked for.
    static var heart: String { pick("Heartbeat", "手心心跳") }
    static var formation: String { pick("Dance together", "家庭舞团") }
    static var roleDance: String { pick("Role dance", "角色编舞") }
    static var voiceChoir: String { pick("Voice choir", "多鸭合奏") }
    static var netPad: String { pick("UDP pad (port 4210)", "网控手柄 (4210)") }
    static var netPadNote: String {
        pick(
            "Set the pair code first; the official DKPD pad then drives this duck.",
            "先设配对码，官方 DKPD 手柄才能开这只鸭。"
        )
    }
    // The parent's daily allowance: when the duck's own powered time for
    // the day is spent, it goes to the same real sleep the switch puts it in.
    // The room's lamp: a render-layer multiplier. iOS has no ambient-light
    // sensor API, so this is the person's hand — the label says so and never
    // claims the duck can see.
    // The big-button page: four verbs a grandparent can hit without
    // hunting. The words are the shell's own; the buttons are 72 pt tall
    // and VoiceOver reads the whole sentence.
    static var bigButtons: String { pick("Big buttons", "大按钮") }
    static var bigButtonsNote: String { pick("Four big taps, large type", "四个大按钮，大字") }
    static func bigButtonAction(_ index: Int) -> String {
        [pick("Pet it", "摸摸它"), pick("Feed it", "喂食"),
         pick("Quack", "叫一声"), pick("Sit down", "坐下")][index]
    }
    static var roomLamp: String { pick("Room lamp", "房间灯") }
    static var roomLampNote: String {
        pick(
            "Manual — iOS has no lux sensor; the duck cannot see it.",
            "手动——iOS 没有照度 API；鸭子看不见它。"
        )
    }
    static var parentTime: String { pick("Parent time", "家长时间") }
    static var parentOff: String { pick("off", "不限") }
    static func parentLeft(_ minutes: Int, _ leftMinutes: Int) -> String {
        String(format: pick("%d min · %d left", "%d 分钟 · 剩 %d 分钟"), minutes, leftMinutes)
    }
    static var parentNote: String {
        pick(
            "When today's play is spent, the duck goes to bed on its own.",
            "今天的游玩时间用完，鸭子就自己睡觉。"
        )
    }
    static var pairCode: String { pick("Pair code", "配对码") }
    static var pairNote: String {
        pick(
            "Both phones type the same six digits before gifts or songs.",
            "两台手机输入同一个 6 位数字，才能互送礼物或一起唱。"
        )
    }
    static var heartRowNote: String {
        pick(
            "Feel its heart in your palm — a reading of its mood.",
            "在手心感受它的心——由心情推出来的读数。"
        )
    }
    static var rulesEmpty: String {
        pick("No rules yet. Build one: when I clap, roll over.",
             "还没有规则。拼一个：拍手，就翻滚。")
    }
    static func ruleTrigger(_ trigger: String) -> String {
        switch trigger {
        case "clap": return pick("clap", "拍手")
        case "whistle": return pick("whistle", "吹口哨")
        case "pet": return pick("pet me", "摸摸我")
        default: return trigger
        }
    }
    static func ruleKind(_ action: String) -> String {
        switch action {
        case "skill": return pick("move", "动作")
        case "emotion": return pick("mood", "心情")
        case "sound": return pick("sound", "声音")
        default: return action
        }
    }
    // Other ducks on the same Wi-Fi.
    static var peers: String { pick("Neighbours", "邻居") }
    static var peerSayHi: String { pick("Say hi", "打招呼") }
    static var peerSayAll: String { pick("Say hi to everyone", "向大家问好") }
    static var peersEmpty: String {
        pick("Looking for ducks on this Wi-Fi…", "正在同一 Wi-Fi 里找鸭子…")
    }
    static var peerFind: String { pick("Find ducks on this Wi-Fi", "在 Wi-Fi 里找鸭子") }
    static var peerNew: String { pick("new", "新朋友") }
    static var peerRepliedLabel: String { pick("answered", "已回应") }
    static func peerGreeted(_ name: String) -> String {
        pick("\(name) says hi!", "\(name) 向你问好！")
    }
    static func peerReplied(_ name: String) -> String {
        pick("\(name) answered!", "\(name) 回应了！")
    }
    // Gifts between neighbours: a treat on the floor, or a cheer.
    static var giftTreat: String { pick("Treat a neighbour", "给邻居一颗") }
    static var giftEmotion: String { pick("Cheer a neighbour", "逗邻居笑") }
    static func giftReceivedTreat(_ name: String) -> String {
        pick("\(name) sent a treat", "\(name) 送来一颗零食")
    }
    static func giftReceivedEmotion(_ name: String) -> String {
        pick("\(name) cheered you up", "\(name) 逗你笑了")
    }
    /// The mood arrived while the duck was busy: it was sent, it was not
    /// played.
    static func giftMissedEmotion(_ name: String) -> String {
        pick("\(name) sent a cheer", "\(name) 送来一份心意")
    }
    // A chorus between neighbours: one duck starts it, the room joins the
    // same moods 1.5 s later.
    static var chorus: String { pick("Chorus", "合唱") }
    static func chorusLine(_ peers: Int) -> String {
        String(format: pick("Singing together with %d duck(s)", "和 %d 只鸭子一起唱"), peers)
    }
    // Bring your own model.
    static var ai: String { pick("AI companion", "AI 伙伴") }
    static var aiOn: String { pick("watching", "观察中") }
    static var aiOff: String { pick("off", "未开启") }
    static var aiThinking: String { pick("thinking…", "思考中…") }
    static var aiOffline: String { pick("no answer", "没有回应") }
    static var aiNeedEndpoint: String { pick("Set an endpoint first", "请先填写接口地址") }
    static var aiEndpoint: String { pick("Endpoint", "接口地址") }
    static var aiModel: String { pick("Model", "模型") }
    static var aiKey: String { pick("API key", "密钥") }
    static var aiInterval: String { pick("Every (seconds)", "每隔（秒）") }
    static var aiPersona: String { pick("Persona", "性格") }
    static var aiThinkNow: String { pick("Think now", "现在想") }
    static var aiSave: String { pick("Save", "保存") }
    static var aiNote: String {
        pick(
            "The duck's state and the last words it heard go to the endpoint you choose. Your key stays on this phone and is never sent anywhere else.",
            "鸭子的状态和它最后听到的话会发给你填写的接口。密钥只保存在这台手机上，不会发往别处。"
        )
    }
    static var aiHint: String {        pick(
            "Any OpenAI-compatible endpoint, including a local Ollama or LM Studio server.",
            "任何兼容 OpenAI 的接口，包括本机的 Ollama 或 LM Studio。"
        )
    }
    static var showMore: String { pick("More", "更多") }
    static var showLess: String { pick("Less", "收起") }
    static var depth: String { pick("depth", "深度") }
    static var provenance: String { pick("Provenance", "出处") }
    /// The label for the API token: what a tool on another machine has to
    /// send, so the duck's own history cannot be rewritten from the network.
    static var apiToken: String { pick("API token (for other machines)", "API 令牌（给别的机器）") }
    /// The core refused the program: the editor puts the old one back.
    static var rulesRefusedText: String {
        pick(
            "The duck would not take that program, so nothing changed",
            "鸭子没有接受这份程序，所以什么都没变"
        )
    }
    static var reload: String { pick("Reload AI", "重载 AI") }
    static var api: String { "AI API :8765 · MCP /mcp" }

    static var bringupText: (Int) -> String = { code in
        switch code {
        case 0: return pick("Limp", "软瘫")
        case 1: return pick("Homing", "归位中")
        case 2: return pick("Ready", "就绪")
        default: return "?"
        }
    }
}

/// The words the core writes into the duck's life record.
///
/// The core says "fell", not "fall": the diary counted the prefix "fall",
/// which matched nothing, so the fall count was always zero — and a zero
/// in the facts is a claim about the duck that is not true. The names live
/// here so every screen reads them the same way.
enum LifeEvent {
    /// A fall. The core's word is "fell"; the prefix form is accepted too,
    /// so a future "fall:hard" would still be counted.
    static func isFall(_ event: String) -> Bool {
        event == "fell" || event.hasPrefix("fall")
    }
}

/// A skill's display name. The wire name is the protocol's; the label is the
/// shell's, so a Chinese interface can say 翻滚 while the API still says
/// `roulade`. English intentionally reuses the wire name — it is already the
/// protocol's human-readable verb, and the picker's accessibility id stays that
/// same wire name in either language.
enum SkillLabel {
    static func of(_ name: String) -> String {
        // English keeps the protocol word, but a wire name is `kick_left`,
        // not a label. The underscore is a protocol detail; the first screen
        // must not show one.
        guard Copy.language == .chinese else {
            return name.replacingOccurrences(of: "_", with: " ")
        }
        switch name {
        case "sit": return "坐下"
        case "stand": return "站起"
        case "ground_pick": return "贴地抓取"
        case "roulade": return "翻滚"
        case "kick_left": return "左踢"
        case "kick_right": return "右踢"
        case "crouch": return "蹲下"
        // `robotd`'s thirteen emotions, published by `/skills` as skills of
        // their own. Each is a scripted timeline with its own voice.
        case "angry": return "生气"
        case "curious": return "好奇"
        case "defiant": return "不服"
        case "devastated": return "崩溃"
        case "excited": return "兴奋"
        case "impatient": return "不耐烦"
        case "laugh": return "大笑"
        case "mmh": return "嗯哼"
        case "mock": return "嘲弄"
        case "no": return "摇头"
        case "sad": return "难过"
        case "yes": return "点头"
        case "yes_fast": return "快速点头"
        // A user-supplied manifest can name any skill. Showing its wire name
        // is honest: inventing a translation would mislabel the policy.
        default: return name
        }
    }
}
