"""Derive build and Cargo versions from Git tags."""
import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(
    r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)

def git(*arguments):
    result = subprocess.run(
        ["git", *arguments], cwd=ROOT, capture_output=True,
        text=True, encoding="utf-8",
    )
    if result.returncode:
        raise RuntimeError(
            f"git {' '.join(arguments)} failed: {result.stderr.strip()}. "
            "Use a Git checkout with its tags and full history."
        )
    return result.stdout.strip()

def cargo_version(value):
    normalized = value[1:] if value.startswith("v") else value
    match = SEMVER.fullmatch(normalized)
    if not match:
        raise RuntimeError(f"Git description is not a Cargo-compatible version: {value!r}.")
    if match[4] and any(
        part.isdigit() and len(part) > 1 and part.startswith("0")
        for part in match[4].split(".")
    ):
        raise RuntimeError(f"Git description has a non-SemVer prerelease number: {value!r}.")
    return normalized

def describe():
    value = git("describe", "--tags")
    return {
        "version": value,
        "tag": value,
        "cargo_version": cargo_version(value),
        "source_commit": git("rev-parse", "HEAD"),
    }

def sync_cargo(info):
    current = info["cargo_version"]
    manifest = ROOT / "Cargo.toml"
    lock = ROOT / "Cargo.lock"
    manifest_text = manifest.read_text(encoding="utf-8")
    lock_text = lock.read_text(encoding="utf-8")
    manifest_text, count = re.subn(
        r'(\[package\]\s*\n(?:(?!\[).+\n)*?version\s*=\s*)"[^"]+"',
        lambda match: match[1] + json.dumps(current), manifest_text, count=1,
    )
    if count != 1:
        raise RuntimeError("Cannot locate the root package version in Cargo.toml.")
    lock_text, count = re.subn(
        r'(\[\[package\]\]\s*\nname = "rsc"\s*\nversion = )"[^"]+"',
        lambda match: match[1] + json.dumps(current), lock_text, count=1,
    )
    if count != 1:
        raise RuntimeError("Cannot locate the root package version in Cargo.lock.")
    for path, text in ((manifest, manifest_text), (lock, lock_text)):
        if path.read_text(encoding="utf-8") != text:
            path.write_text(text, encoding="utf-8", newline="\n")
    return info

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sync", action="store_true", help="Synchronize Cargo.toml and Cargo.lock")
    args = parser.parse_args()
    info = describe()
    if args.sync:
        sync_cargo(info)
    print(json.dumps(info))

if __name__ == "__main__":
    main()
