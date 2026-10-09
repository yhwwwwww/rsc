"""Prepare and publish rsc releases; update the separate Scoop bucket."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
REPOSITORY = "yhwwwwww/rsc"
TARGET = "x86_64-pc-windows-msvc"
SYSTEM_DLLS = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "cfgmgr32.dll",
    "comctl32.dll", "crypt32.dll", "dbghelp.dll", "dnsapi.dll", "gdi32.dll",
    "imm32.dll", "iphlpapi.dll", "kernel32.dll", "kernelbase.dll",
    "msvcrt.dll", "ncrypt.dll", "netapi32.dll", "normaliz.dll", "ntdll.dll",
    "ole32.dll", "oleaut32.dll", "powrprof.dll", "propsys.dll", "psapi.dll",
    "rpcrt4.dll", "secur32.dll", "setupapi.dll", "shell32.dll", "shlwapi.dll",
    "ucrtbase.dll", "user32.dll", "userenv.dll", "version.dll",
    "winhttp.dll", "wininet.dll", "winmm.dll", "wintrust.dll", "ws2_32.dll",
}
ASSETS = ("rsc.exe", "rsc.json", "SHA256SUMS", "LICENSE", "build-info.json")

def run(*args, cwd=None):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        raise RuntimeError(f"{args[0]} failed: {result.stderr.strip()}")
    return result.stdout.strip()

def api(path, missing_ok=False):
    result = subprocess.run(["gh", "api", path], capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        if missing_ok and "HTTP 404" in result.stderr:
            return None
        raise RuntimeError(f"GitHub API failed: {result.stderr.strip()}")
    return json.loads(result.stdout)

def version():
    value = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", value):
        raise RuntimeError("Use a stable MAJOR.MINOR.PATCH version in Cargo.toml.")
    return value

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def write_json(path, value):
    path.write_text(json.dumps(value, indent=4) + "\n", encoding="utf-8")

def release_record(tag):
    return api(f"repos/{REPOSITORY}/releases/tags/{tag}", missing_ok=True)

def metadata():
    if os.environ.get("GITHUB_ACTIONS") == "true":
        if os.environ.get("GITHUB_REF") != "refs/heads/main":
            raise RuntimeError("Run Release from the main branch.")
        if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY:
            raise RuntimeError("This publishing workflow is configured for yhwwwwww/rsc.")
        if os.environ.get("BUCKET_KEY_CONFIGURED") != "true":
            raise RuntimeError("SCOOP_BUCKET_DEPLOY_KEY must be configured before publishing.")
    current = version()
    tag = f"v{current}"
    existing = release_record(tag)
    if existing and not existing["draft"]:
        raise RuntimeError(
            f"{tag} is already published. Bump Cargo.toml and Cargo.lock for a new release; "
            "rerun only a failed bucket job to repair bucket publication."
        )
    if existing and existing["target_commitish"] != os.environ["GITHUB_SHA"]:
        raise RuntimeError(f"The existing draft {tag} belongs to a different source commit.")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        output.write(f"version={current}\ntag={tag}\n")
    print(f"Preparing {tag} from {os.environ['GITHUB_SHA']}.")

def pe_imports(path):
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise RuntimeError("Artifact is not a Windows executable.")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        raise RuntimeError("Invalid PE header.")
    machine, count = struct.unpack_from("<HH", data, pe + 4)
    if machine != 0x8664:
        raise RuntimeError("The release must be a Windows x64 executable.")
    size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    if struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise RuntimeError("Expected a PE32+ executable.")
    sections = []
    for index in range(count):
        base = optional + size + 40 * index
        virtual_size, virtual, raw_size, raw = struct.unpack_from("<IIII", data, base + 8)
        sections.append((virtual, max(virtual_size, raw_size), raw))
    def offset(rva):
        for virtual, extent, raw in sections:
            if virtual <= rva < virtual + extent:
                return raw + rva - virtual
        raise RuntimeError(f"Unmapped PE address {rva}.")
    import_rva = struct.unpack_from("<I", data, optional + 112 + 8)[0]
    imports = []
    if import_rva:
        descriptor = offset(import_rva)
        while any(data[descriptor:descriptor + 20]):
            name_rva = struct.unpack_from("<I", data, descriptor + 12)[0]
            start = offset(name_rva)
            end = data.index(b"\0", start)
            imports.append(data[start:end].decode("ascii"))
            descriptor += 20
    unexpected = [
        name for name in imports
        if name.lower() not in SYSTEM_DLLS
        and not name.lower().startswith(("api-ms-win-", "ext-ms-win-"))
    ]
    if unexpected:
        raise RuntimeError(f"Release requires additional runtime DLLs: {unexpected}")
    return sorted(set(imports), key=str.lower)

def prepare():
    output = ROOT / "dist"
    binary = output / "rsc.exe"
    current = version()
    reported = run(str(binary), "--version")
    if reported != f"rsc {current}":
        raise RuntimeError(f"Built version does not match Cargo.toml: {reported}")
    imports = pe_imports(binary)
    checksum = digest(binary)
    tag = f"v{current}"
    source = os.environ.get("GITHUB_SHA") or run("git", "rev-parse", "HEAD", cwd=ROOT)
    manifest = {
        "version": current,
        "description": "A Windows package manager compatible with Scoop",
        "homepage": f"https://github.com/{REPOSITORY}",
        "license": "GPL-3.0-only",
        "architecture": {
            "64bit": {
                "url": f"https://github.com/{REPOSITORY}/releases/download/{tag}/rsc.exe",
                "hash": checksum,
            }
        },
        "bin": "rsc.exe",
        "checkver": {"github": f"https://github.com/{REPOSITORY}"},
        "autoupdate": {
            "architecture": {
                "64bit": {
                    "url": f"https://github.com/{REPOSITORY}/releases/download/v$version/rsc.exe"
                }
            }
        },
    }
    write_json(output / "rsc.json", manifest)
    (output / "SHA256SUMS").write_text(f"{checksum}  rsc.exe\n", encoding="ascii")
    shutil.copy2(ROOT / "LICENSE", output / "LICENSE")
    write_json(output / "build-info.json", {
        "version": current, "tag": tag, "source_commit": source,
        "target": TARGET, "rustc": run("rustc", "--version"),
        "sha256": checksum, "bytes": binary.stat().st_size,
        "imported_dlls": imports, "core_library": "statically linked rlib",
    })
    print(f"Prepared {tag}: {binary.stat().st_size} bytes, SHA256 {checksum}.")

def validate_bundle(folder):
    folder = Path(folder)
    for name in ASSETS:
        if not (folder / name).is_file():
            raise RuntimeError(f"Missing release asset: {name}")
    info = json.loads((folder / "build-info.json").read_text(encoding="utf-8"))
    manifest = json.loads((folder / "rsc.json").read_text(encoding="utf-8"))
    if info["version"] != version() or info["tag"] != f"v{version()}":
        raise RuntimeError("Release version and source version differ.")
    if info["source_commit"] != os.environ["GITHUB_SHA"]:
        raise RuntimeError("Artifact source commit does not match this workflow.")
    checksum = digest(folder / "rsc.exe")
    expected_url = f"https://github.com/{REPOSITORY}/releases/download/{info['tag']}/rsc.exe"
    if (
        checksum != info["sha256"]
        or manifest["version"] != info["version"]
        or manifest["architecture"]["64bit"] != {"url": expected_url, "hash": checksum}
        or (folder / "SHA256SUMS").read_text(encoding="ascii").strip() != f"{checksum}  rsc.exe"
        or manifest["bin"] != "rsc.exe"
        or manifest["license"] != "GPL-3.0-only"
        or (folder / "LICENSE").read_text(encoding="utf-8") != (ROOT / "LICENSE").read_text(encoding="utf-8")
    ):
        raise RuntimeError("Release bundle integrity check failed.")
    return info, manifest

def publish(folder):
    folder = Path(folder)
    info, _ = validate_bundle(folder)
    tag = info["tag"]
    existing = release_record(tag)
    if existing and not existing["draft"]:
        raise RuntimeError(f"Refusing to replace published assets for {tag}.")
    if existing and existing["target_commitish"] != info["source_commit"]:
        raise RuntimeError("Draft release belongs to a different source commit.")
    tag_ref = api(f"repos/{REPOSITORY}/git/ref/tags/{tag}", missing_ok=True)
    tagged = api(f"repos/{REPOSITORY}/commits/{tag}") if tag_ref else None
    if tagged and tagged["sha"] != info["source_commit"]:
        raise RuntimeError(f"{tag} already points to a different source commit.")
    if not existing:
        run(
            "gh", "release", "create", tag, "--repo", REPOSITORY,
            "--target", info["source_commit"], "--title", f"rsc {info['version']}",
            "--draft", "--generate-notes",
        )
    run("gh", "release", "upload", tag, *(str(folder / name) for name in ASSETS),
        "--repo", REPOSITORY, "--clobber")
    run("gh", "release", "edit", tag, "--repo", REPOSITORY, "--draft=false", "--latest")
    print(f"Published https://github.com/{REPOSITORY}/releases/tag/{tag}")

def update_bucket(folder, bucket):
    info, manifest = validate_bundle(folder)
    release = release_record(info["tag"])
    tagged = api(f"repos/{REPOSITORY}/commits/{info['tag']}")
    if not release or release["draft"] or tagged["sha"] != info["source_commit"]:
        raise RuntimeError("The matching release must be published before updating the bucket.")
    with tempfile.TemporaryDirectory(prefix="rsc-published-") as temporary:
        run("gh", "release", "download", info["tag"], "--repo", REPOSITORY,
            "--pattern", "rsc.exe", "--dir", temporary)
        if digest(Path(temporary) / "rsc.exe") != info["sha256"]:
            raise RuntimeError("Published executable differs from the bucket checksum.")
    bucket = Path(bucket).resolve()
    target = bucket / "bucket/rsc.json"
    if target.exists():
        previous = json.loads(target.read_text(encoding="utf-8"))
        old_version = tuple(map(int, previous["version"].split(".")))
        new_version = tuple(map(int, manifest["version"].split(".")))
        if old_version > new_version:
            raise RuntimeError("Refusing to downgrade the bucket.")
        if old_version == new_version and previous["architecture"]["64bit"]["hash"] != info["sha256"]:
            raise RuntimeError("Refusing to replace an existing version with a different binary.")
    target.parent.mkdir(parents=True, exist_ok=True)
    write_json(target, manifest)
    run("git", "config", "user.name", "github-actions[bot]", cwd=bucket)
    run("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com", cwd=bucket)
    run("git", "add", "bucket/rsc.json", cwd=bucket)
    if not run("git", "diff", "--cached", "--name-only", cwd=bucket):
        print("The Scoop bucket already matches this release.")
        return
    run("git", "commit", "-m", f"Update rsc to {info['version']}", cwd=bucket)
    run("git", "push", "origin", "HEAD:main", cwd=bucket)
    print(f"Updated Scoop bucket to rsc {info['version']}.")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("metadata", "prepare", "publish", "bucket"))
    parser.add_argument("--assets", default="release-assets")
    parser.add_argument("--bucket", default="scoop-bucket")
    args = parser.parse_args()
    if args.action == "metadata":
        metadata()
    elif args.action == "prepare":
        prepare()
    elif args.action == "publish":
        publish(args.assets)
    else:
        update_bucket(args.assets, args.bucket)

if __name__ == "__main__":
    main()
