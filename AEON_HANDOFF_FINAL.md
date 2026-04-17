# AEON Flow 交接文档

**文档日期**: 2026-04-17  
**仓库路径**: `AEON-FLOW/aeon-vm`  
**当前版本**: `aeon-vm 0.2.0`  
**当前状态**: 核心 VM / Snapshot / Assembler / ProgramStore 已实现并通过测试；协作编辑与高级迁移能力未实现

---

## 1. 项目一句话

AEON VM 是一个最小化的寄存器虚拟机，目标是把“程序代码”和“运行状态”分离：程序以内容哈希标识，运行状态可被序列化成快照并在另一端恢复后继续执行。

---

## 2. 当前仓库真实结构

```text
aeon-vm/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── lib.rs              crate 导出入口
│   ├── inst.rs             指令集定义与反汇编
│   ├── program.rs          Program / ProgramMetadata / 内置示例程序
│   ├── vm.rs               VMState / VMError / StepResult
│   ├── snapshot.rs         Snapshot 捕获、恢复、序列化
│   ├── store.rs            ProgramStore，按 ProgramId 保存程序
│   ├── asm.rs              文本汇编器
│   ├── main.rs             aeon-run CLI
│   └── bin/
│       ├── aeon-asm.rs     汇编 CLI
│       ├── aeon-recv.rs    TCP 接收快照并恢复执行
│       ├── check_fib.rs    临时调试工具
│       ├── debug_asm.rs    临时调试工具
│       ├── trace_fib.rs    临时调试工具
│       └── trace_jz.rs     临时调试工具
├── tests/
│   └── tests.rs            28 个集成测试
├── fibonacci.aeon          示例二进制程序
└── programs/
    └── fibonacci.aeon      示例汇编源码
```

注意：旧交接文档里提到的 `editor.rs`、`session.rs`、`console.rs`、`aeon-edit.rs`、`aeon-console.rs` 等文件当前仓库不存在。

---

## 3. 已实现能力

### 3.1 指令执行

当前 VM 使用 256 个 `u64` 寄存器和一个调用栈，支持：

- `LoadImm`
- `Mov`
- `Add`
- `Sub`
- `Mul`
- `Jz`
- `Jump`
- `Call`
- `Ret`
- `Print`
- `Halt`

### 3.2 Program 与 ProgramId

- `Program` 由 `metadata` 和 `instructions` 构成
- `ProgramId` 是对 `instructions` 做 `bincode` 序列化后的 `blake3` 哈希
- `metadata.name` 不参与哈希
- 相同指令序列在任意机器上会得到相同 `ProgramId`

### 3.3 Snapshot

- 快照只保存运行状态，不保存指令
- 可从 `VMState` 捕获
- 可序列化为字节数组
- 可在目标端结合 `ProgramStore` 恢复成 `VMState`
- `to_bytes()` 带有 32 字节 `blake3` 校验前缀

### 3.4 ProgramStore

- 以 `ProgramId -> Arc<Program>` 形式保存程序
- 支持幂等添加
- 支持按文件加载 `.aeon` 程序
- `Snapshot::restore()` 依赖它查找程序

### 3.5 汇编器

- 支持标签
- 支持 `;` 注释
- 支持十进制数字
- 支持 `load/mov/add/sub/mul/jz/jmp/call/ret/halt`
- 输出 `Program`

### 3.6 CLI

公开可用的 CLI 只有三个：

- `aeon-run`
- `aeon-asm`
- `aeon-recv`

其余 `check_fib`、`debug_asm`、`trace_fib`、`trace_jz` 更像调试脚本，不应视为正式接口。

---

## 4. 未实现或与旧文档不一致的内容

以下能力在当前代码中不存在，旧文档不能再按“已完成”描述：

- 快照补丁系统 `Patch / PatchSet / SnapshotEditor`
- 快照检查器 `Inspector`
- 多会话模型 `SessionId / SharedContext / ContextRegistry`
- 交互式控制台 `FlowConsole`
- 堆内存与 `heap / heap_top`
- 快照接收后的交互式修改恢复
- 程序网络分发、下载与完整迁移协议

如果后续要继续做这些能力，应把它们写成 roadmap，而不是当前状态。

---

## 5. 真实核心 API

### 5.1 `Program`

文件：[`aeon-vm/src/program.rs`](/home/arch/AEON-FLOW/aeon-vm/src/program.rs)

```rust
pub struct ProgramMetadata {
    pub name: String,
}

pub struct Program {
    pub metadata: ProgramMetadata,
    pub instructions: Vec<Inst>,
}
```

