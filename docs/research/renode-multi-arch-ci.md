# Renode Multi-Architecture CI Testing for Gale

## Summary

This document details how to use [Renode](https://renode.io/) (Antmicro's open-source
hardware emulator) for multi-architecture CI testing of Gale's kernel primitives on
Cortex-M4F, Cortex-M33, and Cortex-R5 targets. Renode provides cycle-approximate
instruction-set simulation with peripheral models, removing the need for physical
hardware in CI.

PulseEngine maintains a fork of the official Bazel integration at
`pulseengine/renode-bazel-rules` (forked from `antmicro/renode-bazel-rules`).

## Architecture Matrix

| Architecture | Zephyr Board            | Renode Platform              | Rust Target                   | FPU | MPU |
|-------------|------------------------|------------------------------|-------------------------------|-----|-----|
| Cortex-M3   | `qemu_cortex_m3`       | N/A (QEMU, current baseline) | `thumbv7m-none-eabi`          | No  | No  |
| Cortex-M4F  | `stm32f4_disco`        | `stm32f4_discovery.repl`     | `thumbv7em-none-eabihf`       | Yes | Yes |
| Cortex-M33  | `nucleo_l552ze_q`      | `stm32l552.repl`             | `thumbv8m.main-none-eabihf`   | Yes | Yes |
| Cortex-R5   | `qemu_cortex_r5`       | `zynqmp.repl` (RPU)          | `armv7r-none-eabihf`          | Yes | Yes |

### Why These Targets

- **Cortex-M4F** (STM32F4 Discovery): Most widely deployed Cortex-M with FPU.
  Renode has excellent STM32F4 support including Zephyr hello-world tests upstream.
  Exercises the `thumbv7em-none-eabihf` Rust target.

- **Cortex-M33** (STM32L552 / Nucleo L552ZE-Q): ARMv8-M with TrustZone and PMSAv8 MPU.
  Renode has `stm32l552.repl` and upstream tests proving Zephyr MPU/userspace works.
  Exercises the `thumbv8m.main-none-eabihf` Rust target.

- **Cortex-R5** (ZynqMP RPU): Safety-critical real-time processor class.
  Renode has comprehensive Zephyr support on `zynqmp.repl` including kernel tests
  (condition variables, synchronization, FPU sharing, MPU, userspace).
  Exercises the `armv7r-none-eabihf` Rust target.

### Renode Platform Support Evidence

Antmicro's upstream Renode test suite (`tests/platforms/`) confirms:

- **STM32F4**: `STM32F4_Discovery.robot` -- boots Zephyr hello-world, peripheral export,
  RTC, timers, faultmask.
- **STM32L552 (M33)**: `ARMv8M.robot` -- boots Zephyr hello-world, PMSAv8 MPU protection
  test, userspace test, FPU sharing test.
- **ZynqMP R5**: `zynqmp.robot` -- boots Zephyr hello-world, condition variables,
  synchronization, MetaIRQ, philosophers, shell, FPU sharing, MPU test, userspace
  producer/consumer, shared memory.

## Approach: Bazel + renode-bazel-rules

### Why Bazel (Not GitHub Actions Directly)

1. **Hermetic**: `renode-bazel-rules` downloads Renode portable automatically. No system
   install needed.
2. **Cacheable**: Bazel caches test results. Re-running unchanged tests costs nothing.
3. **Composable**: Fits into the existing `BUILD.bazel` test suite hierarchy (precommit /
   ci / nightly).
4. **Linux-only**: Renode portable is Linux x86_64. The Bazel toolchain constraint
   (`@platforms//os:linux, @platforms//cpu:x86_64`) handles this cleanly -- tests are
   silently skipped on macOS dev machines.

### Toolchain Constraint

The renode-bazel-rules toolchain registers with:
```starlark
target_compatible_with = [
    "@platforms//os:linux",
    "@platforms//cpu:x86_64",
]
```

This means `renode_test` targets only execute on Linux x86_64 (CI runners). On macOS
dev machines, `bazel test //renode:all` will report targets as "SKIPPED (incompatible)".

## Integration Plan

### Step 1: Add rules_renode to MODULE.bazel

```starlark
# Renode emulation rules -- multi-architecture hardware-in-the-loop testing
bazel_dep(name = "rules_renode", version = "0.0.0")
git_override(
    module_name = "rules_renode",
    remote = "https://github.com/pulseengine/renode-bazel-rules.git",
    commit = "5fc76ad535034add5be5faead26820a53f8d0d23",
)

renode = use_extension("@rules_renode//renode:extensions.bzl", "renode")
use_repo(renode, "renode_toolchains")
register_toolchains("@renode_toolchains//:all")
```

