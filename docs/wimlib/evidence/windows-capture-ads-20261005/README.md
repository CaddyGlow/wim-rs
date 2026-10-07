# Windows named-stream capture validation

This records the initial ADS gate. The subsequent
[expanded metadata gate](../windows-capture-full-20261005/README.md) adds
handle-based enumeration, protected-file reads, and reparse-owned streams.

The native Windows host-directory scanner now captures ordinary named `$DATA`
streams on files and directories, retaining UTF-16 names, empty streams, deferred
payloads, and shared bindings for hard-link aliases. A reported named-stream
capability followed by an unsupported enumeration error fails capture instead of
silently omitting streams. Ordinary reparse-point streams remain explicitly
rejected; EFS retains its separate raw-export route.

`Wim::capture_image_with_options` exposes `CaptureOptions::strict_security` so a
caller can reject security descriptor fallback. Default directory capture keeps
its existing policy. Inputs must remain unchanged through WIM writing.

The ignored `crates/wim/tests/windows_capture.rs` test runs on an elevated Windows
NTFS environment with DISM available. It initializes capture/apply privileges,
captures with strict security, writes an LZX WIM, reopens and verifies it, and
applies using both the Rust engine and independent DISM. Both results must retain
main bytes, file and hard-link ADS bytes, empty ADS, Unicode stream names,
directory ADS, and root-directory ADS. Mutating an applied alias's stream must
also change the original file's stream, proving the hard-link relationship is
retained by both apply engines. The retained runtime log and artifact hash
identify the selected execution; guest.json records the guest build.

Build:

```sh
RUSTFLAGS='-C target-feature=+crt-static' nix develop --command cargo xwin test \
  --locked -p wim --target x86_64-pc-windows-msvc \
  --test windows_capture --no-run
```

Copy the emitted test executable to an elevated Windows guest and run:

```text
windows_capture.exe --ignored --nocapture
```

This gate proves selected ADS capture and independent apply compatibility.
Strict capture success does not independently prove every ACL/SACL byte.
It does not establish complete Windows filesystem metadata parity, capture of
named streams on reparses, native offline NTFS/QCOW2 parsing, or installation of
a captured Windows operating system. Those gates remain in the conversion plan.
