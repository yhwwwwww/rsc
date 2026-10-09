"""Windows integration checks. Uses isolated Scoop roots and restores user environment."""
import argparse, contextlib, hashlib, http.server, json, os, pathlib, re, ctypes
from ctypes import wintypes
import shutil, socket, sqlite3, subprocess, sys, threading, time, traceback, winreg, zipfile, tempfile
sys.stdout.reconfigure(encoding="utf-8")
sys.stderr.reconfigure(encoding="utf-8")

REPO = pathlib.Path(__file__).resolve().parents[1]
RSC = REPO / "dist/rsc.exe"
PROFILE = pathlib.Path(os.environ["USERPROFILE"])
SCOOP = PROFILE / "scoop/apps/scoop/current/bin/scoop.ps1"
POWERSHELL = pathlib.Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
RESULTS = []
HTTP_LOG = []
HTTP_LOCK = threading.Lock()
ACTIVE = PEAK = 0
FAULTS = {}
PAYLOAD = bytes(range(256)) * 32768

def sha(data):
    return hashlib.sha256(data).hexdigest()

def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, default=str), encoding="utf-8")

@contextlib.contextmanager
def preserve_user_environment():
    names = ["PATH", "PSModulePath", "SCOOP_PATH", "RSC_TEST_VALUE"]
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        snapshot = {}
        for name in names:
            try:
                snapshot[name] = winreg.QueryValueEx(key, name)
            except FileNotFoundError:
                snapshot[name] = None
    try:
        yield
    finally:
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
            for name, old in snapshot.items():
                if old is None:
                    try:
                        winreg.DeleteValue(key, name)
                    except FileNotFoundError:
                        pass
                else:
                    winreg.SetValueEx(key, name, 0, old[1], old[0])
        assert user_environment_snapshot(names) == snapshot, "User environment restoration failed"
        sender=ctypes.windll.user32.SendMessageTimeoutW
        sender.argtypes=[wintypes.HWND,wintypes.UINT,wintypes.WPARAM,wintypes.LPARAM,wintypes.UINT,wintypes.UINT,ctypes.POINTER(ctypes.c_size_t)]
        sender.restype=ctypes.c_ssize_t
        name=ctypes.create_unicode_buffer("Environment")
        result=ctypes.c_size_t()
        sender(0xffff,0x1a,0,ctypes.cast(name,ctypes.c_void_p).value,2,1000,ctypes.byref(result))

def user_environment_snapshot(names):
    values = {}
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        for name in names:
            try:
                values[name] = winreg.QueryValueEx(key, name)
            except FileNotFoundError:
                values[name] = None
    return values