Note: `rules_renode` also pulls in `rules_python 0.36.0` (for the Robot Framework
runner). This may need version alignment with Gale's existing `rules_python` if present.

### Step 2: Build ELF Binaries via West (Out-of-Bazel)

Renode tests consume pre-built ELF files. The Gale+Zephyr build happens via `west build`,
not Bazel. The workflow is:

1. Build Zephyr test suite with Gale for each target board:
   ```bash
   # Cortex-M4F (STM32F4 Discovery)
   west build -b stm32f4_disco \
     -s zephyr/tests/kernel/semaphore/semaphore \
     -- -DZEPHYR_EXTRA_MODULES=/path/to/gale \
        -DOVERLAY_CONFIG=/path/to/gale/zephyr/gale_overlay.conf

   # Cortex-M33 (Nucleo L552ZE-Q nonsecure)
   west build -b nucleo_l552ze_q/stm32l552xx/ns \
     -s zephyr/tests/kernel/semaphore/semaphore \
     -- -DZEPHYR_EXTRA_MODULES=/path/to/gale \
        -DOVERLAY_CONFIG=/path/to/gale/zephyr/gale_overlay.conf

   # Cortex-R5 (ZynqMP)
   west build -b qemu_cortex_r5 \
     -s zephyr/tests/kernel/semaphore/semaphore \
     -- -DZEPHYR_EXTRA_MODULES=/path/to/gale \
        -DOVERLAY_CONFIG=/path/to/gale/zephyr/gale_overlay.conf
   ```

2. Collect ELF files from `build/zephyr/zephyr.elf`.

3. Either:
   - (a) Check ELF files into the repo under `renode/elfs/` (simple, deterministic), or
   - (b) Use `http_file()` to host them (like Antmicro does), or
   - (c) Build them in a CI step before running `bazel test`.

Option (c) is recommended for CI. Option (a) is useful for local reproducibility.

### Step 3: Create Robot Test Files

Each architecture needs a `.robot` file that loads the platform description, loads the
ELF, and asserts UART output.

#### `renode/stm32f4_sem.robot`

```robot
*** Variables ***
${UART}                       sysbus.usart2

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/boards/stm32f4_discovery.repl
    Execute Command           sysbus LoadELF ${ELF}

*** Test Cases ***
Should Pass Gale Semaphore Tests On STM32F4
    Create Machine
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120
```

#### `renode/zynqmp_r5_sem.robot`

```robot
*** Variables ***
${UART}                       sysbus.uart0

*** Keywords ***
Create Machine
    Execute Command           set bin ${ELF}
    Execute Command           include @scripts/single-node/zynqmp_zephyr.resc
    Execute Command           machine SetSerialExecution True

*** Test Cases ***
Should Pass Gale Semaphore Tests On Cortex-R5
    Create Machine
    ${tester}=                Create Terminal Tester  ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120
```

#### `renode/stm32l552_sem.robot`

```robot
*** Variables ***
${UART}                       sysbus.lpuart1

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/cpus/stm32l552.repl
    Execute Command           sysbus LoadELF ${ELF}

*** Test Cases ***
Should Pass Gale Semaphore Tests On Cortex-M33
    Create Machine
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120
```

### Step 4: Create BUILD.bazel for Renode Tests

```starlark
# renode/BUILD.bazel
load("@rules_renode//renode:defs.bzl", "renode_test")

# ---------------------------------------------------------------------------
# Cortex-M4F (STM32F4 Discovery)
# ---------------------------------------------------------------------------
renode_test(
    name = "stm32f4_sem_test",
    timeout = "long",
    robot_test = "stm32f4_sem.robot",
    variables_with_label = {
        "ELF": ":elfs/stm32f4_disco_sem.elf",
    },
)

# ---------------------------------------------------------------------------
# Cortex-M33 (STM32L552)
# ---------------------------------------------------------------------------
renode_test(
    name = "stm32l552_sem_test",
    timeout = "long",
    robot_test = "stm32l552_sem.robot",
    variables_with_label = {
        "ELF": ":elfs/nucleo_l552ze_q_sem.elf",
    },
)

# ---------------------------------------------------------------------------
# Cortex-R5 (ZynqMP RPU)
# ---------------------------------------------------------------------------
renode_test(
    name = "zynqmp_r5_sem_test",
    timeout = "long",
    robot_test = "zynqmp_r5_sem.robot",
    variables_with_label = {
        "ELF": ":elfs/qemu_cortex_r5_sem.elf",
    },
)

# Filegroup for ELF binaries (checked in or built externally)
filegroup(
    name = "elfs",
    srcs = glob(["elfs/**/*.elf"]),
)
```

