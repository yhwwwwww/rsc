"""Read-only local query benchmarks, including process startup and captured output."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("programs", nargs="*", choices=["before", "after", "hok"])
parser.add_argument("--runs", type=int, default=7)
parser.add_argument("--before", type=Path, default=ROOT / ".test-lab/rsc-before-query.exe")
parser.add_argument("--hok", type=Path, default=Path(os.environ["USERPROFILE"]) / "scoop/apps/hok/current/hok.exe")
parser.add_argument("--output", type=Path, default=ROOT / ".test-lab/query-benchmark.json")
args = parser.parse_args()
if args.runs < 1:
    parser.error("--runs must be positive")
selected = args.programs or ["before", "after", "hok"]
paths = {"before": args.before, "after": ROOT / "dist/rsc.exe", "hok": args.hok}
cases = [
    ("search jq", ["search", "jq"], ["search", "-B", "jq"]),
    ("search ^git", ["search", "^git"], ["search", "-B", "^git"]),
    ("list", ["list"], ["list"]),
    ("cat jq", ["cat", "jq"], ["cat", "jq"]),
    ("status --local", ["status", "--local"], None),
]
report = {
    "schema": 1, "date": datetime.datetime.now().astimezone().isoformat(),
    "platform": platform.platform(), "logical_processors": os.cpu_count(),
    "runs": args.runs, "warmup": 1,
    "scope": "same local Scoop roots; process creation and captured output included; warm filesystem cache",
    "hok_search": "-B includes executable aliases; rsc checks names and binaries by default",
    "remote_checks": "excluded; Hok has no status command",
    "programs": {}, "results": [], "verification": {},
}
env = os.environ.copy()
env["NO_COLOR"] = "1"
for name in selected:
    exe = paths[name]
    if not exe.exists():
        report["programs"][name] = {"skipped": "executable unavailable"}
        continue
    version = subprocess.run([str(exe), "--version"], env=env, capture_output=True, timeout=30)
    report["programs"][name] = {
        "version": version.stdout.decode(errors="replace").strip(),
        "sha256": hashlib.sha256(exe.read_bytes()).hexdigest(), "bytes": exe.stat().st_size,
    }
outputs = {}
for label, arguments, hokargs in cases:
    for name in selected:
        exe, command = paths[name], hokargs if name == "hok" else arguments
        if not command or not exe.exists():
            continue
        samples = []
        for i in range(args.runs + report["warmup"]):
            start = time.perf_counter()
            p = subprocess.run([str(exe), *command], env=env, capture_output=True, timeout=90)
            elapsed = 1000 * (time.perf_counter() - start)
            if p.returncode:
                raise RuntimeError(f"{name} {label}: {p.returncode}: {p.stderr.decode(errors='replace')}")
            if name != "hok": assert b"\x1b[" not in p.stdout, f"{name}: ANSI in redirected output"
            if i >= report["warmup"]:
                samples.append(round(elapsed, 3))
        outputs[label, name] = p.stdout
        row = {
            "command": label, "program": name,
            "median_ms": round(statistics.median(samples), 3),
            "min_ms": min(samples), "samples_ms": samples, "output_bytes": len(p.stdout),
        }
        report["results"].append(row)
        print(json.dumps(row), flush=True)
for label, _, _ in cases:
    before, after = outputs.get((label, "before")), outputs.get((label, "after"))
    if before is None or after is None:
        continue
    if label == "cat jq":
        same = json.loads(before) == json.loads(after)
    elif label == "status --local":
        # The new renderer labels the previously blank healthy state.
        same = before == after.replace(b"current", b"       ")
    else:
        same = before == after
    assert same, f"Query result changed: {label}"
    report["verification"][label] = "same result; healthy status labels normalized"
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
