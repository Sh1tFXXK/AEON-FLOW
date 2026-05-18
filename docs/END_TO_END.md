# AEON End-to-End Smoke

## 1. 统一启动

从仓库根目录启动完整桌面栈：

```powershell
.\scripts\aeon.ps1
```

确认终端输出包含：

- `AEON LAN discovery listening on udp://0.0.0.0:8091`
- `AEON embedded Relay started`
- `AEON Flow capture service started`
- `Local: http://localhost:8080`
- `LAN: http://<desktop-lan-ip>:8080`
- `AEON Relay LAN: http://<desktop-lan-ip>:8090`

打开 `http://localhost:8080`。

## 2. 捕获流

- 在 Windows 复制一段文字，确认捕获流出现剪贴板条目。
- 截图，确认捕获流出现图片条目。
- 拖一个文件到 Web UI，确认生成 CID。
- 点击条目详情，确认文本类条目可以编辑并生成新版本。

## 3. 进程面板

- 打开“进程面板”。
- 确认 Claude Desktop、VS Code、Chrome 等已知应用显示深度捕获选项。
- 对未知进程点击“捕获进程信息”，确认捕获流出现进程元数据。
- 对 AEON VM 管理的进程确认显示“迁移 / 快照 / 暂停”操作。

## 4. Android 无线发现

构建并安装 APK：

```powershell
.\scripts\aeon.ps1 -Mode android
```

默认脚本会清理 AEON 端口上的旧 `adb reverse`，用于验证无线链路。

安装后：

1. 打开 AEON Capture。
2. 点击 `AUTO FIND AEON`。
3. endpoint 应自动变为桌面 LAN UI 地址，例如 `http://192.168.x.x:8080`。
4. 桌面 UI 的“设备”页应出现 Android 设备并显示在线。
5. 从任意 Android App 分享文字、图片或文件到 AEON。
6. 桌面捕获流应出现对应条目。

需要 USB 调试 fallback 时：

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

## 5. 跨网络 Relay

如果手机无法直连桌面网络，在公网可达机器上启动 AEON Relay：

```powershell
.\scripts\aeon.ps1 -Mode relay -RelayPort 8090 -Space home
```

桌面连接这个 Relay：

```powershell
.\scripts\aeon.ps1 -Mode desktop -RelayUrl http://your-relay-host:8090 -Space home
```

Android endpoint 填：

```text
http://your-relay-host:8090
```

## 6. 低层 agent smoke（可选）

旧的点对点 agent 仍可单独验证：

```powershell
cd aeon-agent
$env:AEON_AGENT_LISTEN = "0.0.0.0:8787"
cargo run
```

另一个终端：

```powershell
cd aeon-agent
$env:AEON_AGENT_LISTEN = "0.0.0.0:8788"
$env:AEON_AGENT_PEER = "127.0.0.1:8787"
cargo run
```

## 7. Tombstone 语义

- 远端删除只会在本地文件不比 tombstone 更新时应用。
- 启动时会重放持久化 tombstone，保证冷启动一致性。
