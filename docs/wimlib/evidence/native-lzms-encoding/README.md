# Native LZMS encoding evidence

The safe Rust implementation `wim-codec/src/lzms_encode.rs` emits original LZMS
forward binary range coding, reverse 16-bit Huffman coding, adaptive symbol
frequencies, literal/LZ/delta tokens, delayed recent-source queues and the LZMS
x86 preprocessing dialect. It is a child of the decoder module so adaptive
canonical code construction and format tables have one implementation. No C
library is used in production.

`delta-red.log` records the real failing delta-compression regression before
implementation (4096-byte arithmetic ramp compressed to 268 bytes). The final
implementation passes the under-40-byte assertion. `rust-tests.log` contains six
passing tests covering compression ratio, literal alphabet rebuilds, long
30-extra-bit match lengths, exact/odd capacities, eight delta spans and x86.

`oracle.json` records 119 deterministic independent roundtrips using the original
wimlib decoder, revision `cd5e231c348c255ae5088873b5a66ee0eb96fa07`:

- constant/random blocks from 4 to 131072 bytes;
- repeated patterns with eight periods;
- all eight delta powers;
- all supported instruction forms plus excluded relative jump;
- 64 varying random alphabets.

The differential harness allocates 64 physical guard bytes after the compressed
buffer because upstream's word/SIMD reads require readable padded memory. Its
logical compressed length remains exact and output bytes must exactly equal
input bytes. An initial unpadded test-only oracle process faulted on a 131072-byte
constant block; providing the guard memory fixed the observation without changing
any Rust encoding bytes.

Reproduce with:

```sh
cargo build --manifest-path Cargo.toml --target-dir target --locked -p wim-codec --example encode_lzms
python3 scripts/wimlib/check-lzms-encoder.py --output docs/wimlib/evidence/native-lzms-encoding/oracle.json
```

Remaining full upstream compressor parity: this parser uses a bounded 64-entry
hash chain and greedy match choice rather than upstream's near-optimal parser.
Delta search currently considers raw offsets 1 through 4 for each of eight powers,
plus repeat queues. It does not expose compression levels, destructive mode or
memory-estimation APIs. It accepts blocks up to the format's 2^30-byte maximum,
but evidence currently stops at 131072 bytes; huge-block allocation/performance
and compression-level ratio gates remain unverified. Capacity and output size
are bounded independently; all allocations return a recoverable error. This is
real native compressed format interoperability, not yet complete compressor API
parity or an archive/C ABI drop-in replacement.
