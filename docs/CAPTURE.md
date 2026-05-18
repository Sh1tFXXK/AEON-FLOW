# AEON Capture

AEON Capture 把“我正在做什么”转换成统一的 `CaptureEntry`。文件、剪贴板文本、截图、应用状态、浏览器页面、进程元数据、Android 分享内容都会进入同一条捕获流。

## 统一启动

从仓库根目录运行：

```powershell
.\scripts\aeon.ps1
```

这一条命令会启动：

- Web UI: `http://localhost:8080`
- AEON LAN Discovery: UDP `8091`
- 内置 AEON Relay: `http://localhost:8090`
- Windows 剪贴板文本监听
- 截图目录监听
- `~/AEON` 文件监听
- Claude Desktop、VS Code、浏览器等已知应用捕获器
- 进程面板 API
- Desktop 从 Relay 自动拉取远端捕获内容

只调试捕获 UI 时可以关闭 Relay：

```powershell
.\scripts\aeon.ps1 -Mode desktop
```

## HTTP API

- `GET /api/status`: 本机身份、设备和连接地址
- `GET /api/entries`: 最近捕获流
- `GET /api/entry/:cid`: 元数据和 UTF-8 预览
- `GET /api/entry/:cid/raw`: CID 原始字节
- `POST /api/entry/:cid/edit`: 编辑文本类捕获并生成新版本
- `POST /api/capture/text`: JSON body `{ "text": "...", "title": "..." }`
- `POST /api/capture/drop`: multipart field `file`，用于 Web 拖放和 Android 文件分享
- `POST /api/capture/apps`: 捕获所有正在运行的已知应用
- `GET /api/processes`: 进程面板列表
- `POST /api/capture-process`: 对进程执行截图、元数据、VM 快照/迁移等操作

## Android

`aeon-android/` 是最小 Android 捕获入口：

- `ShareReceiverActivity` 注册为任意 MIME 类型的分享目标
- 文本分享调用 `/api/capture/text`
- 文件/图片分享调用 `/api/capture/drop`
- `PhotoWatcherService` 监听 `MediaStore.Images` 并捕获新照片
- `MainActivity` 自动发现 AEON endpoint，并测试 `/api/devices/hello`

构建并安装：

```powershell
.\scripts\aeon.ps1 -Mode android
```

默认不配置 USB reverse。Android 会优先使用 AEON 自研 LAN Discovery：

1. 手机向 UDP `8091` 广播 `AEON_DISCOVER_V1`。
2. 桌面 AEON 返回 UI/Relay 地址。
3. 手机选择桌面 UI endpoint，例如 `http://192.168.x.x:8080`。
4. 手机向 `/api/devices/hello` 注册，随后分享内容直接进入桌面捕获流。

需要 USB 开发 fallback 时显式使用：

```powershell
.\scripts\aeon.ps1 -Mode android -UsbReverse
```

跨网络时，将手机 endpoint 设置为公网可达的 AEON Relay URL；桌面端用 `-RelayUrl` 连接同一个 Relay space。

## 当前边界

- 浏览器捕获读取 Chrome/Firefox 最近历史条目；真正当前活动标签页仍需要浏览器扩展或 native messaging。
- 任意普通进程目前可捕获元数据和窗口截图；完整可运行状态迁移只对 AEON VM 管理的进程成立。
- Windows 托盘 OLE 拖放尚未实现；Web 拖放、目录监听和进程面板是当前工作路径。
# AEON Capture Notes

AEON capture now includes the original capture surfaces plus typed bridge ingress for SMS and email.

See [AEON_FOUNDATIONS.md](AEON_FOUNDATIONS.md) for the current bridge, context, account-profile, vault, and query API contracts.
