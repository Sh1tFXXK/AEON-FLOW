# Language Choice

## Decision

Step 6 selects Forth for the prototype path.

The prototype reached the required target with `10 fib .` producing `55`. It maps cleanly onto the current migration model: the data stack is stored in VM heap memory, `r200` stores the next stack cell address, and the dictionary plus resumable interpreter runtime are stored in `VirtualFS`. Existing `Snapshot` capture/restore therefore preserves language execution state without adding another snapshot field.

## Implemented Shape

- Data stack: 8-byte `u64` cells in heap starting at `forth::STACK_BASE`; `r200` points to the next free cell.
- Dictionary: serialized to `/forth/dict` in `VirtualFS`; `:` and `;` compile user words into that dictionary.
- Runtime state: serialized to `/forth/runtime` in `VirtualFS`; it includes token stream, instruction pointer, call frames, loop frames, variable environments, output, and halt state.
- Supported words: integer literals, `+`, `-`, `*`, `=`, `<`, `>`, `dup`, `drop`, `swap`, `over`, `depth`, `.`, `var`, `set`, `get`, `if`, `then`, `do`, `loop`, `i`, `:`, `;`, `include`.
- VFS entry points: `include <path>` and `ForthPrototype::start_file(vm, path)` read source files from `VirtualFS` and execute them.
- Validation: Forth tests cover direct fib(10), comments, comparisons, local variable frames, VFS include/start-file, stack/dictionary inspection, and snapshot restore.

## Limitations

- Integers are `u64` only; there is no signed arithmetic, comparison vocabulary, strings, or floats.
- `if` supports `if ... then` only; `else` is intentionally not implemented.
- `do ... loop` supports positive counted loops only.
- Variables are lexical runtime bindings managed by `var`/`set`/`get`, not standard Forth address variables.
- Source tokenization is whitespace-only after comment stripping.
- `include` paths cannot contain whitespace.

## Step 7-9 Tasks

- Step 7: complete. Grammar is documented in `docs/FORTH.md`; comments, arithmetic, comparisons, and variable binding are tested.
- Step 8: complete. Function calls use serialized call frames and local variable environments; stack/dictionary inspection APIs are available.
- Step 9: complete. `include` and `start_file` read source from `VirtualFS`; snapshot restore preserves active Forth runtime state.
