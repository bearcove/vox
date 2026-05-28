# vox-bench

`vox-bench` contains Divan benchmarks for the active Vox RPC path and for the
binette codec layer used by that path.

## Codec Benchmarks

Run the current tree directly with Cargo:

```bash
cargo bench -p vox-bench --bench rpc codec -- --sample-count 10
```

Useful filters:

- `codec::args` exercises the generated request argument tuple for
  `echo_gnarly`.
- `codec::response` exercises the generated Vox response wrapper for
  `echo_gnarly`.
- `codec::wide_struct` exercises a flat scalar-heavy struct.
- `codec::many_variants` exercises enum dispatch and payload variants.
- `codec::tree` exercises recursive enum values.
- `codec::numeric_buffer` exercises large homogeneous numeric buffers.

Each codec shape is split into:

- `interp_*`: binette plans without stencil codegen.
- `hybrid_*`: stencil codegen with helper fallback at unsupported subtrees.
- `strict_*`: stencil codegen only; unsupported shapes fail at setup.

## Old-Vs-New Reports

`tools/bench_vox_codec_compare.py` runs the same Divan filter in an old Vox
worktree and in the current binette tree, then emits a standalone HTML report.
The default baseline is:

```text
/Users/amos/.codex/worktrees/vox-postcard-baseline
```

Run a quick smoke comparison:

```bash
python3 tools/bench_vox_codec_compare.py --filter codec::wide_struct --sample-count 1
```

Run additional shapes one at a time:

```bash
python3 tools/bench_vox_codec_compare.py --filter codec::args --sample-count 10
python3 tools/bench_vox_codec_compare.py --filter codec::response --sample-count 10
python3 tools/bench_vox_codec_compare.py --filter codec::many_variants --sample-count 10
python3 tools/bench_vox_codec_compare.py --filter codec::numeric_buffer --sample-count 10
```

`codec::tree` is a recursive `Box<T>` stress shape. It is intentionally kept in
the bench target, but it currently tracks recursive smart-pointer stencil decode
coverage rather than the main Vox request/response path.

Override either side explicitly:

```bash
python3 tools/bench_vox_codec_compare.py \
  --baseline-cwd /path/to/old/vox \
  --current-cwd /path/to/current/vox \
  --filter codec::response \
  --sample-count 10
```

The script writes raw Divan logs under `target/bench-logs/` and the HTML report
under `target/bench-reports/`. Historical `serde_*` and `jit_*` rows are
normalized in the report as `interpreted` and `strict` so they compare against
the current binette rows.
