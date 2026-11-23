# 声音事件对表（QA 遗留 7）

2026-11-24 逐事件核对。声音全部由核心合成/生成（`voice.rs` 的七个合成标签 +
`emotion:*` 官方 wav + 骑乘 segment），外壳只负责把 PCM 送进扬声器。
「可观测字段」是 `/state` 里的人类可读证据——断言只读这些字段，不听模拟器。

`voice.rs` 合成标签：`greet chirp coo peck inquire alarm wheee`

| 事件 | 生产者 | 标签 / 字段 | 自动化断言 |
|---|---|---|---|
| 叫一声（点物件） | `ffi.rs:1626/3720` | `last_sound="chirp"` | iOS `testTheDuckVoiceIsHeard`（到 AVAudioEngine）；Android 冒烟「voice: the quack object queues the chirp」（`last_sound`） |
| 张嘴请求（API） | `POST /sound {"tag":"chirp"}` | `last_sound` | Android 冒烟「the duck cannot be asked to chirp」（API 路径） |
| 开机问候 | `runtime.rs:1778` | `last_sound="greet"` | 无专项（开机时序，冒烟不钉毫秒级首声）——纪录片，不承诺 |
| 睡着/伺服静默前 | `runtime.rs:1624` | `last_sound="peck"` | 无专项（随晚安页行为） |
| 地面拾取 | `runtime.rs:1806` | `last_sound="peck"` | 冒烟有拾取事件（`ground_pick` 计入 life），声音不单钉 |
| 抚摸 | `runtime.rs:2331` | `last_sound="coo"` | 冒烟有 pet 事件；声音不单钉 |
| 空闲小仪式 | `runtime.rs:1382/1447` | `last_sound="coo"` | 无专项（后台行为） |
| 情绪时间线 | `runtime.rs:2299` | `last_sound="emotion:<name>"`, `last_emotion_sound` | iOS 冒烟「an emotion's voice is queued」；Android 冒烟读同一字段 |
| 拍手/口哨默认反应被占时的回答 | `runtime.rs:2651` | `last_sound="coo"/"inquire"`（情绪跑不起来时才响） | 无专项（是降级路径，条件苛刻） |
| 别人家的鸭子打招呼 | `runtime.rs:2808` | `last_sound`（问候声） | 冒烟有 LAN greet 回复断言（协议层），声音不单钉 |
| 礼物事件 | `runtime.rs:2729` | `queue_sound(value)` | 冒烟有 gift 事件断言；声音不单钉 |
| 语音指令的回答 | `runtime.rs:2220/2367/2503`, `ffi.rs:2355/2962` | `last_sound`（由文本映射到已知标签） | iOS 冒烟有语音通道检查（`last_heard`）；声音标签不单钉 |
| 积木规则 `action:"sound"` | 规则引擎 | `rule_event.value=<tag>` | iOS 冒烟「a chain runs its steps in order」（`last=…`） |
| 大声音惊吓 | 房间麦克风分类 | `last_sound="alarm"` | iOS 冒烟「a loud room startles the duck」 |
| wheee 骑乘分段 | `voice.pending_segment()`（`runtime.rs:4831/4862`） | `last_ride_segment` | iOS 冒烟有骑乘段字段检查；Android 冒烟检查 wheee 手势后的字段 |

规则：合成标签以外的字符串会被核心丢弃（`queue_sound` 只认已知标签），
所以「外壳发明一个声音」不可能发生——这条是保真度边界（FIDELITY.md），不是功能。

未单钉的声音事件（开机问候/peck/coo 等）依赖同一 `queue_sound → take_sound →
PCM` 管道，而管道本身已由上述断言覆盖；单钉它们的毫秒级时序只会制造假红，
按 QA 文档走「记录 + 真机 QA」。
