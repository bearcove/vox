# cf. https://github.com/casey/just

BINETTE_DIR := env_var_or_default("BINETTE_DIR", "/Users/amos/binette")

list:
    just --list

check:
    cargo clippy --all-targets
    tracey query validate

rust *args:
    cargo build --package subject-rust
    cargo nextest run -p spec-tests {{ args }}

rust-ffi:
    cargo build --release -p subject-rust

cov *args:
    cargo llvm-cov nextest --summary-only {{ args }}

ts-typecheck:
    pnpm check

ts-codegen:
    cargo xtask codegen --typescript

ts:
    just ts-typecheck
    just ts-codegen
    SUBJECT_CMD="./typescript/subject/subject-ts.sh" cargo nextest run -p spec-tests typescript_

swift:
    cargo build --manifest-path {{BINETTE_DIR}}/Cargo.toml -p binette
    cd {{BINETTE_DIR}}/swift/probes && swift test
    cd swift && swift test

swift-subject-cov *args:
    @echo "Swift coverage has not been restored on the binette local-access path yet."
    @false

swift-subject-cov-tcp *args:
    @echo "Swift TCP coverage has not been restored on the binette local-access path yet."
    @false

swift-subject-cov-html:
    @echo "Swift HTML coverage has not been restored on the binette local-access path yet."
    @false

examples:
    #!/bin/bash -eu
    for path in rust-examples/examples/*.rs; do \
      example="${path##*/}"; \
      example="${example%.rs}"; \
      echo "[examples] running $example"; \
      cargo run -p rust-examples --example "$example"; \
    done

all *args:
    just rust {{ args }}
    just ts {{ args }}
    just examples

wasm-build:
    wasm-pack build --target web rust/wasm-browser-tests --out-dir ../../wasm/tests/browser-wasm/pkg
    wasm-pack build --target web rust/wasm-inprocess-tests --out-dir ../../wasm/tests/browser-inprocess/pkg

ws-wasm *args:
    cd wasm/tests/playwright && pnpm exec playwright test ws-wasm.spec.ts {{ args }}

inprocess-wasm *args:
    cd wasm/tests/playwright && pnpm exec playwright test inprocess-wasm.spec.ts {{ args }}

ws-ts *args:
    cd typescript/tests/playwright && pnpm exec playwright test ws-ts.spec.ts {{ args }}

fuzz-targets:
    @echo "Available fuzz targets:"
    @echo "  protocol_decode      (fuzz/vox-afl)"
    @echo "  testbed_mem_session  (fuzz/vox-afl)"
    @echo ""
    @echo "Use: just fuzz-build [target|all]"
    @echo "Use: just fuzz-run [target|all] [seconds?]"
    @echo "Use: just fuzz [target|all] [seconds?]"

fuzz-build target="all":
    @case "{{ target }}" in \
      all) \
        cargo afl build --manifest-path fuzz/vox-afl/Cargo.toml --bin protocol_decode; \
        cargo afl build --manifest-path fuzz/vox-afl/Cargo.toml --bin testbed_mem_session; \
        ;; \
        ;; \
      protocol_decode|testbed_mem_session) \
        cargo afl build --manifest-path fuzz/vox-afl/Cargo.toml --bin "{{ target }}"; \
        ;; \
      *) \
        echo "Unknown target: {{ target }}" >&2; \
        just fuzz-targets; \
        exit 1; \
        ;; \
    esac

fuzz-run target="all" seconds="":
    just fuzz-build "{{ target }}"
    @mkdir -p \
      fuzz/vox-afl/out/protocol_decode \
      fuzz/vox-afl/out/testbed_mem_session
    @trap 'exit 130' INT TERM; \
    run_fuzz() { \
      if [ -n "$1" ]; then \
        cargo afl fuzz -V "$1" -i "$2" -o "$3" -- "$4"; \
      else \
        cargo afl fuzz -i "$2" -o "$3" -- "$4"; \
      fi; \
      status=$?; \
      case "$status" in \
        0) ;; \
        130|143) exit "$status" ;; \
        *) exit "$status" ;; \
      esac; \
    }; \
    case "{{ target }}" in \
      all) \
        run_fuzz "{{ seconds }}" fuzz/vox-afl/in/protocol_decode fuzz/vox-afl/out/protocol_decode fuzz/vox-afl/target/debug/protocol_decode; \
        run_fuzz "{{ seconds }}" fuzz/vox-afl/in/testbed_mem_session fuzz/vox-afl/out/testbed_mem_session fuzz/vox-afl/target/debug/testbed_mem_session; \
        ;; \
        ;; \
        ;; \
      protocol_decode) \
        run_fuzz "{{ seconds }}" fuzz/vox-afl/in/protocol_decode fuzz/vox-afl/out/protocol_decode fuzz/vox-afl/target/debug/protocol_decode; \
        ;; \
      testbed_mem_session) \
        run_fuzz "{{ seconds }}" fuzz/vox-afl/in/testbed_mem_session fuzz/vox-afl/out/testbed_mem_session fuzz/vox-afl/target/debug/testbed_mem_session; \
        ;; \
      *) \
        echo "Unknown target: {{ target }}" >&2; \
        just fuzz-targets; \
        exit 1; \
        ;; \
    esac

