"""Read-only differential checks against the separately installed Scoop."""
import argparse, json, os, pathlib, re, shutil, subprocess, tempfile

REPO = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--rsc", type=pathlib.Path, default=REPO / "dist/rsc.exe")
parser.add_argument("--output", type=pathlib.Path, default=REPO / ".test-lab/search-scoop.json")
parser.add_argument("queries", nargs="*", default=["git", "^git", "^git$", "gitignore", "git-biodiff", "jq", "^jq$"])
args = parser.parse_args()
reference = pathlib.Path(os.environ["USERPROFILE"]) / "scoop/apps/scoop/current/bin/scoop.ps1"
shell = shutil.which("pwsh") or shutil.which("powershell")
env = dict(os.environ, NO_COLOR="1", RSC_SCOOP_REFERENCE=str(reference))
script = r'''
param([string]$Query)
$ErrorActionPreference = 'Stop'
$items = @(& $env:RSC_SCOOP_REFERENCE search $Query 6>$null)
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$result = @($items | ForEach-Object {
    [ordered]@{ package=[string]$_.Name; version=[string]$_.Version;
        bucket=[string]$_.Source; binaries=[string]$_.Binaries }
})
ConvertTo-Json -InputObject $result -Compress -Depth 5
'''
def run(command):
    p = subprocess.run([str(x) for x in command],env=env,capture_output=True,text=True,
                       encoding="utf-8",errors="replace",timeout=90)
    if p.returncode: raise RuntimeError(f"{command}: {p.stdout}\n{p.stderr}")
    return p.stdout
def parse_rsc(text):
    lines=text.splitlines()
    i=next(i for i,line in enumerate(lines) if line.startswith("Package "))
    header=lines[i]
    fields=[("package","Package"),("version","Version"),("bucket","Bucket"),
            ("installed","Installed"),("state","State"),("binaries","Binaries")]
    positions=[header.index(label) for _,label in fields]+[None]
    rows=[]
    for line in lines[i+1:]:
        if not line.strip(): continue
        row={key:line[positions[j]:positions[j+1]].strip() for j,(key,_) in enumerate(fields)}
        rows.append(row)
    return rows
checks=[]
with tempfile.TemporaryDirectory(prefix="rsc-search-reference-") as temp:
    adapter=pathlib.Path(temp)/"reference.ps1"
    adapter.write_text(script,encoding="utf-8-sig")
    for query in args.queries:
        expected=json.loads(run([shell,"-NoLogo","-NoProfile","-NonInteractive",
                                 "-ExecutionPolicy","Bypass","-File",adapter,query]))
        rows=parse_rsc(run([args.rsc,"search",query]))
        actual=[{key:row[key] for key in ("package","version","bucket","binaries")} for row in rows]
        assert actual==expected,{"query":query,"rsc":actual,"scoop":expected}
        checks.append({"query":query,"matches":len(actual),"same_rows_order_and_binaries":True})
        print(json.dumps(checks[-1]),flush=True)
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps({"checks":checks,"passed":len(checks),"failed":0},indent=2)+"\n",encoding="utf-8")