### Step 5: Add to Test Suites

```starlark
# In root BUILD.bazel, add to the nightly suite:
test_suite(
    name = "nightly",
    tests = [
        # ... existing tests ...
        "//renode:stm32f4_sem_test",
        "//renode:stm32l552_sem_test",
        "//renode:zynqmp_r5_sem_test",
    ],
)
```

## GitHub Actions CI Workflow

Since `renode-bazel-rules` handles Renode download automatically, the CI workflow only
needs to build the ELF files and run `bazel test`.

### `.github/workflows/renode-tests.yml`

```yaml
name: Renode Multi-Arch Tests

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
  schedule:
    - cron: '0 3 * * 1'  # Weekly Monday 3am UTC

env:
  ZEPHYR_BASE: ${{ github.workspace }}/zephyr
  GALE_DIR: ${{ github.workspace }}/gale

jobs:
  build-elfs:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        include:
          - board: stm32f4_disco
            arch: cortex-m4f
          - board: nucleo_l552ze_q/stm32l552xx/ns
            arch: cortex-m33
          - board: qemu_cortex_r5
            arch: cortex-r5
    steps:
      - name: Checkout Gale
        uses: actions/checkout@v4
        with:
          path: gale

      - name: Install Zephyr SDK + west
        run: |
          pip3 install west
          west init -l zephyr  # or clone specific Zephyr version
          west update
          pip3 install -r zephyr/scripts/requirements.txt

          # Install Zephyr SDK
          wget -q https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v0.17.4/zephyr-sdk-0.17.4_linux-x86_64_minimal.tar.xz
          tar xf zephyr-sdk-0.17.4_linux-x86_64_minimal.tar.xz
          ./zephyr-sdk-0.17.4/setup.sh -t arm-zephyr-eabi

      - name: Install Rust targets
        run: |
          rustup target add thumbv7em-none-eabihf   # M4F
          rustup target add thumbv8m.main-none-eabihf  # M33
          rustup target add armv7r-none-eabihf       # R5

      - name: Build Zephyr+Gale semaphore test
        run: |
          source .venv/bin/activate 2>/dev/null || true
          # Build semaphore test for this board
          west build -b ${{ matrix.board }} \
            -s zephyr/tests/kernel/semaphore/semaphore \
            -- -DZEPHYR_EXTRA_MODULES=$GALE_DIR \
               -DOVERLAY_CONFIG=$GALE_DIR/zephyr/gale_overlay.conf

      - name: Upload ELF
        uses: actions/upload-artifact@v4
        with:
          name: elf-${{ matrix.arch }}
          path: build/zephyr/zephyr.elf

  renode-test:
    needs: build-elfs
    runs-on: ubuntu-latest
    steps:
      - name: Checkout Gale
        uses: actions/checkout@v4

      - name: Download all ELFs
        uses: actions/download-artifact@v4
        with:
          path: renode/elfs/

      - name: Rename ELFs
        run: |
          mv renode/elfs/elf-cortex-m4f/zephyr.elf renode/elfs/stm32f4_disco_sem.elf
          mv renode/elfs/elf-cortex-m33/zephyr.elf renode/elfs/nucleo_l552ze_q_sem.elf
          mv renode/elfs/elf-cortex-r5/zephyr.elf renode/elfs/qemu_cortex_r5_sem.elf

      - name: Run Renode tests via Bazel
        run: |
          bazelisk test //renode:all --test_output=all
```

## Cortex-R5 Details (ZynqMP RPU)

The ZynqMP platform in Renode models the full Zynq UltraScale+ including:
- **APU cluster** (4x Cortex-A53, aarch64)
- **RPU cluster** (2x Cortex-R5F, armv7-r)

Zephyr's `qemu_cortex_r5` board targets the RPU. The Renode script
`zynqmp_zephyr.resc` does:

```
cluster0 ForEach IsHalted true    # Halt all A53 cores
cluster1 ForEach IsHalted true    # Halt all R5 cores
rpu0 IsHalted false               # Enable only RPU core 0
sysbus LoadELF $bin cpu=rpu0      # Load ELF onto RPU
```

