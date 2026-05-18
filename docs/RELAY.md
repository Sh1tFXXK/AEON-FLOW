# AEON Relay

AEON Relay 是 AEON 自研的跨网络捕获通道。它替代“临时开隧道、找 VPN、猜局域网 IP”的零散流程：远端设备把内容发到 Relay，桌面 AEON 从 Relay 拉取并导入本地捕获流。

## 模型

```text
Android / remote device
        POST /api/capture/text or /api/capture/drop
        |
        v
AEON Relay
        stores items by space
        |
        v
Desktop AEON
        polls /api/relay/pull
        imports into local capture stream
```

同一网络优先使用 AEON LAN Discovery：Android 向 UDP `8091` 广播，桌面返回 `http://<desktop-lan-ip>:8080`，手机直接连桌面 UI。跨网络时使用公网可达的 AEON Relay 节点。TLS、认证和端到端加密是 Relay 后续能力，不通过第三方隧道临时拼出来。

## 本机完整栈

默认启动会同时启动桌面 UI、LAN Discovery 和内置 Relay：

```powershell
.\scripts\aeon.ps1
```

默认端口：

- UI: `http://localhost:8080`
- Relay: `http://localhost:8090`
- LAN Discovery: UDP `8091`

Windows 首次启动会自动创建入站防火墙规则。如果 Android 显示 `failed to connect` 或 `timeout`，先确认存在这些规则：

```powershell
Get-NetFirewallRule -DisplayName 'AEON Flow*'
```

应看到：

- `AEON Flow UI TCP 8080`
- `AEON Flow Relay TCP 8090`
- `AEON Flow Discovery UDP 8091`

启动脚本会把物理 LAN/Wi-Fi 网络切到 Private profile，并通过 `AEON_LAN_IPS` 固定实际可用的本机地址顺序。多网卡机器上，和手机同网段的地址会优先显示，例如 `http://192.168.0.44:8080`。

桌面会自动从本机 Relay 拉取内容。UI “设备”页会显示可复制的地址，同时 Android 端也会自动发现桌面 endpoint。

## Android 连接

推荐流程：

1. 桌面运行 `.\scripts\aeon.ps1`。
2. Android 安装并打开 AEON Capture。
3. 点击 `AUTO FIND AEON`。
4. 确认 endpoint 自动变成 `http://<desktop-lan-ip>:8080`。

USB reverse 只作为开发 fallback：

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

## 公网 Relay 节点

在公网机器上只启动 Relay：

```powershell
.\scripts\aeon.ps1 -Mode relay -RelayPort 8090 -Space home
```

桌面连接这个 Relay：

```powershell
.\scripts\aeon.ps1 -Mode desktop -RelayUrl http://your-relay-host:8090 -Space home
```

Android endpoint 也填写同一个 Relay URL：

```text
http://your-relay-host:8090
```

## Relay API

- `POST /api/devices/hello`
- `POST /api/capture/text`
- `POST /api/capture/drop`
- `POST /api/relay/push`
- `GET /api/relay/pull?space=home&after=<cursor>`
- `GET /api/relay/status`

## 已实现

- Android/远端文本捕获进入 Relay
- Android/远端文件和图片分享进入 Relay
- Desktop 自动拉取并导入捕获流
- Relay spaces、cursor 和设备身份 header
- `aeon-sync` 默认内嵌 Relay，可用一个进程启动完整栈
- LAN Discovery UDP `8091`，同网设备无需 USB 或第三方隧道

## 后续

- Desktop 推送到 Relay，手机侧也能浏览桌面捕获流
- Relay auth token
- 端到端加密
- Relay 承载 AEON VM 快照交接
- WebSocket 或 long-polling 替代轮询