主要方法：

```rust
let p = Program::new(vec![Inst::LoadImm { dst: 0, val: 42 }, Inst::Halt]);
let p = Program::from_parts("fib".into(), instructions);

let id = p.id();
let n = p.instruction_count();
let asm = p.disassemble();

p.save(path)?;
let p2 = Program::load(path)?;
```

内置程序：

```rust
use aeon_vm::program::programs;

let fib = programs::fibonacci(10); // 结果写到 r2
let fac = programs::factorial(7);  // 结果写到 r1
```

### 5.2 `VMState`

文件：[`aeon-vm/src/vm.rs`](/home/arch/AEON-FLOW/aeon-vm/src/vm.rs)

```rust
pub struct VMState {
    program_id: ProgramId,
    pub regs: Vec<u64>,
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
}
```

主要方法：

```rust
let mut vm = VMState::new(&program);

let result = vm.step(&program);
let total_steps = vm.run(&program)?;
let (steps, halted) = vm.run_bounded(&program, 50);

let program_id = vm.program_id();
```

错误类型：

- `VMError::EmptyCallStack`
- `VMError::RuntimeError(String)` 当前代码定义了该变体，但主流程里基本未使用

### 5.3 `Snapshot`

文件：[`aeon-vm/src/snapshot.rs`](/home/arch/AEON-FLOW/aeon-vm/src/snapshot.rs)

```rust
pub struct Snapshot {
    pub program_id: ProgramId,
    pub regs: Vec<u64>,
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
}
```

主要方法：

```rust
let snap = Snapshot::capture(&vm);
let snap = Snapshot::from_vm(&vm); // capture 的兼容别名

let bytes = snap.to_bytes();
let snap2 = Snapshot::from_bytes(&bytes)?;

snap.save(path)?;
let snap3 = Snapshot::load(path)?;

let vm2 = snap.restore(&store)?;
let size = snap.byte_size();
let id = snap.program_id();
```

两个需要注意的细节：

- `to_bytes()/from_bytes()` 使用“校验和 + payload”格式
- `save()/load()` 直接读写的是纯 `bincode` 序列化，不带校验前缀

这意味着“文件格式”和“网络字节格式”目前并不统一。

### 5.4 `ProgramStore`

文件：[`aeon-vm/src/store.rs`](/home/arch/AEON-FLOW/aeon-vm/src/store.rs)

```rust
let store = ProgramStore::new();

let id = store.add(program);
let p = store.get(&id);
let ok = store.has(&id);
let count = store.count();

let id = store.load_from_file(path)?;
let id = store.load_file(path)?; // 兼容包装
```

### 5.5 `Assembler`

文件：[`aeon-vm/src/asm.rs`](/home/arch/AEON-FLOW/aeon-vm/src/asm.rs)

```rust
let asm = Assembler::new().with_name("demo");
let program = asm.assemble(source)?;
```

汇编语法：

- 注释：`; comment`
- 标签：`loop:`
- 寄存器：`r0` 到 `r255`
- 数字：仅十进制
- 跳转：`jz r0, end` / `jmp loop`

注意：源码注释里写了 `jump`，但汇编器真正接受的是 `jmp`，这一点以实现为准。

---

## 6. 指令集语义

| 指令 | 汇编格式 | 说明 |
|------|----------|------|
| `LoadImm { dst, val }` | `load rD, N` | `regs[D] = N` |
| `Mov { dst, src }` | `mov rD, rS` | `regs[D] = regs[S]` |
| `Add { dst, a, b }` | `add rD, rA, rB` | wrapping 加法 |
| `Sub { dst, a, b }` | `sub rD, rA, rB` | wrapping 减法 |
| `Mul { dst, a, b }` | `mul rD, rA, rB` | wrapping 乘法 |
| `Jz { cond, off }` | `jz rC, label` | `regs[C] == 0` 时做相对跳转 |
| `Jump { offset }` | `jmp label` | 相对跳转 |
| `Call { addr }` | `call label` | 压栈返回地址后跳到绝对地址 |
| `Ret` | `ret` | 从调用栈弹出返回地址 |
| `Print { r }` | 无汇编入口 | 打印寄存器值，仅可通过 Rust 直接构造 |
| `Halt` | `halt` | 停机 |

补充：

- `step()` 在执行 `Halt` 时直接返回 `StepResult::Halted`，不会增加 `steps`
- `pc >= instructions.len()` 也会被视为 `Halted`
- `Jz` 与 `Jump` 的偏移是相对当前指令索引，不是相对下一条

