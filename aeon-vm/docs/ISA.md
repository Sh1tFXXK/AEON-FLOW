# AEON VM ISA

Registers are `r0` through `r255`. Immediates may be decimal (`42`) or hexadecimal (`0x2a`). Assembler labels are resolved to PC-relative offsets for jumps.

## `load`

**Syntax**: `load r<dst>, <imm>`  
**Semantics**: `regs[dst] = imm`  
**Errors**: Invalid register or immediate is rejected by the assembler.  
**Example**: `load r0, 0xFF`

## `mov`

**Syntax**: `mov r<dst>, r<src>`  
**Semantics**: `regs[dst] = regs[src]`  
**Errors**: Invalid register is rejected by the assembler.  
**Example**: `mov r1, r0`

## `add`

**Syntax**: `add r<dst>, r<a>, r<b>`  
**Semantics**: `regs[dst] = regs[a] + regs[b]` with `u64` wrapping.  
**Errors**: Invalid register is rejected by the assembler.  
**Example**: `add r3, r1, r2`

## `sub`

**Syntax**: `sub r<dst>, r<a>, r<b>`  
**Semantics**: `regs[dst] = regs[a] - regs[b]` with `u64` wrapping.  
**Errors**: Invalid register is rejected by the assembler.  
**Example**: `sub r0, r0, r4`

## `mul`

**Syntax**: `mul r<dst>, r<a>, r<b>`  
**Semantics**: `regs[dst] = regs[a] * regs[b]` with `u64` wrapping.  
**Errors**: Invalid register is rejected by the assembler.  
**Example**: `mul r1, r1, r0`

## `jz`

**Syntax**: `jz r<cond>, <label>`  
**Semantics**: If `regs[cond] == 0`, `pc = pc + offset`; otherwise `pc = pc + 1`. `offset` is a signed PC-relative jump offset computed by the assembler, not an absolute address.  
**Errors**: Invalid register or undefined label is rejected by the assembler.  
**Example**: `jz r0, done`

## `jmp`

**Syntax**: `jmp <label>`  
**Semantics**: `pc = pc + offset`. `offset` is a signed PC-relative jump offset computed by the assembler, not an absolute address.  
**Errors**: Undefined label is rejected by the assembler.  
**Example**: `jmp loop`

## `call`

**Syntax**: `call <label>`  
**Semantics**: `call_stack.push(pc + 1); pc = label_address`  
**Errors**: Undefined label is rejected by the assembler.  
**Example**: `call compare_swap`

## `ret`

**Syntax**: `ret`  
**Semantics**: `pc = call_stack.pop()`  
**Errors**: `VMError::EmptyCallStack` if the call stack is empty.  
**Example**: `ret`

## `print`

**Syntax**: `print r<src>`  
**Semantics**: Print `regs[src]` as a decimal value.  
**Errors**: Invalid register is rejected by the assembler.  
**Example**: `print r6`

## `alloc`

**Syntax**: `alloc r<dst>, r<size>`  
**Semantics**: `regs[dst] = heap_top; heap_top += regs[size]`  
**Errors**: `VMError::OutOfMemory` if the allocation exceeds the fixed heap.  
**Example**: `alloc r1, r0`

## `loadmem`

**Syntax**: `loadmem r<dst>, r<addr>`  
**Semantics**: `regs[dst] = heap[regs[addr]]`  
**Errors**: `VMError::MemoryOutOfBounds` if `regs[addr]` is outside the heap.  
**Example**: `loadmem r3, r1`

## `storemem`

**Syntax**: `storemem r<addr>, r<src>`  
**Semantics**: `heap[regs[addr]] = regs[src] as u8`  
**Errors**: `VMError::MemoryOutOfBounds` if `regs[addr]` is outside the heap.  
**Example**: `storemem r1, r2`

## `syscall`

**Syntax**: `syscall <num>`  
**Semantics**: Execute a VM syscall. The syscall number is encoded in the instruction and arguments are passed through registers.  
**Calling convention**:

| num | Name | r1 | r2 | r3 | r0 return |
|-----|------|----|----|----|-----------|
| 0 | `SYS_OPEN` | `path_addr` | `path_len` | `mode` (`0` read, `1` write) | `fd` |
| 1 | `SYS_READ` | `fd` | `buf_addr` | `count` | `bytes_read` |
| 2 | `SYS_WRITE` | `fd` | `buf_addr` | `count` | `bytes_written` |
| 3 | `SYS_CLOSE` | `fd` | - | - | `0` |

**Errors**: `VMError::UnknownSyscall(num)`, `VMError::InvalidUtf8`, `VMError::Fs(error)`, or heap bounds errors.  
**Example**: `syscall 2`

## `halt`

**Syntax**: `halt`  
**Semantics**: Stop execution and return `StepResult::Halted`.  
**Errors**: None.  
**Example**: `halt`
