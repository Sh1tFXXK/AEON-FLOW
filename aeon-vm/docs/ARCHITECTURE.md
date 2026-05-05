# AEON Flow 架构设计

## 三个核心决策

### 决策 1：VMState 不含代码
快照大小与程序大小无关。Program 和 VMState 严格分离，
ProgramId 是 Blake3(instructions)，迁移时先传快照 (~2KB)，
接收方按需请求程序文件。

### 决策 2：PatchSet 是纯数据变换
patchset.apply(&snap) 返回新快照，不修改原快照。
reverse() 生成逆变换，数学保证可撤销。
PatchSet 可序列化，随快照传输，存档审计。

### 决策 3：SharedContext 的当前状态是确定性的
current_snapshot() = base.apply(patch[0]).apply(patch[1])...
删掉某个 patch = 撤销它，不需要额外 undo 机制。
Lamport 时钟保证跨设备补丁顺序一致。

## 为什么不用 WASM
WASM 运行时不暴露调用栈和程序计数器，无法实现透明的
mid-execution 快照。自研 VM 让所有状态显式可见，
snapshot = bincode::serialize(vm_state)，恢复是确定性的。

## 数据流

发送方：run N steps → capture snapshot → send(SNAPSHOT + optional PATCHSET)
接收方：recv → apply patches → restore VMState → FlowConsole → resume