class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *args):
        pass
    def handle(self):
        try:
            super().handle()
        except (ConnectionAbortedError, ConnectionResetError, BrokenPipeError):
            pass
    def do_HEAD(self):
        self.transfer(True)
    def do_GET(self):
        self.transfer(False)
    def transfer(self, head):
        global ACTIVE, PEAK
        path = self.path.split("?")[0]
        parts = path.strip("/").split("/", 1)
        mode = parts[0]
        resource = parts[-1]
        data = self.server.files.get(resource)
        if data is None:
            self.send_error(404)
            return
        requested = self.headers.get("Range")
        start, end = 0, len(data) - 1
        ranged = bool(requested and mode != "ignore")
        if ranged:
            match = re.fullmatch(r"bytes=(\d+)-(\d*)", requested)
            if not match:
                self.send_error(416)
                return
            start, end = int(match[1]), int(match[2] or end)
            if start >= len(data) or end >= len(data):
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{len(data)}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
        with HTTP_LOCK:
            count = FAULTS.get((mode, "gets"), 0)
            if not head:
                FAULTS[(mode, "gets")] = count + 1
            HTTP_LOG.append({"mode": mode, "method": self.command, "range": requested,
                             "start": start, "end": end, "time": time.time()})
        if mode == "retry" and not head and end - start > 1 and count < 3:
            self.send_response(503)
            self.send_header("Content-Length", "0")
            self.send_header("Retry-After", "0")
            self.end_headers()
            return
        if mode == "headers" and (self.headers.get("X-Rsc-Test") != "ok" or "fixture=yes" not in self.headers.get("Cookie", "")):
            self.send_error(403)
            return
        self.send_response(206 if ranged else 200)
        self.send_header("Content-Length", str(end - start + 1))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("ETag", '"fixture-v1"')
        self.send_header("Last-Modified", "Thu, 01 Oct 2026 00:00:00 GMT")
        if ranged:
            wrong = start + 1 if mode == "wrong" else start
            self.send_header("Content-Range", f"bytes {wrong}-{end}/{len(data)}")
        self.end_headers()
        if head:
            return
        with HTTP_LOCK:
            ACTIVE += 1
            PEAK = max(PEAK, ACTIVE)
        try:
            block = data[start:end + 1]
            if mode == "disconnect" and end - start > 1:
                with HTTP_LOCK:
                    cut = not FAULTS.get("disconnected")
                    FAULTS["disconnected"] = True
                if cut:
                    self.wfile.write(block[:len(block)//2])
                    self.wfile.flush()
                    self.close_connection = True
                    return
            size = 8192 if mode == "slow" else 65536
            for offset in range(0, len(block), size):
                self.wfile.write(block[offset:offset + size])
                self.wfile.flush()
                time.sleep(0.015 if mode == "slow" else 0.001)
        except (BrokenPipeError, ConnectionResetError, OSError):
            pass
        finally:
            with HTTP_LOCK:
                ACTIVE -= 1

def run_process(args, env, timeout=180, input=None):
    p = subprocess.run([str(a) for a in args], env=env, input=input,
                       capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
    return p

class Suite:
    def __init__(self, root, url):
        self.root, self.url = root, url
        self.env = os.environ.copy()
        self.env = {k:v for k,v in self.env.items() if k.upper() != "PSMODULEPATH"}
        self.env.update(SCOOP=str(root/"user"), SCOOP_GLOBAL=str(root/"global"),
                        SCOOP_CACHE=str(root/"cache"), XDG_CONFIG_HOME=str(root/"config"))
        self.config = root/"config/scoop/config.json"
        write_json(self.config, {"proxy": "none", "aria2-enabled": False,
                   "show_manifest": False, "last_update": "2099-01-01T00:00:00Z"})
        # Original Scoop resolves its shim executable through the isolated root.
        # Copy its read-only resources rather than exposing the live checkout via a junction.
        shutil.copytree(SCOOP.parents[1]/"supporting",
                        root/"user/apps/scoop/current/supporting")
        self.log = root/"commands.log"
        self.index = 0
    def run(self, *args, manager="rsc", expected=0, timeout=180, input=None):
        cmd = [RSC] if manager == "rsc" else [POWERSHELL, "-NoLogo", "-NoProfile",
              "-ExecutionPolicy", "Bypass", "-File", SCOOP]
        p = run_process(cmd + list(args), self.env, timeout, input)
        with self.log.open("a", encoding="utf-8") as f:
            f.write(f"\n{manager} {args!r}\nexit={p.returncode}\n{p.stdout}\n{p.stderr}\n")
        if expected is not None:
            assert p.returncode == expected, f"{manager} {args}: exit {p.returncode}\n{p.stdout}\n{p.stderr}"
        return p
    def manifest(self, name, mode="range", resource="payload.bin", hash_value=None, **extra):
        value = {"version": "1.0.0", "description": "rsc integration fixture",
                 "homepage": "https://github.com/ScoopInstaller/Scoop",
                 "license": "MIT", "url": f"{self.url}/{mode}/{resource}",
                 "hash": hash_value or sha(PAYLOAD)}
        value.update(extra)
        path = self.root/"manifests"/f"{name}.json"
        write_json(path, value)
        return path
    def cached(self, app):
        return list((self.root/"cache").glob(app+"#*"))
    def case(self, name, body):
        started = time.monotonic()
        try:
            detail = body()
            RESULTS.append({"name": name, "passed": True, "seconds": round(time.monotonic()-started, 2), "detail": detail})
            print("PASS", name, flush=True)
        except Exception as e:
            RESULTS.append({"name": name, "passed": False, "seconds": round(time.monotonic()-started, 2), "error": traceback.format_exc()})
            print("FAIL", name, traceback.format_exc()[-1500:], flush=True)
    def assert_cache(self, app, digest=sha(PAYLOAD)):
        files = self.cached(app)
        assert len(files) == 1, f"{app}: {files}"
        assert sha(files[0].read_bytes()) == digest
        return files[0]

def network_checks(s):
    def ranged():
        global PEAK
        PEAK = 0
        p = s.manifest("net-range")
        s.run("download", p)
        f = s.assert_cache("net-range")
        assert PEAK >= 2, f"No parallel transfers observed (peak={PEAK})"
        count = len(HTTP_LOG)
        s.run("download", p)
        assert len(HTTP_LOG) == count, "Validated cache made new HTTP requests"
        s.run("download", p, "--force")
        assert len(HTTP_LOG) > count, "--force reused cached file"
        return {"bytes": f.stat().st_size, "peak_parallel_requests": PEAK}
    s.case("parallel download, SHA256, cache reuse and force", ranged)
    for mode in ("ignore", "wrong", "retry", "disconnect"):
        def check(mode=mode):
            s.run("download", s.manifest("net-"+mode, mode))
            s.assert_cache("net-"+mode)
        s.case("network "+mode, check)
    def mismatch():
        p=s.run("download", s.manifest("net-bad", hash_value="0"*64), expected=1)
        assert "Hash mismatch" in p.stderr,p.stderr
        assert not s.cached("net-bad"), "Invalid hash published to cache"
    s.case("hash mismatch leaves no published cache", mismatch)
    def cookie_headers():
        config = json.loads(s.config.read_text(encoding="utf-8"))
        config["private_hosts"] = [{"match": "^http://127\\.0\\.0\\.1", "headers": "X-Rsc-Test=ok"}]
        write_json(s.config, config)
        try:
            s.run("download", s.manifest("net-headers", "headers", cookie={"fixture": "yes"}))
            s.assert_cache("net-headers")
        finally:
            config.pop("private_hosts")
            write_json(s.config, config)
    s.case("private_hosts and manifest cookies", cookie_headers)
    def interrupted():
        manifest = s.manifest("net-resume", "slow")
        p = subprocess.Popen([str(RSC), "download", str(manifest)], env=s.env,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        deadline = time.monotonic()+30
        while time.monotonic()<deadline:
            parts=list((s.root/"cache/.rsc-downloads").glob("*/part-*"))
            if parts and sum(f.stat().st_size for f in parts)>65536:
                break
            if p.poll() is not None:
                raise AssertionError(p.communicate())
            time.sleep(0.05)
        else:
            p.kill()
            raise AssertionError("No partial range data written")
        p.kill()
        p.communicate(timeout=10)
        cutoff=len(HTTP_LOG)
        s.run("download",manifest)
        s.assert_cache("net-resume")
        resumed=[r for r in HTTP_LOG[cutoff:] if r["mode"]=="slow" and r["range"] and r["end"]-r["start"]>1]
        assert any(r["start"] % (len(PAYLOAD)//4)!=0 for r in resumed), "No resumed offset observed"
        return {"resumed_offsets": [r["start"] for r in resumed]}
    s.case("interrupted segmented download resumes", interrupted)
    def cache_remove():
        s.run("cache","rm","net-.*")
        assert not list((s.root/"cache").glob("net-*#*"))
        s.run("cache","rm","--all")
        assert not any((s.root/"cache/.rsc-downloads").glob("*"))
    s.case("cache regex removal and --all", cache_remove)

def archive(root, version):
    path=root/f"fixture-{version}.zip"
    with zipfile.ZipFile(path,"w",zipfile.ZIP_DEFLATED) as z:
        z.write(REPO/".test-lab/fixture.exe","fixture.exe")
        z.writestr("data/settings.txt","initial")
        z.writestr("version.txt",version)
        z.writestr("RscFixture.psm1","function Get-RscFixture { 'fixture-module' }; Export-ModuleMember Get-RscFixture")
    return path.read_bytes()

def fixture_manifest(s, version, integration=True):
    m={"version":version,"description":"rsc isolated lifecycle fixture","license":"MIT",
       "homepage":"https://github.com/ScoopInstaller/Scoop",
       "url":f"{s.url}/range/fixture-{version}.zip","hash":sha(s.server.files[f"fixture-{version}.zip"]),
       "bin":[["fixture.exe","rsc-test-echo",["--fixed",'"two words"']]],
       "persist":"data",
       "pre_install":"Set-Content -LiteralPath \"$dir\\pre.txt\" -Value pre -Encoding UTF8",
       "installer":{"script":"Set-Content -LiteralPath \"$dir\\installer.txt\" -Value installer -Encoding UTF8"},
       "post_install":"Set-Content -LiteralPath \"$dir\\post.txt\" -Value post -Encoding UTF8",
       "pre_uninstall":"Set-Content -LiteralPath \"$persist_dir\\pre-uninstall.txt\" -Value pre -Encoding UTF8",
       "uninstaller":{"script":"Set-Content -LiteralPath \"$persist_dir\\uninstaller.txt\" -Value uninstall -Encoding UTF8"},
       "post_uninstall":"Set-Content -LiteralPath \"$persist_dir\\post-uninstall.txt\" -Value post -Encoding UTF8"}
    if integration:
        m.update(env_set={"RSC_TEST_VALUE":"$dir"}, env_add_path=".",
                 shortcuts=[["fixture.exe","rsc isolated test fixture"]],
                 psmodule={"name":"RscFixture"})
    return m

def git_run(s,path,*args):
    p=run_process(["git","-C",path,*args],s.env,60)
    assert p.returncode==0,p.stderr
    return p.stdout

def lifecycle_checks(s):
    seed=s.root/"bucket-seed"
    seed.mkdir()
    git_run(s,seed,"init","-b","main")
    git_run(s,seed,"config","user.name","rsc test")
    git_run(s,seed,"config","user.email","rsc-test@example.invalid")
    manifest=seed/"bucket/rsc-fixture.json"
    write_json(manifest,fixture_manifest(s,"1.0.0"))
    git_run(s,seed,"add",".")
    git_run(s,seed,"commit","-m","fixture version one")
    bare=s.root/"remote.git"
    p=run_process(["git","clone","--bare",seed,bare],s.env,60)
    assert p.returncode==0,p.stderr
    git_run(s,seed,"remote","add","origin",str(bare))
    s.case("bucket add from Git",lambda:s.run("bucket","add","rsc-test",bare))
    app=s.root/"user/apps/rsc-fixture"
    persist=s.root/"user/persist/rsc-fixture"
    shim=s.root/"user/shims/rsc-test-echo.exe"
    def install():
        s.run("install","rsc-test/rsc-fixture")
        for f in ("pre.txt","installer.txt","post.txt","scoop-manifest.json","scoop-install.json"):
            assert (app/"current"/f).is_file(),f
        assert (app/"current/data/settings.txt").read_text(encoding="utf-8-sig")=="initial"
        (persist/"data/settings.txt").write_text("user data",encoding="utf-8")
        assert (app/"current/data/settings.txt").read_text(encoding="utf-8-sig")=="user data"
        assert (s.root/"user/modules/RscFixture").is_dir()
        assert user_environment_snapshot(["RSC_TEST_VALUE"])["RSC_TEST_VALUE"] is not None
        p=s.run("list","^rsc-fixture$",manager="scoop")
        assert "rsc-fixture" in p.stdout
    s.case("rsc install: hooks, persist, env, shortcuts, modules; Scoop list",install)
    def shim_arguments():
        args=["plain","two words",'embedded"quote',"",r"trailing\\","中文"]
        p=run_process([shim,*args],s.env)
        assert p.returncode==0,p.stderr
        assert json.loads(p.stdout)==["--fixed","two words",*args],p.stdout
        p=run_process([shim,"--stdin"],s.env,input="stdin fixture\n")
        assert p.stdout.endswith("stdin fixture\n"),p.stdout
        p=run_process([shim,"--exit-42"],s.env)
        assert p.returncode==42,p.returncode
    s.case("exe shim: fixed args, spaces, quotes, empty, Unicode, stdin, exit 42",shim_arguments)
    def queried():
        s.run("info","rsc-fixture","--verbose")
        s.run("cat","rsc-fixture")
        assert "1.0.0" in s.run("list","^rsc-fixture$").stdout
        assert "fixture.exe" in s.run("which","rsc-test-echo").stdout
        s.run("prefix","rsc-fixture")
        s.run("depends","rsc-fixture")
        s.run("status","--local")
        s.run("alias","add","rsc-test-list","scoop list $args","fixture alias")
        assert "rsc-test-list" in s.run("alias","list","-v").stdout
        assert "rsc-fixture" in s.run("rsc-test-list","^rsc-fixture$").stdout
        s.run("alias","rm","rsc-test-list")
        assert "rsc-test-echo" in s.run("shim","list").stdout
        s.run("shim","info","rsc-test-echo")
    s.case("query commands, custom aliases and shim inspection",queried)
    def sqlite():
        s.run("config","use_sqlite_cache","true")
        assert "rsc-fixture" in s.run("search","rsc-test-echo").stdout
        with sqlite3.connect(s.root/"user/scoop.db") as db:
            assert db.execute("SELECT version,binary FROM app WHERE name='rsc-fixture'").fetchone()==("1.0.0","rsc-test-echo")
    s.case("SQLite initialization and binary search",sqlite)
    def updated():
        s.run("hold","rsc-fixture")
        assert json.loads((app/"current/scoop-install.json").read_text(encoding="utf-8-sig"))["hold"] is True
        write_json(manifest,fixture_manifest(s,"2.0.0"))
        git_run(s,seed,"add",".");git_run(s,seed,"commit","-m","fixture version two")
        git_run(s,seed,"push","origin","main")
        s.run("update","rsc-fixture")
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="1.0.0"
        s.run("unhold","rsc-fixture")
        s.run("update","rsc-fixture")
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="2.0.0"
        assert (persist/"data/settings.txt").read_text(encoding="utf-8-sig")=="user data"
        assert (app/"1.0.0").is_dir()
        s.run("reset","rsc-fixture@1.0.0")
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="1.0.0"
        s.run("reset","rsc-fixture@2.0.0")
        s.run("update","rsc-fixture","--force","--no-cache")
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="2.0.0"
        s.run("cleanup","rsc-fixture","--cache")
        assert not (app/"1.0.0").exists()
        with sqlite3.connect(s.root/"user/scoop.db") as db:
            assert db.execute("SELECT COUNT(*) FROM app WHERE name='rsc-fixture'").fetchone()[0]==2
    s.case("hold, Git update, preserve data, reset, force no-cache, cleanup, SQL history",updated)
    def scoop_uninstall():
        s.run("uninstall","rsc-fixture",manager="scoop")
        assert not app.exists()
        assert (persist/"data/settings.txt").read_text(encoding="utf-8-sig")=="user data"
        assert (persist/"uninstaller.txt").is_file()
        assert not shim.exists()
        assert not (s.root/"user/modules/RscFixture").exists()
    s.case("Scoop uninstalls rsc-installed package and preserves persist",scoop_uninstall)
    def reverse():
        s.run("install","rsc-test/rsc-fixture","--no-update-scoop",manager="scoop")
        assert "rsc-fixture" in s.run("list").stdout
        assert shim.is_file(),"Original Scoop did not create its executable shim"
        assert run_process([shim,"original Scoop"],s.env).returncode==0
        s.run("reset","rsc-fixture")
        s.run("uninstall","rsc-fixture","--purge")
        assert not app.exists() and not persist.exists()
    s.case("Scoop install -> rsc reset/uninstall --purge",reverse)

    def historical():
        s.run("install","rsc-test/rsc-fixture@1.0.0")
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="1.0.0"
        assert str(s.root/"user/workspace") in json.loads((app/"current/scoop-install.json").read_text(encoding="utf-8-sig"))["url"]
        s.run("uninstall","rsc-fixture","--purge")
        s.run("config","use_sqlite_cache","false")
        s.run("download","rsc-test/rsc-fixture@1.0.0")
    s.case("pinned version from SQLite and Git history",historical)
    def no_junction():
        s.run("config","no_junction","true")
        try:
            s.run("install","rsc-test/rsc-fixture")
            assert not (app/"current").exists()
            assert "2.0.0" in s.run("prefix","rsc-fixture").stdout
            assert "rsc-fixture" in s.run("list",manager="scoop").stdout
            s.run("reset","rsc-fixture",manager="scoop")
            s.run("uninstall","rsc-fixture","--purge")
        finally:
            s.run("config","no_junction","false")
    s.case("no_junction installation and Scoop reset",no_junction)
    def failed_retry():
        m=fixture_manifest(s,"1.0.0",False)
        for key in ("persist","pre_uninstall","uninstaller","post_uninstall","post_install","installer"):
            m.pop(key,None)
        m["pre_install"]='throw "intentional fixture failure"'
        path=s.root/"manifests/rsc-failure.json"
        write_json(path,m)
        s.run("install",path,expected=1)
        failed_app=s.root/"user/apps/rsc-failure"
        assert (failed_app/"1.0.0/.rsc-installing.json").is_file()
        assert "broken" in s.run("list","^rsc-failure$").stdout
        m["pre_install"]="Set-Content \"$dir\\pre.txt\" repaired"
        write_json(path,m)
        s.run("install",path)
        assert (failed_app/"current/pre.txt").is_file()
        assert not (failed_app/"current/.rsc-installing.json").exists()
        s.run("uninstall","rsc-failure","--purge")
    s.case("failed hook -> marked broken -> retry repairs installation",failed_retry)
    def scoop_partial():
        m={"version":"1.0.0","url":f"{s.url}/range/fixture.exe",
           "hash":sha(s.server.files["fixture.exe"]),"bin":[["fixture.exe","rsc-test-partial"]]}
        path=s.root/"manifests/rsc-partial.json"
        write_json(path,m)
        partial=s.root/"user/apps/rsc-partial"
        (partial/"0.0.1").mkdir(parents=True)
        (partial/"0.0.1/orphan.txt").write_text("Scoop failed before metadata")
        assert "broken" in s.run("list","^rsc-partial$").stdout
        s.run("install",path)
        assert (partial/"current/scoop-install.json").is_file()
        assert not (partial/"0.0.1").exists()
        s.run("uninstall","rsc-partial")
        partial.mkdir()
        s.run("uninstall","rsc-partial")
        assert not partial.exists()
    s.case("repair Scoop partial/empty installation without metadata",scoop_partial)
    def dependency_checks():
        bucket=s.root/"user/buckets/rsc-test/bucket"
        exe=s.server.files["fixture.exe"]
        dep={"version":"1.0.0","url":f"{s.url}/range/fixture.exe","hash":sha(exe),
             "bin":[["fixture.exe","rsc-test-dep"]]}
        parent={"version":"1.0.0","depends":"rsc-test/rsc-dep",
                "url":f"{s.url}/range/fixture.exe","hash":sha(exe),
                "installer":{"script":"Set-Content \"$dir\\installed.txt\" dependency"}}
        write_json(bucket/"rsc-dep.json",dep)
        write_json(bucket/"rsc-parent.json",parent)
        s.run("install","rsc-test/rsc-parent")
        assert (s.root/"user/apps/rsc-parent/current/installed.txt").is_file()
        assert (s.root/"user/apps/rsc-dep").is_dir()
        s.run("uninstall","rsc-parent","rsc-dep")
        s.run("install","rsc-test/rsc-parent","--independent")
        assert not (s.root/"user/apps/rsc-dep").exists()
        s.run("uninstall","rsc-parent")
        write_json(bucket/"rsc-cycle-a.json",{**dep,"version":"1","depends":"rsc-test/rsc-cycle-b"})
        write_json(bucket/"rsc-cycle-b.json",{**dep,"version":"1","depends":"rsc-test/rsc-cycle-a"})
        p=s.run("install","rsc-test/rsc-cycle-a",expected=1)
        assert "dependency cycle" in p.stderr.lower(),p.stderr
    s.case("dependencies, script installer, independent and cycle detection",dependency_checks)
    def scoopfile():
        s.run("install","rsc-test/rsc-fixture")
        s.run("hold","rsc-fixture")
        exported=s.run("export","--config").stdout
        data=json.loads(exported)
        assert data["apps"][0]["Name"]=="rsc-fixture"
        path=s.root/"scoopfile.json"
        path.write_text(exported,encoding="utf-8")
        s.run("uninstall","rsc-fixture","--purge")
        s.run("import",path)
        assert json.loads((app/"current/scoop-install.json").read_text(encoding="utf-8-sig"))["hold"] is True
        s.run("uninstall","rsc-fixture","--purge")
        s.run("import",path,manager="scoop")
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="2.0.0"
        scoop_export=s.run("export","--config",manager="scoop").stdout
        scoop_data=json.loads(scoop_export)
        assert scoop_data["apps"][0]["Name"]=="rsc-fixture"
        path.write_text(scoop_export,encoding="utf-8")
        s.run("uninstall","rsc-fixture","--purge")
        s.run("import",path)
        assert (app/"current/version.txt").read_text(encoding="utf-8-sig")=="2.0.0"
        s.run("uninstall","rsc-fixture","--purge")
    s.case("Scoopfile export/import in both directions",scoopfile)
    def db_delete():
        s.run("config","use_sqlite_cache","true")
        manifest.unlink()
        git_run(s,seed,"add","-A");git_run(s,seed,"commit","-m","remove fixture")
        git_run(s,seed,"push","origin","main")
        s.run("update")
        with sqlite3.connect(s.root/"user/scoop.db") as db:
            assert db.execute("SELECT COUNT(*) FROM app WHERE name='rsc-fixture'").fetchone()[0]==0
        s.run("bucket","rm","rsc-test")
        assert not (s.root/"user/buckets/rsc-test").exists()
        s.run("config","use_sqlite_cache","false")
    s.case("SQLite deleted manifest cleanup and bucket removal",db_delete)

def real_package_checks(s):
    if not (s.root/"user/apps/scoop/current/supporting").exists():
        shutil.copytree(SCOOP.parents[1]/"supporting",s.root/"user/apps/scoop/current/supporting")
    manifests=PROFILE/"scoop/buckets/main/bucket"
    def jq():
        source=manifests/"jq.json"
        expected=json.loads(source.read_text(encoding="utf-8-sig"))["version"]
        s.run("install",source)
        p=run_process([s.root/"user/shims/jq.exe","--version"],s.env)
        assert p.returncode==0 and expected in p.stdout,p.stdout+p.stderr
        assert "jq" in s.run("list",manager="scoop").stdout
        s.run("uninstall","jq",manager="scoop")
        assert not (s.root/"user/apps/jq").exists()
        return {"package":"jq","version":expected,"source":str(source)}
    s.case("real jq: GitHub download, rsc install/execute -> Scoop uninstall",jq)
    def ripgrep():
        source=manifests/"ripgrep.json"
        expected=json.loads(source.read_text(encoding="utf-8-sig"))["version"]
        s.run("install",source,"--no-update-scoop",manager="scoop",timeout=240)
        assert "ripgrep" in s.run("list").stdout
        p=run_process([s.root/"user/shims/rg.exe","--version"],s.env)
        assert p.returncode==0 and expected in p.stdout,p.stdout+p.stderr
        s.run("reset","ripgrep")
        p=run_process([s.root/"user/shims/rg.exe","--version"],s.env)
        assert p.returncode==0 and expected in p.stdout,p.stdout+p.stderr
        s.run("uninstall","ripgrep")
        assert not (s.root/"user/apps/ripgrep").exists()
        return {"package":"ripgrep","version":expected,"source":str(source)}
    s.case("real ripgrep ZIP: Scoop install -> rsc reset/execute/uninstall",ripgrep)

def native_checks(s):
    # Remove reference-manager resources: this phase must run independently.
    shutil.rmtree(s.root/"user/apps/scoop")
    def multi_archive():
        first=s.root/"multi-first.zip"
        second=s.root/"multi-second.zip"
        with zipfile.ZipFile(first,"w") as z:
            z.writestr("nested/a.txt","first")
        with zipfile.ZipFile(second,"w") as z:
            z.writestr("b.txt","second")
        s.server.files["multi-first.zip"]=first.read_bytes()
        s.server.files["multi-second.zip"]=second.read_bytes()
        m=s.manifest("rsc-multi",resource="multi-first.zip",hash_value=sha(first.read_bytes()),
            url=[f"{s.url}/range/multi-first.zip",f"{s.url}/range/multi-second.zip"],
            hash=[sha(first.read_bytes()),sha(second.read_bytes())],
            extract_dir=["nested",""],extract_to=["first","second"])
        s.run("install",m)
        app=s.root/"user/apps/rsc-multi/current"
        assert (app/"first/a.txt").read_text()=="first"
        assert (app/"second/b.txt").read_text()=="second"
        s.run("uninstall","rsc-multi")
    s.case("native without Scoop: multiple archives, extract_dir and extract_to",multi_archive)
    def file_persist():
        data=b"default settings"
        s.server.files["default.ini"]=data
        m=s.manifest("rsc-persist-file",resource="default.ini",hash_value=sha(data),
                     persist=[["default.ini","settings.ini"]])
        s.run("install",m)
        persistent=s.root/"user/persist/rsc-persist-file/settings.ini"
        persistent.write_text("user settings")
        assert (s.root/"user/apps/rsc-persist-file/current/default.ini").read_text()=="user settings"
        s.run("uninstall","rsc-persist-file")
        assert persistent.read_text()=="user settings"
        s.run("install",m)
        assert (s.root/"user/apps/rsc-persist-file/current/default.ini").read_text()=="user settings"
        s.run("uninstall","rsc-persist-file","--purge")
        assert not persistent.exists()
    s.case("native file persist: hardlink, rename, reinstall and purge",file_persist)
    def script_shim():
        script=b'[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); if($args -contains "--exit-42"){exit 42}; ConvertTo-Json -Compress -InputObject @($args)'
        s.server.files["echo.ps1"]=script
        m=s.manifest("rsc-script",resource="echo.ps1",hash_value=sha(script),
            bin=[["echo.ps1","rsc-test-script","--fixed"]],
            pre_install='$p=Get-HelperPath git; if(-not (Test-Path -LiteralPath $p)){throw "helper path missing"}; Set-Content "$dir/helper.txt" $p')
        s.run("install",m)
        launcher=s.root/"user/shims/rsc-test-script.exe"
        result=run_process([launcher,"two words","中文"],s.env)
        assert result.returncode==0,result.stdout+result.stderr
        assert json.loads(result.stdout)==["--fixed","two words","中文"],result.stdout
        result=run_process([launcher,"--exit-42"],s.env)
        assert result.returncode==42,result.stdout+result.stderr
        s.run("uninstall","rsc-script")
    s.case("native PowerShell shim and helper forwarding, arguments and exit 42",script_shim)
    def gui_shim():
        binary=(REPO/".test-lab/gui.exe").read_bytes()
        s.server.files["gui.exe"]=binary
        m=s.manifest("rsc-gui",resource="gui.exe",hash_value=sha(binary),bin="gui.exe")
        s.run("install",m)
        launcher=s.root/"user/shims/gui.exe"
        data=launcher.read_bytes()
        offset=int.from_bytes(data[60:64],"little")
        assert int.from_bytes(data[offset+92:offset+94],"little")==2
        output=s.root/"gui-result.txt"
        result=run_process([launcher,output],s.env)
        assert result.returncode==0,result.stderr
        assert output.read_text()=="native gui shim executed"
        s.run("uninstall","rsc-gui")
    s.case("native GUI shim subsystem and execution",gui_shim)
    def metalink():
        data=PAYLOAD
        xml=f'<?xml version="1.0"?><metalink xmlns="urn:ietf:params:xml:ns:metalink"><file name="payload.bin"><hash type="sha-256">{sha(data)}</hash><url>{s.url}/range/payload.bin</url></file></metalink>'.encode()
        s.server.files["payload.meta4"]=xml
        m=s.manifest("rsc-metalink",resource="payload.meta4",hash_value=sha(data))
        s.run("download",m)
        s.assert_cache("rsc-metalink")
    s.case("native Metalink follows resource and validates payload hash",metalink)
    def local_file():
        source=s.root/"本地 package.exe"
        source.write_bytes(s.server.files["fixture.exe"])
        m=s.manifest("rsc-local-file",url=source.as_uri(),hash=sha(source.read_bytes()),
                     bin=[["本地 package.exe","rsc-test-local-file"]])
        s.run("install",m)
        p=run_process([s.root/"user/shims/rsc-test-local-file.exe","local file"],s.env)
        assert p.returncode==0,p.stdout+p.stderr
        assert json.loads(p.stdout)==["local file"]
        s.run("uninstall","rsc-local-file")
    s.case("native local file URL: Unicode path, hash, install and execution",local_file)



def query_checks(s):
    def check():
        bucket = s.root/"user/buckets/rsc-query-fixture/bucket"
        value = {"version":"1.0","description":"中文 query fixture",
                 "bin":[["tool.exe","rsc-query-alias"]],
                 "architecture":{"arm64":{"bin":"rsc-query-arm.exe"}},
                 "extra":{"bool":True,"null":None,"number":-12.5,"escaped":chr(34)+" and 中文"}}
        manifest = bucket/"rsc-query-tool.json"
        write_json(manifest,value)
        for query in ("^rsc-query-tool$", "^rsc-query-alias$", "^rsc-query-arm$"):
            result = s.run("search", query)
            assert "rsc-query-tool" in result.stdout, result.stdout
            assert "\x1b[" not in result.stdout, "ANSI escaped into redirected search output"
        for no_color in (False,True):
            env=s.env.copy()
            if no_color: env["NO_COLOR"]="1"
            else: env.pop("NO_COLOR",None)
            result=run_process([RSC,"cat",manifest],env)
            assert result.returncode==0,result.stderr
            assert "\x1b[" not in result.stdout,"ANSI escaped into redirected JSON"
            assert json.loads(result.stdout)==value
        value["version"]="2.0"
        write_json(manifest,value)
        assert "2.0" in s.run("search","^rsc-query-tool$").stdout
        s.run("search","(",expected=1)
        # The query fixture has no registry, shim, or package installation side effects.
        return {"redirected_json_valid":True,"no_color_plain":True,
                "case_alias_architecture_search":True,"edits_visible_immediately":True}
    s.case("native query output: valid JSON, NO_COLOR, aliases and current metadata",check)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--phase",choices=["network","lifecycle","real","native","all"],default="all")
    args=parser.parse_args()
    root=pathlib.Path(tempfile.mkdtemp(prefix="rsc-native-tests-"))/"隔离 Scoop"
    root.mkdir(parents=True)
    files={"payload.bin":PAYLOAD,"fixture.exe":(REPO/".test-lab/fixture.exe").read_bytes()}
    for version in ("1.0.0","2.0.0"):
        files[f"fixture-{version}.zip"]=archive(root,version)
    server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Handler)
    server.daemon_threads=True
    server.files=files
    thread=threading.Thread(target=server.serve_forever,daemon=True)
    thread.start()
    s=Suite(root,f"http://127.0.0.1:{server.server_port}")
    s.server=server
    config=PROFILE/".config/scoop/config.json"
    config_hash=sha(config.read_bytes())
    print("Test root:",root,flush=True)
    try:
        with preserve_user_environment():
            if args.phase in ("network","all"):
                network_checks(s)
            if args.phase in ("lifecycle","all"):
                lifecycle_checks(s)
            if args.phase in ("native","all"):
                native_checks(s)
                query_checks(s)
            if args.phase in ("real","all"):
                real_package_checks(s)
        assert sha(config.read_bytes())==config_hash,"Real Scoop config changed"
    except Exception as e:
        RESULTS.append({"name":"suite setup/cleanup","passed":False,"error":traceback.format_exc()})
        print("FAIL suite setup/cleanup",str(e),flush=True)
    finally:
        server.shutdown()
        write_json(root/"results.json",{"cases":RESULTS,"http_requests":HTTP_LOG,
                   "passed":sum(r["passed"] for r in RESULTS),"failed":sum(not r["passed"] for r in RESULTS),
                   "real_scoop_config_unchanged":sha(config.read_bytes())==config_hash})
        write_json(REPO/".test-lab/latest.json",{"root":str(root)})
    print(json.dumps({"passed":sum(r["passed"] for r in RESULTS),"failed":sum(not r["passed"] for r in RESULTS),
                      "results":str(root/"results.json")},ensure_ascii=False),flush=True)
    raise SystemExit(any(not r["passed"] for r in RESULTS))

if __name__=="__main__":
    main()
