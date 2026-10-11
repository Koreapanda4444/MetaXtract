# Conformance corpus

This directory replaces a generic root test tree for migration-critical verification. The corpus
contains deterministic byte payloads and stable output projections. It never contains private
evidence or files copied from users.

Run the frozen Python oracle from the repository root:

```bash
python conformance/run.py
```

Volatile filesystem fields are removed before comparison. The corpus establishes compatibility
for existing behavior; deeper fields introduced by the native engine require format-spec or
multi-parser evidence and are not constrained to Python's shallow output.
