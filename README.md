# AEON Flow

> 程序可以在任意时刻暂停，迁移到另一台机器，在迁移过程中被检查和修改，然后继续执行。

AEON Flow 是一个用于“可暂停、可迁移、可协作执行”的实验性运行时与工具链，核心能力包括：
- 汇编与执行（`aeon-asm` / `aeon-run`）
- 快照与迁移（`aeon-send` / `aeon-recv`）
- 交互式控制台与多人协作（`aeon-console`）
- 后台守护进程管理（`aeon-daemon` / `aeon`）

---

## 仓库结构

- `aeon-vm/`：虚拟机、汇编器、运行器、控制台、daemon 等核心组件。
- `aeon-store/`：状态/事件存储相关模块。
- `aeon-agent/`：代理与自动化能力。
- `aeon-sync/`：同步与静态资源服务。
- `docs/`：补充文档与设计说明。

---

## 安装

### 方式一：只构建 VM 工具（最常用）

```bash
git clone <repo>
cd AEON-FLOW/aeon-vm
cargo build --release
# 把 target/release/ 加入 PATH
```

### 方式二：在仓库根目录分别构建组件

```bash
git clone <repo>
cd AEON-FLOW

(cd aeon-vm && cargo build --release)
(cd aeon-store && cargo test)
(cd aeon-agent && cargo test)
```

---

## 功能使用指南（按场景）

> 下面按“我想做什么”来组织命令。默认你已经能在终端直接使用 `aeon-*` 命令。

### 1) 编译并运行一个程序

```bash
# 查看示例源码
cat aeon-vm/programs/fibonacci.asm

# 编译 .asm -> .aeon
aeon-asm aeon-vm/programs/fibonacci.asm

# 执行
aeon-run fibonacci.aeon
```

适用场景：本地快速验证指令逻辑、调试寄存器结果。

### 2) 在中途打快照并迁移到另一端继续跑

终端 A（接收端）：
```bash
aeon-recv --session alice@laptop/conv-1 --port 9999
```

终端 B（发送端）：
```bash
aeon-send fibonacci.aeon --snap-at 5 --to 127.0.0.1:9999
```

迁移后你可以在控制台继续修改状态再恢复：
```bash
aeon> set reg 0 3
aeon> resume
```

适用场景：线上问题复盘、跨机器接力执行、人工审查后再放行。

### 3) 用控制台交互调试虚拟机状态

```bash
# 从快照载入会话
aeon-console --session bob@desktop/conv-2 --load fibonacci.snap

# 常见操作
aeon> history         # 查看操作历史
aeon> set reg 1 99    # 修改寄存器
aeon> resume          # 继续执行
```

适用场景：定位 bug、手动修正参数、观察执行轨迹。

### 4) 两个账户协作同一个会话

Alice：
```bash
aeon> set reg 0 5
aeon> say 我把倒计时改成5了
aeon> share collab-1
```

Bob：
```bash
aeon-console --session bob@desktop/conv-2 --load fibonacci.snap
aeon> join collab-1
aeon> history
aeon> resume
```

适用场景：双人排障、教学演示、交接班协同处理。

### 5) 用 daemon 管理多个运行实例

```bash
# 启动后台服务
aeon-daemon

# 提交任务
aeon run fibonacci.aeon

# 查看与管理
aeon ps
aeon log vm-1
aeon resume vm-1
```

适用场景：长任务托管、批量任务管理、统一日志查看。


### 6) 使用 Web UI（上传/查看同步历史）

```bash
# 终端1：启动两个 agent（可选，但推荐用于看到多端同步效果）
(cd aeon-agent && AEON_AGENT_LISTEN=0.0.0.0:8787 cargo run)
(cd aeon-agent && AEON_AGENT_LISTEN=0.0.0.0:8788 AEON_AGENT_PEER=127.0.0.1:8787 cargo run)

# 终端2：启动 UI 服务
(cd aeon-sync && cargo run)
```

然后在浏览器打开：`http://localhost:8080`。

在 UI 中你可以：
- 上传文件到同步目录
- 在右侧面板查看历史与同步结果

适用场景：演示同步流程、人工检查变更历史、验证多端同步链路。

---

## 5 分钟上手（最短路径）

```bash
cat aeon-vm/programs/fibonacci.asm
aeon-asm aeon-vm/programs/fibonacci.asm
aeon-run fibonacci.aeon
```

如果只想先确认环境是否可用，跑完上面三条命令即可。

---

## 文档索引

- 指令集：`aeon-vm/docs/ISA.md`
- 架构说明：`aeon-vm/docs/ARCHITECTURE.md`
- 已知限制：`aeon-vm/KNOWN_LIMITATIONS.md`

建议阅读顺序：**ISA → ARCHITECTURE → 示例程序 → 协作/迁移/Daemon 流程**。
