# 性能预算与门禁（2026-11-24 起）

原则（沿用 QA 口径 5）：**先记录、再设限**。数字来自真实跑，不编阈值。

## 已在测

| 指标 | 采集点 | 现状 |
|---|---|---|
| Android 壳层重组次数 | debug 构建每 5 s 打 `duck-perf: recompose=N`；冒烟读末值 | **门禁已设**：整轮 2–3 次属正常，>50 判整树重组回归（`smoke-test.sh` D7 段） |
| Android 掉帧率 / 帧耗时分位 | 冒烟末段 `dumpsys gfxinfo` 重置后驱动「叫一声→拖动→开抽屉→返回」，读 `Janky frames` 与 90/95/99th percentile | **本轮起记录**，日志行 `frame pacing: …`。首轮实测（模拟器软件 GL）：Janky 86.46%、90th 57ms、95th 150ms、99th 350ms——模拟器基线极高，上限需按真机或多次运行 band 另定 |
| Android 首帧内容 | `check-frame.py`：非空白、两帧有差异（场景活着） | 门禁（既有） |
| Android 大字号布局 | 首屏 + 12 个 dense 页在 font scale 2.0 下：首焦点、47 dp 底线、不出屏、无叠压 | 门禁（既有） |
| iOS 帧抖动 | 无宿主侧稳定测量（模拟器数字不可信） | **记录型缺口**：按 QA 文档走真机 QA |

## 下一阶梯

1. 用两轮 `frame pacing` 的真实数字定 Android 掉帧上限（预计模拟器 Janky 偏高，阈值要按本机 band 定，不抄桌面数）。
2. iOS 真机用 Instruments 的 Animation Hitches 做一次记录（QA 遗留 3 的关闭方式）。
3. 长列表 Lazy 化（遗留 4）若做，重测本表两侧数字。

## 相关

- `docs/QA_DEEP_REFACTOR_2026-11-20.md` §三 口径 5 与遗留 3/4
- `android/app/src/main/java/ai/duck/microduck/DuckPerf.kt`、`android/scripts/smoke-test.sh` 末段
