# AEON Flow — 项目交接文档
**版本**: v3.0  
**代码库**: `aeon-vm-v2`  
**状态**: Step 1 已完成并通过测试，Step 2 起待实现

---

## 一、项目目标（一句话）

> 建立一个能完整捕获和恢复程序运行状态的虚拟机，使程序可以在任意时刻、任意设备间迁移，迁移过程中可以被任何授权会话检查和修改状态，多个账户可以共享同一个运行上下文。

---

## 二、已完成的代码结构

```
aeon-vm-v2/
├── Cargo.toml
├── src/
│   ├── lib.rs            模块入口
│   ├── instruction.rs    Inst 枚举（10 条指令）
│   ├── program.rs        Program + programs::fibonacci/factorial
│   ├── vm.rs             VMState（无代码字段）
│   ├── snapshot.rs       Snapshot（含 heap/heap_top 预留字段）
│   ├── store.rs          ProgramStore（内容寻址，幂等）
│   ├── asm.rs            文本汇编器（.asm → Program）
│   ├── editor.rs         Patch / PatchSet / SnapshotEditor / Inspector
│   ├── session.rs        SessionId / Lamport / SharedContext / ContextRegistry
│   ├── console.rs        FlowConsole（交互式心流控制台）
│   ├── main.rs           aeon-run CLI
│   ├── tests.rs          28 个基础测试
│   ├── tests_editor.rs   20 个编辑器测试
│   └── tests_session.rs  19 个会话测试
├── bin/
│   ├── aeon-asm.rs       汇编器 CLI
│   ├── aeon-recv.rs      TCP 接收端（骨架，Step 2 补全）
│   ├── aeon-edit.rs      快照编辑器 CLI
│   └── aeon-console.rs   心流控制台（接收后交互）
└── programs/
    ├── fibonacci.asm
    └── factorial.asm
```

**当前测试总数：67 个。任何修改后必须全部通过。**

```bash
cargo test
```

---

## 三、核心 API（写新代码前必须熟悉）

以下是所有模块的精确 API，字段名、方法签名均与代码一致。

### Program

```rust
// 创建（只接受 Vec<Inst>，不接受名称参数）
let p = Program::new(vec![Inst::LoadImm { dst: 0, val: 42 }, Inst::Halt]);
p.metadata.name = "my_prog".into();  // 名称单独设置

// 预置程序（返回 Program，不是 Vec<Inst>）
use aeon_vm::program::programs;
let p = programs::fibonacci(10);  // r2 = fib(10) = 55
let p = programs::factorial(7);   // r1 = 7! = 5040

// 内容哈希（程序身份）
let id: [u8; 32] = p.id();       // Blake3，只依赖指令，不含元数据

// 序列化
let bytes = p.to_bytes();
let p2 = Program::from_bytes(&bytes)?;
p.save(Path::new("fibonacci.aeon"))?;
let p3 = Program::load(Path::new("fibonacci.aeon"))?;
```

### VMState

```rust
// 创建（从 Program）
let mut state = VMState::new(&program);  // 接收 &Program

// 执行
let total_steps: u64 = state.run(&program)?;          // 运行到 Halt，返回 Result<u64, VMError>
let (taken, halted): (usize, bool) = state.run_bounded(&program, 50);  // 最多 50 步
let result: StepResult = state.step(&program);         // 单步

// 读取状态
state.regs[2]        // 寄存器（u64，共 256 个）
state.pc             // 程序计数器
state.call_stack     // Vec<usize>，返回地址栈
state.steps          // u64，跨快照累计步数
state.program_id     // [u8; 32]，当前程序的内容哈希
```

### Snapshot

```rust
// 捕获（注意：方法名是 capture，不是 from_vm）
let snap = Snapshot::capture(&state);   // 接收 &VMState

// 序列化
let bytes = snap.to_bytes();
let snap2 = Snapshot::from_bytes(&bytes)?;
snap.save(Path::new("vm.snap"))?;
let snap3 = Snapshot::load(Path::new("vm.snap"))?;
snap.byte_size()   // usize

// 恢复（需要 program 已在 store 中）
let mut state2 = snap.restore(&store)?;   // Result<VMState, SnapshotError>
// SnapshotError::ProgramNotFound(program_id) 表示需要先获取程序

// 当前预留字段（Step 3 实现堆后填充）
snap.heap: Option<Vec<u8>>     // None 直到 Step 3
snap.heap_top: Option<usize>   // None 直到 Step 3
```

### ProgramStore

```rust
let store = ProgramStore::new();

// 添加（幂等，同一程序加两次只存一份）
let id: [u8; 32] = store.add(program);

// 查询
let prog: Option<Arc<Program>> = store.get(&id);
let exists: bool = store.has(&id);
let count: usize = store.count();

// 从文件加载
let id = store.load_file(Path::new("fibonacci.aeon"))?;
```

### SnapshotEditor / PatchSet / Inspector

