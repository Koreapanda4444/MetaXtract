# MetaXtract

MetaXtract is a local-first metadata extraction and forensic analysis application. The production
architecture is being migrated to a Rust application with a narrow C++ extraction core.

## Repository layout

- `crates/`: Rust-owned product logic, including GUI, CLI, scanning, storage, and analysis.
- `native/`: C++ binary parsing, decoding, raw extraction, and native library adapters.
- `legacy/python/`: frozen Python reference used until behavioral parity is demonstrated.
- `reference/`: stable compatibility contracts captured from the Python implementation.
- `conformance/`: migration fixtures and Python/Rust/C++ compatibility checks.

The Python reference remains runnable during the migration:

```bash
python -m pip install -e ./legacy/python
metaxtract --help
```

No production feature may introduce a runtime dependency on an externally installed metadata
executable. Native libraries are linked or shipped with MetaXtract instead.

The native migration foundation is buildable with Rust 1.88, CMake 3.24 or newer, and a C++20
compiler. Run `cargo build --workspace --locked`, then `python conformance/run.py` to check the
language boundary. Native format adapters and the Rust desktop application are later migration
steps; the Python GUI remains the usable application during this stage.
