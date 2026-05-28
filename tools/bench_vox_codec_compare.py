#!/usr/bin/env python3
import argparse
import re
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True

from bench_compare import emit_html, parse_divan_log


DEFAULT_BASELINE = Path("/Users/amos/.codex/worktrees/vox-postcard-baseline")


def slug(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "-", value).strip("-")
    return cleaned or "all"


def run_bench(cwd: Path, filter_: str, sample_count: int, output: Path) -> None:
    cmd = [
        "cargo",
        "bench",
        "-p",
        "vox-bench",
        "--bench",
        "rpc",
        filter_,
        "--",
        "--sample-count",
        str(sample_count),
    ]
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", encoding="utf-8") as log:
        proc = subprocess.run(
            cmd,
            cwd=cwd,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        log.write(proc.stdout)
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout)
        raise SystemExit(proc.returncode)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Run old-postcard vs current-binette Vox codec benches and emit an HTML report."
    )
    parser.add_argument(
        "--baseline-cwd",
        type=Path,
        default=DEFAULT_BASELINE,
        help=f"Old Vox worktree to benchmark. Default: {DEFAULT_BASELINE}",
    )
    parser.add_argument(
        "--current-cwd",
        type=Path,
        default=Path.cwd(),
        help="Current Vox worktree to benchmark. Default: current directory.",
    )
    parser.add_argument(
        "--filter",
        default="codec::wide_struct",
        help="Divan benchmark filter passed after the bench target.",
    )
    parser.add_argument(
        "--sample-count",
        type=int,
        default=1,
        help="Divan sample count for each benchmark run.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="HTML report path. Default: target/bench-reports/<filter>.html",
    )
    args = parser.parse_args()

    current_cwd = args.current_cwd.resolve()
    baseline_cwd = args.baseline_cwd.resolve()
    if not baseline_cwd.exists():
        raise SystemExit(f"baseline worktree does not exist: {baseline_cwd}")
    if not current_cwd.exists():
        raise SystemExit(f"current worktree does not exist: {current_cwd}")

    name = slug(args.filter)
    log_dir = current_cwd / "target" / "bench-logs"
    report = args.output or (current_cwd / "target" / "bench-reports" / f"{name}.html")
    baseline_log = log_dir / f"old-postcard-{name}.txt"
    current_log = log_dir / f"current-binette-{name}.txt"

    run_bench(baseline_cwd, args.filter, args.sample_count, baseline_log)
    run_bench(current_cwd, args.filter, args.sample_count, current_log)

    rows = [
        *parse_divan_log(baseline_log, "old-postcard"),
        *parse_divan_log(current_log, "current-binette"),
    ]
    report.parent.mkdir(parents=True, exist_ok=True)
    emit_html(rows, report)

    print(f"baseline log: {baseline_log}")
    print(f"current log:  {current_log}")
    print(f"report:       {report}")


if __name__ == "__main__":
    main()
