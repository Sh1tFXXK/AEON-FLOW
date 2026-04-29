# Language Choice

## Decision

Step 6 selects Forth for the prototype path.

The prototype reached the required target with `10 fib .` producing `55`. It maps cleanly onto the current migration model: the data stack is stored in VM heap memory, `r200` stores the next stack cell address, and the dictionary plus resumable interpreter runtime are stored in `VirtualFS`. Existing `Snapshot` capture/restore therefore preserves language execution state without adding another snapshot field.

## Prototype Shape

- Data stack: 8-byte `u64` cells in heap starting at `forth::STACK_BASE`; `r200` points to the next free cell.
- Dictionary: serialized to `/forth/dict` in `VirtualFS`; `:` and `;` compile user words into that dictionary.
- Runtime state: serialized to `/forth/runtime` in `VirtualFS`; it includes token stream, instruction pointer, call frames, loop frames, variables, output, and halt state.
- Supported words: integer literals, `+`, `-`, `*`, `dup`, `drop`, `.`, `var`, `set`, `get`, `if`, `then`, `do`, `loop`, `i`, `:`, `;`.
- Validation: `forth_fib10_outputs_55` checks the direct run path; `forth_state_survives_snapshot` checks pause, snapshot, restore, and resume.

## Limitations

- Integers are `u64` only; there is no signed arithmetic, comparison vocabulary, strings, or floats.
- `if` supports `if ... then` only; `else` is intentionally not implemented.
- `do ... loop` supports positive counted loops only.
- Variables are global bindings managed by `var`/`set`/`get`, not standard Forth address variables.
- Source tokenization is whitespace-only and has no comment syntax.
- The interpreter is a prototype library plus `aeon-forth` runner, not a stable language frontend.

## Step 7-9 Tasks

- Step 7: turn the Forth prototype into a first-class language frontend with a documented grammar, stable error messages, comment support, and sample programs beyond Fibonacci.
- Step 8: expand the standard vocabulary around comparisons, stack inspection, memory/VFS access, and a console command that can inspect Forth stack, dictionary, and runtime state.
- Step 9: make Forth migration a product demo: run a long Forth program, snapshot mid-execution over TCP, restore on the receiver, resume, and verify output plus stack/dictionary state.