```rust
use aeon_vm::editor::{Inspector, PatchSet, SnapshotEditor};

// 构建 PatchSet（fluent builder，每步自动记录 old_value）
let patchset = SnapshotEditor::new(&snap, "描述")
    .set_reg(0, 42)?                  // 修改寄存器
    .set_pc(5)?                       // 修改 PC
    .set_heap_byte(0x100, 0xFF)?      // 修改堆字节（需 snap.heap.is_some()）
    .set_heap_range(0x10, vec![1,2,3])?
    .set_heap_str(0x20, "hello")?
    .set_heap_u64(0x30, 12345)?
    .set_heap_top(512)?
    .set_call_stack_entry(0, 10)?
    .build();                         // → PatchSet

// 应用（不修改 snap，返回新快照）
let patched: Snapshot = patchset.apply(&snap)?;

// 撤销（生成逆 PatchSet）
let undo: PatchSet = patchset.reverse();
let restored: Snapshot = undo.apply(&patched)?;

// 序列化（用于传输或存档）
let bytes = patchset.to_bytes();
let ps2 = PatchSet::from_bytes(&bytes)?;

// 检查
Inspector::new(&snap).summary();              // 打印摘要
Inspector::new(&snap).dump_regs(0, 15);       // 打印 r0-r15
Inspector::new(&snap).dump_heap(0x100, 64);   // 十六进制 dump
let diffs: Vec<String> = Inspector::diff(&snap_a, &snap_b);
```

### SessionId / SharedContext

```rust
use aeon_vm::session::{SessionId, SharedContext, ContextRegistry};

// 身份（格式："username@device/conversation"）
let alice = SessionId::new("alice", "laptop", "conv-1");
let bob   = SessionId::from_str("bob@desktop/conv-2");

// 共享上下文（base 快照 + 叠加补丁 + 消息）
let mut ctx = SharedContext::new("collab-id", base_snap, alice.clone());
ctx.join(bob.clone());

// 应用补丁（验证后提交，返回 Lamport 时钟）
let clock = ctx.apply_patch(alice.clone(), "说明", patchset)?;

// 消息
let clock = ctx.post_message(bob.clone(), "这里改一下 r0");

// 当前快照（base + 所有补丁叠加）
let current: Snapshot = ctx.current_snapshot()?;

// 序列化（用于跨设备共享）
let bytes = ctx.to_bytes();
let ctx2 = SharedContext::from_bytes(&bytes)?;

// 本地注册表（管理多个 context）
let registry = ContextRegistry::new();
let arc_ctx = registry.create("my-ctx", snap, alice.clone());
let arc_ctx = registry.get("my-ctx").unwrap();
```

### FlowConsole

```rust
use aeon_vm::console::{FlowConsole, ConsoleResult};

// 创建并启动（阻塞直到用户输入 resume/discard/quit）
let console = FlowConsole::new(session_id, base_snapshot);
let result = console.run(&store);

match result {
    ConsoleResult::Resume(patched_snap) => { /* 用 patched_snap 继续执行 */ }
    ConsoleResult::Discard(original)    => { /* 用原始快照继续 */ }
    ConsoleResult::Quit                 => { /* 用户退出 */ }
}
```

---

## 四、指令集（当前 10 条）

所有指令执行后 PC += 1，除非明确说明。

| 指令 | 汇编语法 | 语义 |
|------|---------|------|
| `LoadImm { dst, val }` | `load rD, imm` | `regs[D] = imm` |
| `Mov { dst, src }` | `mov rD, rS` | `regs[D] = regs[S]` |
| `Add { dst, a, b }` | `add rD, rA, rB` | `regs[D] = regs[A] + regs[B]`（wrapping） |
| `Sub { dst, a, b }` | `sub rD, rA, rB` | `regs[D] = regs[A] - regs[B]`（wrapping） |
| `Mul { dst, a, b }` | `mul rD, rA, rB` | `regs[D] = regs[A] * regs[B]`（wrapping） |
| `Jz { cond, off }` | `jz rC, label` | `if regs[C] == 0: pc += off`（相对，i16） |
| `Jmp { off }` | `jmp label` | `pc += off`（相对，i16） |
| `Call { addr }` | `call label` | `call_stack.push(pc+1); pc = addr` |
| `Ret` | `ret` | `pc = call_stack.pop()`（空栈 → EmptyCallStack） |
| `Halt` | `halt` | 停机 |

**汇编语法规则**：注释用 `;`，标签以 `:` 结尾，寄存器写 `r0`-`r255`，数字支持十进制和 `0x` 十六进制。

---

## 五、不变量（任何时候都不能违反）

**不变量 1：Snapshot 大小与程序大小无关**  
`VMState` 里没有 `code` 字段。`Snapshot::capture` 不含任何指令。`snapshot_does_not_contain_instructions` 测试验证了这一点：一个 1010 条指令的程序和一个 10 条指令的程序在相同步数下产生完全相同大小的快照。

