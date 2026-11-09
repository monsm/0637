# iOS 签名与真机安装

## 现在 CI 产出什么

| 产物 | 用途 |
|---|---|
| `Duck-ios-simulator.zip` | **模拟器包**：解压后 `xcrun simctl install booted Duck.app`，或直接拖进 Simulator。仅 arm64 Mac 上的 iOS 模拟器可用 |
| `Duck-ios-app` | 未压缩的同一份 `.app` |
| `Duck-ios-ipa` | **真机 IPA**：仅在配置了签名 secrets 时产出（见下） |

模拟器包做的是 ad-hoc 签名（`CODE_SIGN_IDENTITY=-`），**不能装进物理 iPhone** ——
iOS 真机只接受用 Apple 开发者证书 + provisioning profile 签名的 App。这是平台限制，
不是构建配置问题。

## 让 CI 产出可安装的 IPA

需要 Apple 开发者账号（付费账号才能做 CI 签名；免费 Apple ID 只能在 Xcode 本地
用自己的 Apple ID 做 7 天有效期的开发签名）。

在仓库 `Settings → Secrets and variables → Actions` 添加：

| Secret | 内容 |
|---|---|
| `IOS_CERT_P12_BASE64` | `.p12` 证书的 base64（`base64 -i cert.p12 \| pbcopy`） |
| `IOS_CERT_PASSWORD` | `.p12` 的导出密码 |
| `IOS_PROVISIONING_PROFILE_BASE64` | `.mobileprovision` 的 base64 |
| `IOS_PROVISIONING_PROFILE_NAME` | profile 的 Name 字段（Xcode 里那一行名字） |
| `IOS_TEAM_ID` | 10 位 Team ID（developer.apple.com → Membership） |

可选仓库 **Variable**：

| Variable | 默认 | 说明 |
|---|---|---|
| `IOS_EXPORT_METHOD` | `development` | `development`（已注册 UDID 的设备）/ `ad-hoc` / `app-store-connect` |

推送后 workflow 里会多出 `check-signing` 与 `device` 两个 job：
前者检查 secrets 是否齐全，后者在 Mac runner 上编译 device slice、签名、归档并导出
`Duck-ios-ipa` artifact。没有 secrets 时 `device` 直接 skipped，不影响模拟器构建。

## 设备 UDID

`development` / `ad-hoc` 分发都要求目标 iPhone 的 UDID 已登记在该 provisioning
profile 里（developer.apple.com → Devices）。装到未登记设备会报
"无法安装此 App"。

## 本地安装 IPA

- Apple Configurator / Finder（macOS）：连接 iPhone 后拖入
- 或 `xcrun devicectl device install app --device <UDID> <path>.ipa`（Xcode 15+）
