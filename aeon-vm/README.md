# AEON VM

`aeon-vm` 是 AEON Flow 的可运行状态层：程序可以被暂停、快照、检查、修改，然后恢复或迁移。捕获层负责“接住正在做的事”，VM 层负责让被 AEON 管理的进程具备真正可恢复的运行状态。

完整桌面系统请从仓库根目录统一启动：

```powershell
.\scripts\aeon.ps1
```

本 README 只覆盖 VM 工具本身。

## 构建

```powershell
cd aeon-vm
cargo build --release
```

## 5 分钟上手

编译并运行示例程序：

```powershell
cargo run --bin aeon-asm -- programs\fibonacci.asm -o fibonacci.aeon
cargo run --bin aeon-run -- fibonacci.aeon
```

运行到指定步数并保存快照：

```powershell
cargo run --bin aeon-run -- fibonacci.aeon --snap-at 5
```

从快照恢复：

```powershell
cargo run --bin aeon-run -- fibonacci.aeon --restore fibonacci.snap
```

## 迁移/接收

接收端：

```powershell
cargo run --bin aeon-recv -- fibonacci.aeon --port 9999
```

发送端：

```powershell
cargo run --bin aeon-send -- fibonacci.aeon --snap-at 5 --to 127.0.0.1:9999
```

## 控制台检查

```powershell
cargo run --bin aeon-console -- --load fibonacci.snap
```

常用命令：

```text
history
regs
set reg 0 3
resume
```

## Daemon

AEON daemon 用于托管多个 VM 运行实例。桌面进程面板会把 AEON VM 管理的进程识别为可迁移/可快照对象。

```powershell
cargo run --bin aeon-daemon
cargo run --bin aeon -- ps
```

## 文档

- [指令集](docs/ISA.md)
- [架构](docs/ARCHITECTURE.md)
- [Forth 原型](docs/FORTH.md)
- [语言选择](docs/LANGUAGE_CHOICE.md)
- [已知限制](KNOWN_LIMITATIONS.md)