---

## 7. CLI 用法

### 7.1 `aeon-asm`

文件：[`aeon-vm/src/bin/aeon-asm.rs`](/home/arch/AEON-FLOW/aeon-vm/src/bin/aeon-asm.rs)

```bash
cargo run --bin aeon-asm -- programs/fibonacci.aeon -o fibonacci.aeon
```

默认输出路径：

- 如果不传 `-o`，输出到当前工作目录下的 `<stem>.aeon`
- 不是输出到输入文件所在目录

### 7.2 `aeon-run`

文件：[`aeon-vm/src/main.rs`](/home/arch/AEON-FLOW/aeon-vm/src/main.rs)

```bash
cargo run --bin aeon-run -- fibonacci.aeon
cargo run --bin aeon-run -- fibonacci.aeon --snap-at 5
cargo run --bin aeon-run -- fibonacci.aeon --restore fibonacci.snap
```

行为说明：

- `--snap-at n` 会最多执行 `n` 步
- 若 `n` 步内未停机，则把快照保存为 `<program>.snap`
- 若程序已经停机，则不会额外保存快照
- 执行完成后会打印所有非零寄存器

### 7.3 `aeon-recv`

文件：[`aeon-vm/src/bin/aeon-recv.rs`](/home/arch/AEON-FLOW/aeon-vm/src/bin/aeon-recv.rs)

```bash
cargo run --bin aeon-recv -- fibonacci.aeon
```

行为说明：

- 监听 `0.0.0.0:9999`
- 先读 8 字节大端长度，再读快照字节
- 使用 `Snapshot::from_bytes()` 反序列化
- 要求本地已预加载对应程序
- 收到后直接恢复并跑到结束

当前限制：

- 仓库里没有对应的发送端 CLI
- 接收端不验证 `snap.program_id` 与命令行传入程序是否匹配，只要 store 里能找到同 ID 程序即可恢复

---

## 8. 当前不变量

根据实现和测试，当前可以确认的不变量有：

1. `ProgramId` 只取决于 `instructions`
2. `Snapshot` 不包含程序指令，快照大小与程序长度无关
3. `Snapshot::restore()` 必须依赖 `ProgramStore`
4. 相同执行路径下，中途快照并恢复后的最终结果应与不中断执行一致
5. `ProgramStore::add()` 对相同 `ProgramId` 是幂等的

这些不变量都在 [`aeon-vm/tests/tests.rs`](/home/arch/AEON-FLOW/aeon-vm/tests/tests.rs) 有覆盖。

---

## 9. 测试现状

执行命令：

```bash
cargo test
```

2026-04-17 在本仓库实测结果：

- `28 passed`
- `0 failed`

覆盖范围主要包括：

- 指令执行正确性
- Fibonacci / Factorial 示例程序
- ProgramId 稳定性
- Snapshot 捕获、恢复、序列化与损坏检测
- ProgramStore 幂等性
- Assembler 正常路径和错误路径

当前没有覆盖的方向：

- CLI 端到端测试
- TCP 迁移链路的集成测试
- 文件格式与网络格式不一致导致的边界情况

---

## 10. 已知问题和技术债

### 10.1 文档曾严重超前于实现

本次整理前，文档把多个未落地模块写成了已完成。后续必须区分：

- `Current state`
- `Planned roadmap`

不能混写。

### 10.2 `Snapshot` 文件格式与字节流格式不统一

- `save()/load()` 使用纯 `bincode`
- `to_bytes()/from_bytes()` 使用 `checksum + payload`

这会让“文件快照”和“网络快照”表现不同，建议后续统一。

### 10.3 汇编器和指令集存在轻微接口割裂

- `Inst` 支持 `Print`
- 汇编器不支持 `print`

这意味着某些指令只能从 Rust 代码构造，不能从汇编文本得到。

### 10.4 代码里有若干未清理告警

`cargo test` 当前通过，但存在 warning，例如：

- 未使用 import
- 未使用变量
- `#![doc(test = false)]` 写法无效
- `Assembler.version` 未使用

这些不影响当前功能，但说明代码还没做过一轮收口。

---

## 11. 建议的下一步

如果继续推进项目，建议按下面顺序做：

1. 先统一 snapshot 文件格式和网络格式
2. 给 CLI 增加端到端测试
3. 明确发送端协议，再补 `aeon-send`
4. 再决定是否引入堆内存、快照编辑和协作能力

原因很简单：现阶段基础 VM 已经可用，但协议边界和工具链还不稳定，直接堆高级能力只会继续让文档和实现脱节。
