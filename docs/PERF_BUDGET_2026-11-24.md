# 性能预算与门禁（2026-11-24 起）

原则（沿用 QA 口径 5）：**先记录、再设限**。数字来自真实跑，不编阈值。

## 已在测

| 指标 | 采集点 | 现状 |
|---|---|---|
| Android 壳层重组次数 | debug 构建每 5 s 打 `duck-perf: recompose=N`；冒烟读末值 | **门禁已设**：整轮 2–3 次属正常，>50 判整树重组回归（`smoke-test.sh` D7 段） |
| Android 掉帧率 / 帧耗时分位 | 冒烟末段 `dumpsys gfxinfo` 重置后驱动「叫一声→拖动→开抽屉→返回」，读 `Janky frames` 与 90/95/99th percentile | **门禁已设（2026-11-24 第二轮后）**：两轮绿跑定出 band（Janky 56.7–86.5%、99th 121–350 ms），门禁只抓崩溃级退化——Janky ≤ 95%、99th ≤ 500 ms；正常的 CPU 模拟 GPU 波动不触发。真机上限另定 |
| Android 首帧内容 | `check-frame.py`：非空白、两帧有差异（场景活着） | 门禁（既有） |
| Android 大字号布局 | 首屏 + 12 个 dense 页在 font scale 2.0 下：首焦点、47 dp 底线、不出屏、无叠压 | 门禁（既有） |
| iOS 帧抖动 | 无宿主侧稳定测量（模拟器数字不可信） | **记录型缺口**：按 QA 文档走真机 QA |

## 下一阶梯

1. ~~用两轮 `frame pacing` 的真实数字定 Android 掉帧上限~~ 已设崩溃级门禁（Janky ≤95%、99th ≤500ms）；真机 band 仍待真机 QA。
2. iOS 真机用 Instruments 的 Animation Hitches 做一次记录（QA 遗留 3 的关闭方式）。
3. 长列表 Lazy 化（遗留 4）若做，重测本表两侧数字。

## 相关

- `docs/QA_DEEP_REFACTOR_2026-11-20.md` §三 口径 5 与遗留 3/4
- `android/app/src/main/java/ai/duck/microduck/DuckPerf.kt`、`android/scripts/smoke-test.sh` 末段
