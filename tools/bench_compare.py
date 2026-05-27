#!/usr/bin/env python3
import argparse
import html
import json
import re
from dataclasses import dataclass
from pathlib import Path


TIME_RE = re.compile(r"^(?P<value>[0-9]+(?:\.[0-9]+)?)\s+(?P<unit>ns|µs|us|ms|s)$")
INLINE_TIME_RE = re.compile(r"(?P<value>[0-9]+(?:\.[0-9]+)?)\s+(?P<unit>ns|µs|us|ms|s)")


@dataclass(frozen=True)
class BenchRow:
    source: str
    path: tuple[str, ...]
    fastest_ns: float
    median_ns: float
    mean_ns: float


def parse_time(text: str) -> float:
    match = TIME_RE.match(text.strip())
    if match is None:
        raise ValueError(f"not a time value: {text!r}")
    value = float(match.group("value"))
    unit = match.group("unit")
    if unit == "ns":
        return value
    if unit in ("µs", "us"):
        return value * 1_000.0
    if unit == "ms":
        return value * 1_000_000.0
    if unit == "s":
        return value * 1_000_000_000.0
    raise ValueError(f"unsupported time unit: {unit!r}")


def format_ns(value: float) -> str:
    if value < 1_000.0:
        return f"{value:.3g} ns"
    if value < 1_000_000.0:
        return f"{value / 1_000.0:.3g} µs"
    return f"{value / 1_000_000.0:.3g} ms"


def parse_divan_log(path: Path, source: str) -> list[BenchRow]:
    rows: list[BenchRow] = []
    stack: list[str] = []
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.rstrip()
        marker_pos = max(line.rfind("├─"), line.rfind("╰─"))
        if marker_pos < 0:
            continue

        depth = marker_pos // 3
        time_matches = list(INLINE_TIME_RE.finditer(line))
        if time_matches:
            label_end = time_matches[0].start()
        else:
            separator_pos = line.find("│", marker_pos + 2)
            label_end = separator_pos if separator_pos >= 0 else len(line)

        label = line[marker_pos + 2 : label_end].strip()
        if not label:
            continue

        while len(stack) > depth:
            stack.pop()
        if len(stack) == depth:
            stack.append(label)
        else:
            stack.extend(["?"] * (depth - len(stack)))
            stack.append(label)

        if len(time_matches) < 4:
            continue

        try:
            fastest_ns = parse_time(time_matches[0].group(0))
            median_ns = parse_time(time_matches[2].group(0))
            mean_ns = parse_time(time_matches[3].group(0))
        except ValueError:
            continue

        rows.append(
            BenchRow(
                source=source,
                path=tuple(stack),
                fastest_ns=fastest_ns,
                median_ns=median_ns,
                mean_ns=mean_ns,
            )
        )
    return rows


def group_key(path: tuple[str, ...]) -> str:
    if len(path) < 2:
        return "other"
    return " / ".join(path[:2])


def bench_label(path: tuple[str, ...]) -> str:
    if len(path) <= 2:
        return path[-1]
    return " / ".join(path[2:])


def comparison_label(row: BenchRow) -> str:
    if len(row.path) <= 2:
        return bench_label(row.path)

    op = row.path[2]
    tail = row.path[3:]
    aliases = {
        "jit_decode": "strict decode",
        "strict_decode": "strict decode",
        "stencil_decode": "hybrid decode",
        "hybrid_decode": "hybrid decode",
        "jit_encode": "strict encode",
        "strict_encode": "strict encode",
        "stencil_encode": "hybrid encode",
        "hybrid_encode": "hybrid encode",
        "reflective_decode": "interpreted decode",
        "interp_decode": "interpreted decode",
        "serde_decode": "interpreted decode",
        "reflective_encode": "interpreted encode",
        "interp_encode": "interpreted encode",
        "serde_encode": "interpreted encode",
    }
    label = aliases.get(op, op)
    if tail:
        return " / ".join((label, *tail))
    return label


def comparison_sort_key(label: str) -> tuple[int, int, str]:
    mode_order = {
        "interpreted": 0,
        "hybrid": 1,
        "strict": 2,
    }
    op_order = {
        "encode": 0,
        "decode": 1,
    }
    words = label.split()
    mode = mode_order.get(words[0], 9) if words else 9
    op = op_order.get(words[1], 9) if len(words) > 1 else 9
    return (op, mode, label)


def emit_html(rows: list[BenchRow], output: Path) -> None:
    source_names = sorted({row.source for row in rows})
    by_group: dict[str, dict[str, dict[str, BenchRow]]] = {}
    for row in rows:
        by_group.setdefault(group_key(row.path), {}).setdefault(comparison_label(row), {})[
            row.source
        ] = row

    groups = []
    for group, labels in sorted(by_group.items()):
        entries = []
        for label, source_rows in sorted(labels.items(), key=lambda item: comparison_sort_key(item[0])):
            values = [
                {
                    "source": source,
                    "median_ns": source_rows[source].median_ns,
                    "mean_ns": source_rows[source].mean_ns,
                    "fastest_ns": source_rows[source].fastest_ns,
                    "median": format_ns(source_rows[source].median_ns),
                }
                for source in source_names
                if source in source_rows
            ]
            if values:
                entries.append({"label": label, "values": values})
        if entries:
            groups.append({"name": group, "entries": entries})

    data = {"sources": source_names, "groups": groups}
    output.write_text(render_html(data), encoding="utf-8")


