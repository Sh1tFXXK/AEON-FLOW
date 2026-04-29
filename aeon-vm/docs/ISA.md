# AEON VM ISA

This document covers the 13 assembler-supported instructions. Registers are `r0` through `r255`. Immediates may be decimal (`42`) or hexadecimal (`0x2a`). Label jumps are resolved by the assembler.

## `load`

Syntax: `load r<dst>, <imm>`
Semantics: `regs[dst] = imm`
Errors: invalid register or immediate is rejected by the assembler.
Example: `load r0, 0xFF`

## `mov`

Syntax: `mov r<dst>, r<src>`
Semantics: `regs[dst] = regs[src]`
Errors: invalid registers are rejected by the assembler.
Example: `mov r1, r0`

## `add`

Syntax: `add r<dst>, r<a>, r<b>`
Semantics: `regs[dst] = regs[a] + regs[b]` with u64 wrapping.
Errors: invalid registers are rejected by the assembler.
Example: `add r3, r1, r2`

## `sub`

Syntax: `sub r<dst>, r<a>, r<b>`
Semantics: `regs[dst] = regs[a] - regs[b]` with u64 wrapping.
Errors: invalid registers are rejected by the assembler.
Example: `sub r0, r0, r4`

## `mul`

Syntax: `mul r<dst>, r<a>, r<b>`
Semantics: `regs[dst] = regs[a] * regs[b]` with u64 wrapping.
Errors: invalid registers are rejected by the assembler.
Example: `mul r1, r1, r0`

## `jz`

Syntax: `jz r<cond>, <label>`
Semantics: if `regs[cond] == 0`, `pc = pc + offset`; otherwise `pc = pc + 1`. The offset is a signed i16 relative to the current PC; the assembler computes it from the label. The Rust enum currently stores this field as `isize`.
Errors: invalid register or undefined label is rejected by the assembler.
Example: `jz r0, done`

## `jmp`

Syntax: `jmp <label>`
Semantics: `pc = pc + offset`. The offset is a signed i16 relative to the current PC, not an absolute address; the assembler computes it from the label. The Rust enum currently stores this field as `isize`.
Errors: undefined label is rejected by the assembler.
Example: `jmp loop`

## `call`

Syntax: `call <label>`
Semantics: `call_stack.push(pc + 1); pc = label_address`
Errors: undefined label is rejected by the assembler.
Example: `call compare_swap`

## `ret`

Syntax: `ret`
Semantics: `pc = call_stack.pop()`
Errors: returns `VMError::EmptyCallStack` if the call stack is empty.
Example: `ret`

## `alloc`

Syntax: `alloc r<dst>, r<size>`
Semantics: `regs[dst] = heap_top; heap_top += regs[size]`
Errors: returns `VMError::OutOfMemory` if the allocation exceeds the fixed heap.
Example: `alloc r1, r0`

## `loadmem`

Syntax: `loadmem r<dst>, r<addr>`
Semantics: `regs[dst] = heap[regs[addr]]`
Errors: returns `VMError::MemoryOutOfBounds` if `regs[addr]` is outside the heap.
Example: `loadmem r3, r1`

## `storemem`

Syntax: `storemem r<addr>, r<src>`
Semantics: `heap[regs[addr]] = regs[src] as u8`
Errors: returns `VMError::MemoryOutOfBounds` if `regs[addr]` is outside the heap.
Example: `storemem r1, r2`

## `halt`

Syntax: `halt`
Semantics: stop execution and return `StepResult::Halted`.
Errors: none.
Example: `halt`