This is exactly what Gale needs -- single-core R5 execution of Zephyr kernel tests.

The ZynqMP R5 in Renode supports:
- FPU (FP extensions verified via `fpu_sharing` test)
- PMSAv7 MPU (protection/userspace tests pass)
- Full Zephyr kernel primitives (condition variables, synchronization proven upstream)

## UART Mapping

Each platform uses a different UART peripheral name:

| Platform         | UART for Zephyr Console  |
|-----------------|--------------------------|
| STM32F4 Disco    | `sysbus.usart2`          |
| STM32L552 (M33)  | `sysbus.lpuart1`         |
| ZynqMP R5        | `sysbus.uart0`           |

The Robot test must match the UART used in the Zephyr board's devicetree.

## Open Issues and Limitations

### 1. Linux-Only Renode Portable

The `renode-bazel-rules` toolchain only supports `@platforms//os:linux`. Renode does have
macOS builds (`renode-1.16.1-dotnet.osx-arm64-portable.dmg`) but the Bazel rules don't
support it. This means:

- Dev machines (macOS): Use QEMU via `west build -t run` as today
- CI (Linux): Use Renode via `bazel test //renode:all`

### 2. ELF Build Pipeline

The ELFs must be built via `west` (CMake-based) outside of Bazel. Options:

- **CI-only**: Build in GitHub Actions, pass as artifacts (recommended)
- **Checked-in**: Commit ELFs to `renode/elfs/` (deterministic but large)
- **http_file**: Host on artifact server, reference via `http_file()` rule

### 3. Board Variant Naming

Some Zephyr boards have complex qualifiers (e.g., `nucleo_l552ze_q/stm32l552xx/ns` for
the nonsecure variant). The `west build -b` command handles this but it needs to match
the board's `board.yml` specification.

For the M33 target, we specifically want the nonsecure (`ns`) variant since TrustZone
secure-world testing requires additional setup.

### 4. Test Output Format

Zephyr's test framework (ztest) prints `PROJECT EXECUTION SUCCESSFUL` on success and
`PROJECT EXECUTION FAILED` on failure. The Robot tests match on this string. All 6
Gale kernel primitives use ztest, so this pattern works uniformly.

### 5. Renode Version Pinning

The `renode-bazel-rules` default portable URL points to nightly builds
(`builds.renode.io`). For CI stability, pin to a release:

```starlark
renode.download_portable(
    name = "renode_toolchain",
    url = "https://github.com/renode/renode/releases/download/v1.16.1/renode-1.16.1.linux-portable-dotnet.tar.gz",
    sha256 = "<sha256-of-release-tarball>",
)
```

## Expanding Test Coverage

Once semaphore tests pass on all three architectures, extend to other primitives:

```
tests/kernel/semaphore/semaphore     --> sem_test
tests/kernel/mutex/mutex_api         --> mutex_test
tests/kernel/condvar/condvar_api     --> condvar_test
tests/kernel/msgq/msgq_api           --> msgq_test
tests/kernel/stack/stack              --> stack_test
tests/kernel/pipe/pipe_api            --> pipe_test
```

Each primitive needs its own Robot test file (same pattern, different ELF). The
`variables_with_label` mechanism in `renode_test` makes this a one-line change per
primitive per architecture.

## Full Test Matrix (Target State)

| Primitive | Cortex-M3 (QEMU) | Cortex-M4F (Renode) | Cortex-M33 (Renode) | Cortex-R5 (Renode) |
|-----------|:-:|:-:|:-:|:-:|
| Semaphore | PASS (24/24) | target | target | target |
| Mutex     | PASS (12/12) | target | target | target |
| Condvar   | PASS (11/11) | target | target | target |
| MsgQ      | PASS (13/13) | target | target | target |
| Stack     | PASS (12/12) | target | target | target |
| Pipe      | PASS (18/18) | target | target | target |

## Next Steps

1. Pin `pulseengine/renode-bazel-rules` master SHA.
2. Create `renode/` directory with BUILD.bazel and .robot files.
3. Build test ELFs for all three targets locally and validate via `renode-test` directly.
4. Wire into GitHub Actions with the `build-elfs` / `renode-test` two-stage pipeline.
5. Expand from semaphore to all 6 primitives.
6. Add Renode tests to the `nightly` Bazel test suite.
