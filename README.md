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

> 建议先完成 `aeon-vm` 的构建，再体验下面的 5 分钟上手。

---

## 5 分钟上手

```bash
# 1) 查看示例程序
cat aeon-vm/programs/fibonacci.asm

# 2) 编译
aeon-asm aeon-vm/programs/fibonacci.asm

# 3) 运行
aeon-run fibonacci.aeon
# 预期：r2 = 55

# 4) 暂停并迁移（两个终端）
aeon-recv --session alice@laptop/conv-1 --port 9999
aeon-send fibonacci.aeon --snap-at 5 --to 127.0.0.1:9999

# 5) 在控制台里修改后继续
aeon> set reg 0 3
aeon> resume
# 预期：总步数 23，r2 = 3
```

---

## 两账户协作示例

```bash
# Alice
aeon> set reg 0 5
aeon> say 我把倒计时改成5了
aeon> share collab-1

# Bob
aeon-console --session bob@desktop/conv-2 --load fibonacci.snap
aeon> join collab-1
aeon> history
aeon> set reg 1 99
aeon> resume
```

---

## Daemon 管理示例

```bash
aeon-daemon
aeon run fibonacci.aeon
aeon ps
aeon log vm-1
aeon resume vm-1
```

---

## 文档索引

- 指令集：`aeon-vm/docs/ISA.md`
- 架构说明：`aeon-vm/docs/ARCHITECTURE.md`
- 已知限制：`aeon-vm/KNOWN_LIMITATIONS.md`

如果你是首次接触本项目，建议阅读顺序：**ISA → ARCHITECTURE → 示例程序 → Daemon/协作流程**。
