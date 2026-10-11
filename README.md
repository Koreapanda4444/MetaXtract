# MetaXtract

MetaXtract is a local-first metadata extraction and forensic analysis application. The production
architecture is being migrated to a Rust application with a narrow C++ extraction core.

## Repository layout

- `crates/`: Rust-owned product logic, including GUI, CLI, scanning, storage, and analysis.
- `native/`: C++ binary parsing, decoding, raw extraction, and native library adapters.
- `legacy/python/`: frozen Python reference used until behavioral parity is demonstrated.
- `reference/`: stable compatibility contracts captured from the Python implementation.

The Python reference remains runnable during the migration:

```bash
python -m pip install -e ./legacy/python
metaxtract --help
```

No production feature may introduce a runtime dependency on an externally installed metadata
executable. Native libraries are linked or shipped with MetaXtract instead.
