# QA · 按对比报告全量修复批次（2026-11-18 晚）

- 对象：`docs/COMPARISON_OFFICIAL_VS_MOBILE_2026-11-18.md` §5/§5.1 可修项的全量修复（38 文件修改 + 10 新增，约 +6400/-1600 行；含全 crate rustfmt 扫尾——HEAD 本就不 fmt-clean，新 CI fmt 门禁迫使）。
- 方式：6 维对抗审查（runtime / ffi-pad / ipc-protocol / sensors-ci / shells / contracts-docs）→ 每条发现双镜头验证 → 定点复审（仅查上轮 must-fix 的落实）→ 修复复审。共 4 轮，35+4+3 agent。
- 判定历程：round 1 **block**（6 major）→ 全修；round 2 **block**（1 blocker + 4 major）→ 全修；round 3 **block**（1 blocker + 6 major）→ 全修；round 4 定点整改后本地全绿，剩余 minor 与「CI 首跑裁决」项如下。
- 本地验证（修复后最终树）：默认特性 `cargo test --workspace --include-ignored` **243+9 全绿**；`--features mujoco --test-threads=1` **272+9 全绿**；Android `compileDebugKotlin + testDebugUnitTest` **70 全绿**；六道静态契约门禁全绿；`cargo fmt -p duck-mobile-core --check` 0。每项修复带「临时回退→新测试红→恢复→绿」证据（见各工单交付说明与测试名）。

## 已修（对抗复审确认成立的全部缺陷）

1. rest/shutdown 状态机：AtRest 稳态可达（每拍 REST_POSITION/gain200/torque、不 power-off）；Limp/摔倒/中坡 request_rest 走官方 cannot-sit（切扭矩、forget、不硬坐）；relax/disable 离开 AtRest；关机同拍 label=off。
2. AtRest 退出与门：enable/Start 清 AtRest；rest 后 pad Init/Home 不被拒；技能/情绪/sit/ground_pick/posture/heard 与 C ABI 三入口（技能环、大按钮、语音）在非驱动态拒绝而非 accepted-then-frozen；mode 切换保持 limp、且等待在途换包。
3. init 官方四分支（Limp 只上力矩+Homing，不开策略、不 greet）；enable(false) 停策略回 home/实测座位；坡道完成刷新 hold。
4. health：启动判据改「从未成功读」→官方两分支（degraded/unhealthy）；policy_error、连续读错误入 unhealthy；Err 时 IPC health 仍可读（并行工单补 publish）。
5. 读取失败 coast（阈值 10 次 unhealthy）；Checkpoint/fork 补齐 rest_requested/shutdown_power_off/policy_error/consecutive_errors/startup_bus_failures/disabled_hold/coast/sensors_read_ns/has_read（父子轨迹与 rewind 一致）。
6. disturb 门：按官方 change_disturbs 逐驱动槽比较 + Reload 语义；swap 落地绑定 owner 鸭（分支会话不误报/不丢发布）；落地时按 active 重发技能表、清 degraded；tick/step/seek 共用落地钩子。
7. DKRB 恢复 1s；pad BodyPose 离开释放、Home=关策略+init、sit/stand 幂等、Rest 独立、Drive 官方四轴。
8. robot.* 扩到 21+ 方法（全部接 intent 执行面）；subscribe 身份字段；thermin/chorale/setSkill/removeSkill 保持 METHOD_NOT_FOUND（登记）。
9. to_tof_grid 官方 Reprojector 几何；fetch-policies 读 manifest（路径穿越拒绝）；mujoco 测试去 Windows 硬编码（fixtures/DUCK_POLICIES_DIR）。
10. 双端：walk 覆盖持久化重放与 `"none"` 归一化；规则分享 timer 全链路；Android shake→push（与 iOS 逐值同阈值）。
11. CI：fmt/clippy（存量 lint 精确白名单）、MuJoCo 测试 job、shellcheck（存量告警按文件精确豁免）；COMPANION 七动词；对比报告 §0 状态回填。

## 遗留（如实登记，非本轮缺陷）

- **CI 首跑裁决**：新增的 ubuntu MuJoCo job、fmt/clippy 白名单、shellcheck 在 runner 上的版本差异；Swift/Kotlin 全量由 ios.yml/android.yml 裁决。
- **已知 minor（交互级，已评估不影响主路径）**：coast 与 fork 的极端组合（手机 MuJoCo 不会读失败，仅测试 harness 可达）、分支会话下 GoingHome 回退原因的保留细节、`seated hold` 用实测位姿已改（原常量断言已替换）。
- **刻意保留（对比报告 §0.2 逐条有理由）**：BAM 执行器路径、齿隙编码器读数、传感器回报语义、麦克风替身、多鸭同世界、ToF 场景射线/相机帧流、命令延迟、运行期增删技能、社区策略门禁、setMode 整机重建、updaterd/configd 族、robotctl CLI、chorale 乐谱、head_imu、API_VERSION 握手、coverage 基线。

## 附：CI 首跑（2026-11-18 晚）

- **Core**：首红两因——(a) `cargo fmt -p` 顺着 `#[path]` 格式化 upstream 文件（不可改），改为 rustfmt 逐自有文件 + `skip_children`；(b) ONNX Runtime 的 GitHub release 404（仓库整体不可达），Linux 取库回退 PyPI 同版本 wheel。新 MuJoCo job 另经三次链接方案迭代：`ld -r` 折叠 → `ar` 打包 → 直链 whole-archive 均失败（lld 在该 runner 上取不到静态档案符号），最终改为源码构建**共享库** + build.rs rpath，**Core 全绿（含 ffi 测试首次进 CI）**。
- **Contracts**：shellcheck 抓到 fetch-ort 脚本 SC2155（先声明后赋值），修后绿。
- **Android**：首红为两处 Compose import（已在本批 earlier 修掉）；二红是冒烟断言——POST `curious` 恰好完成当日「做 2 个表情」任务，庆祝播 `excited` 覆盖 curious，轮询永远看不到。修：`play_emotion` 落实官方「一次只播一个」；冒烟改用终身记录计数（与 wav 入队同一调用），并把大 JSON 的 `grep -q` 管道改为 here-string（`pipefail` 下 SIGPIPE 假阴性）。**Android 全绿**。
- **iOS**：由 ios.yml 裁决。
