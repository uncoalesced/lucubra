# ADR 0001: Wheel layout for the binary and the Python workers

- Status: Accepted
- Date: 2026-11-13
- Decision references: D10, SPEC section 4.3

## Context

Lucubra ships a Rust binary and a Python worker package in one wheel, started with `uvx lucubra`. SPEC section 4.3 left two maturin layouts open:

1. `bindings = "bin"` with a `python-source` directory holding the `lucubra_workers` package.
2. The core exposed as a PyO3 extension module and launched with `python -m lucubra`.

## Evidence

Both layouts were built with maturin on Windows x64 (rustc 1.97.1, uv 0.11.31, Python 3.11.15) and run through `uvx --from <wheel>`.

| Measure | Layout 1 (`bin`) | Layout 2 (PyO3, abi3) |
|---|---|---|
| Spike wheel size | 121,374 bytes | 91,805 bytes |
| Wheel contents | `*.data/scripts/<name>.exe` plus the pure Python package | Extension module plus a console script shim |
| Startup through `uvx`, five runs | 246 to 402 ms | 398 to 567 ms |
| Worker started from the binary | Yes. The binary found `python.exe` beside itself in the environment's `Scripts` folder and ran `python -m lucubra_workers` | Not applicable; the engine runs inside the interpreter |

Layout 1 needed no build configuration beyond `bindings`, `python-source` and `module-name`. Layout 2 required an explicit `module-name` and a `cdylib` crate type, and it places the Rust engine inside the interpreter's process.

The repository wheel built for gate G3 uses layout 1: 82,258 bytes, containing `lucubra.exe`, `lucubra_workers`, `LICENSE` and `NOTICE`. `uvx --from <wheel> lucubra --version` printed `lucubra 0.0.1`, and a ping sent to `python -m lucubra_workers` in the same environment returned `{"id": "r1", "ok": true, "worker": "0.0.1"}`.

## Decision

Use layout 1: `bindings = "bin"`, `manifest-path = "crates/lucubra-cli/Cargo.toml"`, `python-source = "python"`, `module-name = "lucubra_workers"`. The wheel is tagged `py3-none-<platform>`, so one wheel per platform serves every supported Python version.

The engine starts workers with an interpreter found in this order:

1. The path in the `LUCUBRA_PYTHON` environment variable, when set.
2. A `python` executable in the same directory as the `lucubra` binary (the `Scripts` or `bin` folder of the environment that installed the wheel).
3. A `python3` or `python` executable on `PATH` that can import `lucubra_workers`.

## Consequences

- The engine runs as a native process with no interpreter in its address space. Workers stay separate processes, as SPEC section 4.2 requires.
- Step 2 covers `uvx`, `uv tool install` and virtual environments. Installs where the binary and the interpreter live in different folders (for example `pip install --user` on some platforms) depend on step 1 or step 3. Those install types are not tested in M0.
- Startup includes the `uvx` resolution cost, which dominates both measurements.
