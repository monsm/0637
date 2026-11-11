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
    static var waking: String { pick("waking up…", "正在唤醒…") }
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
    static var face: String { pick("Face", "面部") }
    static var ledStatus: String { pick("status", "状态灯") }
    static var gauge: String { pick("gauge", "电量灯") }
    static var flashlight: String { pick("flashlight", "手电筒") }
    static var flashlightOn: String { pick("on", "开") }
    static var flashlightOff: String { pick("off", "关") }
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
    static var menu: String { pick("Menu", "菜单") }
    static var cameraUnavailable: String { pick("NO CAMERA", "无相机") }
    static var feed: String { pick("Feed", "喂食") }
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
    static var trickBook: String { pick("Trick book", "技巧图鉴") }
    // The vet book, and the file it exports.
    static var lifeRecord: String { pick("Life record", "终身记录") }
    // Dance Studio: record actions and replay them.
    static var dance: String { pick("Dance Studio", "编舞铺") }
    static var danceShort: String { pick("Dance", "编舞") }
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
    // Morning postcard.
    static var postcard: String { pick("Morning postcard", "早安明信片") }
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
    static var dexMoves: String { pick("Moves", "动作") }
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
    static var reload: String { pick("Reload AI", "重载 AI") }
    static var api: String { "AI API :8765 · MCP /mcp" }
    static var languageLabel: String { pick("Language", "语言") }

    static var bringupText: (Int) -> String = { code in
        switch code {
        case 0: return pick("Limp", "软瘫")
        case 1: return pick("Homing", "归位中")
        case 2: return pick("Ready", "就绪")
        default: return "?"
        }
    }
}

/// A skill's display name. The wire name is the protocol's; the label is the
/// shell's, so a Chinese interface can say 翻滚 while the API still says
/// `roulade`. English intentionally reuses the wire name — it is already the
/// protocol's human-readable verb, and the picker's accessibility id stays that
/// same wire name in either language.
enum SkillLabel {
    static func of(_ name: String) -> String {
        guard Copy.language == .chinese else { return name }
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
