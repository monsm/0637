# Contracts · UI 契约门禁（B1，2026-11-15）

> 来源：`docs/UIUX_REFACTOR_2026-11-15.md` §5 批次 B1。静态检查，零 UI 代码，Windows/ubuntu/macos 都能跑（纯 Python 标准库）。

## 这是什么

双端 UI 的四层静态护栏。规矩只有一条：**每个阈值由脚本从代码现场算出（`--update`），只许变好不许变坏**；想改基线必须带着解释单独提交。

| 脚本 | 守什么 | 基线/例外 |
|---|---|---|
| `../scripts/check-copy-keys.py` | 两端 Copy 键**集合**相等（B4 §6.4 原有） | 无 |
| `../scripts/check-copy-values.py` | 两端 Copy 键**值**一致（插值/格式占位归一后比较；单侧形状重组豁免） | `copy-exceptions.json`（platform_facts + pending，只缩不扩） |
| `../scripts/check-ui-baseline.py` | 债务棘轮：固定字号 193/143、深色硬写 16、按压零反馈 13、模态布尔 14、god 文件行数、颜色字面量；护栏指标（saveable/BackHandler/customActions/播报/减动效）只许增 | `ui-baseline.json`（2458dec 现算） |
| `../scripts/check-ui-entries.py` | 功能台账：46 项、84 个源码锚点——入口不许悄悄消失；not-implemented 行必须写明原因 | `ui-entries.json` |
| `../scripts/check-protected-paths.py` | **警告级**：列出本 push 触碰的保真承载文件（sensors/mujoco/runtime/control/bam/pickup/upstream/policies），交人工复核。刻意不拦——它拦得住 UI 批次夹带语义改动，也会拦住老周的正当核心修 bug；升级为强制门禁前先跑一段时间 | 无 |

## 已知边界（不藏着）

- **源码级锚点证不了运行时可达性**：iOS 的 pad-send chip 带合法 id 却嵌在永不渲染的闭包里（FE-1）——tag 断言对死链永远绿。可达性归 UI 测试（B2/B3 批补），台账是地图不是证明。
- **值比较的解析边界**：`pick("…","…")` 字面量（含嵌套引号与插值）都覆盖；不经 pick 组装的字符串不可见（脚本头注释有写）。
- **棘轮防倒退不修已坏**：基线就是今天的债，锁它只保证不再恶化；把数字降下去是 B2–B8 批次的事。

## 跑法

```bash
python3 scripts/check-copy-values.py .        # 等四个脚本
python3 scripts/check-ui-baseline.py . --update   # 仅当有意重定基线；单独提交并解释
```

CI：`contracts.yml`（push 全仓路径 + workflow_dispatch）+ ios.yml/android.yml 各挂同组步骤（UI 推送就地验，失败归因单端）。
