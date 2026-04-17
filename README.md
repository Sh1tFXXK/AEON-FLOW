# AEON-FLOW

AEON-FLOW 当前包含一个 Rust 实现的最小可迁移虚拟机项目：[`aeon-vm`](/home/arch/AEON-FLOW/aeon-vm)。它支持：

- 定义和执行小型寄存器指令集程序
- 将运行中 VM 状态捕获为快照
- 在另一端基于相同 `ProgramId` 恢复并继续执行
- 将文本汇编源码组装成 `.aeon` 二进制程序

根目录的 [`AEON_HANDOFF_FINAL.md`](/home/arch/AEON-FLOW/AEON_HANDOFF_FINAL.md) 是按当前仓库状态整理的完整交接文档。

## 项目结构

```text
AEON-FLOW/
├── README.md
├── AEON_HANDOFF_FINAL.md
└── aeon-vm/
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs
    │   ├── inst.rs
    │   ├── program.rs
    │   ├── vm.rs
    │   ├── snapshot.rs
    │   ├── store.rs
    │   ├── asm.rs
    │   ├── main.rs
    │   └── bin/
    │       ├── aeon-asm.rs
    │       └── aeon-recv.rs
    ├── tests/
    │   └── tests.rs
    └── programs/
        └── fibonacci.aeon
```

## 快速开始

在 `aeon-vm/` 下执行：

```bash
cargo test
```

当前测试结果：`28 passed; 0 failed`。

构建汇编器并生成程序：

```bash
cargo run --bin aeon-asm -- programs/fibonacci.aeon -o fibonacci.aeon
```

运行程序：

```bash
cargo run --bin aeon-run -- fibonacci.aeon
```

在第 `n` 步后生成快照：

```bash
cargo run --bin aeon-run -- fibonacci.aeon --snap-at 5
```

从快照恢复：

```bash
cargo run --bin aeon-run -- fibonacci.aeon --restore fibonacci.snap
```

接收远端发来的快照并继续执行：

```bash
cargo run --bin aeon-recv -- fibonacci.aeon
```

## 当前实现边界

当前仓库只实现了 VM、快照、程序仓库、汇编器和基础 TCP 接收恢复流程。

没有实现的内容包括：

- 快照在线编辑
- 多会话协作上下文
- 交互式控制台
- 堆内存模型
- 完整的双端迁移协议