**不变量 2：ProgramId 是内容哈希**  
`program.id()` 只依赖 `instructions`，不依赖 `metadata`。相同程序在任意机器上有相同 ID。`metadata_does_not_affect_id` 测试验证了这一点。

**不变量 3：restore() 必须显式传入 ProgramStore**  
`snap.restore(&store)` 会检查 `store.has(&snap.program_id)`，不在本地时返回 `SnapshotError::ProgramNotFound(id)`，调用方凭此 ID 去网络获取程序。不隐藏依赖。

**不变量 4：PatchSet 是纯数据变换**  
`patchset.apply(&snap)` 不修改 `snap`，返回新快照。`reverse()` 生成逆变换，`reverse().apply(patched) == original` 数学成立。

**不变量 5：SharedContext 的当前状态是确定性的**  
`ctx.current_snapshot()` 每次从 `base_snapshot` 重新 apply 所有 `patches`。删掉某个 patch 就等于撤销它，不需要额外机制。

---

## 六、步骤路线图

每个步骤有明确的前置条件、任务清单和成功标准。只做当前步骤，完成后再看下一个。

---

### Step 2：TCP 迁移 + 心流控制台

**前置条件**：`cargo test` 67 个全部通过。

**目标**：发送方运行程序到一半，通过 TCP 把快照传给接收方；接收方打开心流控制台，可以检查和修改任何字段，然后恢复执行。

#### 子任务 A：定义传输协议

新建 `src/protocol.rs`，实现消息帧的读写：

```rust
pub struct Message {
    pub msg_type: u8,
    pub payload: Vec<u8>,
}

// 消息类型
pub const SNAPSHOT:     u8 = 0x01;  // 负载 = Snapshot::to_bytes()
pub const PATCHSET:     u8 = 0x02;  // 负载 = PatchSet::to_bytes()（可选，跟在 SNAPSHOT 后）
pub const NEED_PROGRAM: u8 = 0x03;  // 负载 = ProgramId（32 字节）
pub const PROGRAM:      u8 = 0x04;  // 负载 = Program::to_bytes()
pub const OK:           u8 = 0x05;  // 负载为空
pub const ERROR:        u8 = 0x06;  // 负载 = UTF-8 错误消息

// 帧格式：[1字节类型][8字节长度（大端）][N字节负载]
pub fn write_msg(stream: &mut impl Write, msg_type: u8, payload: &[u8]) -> io::Result<()>;
pub fn read_msg(stream: &mut impl Read) -> io::Result<Message>;
```

#### 子任务 B：补全 `bin/aeon-recv.rs`

当前文件已有骨架，需要填充完整逻辑：

```rust
// 接收端完整流程
let snap_msg = read_msg(&mut stream)?;
let mut snap = Snapshot::from_bytes(&snap_msg.payload)?;

// 接收可选的 PatchSet
let next = read_msg(&mut stream)?;
if next.msg_type == PATCHSET {
    let ps = PatchSet::from_bytes(&next.payload)?;
    snap = ps.apply(&snap)?;   // 改动生效后再恢复
}

// 检查程序
if !store.has(&snap.program_id) {
    write_msg(&mut stream, NEED_PROGRAM, &snap.program_id)?;
    let prog_msg = read_msg(&mut stream)?;
    let prog = Program::from_bytes(&prog_msg.payload)?;
    store.add(prog);
}
write_msg(&mut stream, OK, &[])?;

// 打开心流控制台
let console = FlowConsole::new(session_id, snap);
let result = console.run(&store);

match result {
    ConsoleResult::Resume(patched) => {
        let mut state = patched.restore(&store)?;
        let prog = store.get(&state.program_id).unwrap();
        state.run(&*prog)?;
    }
    ConsoleResult::Discard(original) => { /* 用原始快照 */ }
    ConsoleResult::Quit => { /* 退出 */ }
}
```

#### 子任务 C：补全发送端 `--send` 参数

在 `src/main.rs` 的 `send` 子命令里：
1. 运行 N 步（由 `--snap-at` 指定）
2. `Snapshot::capture(&state)` 生成快照
3. 发送 `SNAPSHOT` 消息
4. 如果有 `--patch <file>`，读取并发送 `PATCHSET` 消息
5. 等待 `NEED_PROGRAM` 或 `OK`
6. 如果是 `NEED_PROGRAM`：读取 `.aeon` 文件，发送 `PROGRAM`

#### 子任务 D：新建 `BENCHMARKS.md`

```markdown
# AEON VM 性能基线
测试程序：fibonacci(10)，快照于第 5 步

| 指标 | 数值 |
|------|------|
| 快照大小 | ? bytes |
| 快照生成时间 | ? µs |
| TCP 传输时间（局域网）| ? ms |
| 恢复时间 | ? µs |
| 总中断时间 | ? ms |
```

