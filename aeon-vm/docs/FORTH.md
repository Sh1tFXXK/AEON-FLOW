# AEON Forth

AEON Forth is the selected language path for Steps 7-9. It is a compact stack language whose complete interpreter state is stored inside `VMState`: the data stack lives in heap memory, `r200` points to the next stack cell, and the dictionary/runtime are serialized into `VirtualFS`.

## Grammar

Source is whitespace-tokenized after comments are removed.

```text
program     = item*
item        = definition | include | token
definition  = ":" name token* ";"
include     = "include" path
token       = integer | word | variable-name | path
comment     = "\" until newline | "(" until ")"
integer     = unsigned decimal u64
```

`include <path>` is compile-time: AEON Forth reads the file from `VirtualFS`, compiles its definitions, and inserts its top-level tokens at the include point. `ForthPrototype::start_file(vm, path)` starts execution from a Forth source file stored in `VirtualFS`.

## Core Words

- Arithmetic: `+`, `-`, `*`.
- Comparisons: `=`, `<`, `>` return `1` for true and `0` for false.
- Stack: `dup`, `drop`, `swap`, `over`, `depth`, `.`.
- Variables: `var <name>`, `set <name>`, `get <name>`.
- Control flow: `if ... then`, `do ... loop`, `i`.
- Definitions: `: <name> ... ;`.
- VFS loading: `include <path>`.

## Snapshot State

The following language-layer state is captured by normal `Snapshot::capture` because it is contained in `VMState`:

- Data stack bytes in `VMState.heap`.
- Stack pointer in `VMState.regs[200]`.
- Dictionary in `/forth/dict` inside `VMState.vfs`.
- Runtime in `/forth/runtime` inside `VMState.vfs`, including token stream, instruction pointer, call frames, loop frames, variable environments, output, and halt state.

Restoring a VM snapshot therefore restores active Forth function calls, local variables, loops, and pending output without a separate language snapshot format.
