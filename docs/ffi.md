# Rust and C++ boundary

[English](ffi.md) | [한국어](ffi_KR.md) | [Project README](../README.md)

The bridge is intentionally value-oriented. Rust owns requests, product state, and every result
after conversion into the canonical schema. C++ owns parser-local allocations only for the
duration of a call.

- No parser or third-party library object crosses the boundary.
- No borrowed pointer is retained after a call.
- C++ entry points are `noexcept` and return structured status values.
- Rust checks native API and schema revisions before extraction.
- Native error codes are converted to Rust errors before they reach product logic.
- Raw byte payloads returned by future parsers transfer into Rust-owned vectors.

`cxx` generates the ABI glue and compile-time signature checks. It does not make parser
implementations memory-safe, so native code remains subject to sanitizers, resource budgets, and
the parser isolation planned for the scanner.

Cargo builds use the same MSVC runtime mode for the CMake core and generated bridge, with
iterator debugging disabled at that boundary. Rust's debug profile still uses the release CRT;
selecting a debug CRT only for native code changes the C++ ABI and cannot be mixed safely.
Standalone CMake builds keep their own internally consistent runtime defaults.

See [architecture](architecture.md) for responsibility boundaries and
[resource limits](resource-limits.md) for the limits of budget and deadline enforcement.