**成功标准**：
- [ ] 两个终端完成 Fibonacci 迁移，r[2] = 55，steps 跨机器累计正确
- [ ] 接收端打开心流控制台，`set reg 0 3` 后 `resume`，程序只再跑 3 次迭代
- [ ] `BENCHMARKS.md` 填写完整

---

### Step 3：快照版本号 + 堆内存

**前置条件**：Step 2 完成。

#### 子任务 A：快照格式版本号

这是必须最先做的，因为后续所有快照结构变化都依赖版本检查：

在 `Snapshot` 结构体最前面加：

```rust
pub struct Snapshot {
    pub format_version: u32,  // 新增，当前值为 1
    // ...其余字段不变
}

impl Snapshot {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let snap: Snapshot = bincode::deserialize(data)
            .map_err(|e| format!("deserialize: {}", e))?;
        if snap.format_version != Self::CURRENT_VERSION {
            return Err(format!(
                "snapshot version {} not supported (current: {})",
                snap.format_version, Self::CURRENT_VERSION
            ));
        }
        Ok(snap)
    }
}
```

`capture()` 里加 `format_version: Self::CURRENT_VERSION`。

#### 子任务 B：堆内存字段填充

`Snapshot::heap` 和 `Snapshot::heap_top` 目前是 `Option`（`None`）。Step 3 要把 `VMState` 加上堆：

```rust
// vm.rs
pub struct VMState {
    // 原有字段不变...
    pub heap: Vec<u8>,       // 初始 1MB：vec![0u8; 1024 * 1024]
    pub heap_top: usize,     // bump allocator 指针，初始为 0
}

// snapshot.rs capture() 改为：
heap: Some(state.heap.clone()),
heap_top: Some(state.heap_top),

// snapshot.rs restore() 改为：
Ok(VMState {
    // ...原有字段...
    heap: self.heap.clone().unwrap_or_else(|| vec![0u8; 1024 * 1024]),
    heap_top: self.heap_top.unwrap_or(0),
})
```

#### 子任务 C：新增三条指令

在 `instruction.rs` 的 `Inst` 枚举末尾（Halt 之前）添加：

```rust
/// r[dst] = heap[r[addr]]（读 1 字节）
LoadMem { dst: u8, addr: u8 },
/// heap[r[addr]] = r[src] as u8（写低 8 位）
StoreMem { addr: u8, src: u8 },
/// r[dst] = heap_top; heap_top += r[size]
Alloc { dst: u8, size: u8 },
```

在 `VMError` 新增两个变体：

```rust
MemoryOutOfBounds { addr: usize, heap_len: usize },
OutOfMemory { requested: usize, available: usize },
```

在 `vm.rs` 的 `step()` match 里添加处理：

```rust
Inst::LoadMem { dst, addr } => {
    let a = self.regs[addr as usize] as usize;
    if a >= self.heap.len() {
        return StepResult::Error(VMError::MemoryOutOfBounds { addr: a, heap_len: self.heap.len() });
    }
    self.regs[dst as usize] = self.heap[a] as u64;
    self.pc += 1;
}
Inst::StoreMem { addr, src } => {
    let a = self.regs[addr as usize] as usize;
    if a >= self.heap.len() {
        return StepResult::Error(VMError::MemoryOutOfBounds { addr: a, heap_len: self.heap.len() });
    }
    self.heap[a] = self.regs[src as usize] as u8;
    self.pc += 1;
}
Inst::Alloc { dst, size } => {
    let n = self.regs[size as usize] as usize;
    let available = self.heap.len() - self.heap_top;
    if n > available {
        return StepResult::Error(VMError::OutOfMemory { requested: n, available });
    }
    self.regs[dst as usize] = self.heap_top as u64;
    self.heap_top += n;
    self.pc += 1;
}
```

在 `asm.rs` 添加解析规则（`parse_raw` 的 match 里）：

```
"loadmem" → Inst::LoadMem { dst: parse_reg(tokens[1])?, addr: parse_reg(tokens[2])? }
"storemem" → Inst::StoreMem { addr: parse_reg(tokens[1])?, src: parse_reg(tokens[2])? }
"alloc"    → Inst::Alloc { dst: parse_reg(tokens[1])?, size: parse_reg(tokens[2])? }
```

#### 子任务 D：新增测试（加入 `tests.rs`）