def render_html(data: dict) -> str:
    payload = json.dumps(data)
    title = "Vox Codec Benchmark Comparison"
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{html.escape(title)}</title>
  <style>
    :root {{
      color-scheme: dark;
      --bg: #101214;
      --fg: #eef1f3;
      --muted: #a7b0b7;
      --line: #2d343a;
      --a: #64d2ff;
      --b: #ffd166;
      --c: #8bd17c;
      --panel: #171b1f;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      background: var(--bg);
      color: var(--fg);
      font: 14px/1.45 ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    main {{
      max-width: 1240px;
      margin: 0 auto;
      padding: 28px 22px 60px;
    }}
    h1 {{
      margin: 0 0 6px;
      font-size: 28px;
      font-weight: 720;
    }}
    .lede {{
      margin: 0 0 26px;
      color: var(--muted);
    }}
    .group {{
      margin: 0 0 34px;
      padding-top: 18px;
      border-top: 1px solid var(--line);
    }}
    h2 {{
      margin: 0 0 16px;
      font-size: 18px;
      font-weight: 680;
    }}
    .chart {{
      display: grid;
      gap: 10px;
    }}
    .row {{
      display: grid;
      grid-template-columns: minmax(180px, 280px) 1fr;
      gap: 14px;
      align-items: center;
      min-height: 34px;
    }}
    .label {{
      color: var(--muted);
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: 12px;
      overflow-wrap: anywhere;
    }}
    .bars {{
      display: grid;
      gap: 4px;
    }}
    .bar-line {{
      display: grid;
      grid-template-columns: minmax(0, 1fr) 92px;
      gap: 10px;
      align-items: center;
    }}
    .track {{
      height: 13px;
      background: var(--panel);
      border: 1px solid var(--line);
      overflow: hidden;
    }}
    .bar {{
      height: 100%;
      min-width: 2px;
    }}
    .value {{
      color: var(--fg);
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: 12px;
      text-align: right;
      white-space: nowrap;
    }}
    .legend {{
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      margin: 18px 0 22px;
      color: var(--muted);
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: 12px;
    }}
    .swatch {{
      display: inline-block;
      width: 10px;
      height: 10px;
      margin-right: 6px;
      vertical-align: -1px;
    }}
    @media (max-width: 760px) {{
      .row {{
        grid-template-columns: 1fr;
        gap: 5px;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <h1>{html.escape(title)}</h1>
    <p class="lede">Median times from captured Divan output. Shorter bars are faster; each group is scaled independently.</p>
    <div id="legend" class="legend"></div>
    <div id="groups"></div>
  </main>
  <script>
    const DATA = {payload};
    const colors = ["var(--a)", "var(--b)", "var(--c)"];
    const legend = document.getElementById("legend");
    DATA.sources.forEach((source, index) => {{
      const item = document.createElement("span");
      const swatch = document.createElement("span");
      swatch.className = "swatch";
      swatch.style.background = colors[index % colors.length];
      item.append(swatch, source);
      legend.append(item);
    }});

    const groups = document.getElementById("groups");
    DATA.groups.forEach((group) => {{
      const section = document.createElement("section");
      section.className = "group";
      const heading = document.createElement("h2");
      heading.textContent = group.name;
      const chart = document.createElement("div");
      chart.className = "chart";
      const max = Math.max(...group.entries.flatMap((entry) => entry.values.map((value) => value.median_ns)));

      group.entries.forEach((entry) => {{
        const row = document.createElement("div");
        row.className = "row";
        const label = document.createElement("div");
        label.className = "label";
        label.textContent = entry.label;
        const bars = document.createElement("div");
        bars.className = "bars";

        entry.values.forEach((value) => {{
          const sourceIndex = DATA.sources.indexOf(value.source);
          const line = document.createElement("div");
          line.className = "bar-line";
          const track = document.createElement("div");
          track.className = "track";
          const bar = document.createElement("div");
          bar.className = "bar";
          bar.style.width = `${{Math.max(0.8, (value.median_ns / max) * 100)}}%`;
          bar.style.background = colors[sourceIndex % colors.length];
          bar.title = `${{value.source}} median ${{value.median}}`;
          const text = document.createElement("div");
          text.className = "value";
          text.textContent = value.median;
          track.append(bar);
          line.append(track, text);
          bars.append(line);
        }});

        row.append(label, bars);
        chart.append(row);
      }});

      section.append(heading, chart);
      groups.append(section);
    }});
  </script>
</body>
</html>
"""


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build a standalone HTML comparison report from Divan benchmark logs."
    )
    parser.add_argument(
        "--input",
        action="append",
        required=True,
        metavar="NAME=PATH",
        help="Benchmark log input. Can be passed more than once.",
    )
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    rows: list[BenchRow] = []
    for spec in args.input:
        if "=" not in spec:
            raise SystemExit(f"--input must be NAME=PATH, got {spec!r}")
        name, raw_path = spec.split("=", 1)
        rows.extend(parse_divan_log(Path(raw_path), name))

    args.output.parent.mkdir(parents=True, exist_ok=True)
    emit_html(rows, args.output)
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
