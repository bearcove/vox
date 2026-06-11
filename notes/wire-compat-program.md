# Wire-compatibility program: loud failure, never silent drops

Priority directive (Amos, 2026-06-11): the vox/phon wire contract is
"seamlessly compatible when compatible, LOUDLY incompatible when not,
and NEVER silently dropping fields/payloads." An incident proved the
current surface violates this. Full spec coverage + a battery of tests
in all 3 languages (Rust, Swift, TypeScript), in BOTH phon and vox.

## The incident (2026-06-11, bee↔stax GPU-span integration)

A client built against crates.io vox 0.9.0-rc.0 + phon 0.2.0-rc.0
talked to a server built from local ~/vox HEAD (one wire-relevant
commit past the rc tag: `4042d6a9` "Route Vox connections by handshake
service" — touches vox-types/calls.rs, vox-core session, macros).
Observed, all on ONE connection:

- connect: succeeded
- `should_report(u32) -> bool`: round-tripped correctly
- `ingest(TargetSpanBatch)` (struct arg, unit return): **silently
  vanished** — client saw Ok (fire-and-forget), server never ran the
  method body, NOTHING was logged anywhere.

Cost: hours of debugging across three repos, because the failure
surface was "absence of data" instead of an error.

**Controlled experiment result (2026-06-11, /tmp/vox-skew, CORRECTED)**
-- pure crates.io-rc.0 client vs pure local-HEAD server, 4-method
matrix (scalar/struct x unit/bool): **ALL FOUR SHAPES INTEROP
PERFECTLY.** The three-level architecture (self-describing handshake /
schema-exchanged Message envelope per
session.handshake.protocol-schema / schema-exchanged payloads) works
exactly as designed across these versions.

A first run of this experiment appeared to show every call dying
ConnectionClosed with a clean server-side end -- that was a HARNESS
BUG: the scratch server dropped its NoopClient immediately after
establish(), and per rpc.caller.liveness.last-drop-closes-connection
the drop deliberately (and correctly) closed the session. Lesson
recorded because it is itself instructive: a correct-by-design clean
close is easily misread as a protocol failure; diagnostics
distinguishing 'closed by local drop' from 'closed by peer/protocol'
would have saved an hour (candidate for the battery's assertions).

CONSEQUENCES, honestly:
- The earlier 'vox/phon dialect skew' conclusion for the bee/stax
  incident is RETRACTED. The bee [patch.crates-io] vox-family pin
  was justified by that wrong conclusion; whether to keep it (dev
  lockstep with the living ~/vox tree) is Amos's call, but it is not
  known to FIX anything.
- The GPU-span incident's root cause is back to UNKNOWN. Prime
  suspects are operational, not wire: the wedged staxd kperf session,
  run/aggregator lifecycle races (aggregator resets on new runs), and
  sequencing chaos during debugging. Re-test cleanly after a staxd
  restart, with bee UNPATCHED, before attributing anything to the
  wire.
- The spec rules and the battery remain the deliverable: they pin the
  (now verified-working) contract against regression, and they add
  the loudness guarantees whose absence made this investigation
  expensive.

**Open forensic question** (answer while building the battery): which
axis broke — struct-vs-scalar payload encoding, or unit-return
(no-response) framing vs request/response framing? The cross-version
matrix test (below) must cover both axes regardless.

Related: `4042d6a9` itself FIXED one instance of this class — the
`()` Noop handler used to silently swallow every call
(`unit_handler_is_noop` → `unit_handler_reports_unknown_method`).
Generalize that contract; don't fix instances one at a time.

## The contract (to become spec rules)

vox (docs/content/spec/conn.md, rpc.md):

1. **conn.handshake.protocol-identity** — the handshake carries a
   protocol identity (version/feature set). A peer whose identity is
   incompatible is REFUSED at connect with an explicit diagnostic on
   both sides. Wire-shape changes (envelope, framing, codec) MUST bump
   the identity. (The rc→HEAD skew would then be a loud connect error.)
2. **rpc.decode-failure-is-loud** — a frame or payload that fails to
   decode is a protocol error: surfaced to the local app (error
   callback/log at error level) AND to the peer (protocol-error frame
   or connection teardown). Skip-and-continue is forbidden.
3. **rpc.no-response-still-errors** — methods without a response
   channel (unit return / fire-and-forget) still surface decode and
   dispatch failures via rule 2's mechanism. "No response expected"
   never means "no error path".
4. **rpc.unknown-is-an-error** — unknown service, unknown method,
   unknown schema: explicit error to the peer, never a no-op. (The old
   Noop-handler behavior is the canonical violation.)

phon (docs/content/spec.md):

5. **phon.decode.exact** — decoding a value against a schema is exact:
   every field accounted for, no partial decode, no silent field drops.
   Mismatch = error.
6. **phon.compat.planner-only** — cross-schema decode happens only
   through the translation planner; the planner either produces a
   complete plan (seamless) or an explicit incompatibility error
   (loud). No best-effort translation.

## The battery (3 languages × both repos)

phon (extend `conformance/` — corpus pattern already exists; compat
vectors live in conformance/compat/vectors.json):
- **Compatible-evolution vectors**: pairs (schema A, schema B, value
  bytes) where the planner MUST succeed and the translated value MUST
  equal a golden — byte-for-byte across rust/swift/ts.
- **Incompatible vectors**: pairs where the planner MUST return an
  error (not a lossy plan) — same error CLASS across all 3 languages.
- **Truncation/corruption vectors**: every prefix-truncated and
  bit-flipped variant of each golden decodes to an ERROR, never Ok.

vox (protocol-error matrix, rust first, then swift/ts runtimes):
- **Cross-version matrix**: client/server pairs differing in envelope
  details (simulated old frames from pinned byte fixtures): connect
  must fail loudly at handshake once rule 1 exists; pinned regression
  bytes from the rc.0 envelope so the incident shape stays covered.
- **Per-method-shape matrix**: {scalar arg, struct arg} × {unit
  return, value return, Tx/Rx} × {well-formed, undecodable payload,
  unknown method, unknown schema} — every cell asserts either correct
  dispatch or LOUD failure (error reply observed by caller, or
  connection error + error-level diagnostic). No cell may pass via
  silence.
- The Noop-handler regression (already partly covered by
  `unit_handler_reports_unknown_method`) folded into the matrix.

Coverage gate: every new rule gets r[impl]/r[verify] annotations in
all three implementations; tracey full coverage in both repos is the
done criterion. Baseline (2026-06-11): vox rust 174/174, ts 174/174,
swift 164/174 (10 uncovered); phon rust 60/60, swift 54/60,
ts 49/60 — the swift/ts backlog closes as part of this program.

## Execution order

1. Spec rules (vox conn/rpc + phon spec) — wording first, ids stable.
2. phon vectors + rust loader assertions; then swift/ts loaders.
3. vox rust matrix (in-tree adversarial: handshake gate, undecodable
   payload, unknown method/schema, unit-return loudness) + pinned
   rc.0 envelope fixtures for the cross-version case.
4. swift/ts vox runtime parity tests.
5. tracey to 100% on all six (repo × language) axes.
