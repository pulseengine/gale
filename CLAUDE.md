# CLAUDE.md

See [AGENTS.md](AGENTS.md) for project instructions.

## Formal Verification

Follow the [PulseEngine Formal Verification Guide](https://pulseengine.eu/guides/VERIFICATION-GUIDE.md).

Key: code must satisfy all tracks simultaneously (Verus + Rocq + Kani).
Write to the intersection — no trait objects, closures, or async in verified code.

## Additional Settings
- Use `rivet validate` to verify changes to artifact YAML files
- Use `rivet list --format json` for machine-readable artifact queries
