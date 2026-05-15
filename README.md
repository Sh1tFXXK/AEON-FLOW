# AEON Flow

AEON Flow 不是文件上传工具。它把“你正在做的事”捕获成可寻址、可同步、可恢复的数据流：剪贴板、截图、拖入文件、进程信息、已知应用状态、Android 分享内容，以及 AEON VM 快照。

## 一键启动

Windows PowerShell:

```powershell
.\scripts\aeon.ps1
```

CMD:

```cmd
scripts\aeon.cmd
```

Git Bash:

```bash
./scripts/aeon.sh
```

默认启动完整桌面栈：

- Web UI: `http://localhost:8080`
- AEON LAN Discovery: UDP `8091`
- 内置 AEON Relay: `http://localhost:8090`
- 捕获源：剪贴板、截图目录、`~/AEON` 文件夹、Web 拖放
- 应用捕获：Claude Desktop、VS Code、Chrome/Firefox 等已知应用
- 进程面板：列出运行进程，并展示可捕获/迁移选项
- Desktop 自动从 Relay 拉取远端捕获内容

首次启动会创建 Windows 防火墙入站规则：

- `AEON Flow UI TCP 8080`
- `AEON Flow Relay TCP 8090`
- `AEON Flow Discovery UDP 8091`

脚本还会把当前物理 LAN/Wi-Fi 网络切到 Private profile，并把实际 LAN IP 写入 `AEON_LAN_IPS`，避免 Windows Public profile 或多网卡顺序导致 Android 连到错误地址。如果不是管理员 PowerShell，脚本会打印需要手动执行的防火墙命令。

重复运行启动脚本时，如果 `8080` 或 `8090` 已被本仓库旧的 `aeon-sync.exe` 占用，脚本会自动停止旧进程再启动新进程。手动停止：

```powershell
.\scripts\aeon.ps1 -Mode stop
```

## 设备互联

同一网络内不需要 USB、Tailscale 或其他隧道。Android 端打开 AEON Capture 后点击 `AUTO FIND AEON`，应用会广播 `AEON_DISCOVER_V1` 到 UDP `8091`，桌面端返回可用的 UI/Relay 地址，手机随后向桌面 `/api/devices/hello` 注册。

跨网络时使用 AEON 自研 Relay：在公网可达机器上运行 Relay，桌面和手机都连到同一个 Relay space。后续跨公网发现、认证和端到端加密会继续在 AEON Relay 内完成，不依赖第三方隧道。

## 启动模式

完整本机栈，默认推荐：

```powershell
.\scripts\aeon.ps1
```

只启动桌面 UI，不启动本机 Relay：

```powershell
.\scripts\aeon.ps1 -Mode desktop
```

桌面连接到公网 AEON Relay：

```powershell
.\scripts\aeon.ps1 -Mode desktop -RelayUrl http://your-relay-host:8090 -Space home
```

只启动公网 Relay 节点：

```powershell
.\scripts\aeon.ps1 -Mode relay -RelayPort 8090 -Space home
```

构建并安装 Android 调试包：

```powershell
.\scripts\aeon.ps1 -Mode android
```

默认 Android 安装不再配置 `adb reverse`，用于验证无线自研发现。需要 USB 调试通道时显式加：

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

只构建不安装：

```powershell
.\scripts\aeon.ps1 -Mode android -NoInstall
```

常用端口可改：

```powershell
.\scripts\aeon.ps1 -Port 8081 -RelayPort 8090 -DiscoveryPort 8091 -Space home
```

## 仓库结构

- `aeon-sync/`: 桌面 Web UI、捕获 API、进程面板、内置 Relay、LAN Discovery
- `aeon-capture/`: 捕获条目、捕获引擎、剪贴板/截图/文件/应用捕获
- `aeon-android/`: Android 分享入口、照片监听、自动发现
- `aeon-vm/`: 可快照、可恢复、可迁移的 AEON VM 原型
- `aeon-store/`: CID 存储、身份和底层数据结构
- `aeon-agent/`: 早期点对点同步代理
- `docs/`: 捕获、Relay、端到端验证文档

## Android 使用

1. 在桌面运行 `.\scripts\aeon.ps1`。
2. 构建并安装 Android：`.\scripts\aeon.ps1 -Mode android`。
3. 打开 AEON Capture，点击 `AUTO FIND AEON`。
4. 确认 endpoint 自动变成桌面的 LAN UI 地址，例如 `http://192.168.x.x:8080`。
5. 从微信、相册、浏览器或任意 App 分享文字/图片/文件到 AEON。
6. 桌面 AEON 捕获流会自动出现这条数据。

## AEON VM 快速试用

```powershell
cd aeon-vm
cargo build --release
cargo run --bin aeon-asm -- programs\fibonacci.asm -o fibonacci.aeon
cargo run --bin aeon-run -- fibonacci.aeon
```

更多 VM 细节见 [aeon-vm/README.md](aeon-vm/README.md)。

## 开发检查

```powershell
cd aeon-sync
cargo check
```

本机推荐使用 `stable-x86_64-pc-windows-gnu` Rust toolchain 和 Scoop GCC/MinGW。若出现 `dlltool.exe` 或 MSVC `link.exe` 相关错误：

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup override set stable-x86_64-pc-windows-gnu
scoop install gcc
```

## 文档

- [捕获层](docs/CAPTURE.md)
- [AEON Relay](docs/RELAY.md)
- [端到端 smoke](docs/END_TO_END.md)