```rust
#[test]
fn heap_alloc_write_read() {
    let p = Program::new(vec![
        Inst::LoadImm { dst: 0, val: 10 },
        Inst::Alloc   { dst: 1, size: 0 },       // r1 = alloc(10)
        Inst::LoadImm { dst: 2, val: 42 },
        Inst::StoreMem { addr: 1, src: 2 },       // heap[r1] = 42
        Inst::LoadMem  { dst: 3, addr: 1 },       // r3 = heap[r1]
        Inst::Halt,
    ]);
    let mut state = VMState::new(&p);
    state.run(&p).unwrap();
    assert_eq!(state.regs[3], 42);
}

#[test]
fn heap_survives_snapshot() {
    let p = Program::new(vec![
        Inst::LoadImm  { dst: 0, val: 4 },
        Inst::Alloc    { dst: 1, size: 0 },
        Inst::LoadImm  { dst: 2, val: 77 },
        Inst::StoreMem { addr: 1, src: 2 },
        Inst::Halt,
    ]);
    let store = ProgramStore::new();
    store.add(p.clone());

    let mut state = VMState::new(&p);
    state.run_bounded(&p, 4);                     // 分配并写入，未到 Halt

    let snap = Snapshot::capture(&state);
    let mut state2 = snap.restore(&store).unwrap();
    state2.run(&p).unwrap();

    let addr = state2.regs[1] as usize;
    assert_eq!(state2.heap[addr], 77);
}

#[test]
fn out_of_bounds_returns_error() {
    let p = Program::new(vec![
        Inst::LoadImm { dst: 0, val: u64::MAX },
        Inst::LoadMem { dst: 1, addr: 0 },
        Inst::Halt,
    ]);
    let mut state = VMState::new(&p);
    assert!(matches!(
        state.step(&p),
        StepResult::Error(VMError::MemoryOutOfBounds { .. })
    ));
}
```

**已知限制（记入 `docs/KNOWN_LIMITATIONS.md`）**：
- Bump allocator：只分配不释放，无 GC
- 堆大小硬编码 1MB
- StoreMem/LoadMem 只操作单字节

**成功标准**：
- [ ] 原有 67 个测试仍全部通过
- [ ] 3 个新测试通过
- [ ] 越界访问返回 Error，不 panic
- [ ] 堆内容包含在快照中，跨机器迁移后正确恢复

---

### Step 4：汇编器增强 + 示例程序 + ISA 文档

**前置条件**：Step 3 完成。

#### 汇编器：支持十六进制立即数

修改 `asm.rs` 的 `parse_u64`：

```rust
fn parse_u64(s: &str, line: usize) -> Result<u64, AsmError> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).map_err(|_| AsmError {
            line, message: format!("invalid hex: '{}'", s)
        })
    } else {
        s.parse::<u64>().map_err(|_| AsmError {
            line, message: format!("expected number, got '{}'", s)
        })
    }
}
```

#### 示例程序

新建 `programs/bubble_sort.asm`：堆上分配 5 字节数组 `[5,2,8,1,9]`，冒泡排序，结果为 `[1,2,5,8,9]`。要求：可以在中途快照，恢复后继续排序，结果正确。

新建 `programs/countdown.asm`：从 10 倒数到 0，方便测试任意步骤的快照迁移场景。

#### 文档

新建 `docs/ISA.md`，覆盖全部 13 条指令（原 10 条 + Step 3 新增 3 条），格式：语法、语义伪代码、错误条件、示例。

**成功标准**：
- [ ] `bubble_sort.asm` 在第 20 步快照，恢复后结果正确
- [ ] `docs/ISA.md` 覆盖所有 13 条指令

---

### Step 5：VirtualFS（文件系统虚拟化）

**前置条件**：Step 4 完成。

#### 设计

VirtualFS 完全在内存中，是 `VMState` 的一个字段，快照时自动包含。

