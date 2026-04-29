# Known Limitations

- Heap allocation is a bump allocator with no free operation.
- Heap size is hardcoded to 1MB.
- `StoreMem` writes one byte only.