fuzz target="all" seconds="":
    just fuzz-run "{{ target }}" "{{ seconds }}"

fuzz-asan-build target="all":
    AFL_USE_ASAN=1 just fuzz-build "{{ target }}"

fuzz-asan-run target="all" seconds="":
    AFL_USE_ASAN=1 ASAN_OPTIONS=abort_on_error=1:symbolize=1:detect_leaks=0 just fuzz-run "{{ target }}" "{{ seconds }}"

fuzz-asan target="all" seconds="":
    just fuzz-asan-build "{{ target }}"
    just fuzz-asan-run "{{ target }}" "{{ seconds }}"

fuzz-ubsan-build target="all":
    AFL_USE_UBSAN=1 just fuzz-build "{{ target }}"

fuzz-ubsan-run target="all" seconds="":
    AFL_USE_UBSAN=1 UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 just fuzz-run "{{ target }}" "{{ seconds }}"

fuzz-ubsan target="all" seconds="":
    just fuzz-ubsan-build "{{ target }}"
    just fuzz-ubsan-run "{{ target }}" "{{ seconds }}"

fuzz-sand-build target="all":
    @build_one() { \
      t="$1"; \
      case "$t" in \
          ;; \
        protocol_decode|testbed_mem_session) \
          manifest="fuzz/vox-afl/Cargo.toml"; \
          bin_path="fuzz/vox-afl/target/debug/$t"; \
          ;; \
        *) \
          echo "Unknown target: $t" >&2; \
          just fuzz-targets; \
          exit 1; \
          ;; \
      esac; \
      out_dir="fuzz/.sand/$t"; \
      mkdir -p "$out_dir"; \
      cargo afl build --manifest-path "$manifest" --bin "$t"; \
      cp "$bin_path" "$out_dir/native"; \
      AFL_USE_ASAN=1 AFL_LLVM_ONLY_FSRV=1 cargo afl build --manifest-path "$manifest" --bin "$t"; \
      cp "$bin_path" "$out_dir/asan"; \
      AFL_USE_UBSAN=1 AFL_LLVM_ONLY_FSRV=1 cargo afl build --manifest-path "$manifest" --bin "$t"; \
      cp "$bin_path" "$out_dir/ubsan"; \
    }; \
    case "{{ target }}" in \
      all) \
        build_one protocol_decode; \
        build_one testbed_mem_session; \
        ;; \
      *) \
        build_one "{{ target }}"; \
        ;; \
    esac

fuzz-sand-run target="all" seconds="":
    @run_one() { \
      t="$1"; \
      case "$t" in \
        protocol_decode|testbed_mem_session) in_dir="fuzz/vox-afl/in/$t" ;; \
        *) \
          echo "Unknown target: $t" >&2; \
          just fuzz-targets; \
          exit 1; \
          ;; \
      esac; \
      just fuzz-sand-build "$t"; \
      out_dir="fuzz/.sand/out/$t"; \
      bin_dir="fuzz/.sand/$t"; \
      mkdir -p "$out_dir"; \
      trap 'exit 130' INT TERM; \
      if [ -n "{{ seconds }}" ]; then \
        cargo afl fuzz -V "{{ seconds }}" -i "$in_dir" -o "$out_dir" -w "$bin_dir/asan" -w "$bin_dir/ubsan" -- "$bin_dir/native"; \
      else \
        cargo afl fuzz -i "$in_dir" -o "$out_dir" -w "$bin_dir/asan" -w "$bin_dir/ubsan" -- "$bin_dir/native"; \
      fi; \
      status=$?; \
      case "$status" in \
        0|130|143) ;; \
        *) exit "$status" ;; \
      esac; \
    }; \
    case "{{ target }}" in \
      all) \
        run_one protocol_decode; \
        run_one testbed_mem_session; \
        ;; \
      *) \
        run_one "{{ target }}"; \
        ;; \
    esac

npm-publish *args:
    pnpm run build
    pnpm -r publish --access public --no-git-checks {{ args }}

fuzz-sand target="all" seconds="":
    just fuzz-sand-run "{{ target }}" "{{ seconds }}"