新建 `src/vfs.rs`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VirtualFS {
    files: HashMap<String, Vec<u8>>,      // 路径 → 内容
    open_fds: Vec<Option<OpenFile>>,       // fd 索引
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFile {
    pub path: String,
    pub cursor: usize,
    pub writable: bool,
}

impl VirtualFS {
    pub fn open(&mut self, path: &str, writable: bool) -> Result<usize, FsError>;
    pub fn read(&mut self, fd: usize, buf: &mut [u8]) -> Result<usize, FsError>;
    pub fn write(&mut self, fd: usize, data: &[u8]) -> Result<usize, FsError>;
    pub fn close(&mut self, fd: usize) -> Result<(), FsError>;
}
```

#### 系统调用指令

在 `Inst` 枚举添加（Step 4 新建的 ISA 文档里同步补充）：

```rust
Syscall { num: u8 }  // 调用号在指令字段，参数通过寄存器传递
```

调用约定（num 值与寄存器用途）：

| num | 名称 | 参数寄存器 | 返回（r0） |
|-----|------|----------|----------|
| 0 | SYS_OPEN | r1=path_addr, r2=path_len, r3=mode(0=读,1=写) | fd |
| 1 | SYS_READ | r1=fd, r2=buf_addr, r3=count | bytes_read |
| 2 | SYS_WRITE | r1=fd, r2=buf_addr, r3=count | bytes_written |
| 3 | SYS_CLOSE | r1=fd | 0 |

在 `VMState` 加入 `pub vfs: VirtualFS`，在 `VMError` 加入 `Fs(FsError)` 和 `UnknownSyscall(u8)` 和 `InvalidUtf8`。

`vm.rs` 的 `step()` 处理 `Syscall`：

```rust
Inst::Syscall { num } => {
    self.handle_syscall(*num).map_err(|e| return StepResult::Error(e))?;
    self.pc += 1;
}
```

#### 测试

- `vfs_write_read_roundtrip`：写入再读出，内容一致
- `vfs_survives_snapshot`：写入 → 快照 → 恢复 → 读取，内容一致

**成功标准**：
- [ ] 两个 VFS 测试通过
- [ ] 迁移时 VFS 文件内容随快照传输

---

### Step 6：语言支持（Scheme 或 Forth）

**前置条件**：Step 5 完成。

**选择策略**：两种语言各用两周时间做最小原型（只需支持整数运算、变量绑定、函数定义、条件、循环），哪个先能在 AEON VM 上运行递归的 `fib(10)` 就选哪个。

**Forth 核心约 150-200 行**：
- 数据栈：用堆上一段内存实现，`r200` 指向栈顶
- 词典：存入 VirtualFS 文件
- 每个 Forth 词翻译为 AEON 字节码序列

**Scheme 核心约 400 行**：
- S 表达式解析：堆上的 cons 链表（car/cdr）
- eval 是递归函数，用 `Call`/`Ret` 实现
- 先用 bump allocator，不做 GC

选定后，决策理由写入 `docs/LANGUAGE_CHOICE.md`，Step 7-9 的具体任务由此文档展开。

**成功标准**：
- [ ] 选定语言，`fib(10)` 在该语言中运行正确
- [ ] 在中途快照，恢复后继续，结果仍正确
- [ ] `docs/LANGUAGE_CHOICE.md` 完成

---

### Step 7-9：语言完整实现

由 Step 6 的 `docs/LANGUAGE_CHOICE.md` 决定具体内容。通用目标：

- Step 7：基础类型 + 算术 + 变量绑定
- Step 8：函数定义与调用
- Step 9：标准库 + VirtualFS 集成（能读文件并执行）

**三步完成后的成功标准**：
- [ ] 至少 10 个测试程序运行正确（Fibonacci、阶乘、排序等）
- [ ] 解释器自身的调用帧、变量环境完整包含在 VMState 快照中
- [ ] 跨机器迁移后，语言层的执行状态正确恢复

---

### Step 10：性能基线 + JIT（两个子步骤）

**前置条件**：Step 9 完成。

#### Step 10a：基准测试基线

在做任何优化之前建立基线。

```toml
# Cargo.toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "vm_benchmarks"
harness = false
```

新建 `benches/vm_benchmarks.rs`：

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use aeon_vm::program::programs;
use aeon_vm::snapshot::Snapshot;
use aeon_vm::store::ProgramStore;
use aeon_vm::vm::VMState;

fn fib_bench(c: &mut Criterion) {
    let program = programs::fibonacci(20);  // 返回 Program，直接使用
    c.bench_function("fib(20) interpreter", |b| {
        b.iter(|| {
            let mut state = VMState::new(black_box(&program));
            state.run(&program).unwrap()
        });
    });
}

fn snapshot_bench(c: &mut Criterion) {
    let program = programs::fibonacci(10);
    let mut state = VMState::new(&program);
    state.run_bounded(&program, 20);         // run_bounded，不是 run

    c.bench_function("snapshot capture", |b| {
        b.iter(|| Snapshot::capture(black_box(&state)));
    });
}

fn restore_bench(c: &mut Criterion) {
    let program = programs::fibonacci(10);
    let store = ProgramStore::new();
    store.add(program.clone());
    let mut state = VMState::new(&program);
    state.run_bounded(&program, 20);
    let snap = Snapshot::capture(&state);
    let bytes = snap.to_bytes();

    c.bench_function("snapshot restore", |b| {
        b.iter(|| {
            let s = Snapshot::from_bytes(black_box(&bytes)).unwrap();
            s.restore(&store).unwrap()
        });
    });
}

criterion_group!(benches, fib_bench, snapshot_bench, restore_bench);
criterion_main!(benches);
```

**成功标准**：`cargo bench` 运行成功，`BENCHMARKS.md` Before 列填写完整。

#### Step 10b：JIT（Cranelift）

```toml
[dependencies]
cranelift         = "0.109"
cranelift-jit     = "0.109"
cranelift-frontend = "0.109"
```

**实现策略**：
1. 新增 `src/jit.rs`，包含 `JITCache: HashMap<usize, CompiledFn>`
2. `VMState` 加执行计数器 `hot_counts: HashMap<usize, u32>`（不进快照）
3. 某函数执行次数超过 1000 次时，触发 JIT，翻译为 Cranelift IR 并编译
4. 后续调用直接执行机器码，`regs` 和 `pc` 通过 mutable raw pointer 传入
5. JIT 缓存不进入 `Snapshot`，目标机器收到快照后重新 JIT

**核心约束**：JIT 之后所有原有测试仍须通过（`cargo test`）。

**成功标准**：
- [ ] `cargo bench` 显示 fib(40) 至少快 5x
- [ ] 所有 67 个原有测试仍通过
- [ ] `BENCHMARKS.md` After 列填写完整

---

### Step 11：事件日志

**前置条件**：Step 10 完成。

新建 `src/eventlog.rs`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AeonEvent {
    VMStarted   { program_id: ProgramId, timestamp_ms: u64, device_id: [u8; 16] },
    Checkpoint  { snapshot_hash: [u8; 32], steps: u64 },
    VMMigrated  { from_device: [u8; 16], to_device: [u8; 16] },
    PatchApplied { author: String, description: String },
    VMCompleted { final_regs: Box<[u64; 256]> },
}

pub struct EventLog {
    entries: Vec<LogEntry>,
}

pub struct LogEntry {
    pub event: AeonEvent,
    pub timestamp_ms: u64,
    pub prev_hash: [u8; 32],  // Blake3(prev_entry) 形成哈希链
    pub self_hash: [u8; 32],
}
```

每次 `Snapshot::capture` 自动 append `AeonEvent::Checkpoint`，每次迁移 append `VMMigrated`，每次在心流控制台提交补丁 append `PatchApplied`。

**成功标准**：
- [ ] `aeon log` 命令列出所有事件
- [ ] 事件链完整性可验证（`prev_hash` 链正确）

---

### Step 12：P2P 同步（CRDT GSet）

**前置条件**：Step 11 完成。

**技术选择**：GSet（只增长的集合），合并取并集，数学保证无冲突。不用 Raft/Paxos。

```rust
pub struct EventGSet {
    known: HashSet<[u8; 32]>,  // 已知事件的 self_hash
}

impl EventGSet {
    pub fn merge(&mut self, other: &EventGSet) {
        self.known.extend(other.known.iter());
    }
}
```

**心跳协议**（每 5 秒）：

```
A → B: { device_id, latest_hash, bloom_filter(known_hashes) }
B → A: { events_A_is_missing }
A → B: { events_B_is_missing }
```

**成功标准**：
- [ ] 两台设备离线 1 小时，重连后 30 秒内事件集完全相同

---

### Step 13：COW 增量快照

**前置条件**：Step 12 完成。

把 `VMState::heap: Vec<u8>` 替换为 COW 页追踪：

```rust
pub struct COWHeap {
    pages: Vec<[u8; 4096]>,
    dirty: BTreeSet<usize>,          // 脏页页号
}

impl COWHeap {
    pub fn read(&self, addr: usize) -> u8 { self.pages[addr/4096][addr%4096] }
    pub fn write(&mut self, addr: usize, val: u8) {
        self.dirty.insert(addr / 4096);
        self.pages[addr/4096][addr%4096] = val;
    }
    pub fn incremental_snapshot(&self) -> IncrementalSnapshot {
        IncrementalSnapshot {
            dirty_pages: self.dirty.iter()
                .map(|&i| (i, self.pages[i]))
                .collect(),
        }
    }
    pub fn clear_dirty(&mut self) { self.dirty.clear(); }
}
```

迁移时只传脏页，接收方 apply 到已有的 base 上。

**成功标准**：
- [ ] 只修改 10KB 的 1MB 堆程序，传输只需 ~10KB
- [ ] `BENCHMARKS.md` 增量快照对比数据填写完整

---

### Step 14：Daemon + CLI 工具

**前置条件**：Step 13 完成。

新建 `aeon-daemon`（后台进程，通过 Unix domain socket 管理 VM 实例）和 `aeon` CLI（命令行客户端）：

```bash
aeon daemon                              # 启动后台服务
aeon ps                                  # 列出运行中的 VM
aeon run <file.aeon>                     # 启动 VM
aeon pause <id>                          # 暂停并快照
aeon resume <id>                         # 从最近快照恢复
aeon migrate <id> --to <host:port>       # 迁移到另一台机器
aeon log <id>                            # 事件日志
aeon devices                             # 已知 AEON 节点
aeon share <id>                          # 导出 SharedContext 给其他会话加入
```

协议：daemon 和 CLI 之间用 JSON over Unix socket（可读性优先）。

**成功标准**：
- [ ] `aeon migrate` 完成跨机器迁移，接收方进入心流控制台
- [ ] daemon 崩溃后，VM 从最近快照自动恢复

---

### Step 15：端到端测试与文档

**前置条件**：Step 14 完成。

#### 验收场景（需录制视频）

1. **迁移 + 编辑**：笔记本运行 Scheme 程序，`aeon migrate` 到台式机，台式机进入心流控制台，修改一个寄存器，`resume`，程序继续执行，结果正确。

2. **两账户协作**：Alice 的控制台 `share ctx-1`，Bob 的控制台 `join ctx-1`，Bob 在控制台里看到 Alice 的补丁和消息，Bob 叠加自己的修改，`resume`，执行结果包含两人的改动。

3. **断电恢复**：`kill -9 aeon-daemon`，重启 daemon，VM 从最近检查点自动恢复，事件日志连续。

#### 必须完成的文档

- `README.md`：10 分钟快速上手（安装 → 第一个程序 → 第一次迁移 → 第一次协作）
- `docs/ISA.md`：Step 4 完成，Step 15 审查更新
- `docs/ARCHITECTURE.md`：三个核心设计决策 + 为什么不用 WASM
- `docs/KNOWN_LIMITATIONS.md`：见下一节
- `docs/LANGUAGE_CHOICE.md`：Step 6 完成

**成功标准**：
- [ ] 三个场景视频录制完成
- [ ] 外部开发者能在 30 分钟内完成第一次跨机器迁移
- [ ] `KNOWN_LIMITATIONS.md` 至少 10 条

---

## 七、已知限制

维护于 `docs/KNOWN_LIMITATIONS.md`，每个步骤完成后更新。

| 限制 | 原因 | 解决计划 | 对应步骤 |
|------|------|---------|---------|
| TCP 连接迁移时断开 | TCP 状态在 OS 内核 | 应用层重连（优雅降级） | 可选 |
| GPU 迁移需要几分钟 | PCIe 带宽 + GPU 读回慢 | 显式暂停，非透明 | 可选 |
| 无 GC，堆只增不减 | Bump allocator 设计选择 | 不做（明确决定） | — |
| JIT 比 native 慢 ~2x | 解释器框架开销 | 目标 50% native | Step 10b |
| 不支持多线程 | 线程状态无法序列化 | 不做（设计决定） | — |
| VFS 不持久化磁盘 | 完全在内存 | Step 14 daemon 实现 | Step 14 |
| 堆大小硬编码 1MB | 常量，未参数化 | Step 13 COW 后可配置 | Step 13 |
| StoreMem/LoadMem 只操作 1 字节 | 简化设计 | 添加 StoreMem64 | Step 4 |
| 快照格式无版本号 | Step 1 遗漏 | Step 3 修复 | Step 3 |

---

## 八、技术债务

| 债务 | 引入于 | 优先级 | 解决于 |
|------|-------|--------|-------|
| 快照格式无版本号 | Step 1 | 高 | Step 3 |
| `aeon-recv.rs` TCP 协议未完成 | Step 1 | 高 | Step 2 |
| 堆内存无边界检查（runtime panic） | Step 1 | 高 | Step 3 |
| `run_bounded` 错误 panic 而非返回 | Step 1 | 中 | Step 3 |
| 汇编器遇第一个错误即停止 | Step 1 | 低 | Step 4 |
| console.rs 的 `join` 只写入文件，未实时同步 | Step 1 | 中 | Step 12 |

---

## 九、开发规范

**提交格式**：
```
Step N: <一句描述>

- 改动 1
- 改动 2

Tests: <新增/修改测试>
Benchmarks: <如有性能数据>
Debt: <新增债务>
```

**强制规则**：
- 每次提交前：`cargo test` 全部通过（当前 67 个）
- 每个新功能：对应测试写在 `tests.rs` 或新的 `tests_*.rs` 文件
- 生产路径：禁止 `.unwrap()`，用 `?` 传播或 `map_err` 转换
- 性能改动：`BENCHMARKS.md` 必须有 Before/After 数字
- 新限制：先加 `KNOWN_LIMITATIONS.md`，再决定是否解决

**Rust 版本**：edition 2021，MSRV 1.75.0

---

## 十、参考资源

- BEAM 进程迁移：https://www.erlang.org/doc/
- Cranelift JIT：https://cranelift.dev/
- IPFS 内容寻址：https://docs.ipfs.tech/concepts/content-addressing/
- RustPython：https://github.com/RustPython/RustPython
- Lamport 时钟论文（1978）：理解 Step 11-12 的逻辑时钟
- CRDTs（Shapiro 2011）：理解 Step 12 的无冲突合并

---

## 十一、第一步（唯一的起点）

```bash
# 验证基础
cargo test
# 必须全部通过（67 个）

# 然后开始 Step 2
# 终端 1（接收方）
cargo run --bin aeon-console --session alice@laptop/conv-1

# 终端 2（发送方）
cargo run --bin aeon-asm programs/fibonacci.asm
cargo run --bin aeon-run fibonacci.aeon --snap-at 5 --send 127.0.0.1:9999
```

接收方出现心流控制台后，试验：

```
aeon> info
aeon> regs 0 4
aeon> set reg 0 3
aeon> diff
aeon> resume
```

Step 2 完成的标志：fibonacci 计算在中途迁移，接收方在控制台里修改 r0=3 后继续执行，总步数跨机器累计，最终结果正确，`BENCHMARKS.md` 有数据。

---

*文档与代码不一致时，以代码为准，立即更新文档。*
