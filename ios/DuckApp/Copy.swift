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
        pick("tap ahead to walk · long-press the duck", "点前方走路 · 长按鸭子")
    }
    /// Shown once, above the bottom bar, then never again. Distinct from the
    /// persistent `tapHint`: this one teaches the two gestures on first launch.
    static var firstLaunchHint: String {
        pick(
            "Tap ahead to walk · long-press the duck for skills",
            "点前方走路 · 长按鸭子出技能"
        )
    }
    static var live: String { pick("Live", "实时") }
    static var director: String { pick("Director", "取景") }
    static var time: String { pick("Time", "时间") }
    static var skills: String { pick("Skills", "技能") }
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
/// protocol's human-readable verb, and the ring's accessibility id stays that
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
        // A user-supplied manifest can name any skill. Showing its wire name
        // is honest: inventing a translation would mislabel the policy.
        default: return name
        }
    }
}
