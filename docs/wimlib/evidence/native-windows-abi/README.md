# Windows C ABI and guest execution

This is actual Windows x64 evidence, separately from Linux ABI checks. A disposable
Windows 11 guest (`10.0.26200.0`) ran under QEMU TCG because `/dev/kvm` was unavailable.
The launcher used the immutable `win11-dev` base, its own writable overlay and TPM
state, no networking, no display, and QGA for bounded file transfer and execution.
No other VM or base image was changed. The owned instance is `wim-abi-20261003`,
under `/tmp/wim-windows-abi-vm/vm-state/`.

## Measured results

- `compile-layout.json`: 44 constants from the unchanged original Windows C
  header, compiled for MSVC and read from the PE image. This alone is compile proof.
- `compile-rust-comparison.json`: 38 actual Rust facade layout constants match
  the original C declarations. Six C-only capture/scan/unmount measurements have
  no matching Windows native type in this check.
- `runtime-native.json`: the MSVC C executable and Rust layout executable both
  exit 0 in Windows. All 38 common runtime layout measurements agree.
- `runtime-original.json`: a separately built original Windows DLL and its
  matching MinGW C executable also exit 0 in the same guest.
- `runtime-native-gnu.json` and `runtime-gnu-comparison.json`: the identical
  MinGW/MSVCRT caller also runs against the native GNU DLL. Both processes exit 0;
  278 non-export measurements and 65 existing exports match the original exactly.
  The remaining seven export names and export count are the same explicit gates.
- `runtime-complete-layout.json`: the later real Rust facade now measures all
  44 C constants in the guest, including capture source, scan and unmount payload
  types. All 44 agree. This run has 70 exports; `runtime-complete-comparison.json`
  matches 278 non-export rows and 70 present exports, with only the two pipe names
  and export count still differing. Earlier 38-field/65-export records are retained
  as historical evidence.
- `runtime-comparison.json`: 343 of 351 C output rows match exactly. These
  matches comprise 278 non-export measurements and 65 present exports. The remaining
  eight rows are the seven missing native exports and the export count, retained
  as explicit gates. There are no measured behavior mismatches.

The probe measures original bitfield bytes, union/structure sizes and offsets,
wide version/compression/error strings, handle creation/free, guarded info output,
Unicode image names and descriptions (including a surrogate pair), name resolution,
Unicode filesystem paths, XML allocation released by the caller, and XML output
through a caller-created CRT `FILE`. Both DLLs write a 1,210-byte empty-image WIM
with a nonzero OS-generated GUID. QGA returns those actual bytes to the host, where
the unchanged original reader verifies and applies each successfully.

The original DLL imports `msvcrt.dll`, while the native MSVC DLL uses UCRT. Each
FILE/allocation test therefore uses a caller with the same CRT as its DLL. Passing
a UCRT `FILE` to the original MSVCRT DLL is not part of this evidence. Layout and
semantic rows compare across the two matched configurations, rather than assuming
CRT objects are interchangeable. The additional GNU run uses the same caller
bytes and MSVCRT for both DLLs; its 1,210-byte WIM independently verifies and applies
too. `original-build.json` records the oracle source
commit, header/DLL hashes, compiler and imports. All implementation/header files
match the preserved source; two inherited Linux test-runner edits in the disposable
build copy are explicitly listed and were neither built nor executed.

## Retained failures and prerequisites

`runtime-dependency-red.json` records the initial `STATUS_DLL_NOT_FOUND` before
installing the required VC runtime. The official x64 VC redistributable was fetched
from `https://aka.ms/vc14/vc_redist.x64.exe`, as linked by Microsoft's
[supported runtime documentation](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170).
Only the owned guest was provisioned. Before installation, PowerShell checked a
valid Authenticode signature signed by Microsoft Corporation. Installer hash,
signature output, exit 0 and runtime version are in `runtime-provision.json`.

`runtime-write-guid-red.json` records native write returning 68 before the native
Windows GUID provider existed. The final run uses `SystemFunction036` from
`advapi32`, as the original implementation does; the guest-generated GUID is
nonzero and the resulting WIM passes the independent reader.

## Reproduction

Compile `scripts/wimlib/probe-windows-abi.c` with
`scripts/wimlib/build-windows-abi-probe.py --clang <unwrapped-clang> --lld <lld-link>`.
The exact SDK, compiler and link commands are recorded in `compile-layout.json`.
Build the original in a disposable source copy using the configure flags and
toolchain from `original-build.json`, then `make -j4 libwim.la`. Build its caller
with the same MinGW compiler, `-municode -I/tmp/wimlib/include`, the probe source,
and the recorded extra library directory.

Boot an owned disposable guest with the vm-runner skill. Provision the official
runtime only in that guest if necessary; retain the signature and installer hash.
Run `scripts/wimlib/run-windows-abi-guest.py --qga-socket <owned-qga.sock>` for the
native DLL. For the original, supply `--implementation original --probe <MinGW
caller.exe> --dll <original.dll> --guest-directory <separate-owned-directory>
--output docs/wimlib/evidence/native-windows-abi/runtime-original.json`.
Finally run `scripts/wimlib/check-windows-runtime-abi.py`. WIM byte hashes and GUIDs
can change on rerun because timestamps and OS randomness are real.

For the GNU build, use the same original MinGW caller with the native GNU DLL,
`--implementation native-gnu`, a third owned guest directory and
`--output docs/wimlib/evidence/native-windows-abi/runtime-native-gnu.json`. Compare
that record with `check-windows-runtime-abi.py --native <GNU-record> --output
<GNU-comparison-record>`.

## Remaining gates

The initial native Windows DLL had 65 of 72 exports. The later complete-layout
run has 70: capture and print names are present; the two pipe extraction entry
points remain absent. [Capture](../native-windows-capture/README.md) and print
execution have separate evidence and explicit partial scopes. This is not a
full Windows drop-in result. Empty-image writing and host-side independent apply
do not establish native Windows capture, extraction, NTFS security, ACLs, named
streams, reparse points, privileged operations, servicing or installation.
Broader resource/error/progress/allocator behavior also needs Windows execution.

The capture integration enables five additional actual Rust layout measurements;
`compile-capture-rust-comparison.json` records 43/43 matching constants. Adding
the original unmount progress payload declaration then gives 44/44 matches for
all constants in this C probe (`compile-complete-probe-rust-comparison.json`).
That declaration describes the ABI only and does not enable FUSE or emit unmount
events. The historical 38-value runtime result remains attached to its original
executable. `runtime-complete-layout.json` separately records the newer
44-value executable and C caller both exiting 0 with all 44 measured values
matching in the actual Windows guest. Its newer GNU DLL has 70 exports, and the
written empty-image WIM independently verifies and applies.
Matching every constant in this probe does not measure every field in the public
header or establish full ABI behavior.
