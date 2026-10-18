"""Input-replay sync against Ryujinx: the same scripted session in the SMM2 recomp and in the Ryujinx trace build,
compared update for update.

    replay_sync.py build   COURSE [--pre TRACE]   make the session (WORK/sessions/COURSE.pad + .json)
    replay_sync.py recomp  COURSE [--tag T]       play it in the recomp   -> WORK/out/recomp/COURSE.bin.gz
    replay_sync.py ryujinx COURSE [--tag T]       play it in Ryujinx      -> WORK/out/ryujinx/COURSE.bin.gz
    replay_sync.py cmp     COURSE [A B]           per-test table (+ whole player object, pointer-aware)
    replay_sync.py prep    COURSE...              build, size the resets from a recomp pre-run, rebuild, recomp run

COURSE is a test course made by SMCraft's `mmk trace-courses` (<style>_<kind>: M1_flat, MW_wall, 3W_items, ...),
installed as all ten title courses (recomp: RECOMP_MOD_DIR; Ryujinx: LayeredFS), or `stock_<name>` for the game's own
title course with a hand-written session (WORK/sessions/stock_<name>.pad + .json).

The Ryujinx side is the trace build in research/tools/ryujinx-trace (RYU_TRACE_DEV=<its working folder>, holding
tools/ryu.py, scripts/*.txt, build/, root/): it patches the player's per-frame update (main+0x158a5c4) into an SVC,
records the player object there and applies one script frame per update. The recomp does the same at the same
instruction (games/smm2/trace.c, RECOMP_PAD_SCRIPT / RECOMP_TRACE_OUT). Both start at the first update of the
title-course player and play one script: settle, [grab the power-up], then per test a reset (run left into the
course's edge, settle, walk right to about x = 100, settle) and the test's frames. Record k is written at update k
before script frame k applies (a frame shows from record k + 2 on) in both runtimes, so record k of one is compared
with record k of the other.

SEEDALL=1 seeds every sead::Random with 4 in both (recomp SMM2_SEED_ALL=1; Ryujinx: an IPS mod turning the tick read in
sead::Random::init() into the constant) for courses whose enemies draw random numbers.

Environment: RYU_TRACE_DEV (Ryujinx tooling), RYU_LOCK (a directory taken while Ryujinx runs), WORK (default
recomp/private/smm2/replay), MMK (an `mmk` that still has `trace-courses`: SMCraft's full CLI, built from the
`smcraft-archive` branch with `cargo build --release -p mmk-cli`; main's cut-down `mmk` has only the server commands;
default core/target/release/mmk), SMM2_COURSE_TABLE (the course encryption table
`mmk trace-courses --table` needs). Traces are game memory: they stay in WORK (git-ignored), never in the repository.
"""
import gzip
import json
import os
import shutil
import struct
import subprocess
import sys
import time
from collections import Counter

RECOMP = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(RECOMP)
WORK = os.environ.get("WORK", os.path.join(RECOMP, "private", "smm2", "replay"))
RYU = os.environ.get("RYU_TRACE_DEV", "")
LOCK = os.environ.get("RYU_LOCK", "")
MMK = os.environ.get("MMK", os.path.join(REPO, "core", "target", "release", "mmk"))
TABLE = os.environ.get("SMM2_COURSE_TABLE", os.path.expanduser("~/smm-work/cache/smm2-3.0.3/keys/course_table.bin"))
SEEDALL = os.environ.get("SEEDALL") == "1"
TID = "01009b90006dc000"
BUILDID = "C2DC405AC414C37C8B1C50219C7A0F0C"  # main, v3.0.3
SEED_IPS = b"IPS32" + struct.pack(">IH", 0x29a060 + 0x100, 4) + struct.pack("<I", 0xD2800080) + b"EEOF"
PAD = {"JUMP": "A", "RUN": "Y", "SPIN": "ZR"}
RYU_MAIN = 0x8504000  # where Ryujinx loads main (the trace build's status file says so)
MAIN_SIZE = 0x2e73000


def path(*p):
    return os.path.join(WORK, *p)


# ---------------------------------------------------------------- records (Ryujinx TraceHook format)
def records(p):
    data = gzip.open(p, "rb").read() if p.endswith(".gz") else open(p, "rb").read()
    o, out = 0, []
    while o + 24 <= len(data):
        magic, idx, this, inp, ln = struct.unpack_from("<IiQIi", data, o)
        assert magic == 0x31435254, hex(o)
        o += 24
        if o + ln + 4 > len(data):
            break  # (a recording stopped mid-record)
        body = data[o:o + ln]
        o += ln
        nd, = struct.unpack_from("<i", data, o)
        o += 4
        der = []
        for _ in range(nd):
            if o + 12 > len(data):
                break
            ptr, dl = struct.unpack_from("<Qi", data, o)
            o += 12
            der.append((ptr, data[o:o + dl]))
            o += dl
        if o > len(data) or len(der) < nd:
            break
        out.append(dict(idx=idx, this=this, inp=inp, body=body, der=der))
    return out


def f32(b, off):
    return struct.unpack_from("<f", b, off)[0]


# player fields (v3.0.3)
F = [("x", 0x230, "f"), ("y", 0x234, "f"), ("vx", 0x23c, "f"), ("vy", 0x240, "f"), ("facing", 0x26c, "I"),
     ("speedF", 0x274, "f"), ("state", 0x3f8, "I"), ("frames", 0x3fc, "I")]


def fields(b):
    return tuple(struct.unpack_from("<" + t, b, o)[0] for _, o, t in F)


def key(b):
    return b[0x230:0x244] + b[0x26c:0x270] + b[0x274:0x278] + b[0x3f8:0x400]


def canon(v, ryu):
    """A qword as something comparable across the two address spaces (pointers by what they point into)."""
    main = RYU_MAIN if ryu else 0x7000000000
    if v == 0:
        return 0
    if main <= v < main + MAIN_SIZE:
        return ("main", v - main)
    if ryu and 0x8000000 <= v < 0x8000000000:
        return "ptr"
    if not ryu and 0x7000000000 <= v < 0x7320000000:
        return "ptr"
    return v


def body_diff(a, b):
    out = []
    for o in range(0, min(len(a), len(b)) - 7, 8):
        if a[o:o + 8] != b[o:o + 8]:
            x, y = struct.unpack_from("<Q", a, o)[0], struct.unpack_from("<Q", b, o)[0]
            if canon(x, True) != canon(y, False):
                out.append(o)
    return out


# ---------------------------------------------------------------- sessions
def read_script(p):
    out = []
    for raw in open(p):
        line = raw.split("#")[0].strip()
        if line:
            t = line.split()
            out.append((int(t[0]), [PAD.get(x.upper(), x.upper()) for x in t[1:] if x != "-"]))
    return out


def tests_for(course):
    kind = course.split("_", 1)[1]
    kind = "flat" if kind == "items" else kind
    sd = os.path.join(RYU, "scripts")
    out = []
    for f in sorted(os.listdir(sd)):
        if f.endswith(".txt") and not f.startswith("_"):
            head = open(os.path.join(sd, f)).readline()
            if head.startswith("#") and "courses:" in head and kind in head.split("courses:")[1].split():
                out.append(f[:-4])
    # a ledge test can end in the pit (a death: the course restarts with a new player object): last
    return [t for t in out if not t.startswith("ledge")] + [t for t in out if t.startswith("ledge")]


def build(course, pre=None, default_k=300):
    ks = {}
    if pre:
        r = records(pre)
        for seg in json.load(open(path("sessions", course + ".json")))["segments"]:
            if seg["kind"] == "reset":
                s0, hit = seg["start"], None
                for j in range(seg["k"]):
                    if s0 + j + 3 >= len(r):
                        break
                    a, b = f32(r[s0 + j + 2]["body"], 0x230), f32(r[s0 + j + 3]["body"], 0x230)
                    if a == b and a < 40:
                        hit = j
                        break
                ks[seg["test"]] = hit + 20 if hit is not None else seg["k"] * 2
    lines, segs, n = [], [], 0

    def add(cnt, btn):
        nonlocal n
        lines.append("%d %s" % (cnt, " ".join(btn) if btn else "-"))
        n += cnt

    add(60, [])
    if course.endswith("_items") or course.endswith("_form"):  # grab the power-up first; every test then has it
        s = n
        add(150, ["RIGHT"])
        add(90, [])
        segs.append(dict(kind="grab", test="_grab", start=s, len=n - s))
    tests = os.environ["TESTS"].split(",") if os.environ.get("TESTS") else tests_for(course)
    for t in tests:
        k, s = ks.get(t, default_k), n
        add(k, ["LEFT", "Y"])
        add(40, [])
        add(66, ["RIGHT"])
        add(90, [])
        segs.append(dict(kind="reset", test=t, start=s, len=n - s, k=k))
        s = n
        for cnt, btn in read_script(os.path.join(RYU, "scripts", t + ".txt")):
            add(cnt, btn)
        segs.append(dict(kind="test", test=t, start=s, len=n - s))
    add(30, [])  # (the last test's frames show up to two records later)
    os.makedirs(path("sessions"), exist_ok=True)
    open(path("sessions", course + ".pad"), "w").write("\n".join(lines) + "\n")
    json.dump(dict(course=course, frames=n, segments=segs), open(path("sessions", course + ".json"), "w"), indent=1)
    print("%s: %d frames, %d tests" % (course, n, len([s for s in segs if s["kind"] == "test"])))


def make_title_courses(course, dest):
    os.makedirs(dest, exist_ok=True)
    os.makedirs(path("courses"), exist_ok=True)
    subprocess.run([MMK, "trace-courses", path("courses"), "--only", course, "--table", TABLE, "--title", dest],
                   check=True, stdout=subprocess.DEVNULL)


# ---------------------------------------------------------------- runtimes
def run_recomp(course, tag=""):
    stock = course.startswith("stock_")
    mod = path("mods", course)
    if not stock:
        make_title_courses(course, os.path.join(mod, "romfs", "Course"))
    frames = json.load(open(path("sessions", course + ".json")))["frames"]
    os.makedirs(path("out", "recomp"), exist_ok=True)
    os.makedirs(path("logs"), exist_ok=True)
    out = path("out", "recomp", course + tag + ".bin")
    env = dict(os.environ, SMM2_SEED=os.environ.get("SMM2_SEED", "4"), RECOMP_PAD_SCRIPT=path("sessions", course + ".pad"),
               RECOMP_TRACE_OUT=out, RECOMP_TRACE_EXIT="1", LARECOMP_WINDOW="1280x720")
    if SEEDALL:
        env["SMM2_SEED_ALL"] = "1"
    if not stock:
        env["RECOMP_MOD_DIR"] = mod
    t0 = time.time()
    with open(path("logs", "recomp_" + course + tag + ".log"), "w") as log:
        st = subprocess.run([os.path.join(RECOMP, "tools", "run_for.sh"), str(int(frames / 60 * 1.5) + 120),
                             os.path.join(RECOMP, "build", "smm2", "smm2recomp"), "extracted"],
                            cwd=os.path.join(RECOMP, "private", "smm2"), env=env, stdout=log,
                            stderr=subprocess.STDOUT).returncode
    print("recomp %s: exit %s, %.0fs, %d records" % (course, st, time.time() - t0, len(records(out))))
    subprocess.run(["gzip", "-f", out], check=True)


def run_ryujinx(course, tag=""):
    if not RYU:
        raise SystemExit("set RYU_TRACE_DEV to the Ryujinx trace tooling's working folder")
    sys.path.insert(0, os.path.join(RYU, "tools"))
    import ryu
    if LOCK:
        try:
            os.mkdir(LOCK)
        except FileExistsError:
            raise SystemExit("Ryujinx lock held: " + LOCK)
    stock = course.startswith("stock_")
    contents = os.path.join(ryu.ROOT, "mods", "contents", TID)
    md, off, seedmod = os.path.join(contents, "courses"), os.path.join(RYU, "courses-mod-off"), os.path.join(contents, "r2seed")
    try:
        if subprocess.run(["pgrep", "-f", "Ryujinx"], capture_output=True).stdout.strip():
            raise SystemExit("another Ryujinx is running")
        if SEEDALL:
            os.makedirs(os.path.join(seedmod, "exefs"), exist_ok=True)
            open(os.path.join(seedmod, "exefs", BUILDID + ".ips"), "wb").write(SEED_IPS)
        if stock:
            if os.path.exists(md):
                os.rename(md, off)
        else:
            make_title_courses(course, os.path.join(md, "romfs", "Course"))
        pad = path("sessions", course + ".pad")
        frames = json.load(open(path("sessions", course + ".json")))["frames"]
        os.makedirs(path("out", "ryujinx"), exist_ok=True)
        out = path("out", "ryujinx", course + tag + ".bin")
        for p in (out, out + ".part"):
            if os.path.exists(p):
                os.remove(p)
        ctl = os.path.join(ryu.TR, "ctl")
        before = os.path.getmtime(ctl) if os.path.exists(ctl) else 0
        t0 = time.time()
        ryu.launch()
        # the script must be armed before the first player update: wait for the hook to reset its control file
        while not (os.path.exists(ctl) and os.path.getmtime(ctl) > before and os.path.getsize(ctl) == 0):
            if time.time() - t0 > 120 or (time.time() - t0 > 5 and not ryu.pids()):
                raise SystemExit("ryujinx did not start")
            time.sleep(0.05)
        time.sleep(0.2)
        ryu.cmd("run %s %s 0x3000 0x550,0x200" % (pad, out))
        last, last_t = None, time.time()
        while not os.path.exists(out):
            if not ryu.pids():
                raise SystemExit("ryujinx died")
            st = ryu.status().get("recording", "")
            if st != last:
                last, last_t = st, time.time()
            elif time.time() - last_t > 60 and st[:1].isdigit():
                print("ryujinx: recording stalled at", st, "(the followed player is gone)")
                break
            if time.time() - t0 > 240 + frames / 60 * 2:
                print("ryujinx: timeout at", st)
                break
            time.sleep(0.5)
        ryu.stop()
        if not os.path.exists(out) and os.path.exists(out + ".part"):
            os.rename(out + ".part", out)
        print("ryujinx %s: %.0fs, %d records" % (course, time.time() - t0, len(records(out))))
        subprocess.run(["gzip", "-f", out], check=True)
    finally:
        ryu.stop()
        if os.path.exists(off) and not os.path.exists(md):
            os.rename(off, md)
        shutil.rmtree(seedmod, ignore_errors=True)
        if LOCK:
            try:
                os.rmdir(LOCK)
            except FileNotFoundError:
                print("warning: the Ryujinx lock was removed by someone else during this run")


# ---------------------------------------------------------------- comparison
def cmp(course, a=None, b=None):
    a = a or path("out", "ryujinx", course + ".bin.gz")
    b = b or path("out", "recomp", course + ".bin.gz")
    ra, rb = records(a), records(b)
    n = min(len(ra), len(rb))
    sess = json.load(open(path("sessions", course + ".json")))
    whole = sum(1 for k in range(n) if key(ra[k]["body"]) == key(rb[k]["body"]))
    print("%s: records ryujinx %d recomp %d; position/speed/state identical at %d of %d updates" % (
        course, len(ra), len(rb), whole, n))
    print("| test | frames compared | identical | first divergence |")
    print("|---|---|---|---|")
    tot = [0, 0]
    for seg in sess["segments"]:
        if seg["kind"] == "reset":
            continue
        lo, hi = seg["start"] + 2, seg["start"] + seg["len"] + 2
        cnt = same = 0
        first = None
        for k in range(lo, min(hi, n)):
            cnt += 1
            if key(ra[k]["body"]) == key(rb[k]["body"]):
                same += 1
            elif first is None:
                fa, fb = fields(ra[k]["body"]), fields(rb[k]["body"])
                first = "frame %d (%s)" % (k - lo, ",".join(F[i][0] for i in range(len(F)) if fa[i] != fb[i]))
        tot[0] += cnt
        tot[1] += same
        print("| %s | %d%s | %d | %s |" % (seg["test"], cnt, "" if cnt == hi - lo else " (of %d)" % (hi - lo), same,
                                           first or "-"))
    print("| **all** | %d | %d | |" % tuple(tot))
    c, ci = Counter(), Counter()
    for k in range(n):
        c.update(body_diff(ra[k]["body"], rb[k]["body"]))
        if ra[k]["der"] and rb[k]["der"]:
            ci.update(body_diff(ra[k]["der"][0][1], rb[k]["der"][0][1]))
    print("\nwhole player object (%#x bytes; pointers compared by target): %d qwords differ%s" % (
        len(ra[0]["body"]), len(c), "".join("; +%#x in %d updates" % (o, m) for o, m in sorted(c.items())[:12])))
    print("input block (+0x550 -> %#x bytes): %d qwords differ" % (len(ra[0]["der"][0][1]) if ra[0]["der"] else 0,
                                                                 len(ci)))


def prep(courses):
    for c in courses:
        build(c)
        run_recomp(c, "_pre")
        build(c, path("out", "recomp", c + "_pre.bin.gz"))
        os.remove(path("out", "recomp", c + "_pre.bin.gz"))
        run_recomp(c)


if __name__ == "__main__":
    a = sys.argv[1:]
    tag = a[a.index("--tag") + 1] if "--tag" in a else ""
    if a[0] == "build":
        build(a[1], a[a.index("--pre") + 1] if "--pre" in a else None)
    elif a[0] == "recomp":
        run_recomp(a[1], tag)
    elif a[0] == "ryujinx":
        run_ryujinx(a[1], tag)
    elif a[0] == "cmp":
        cmp(a[1], *(a[2:4] if len(a) > 3 else (None, None)))
    elif a[0] == "prep":
        prep(a[1:])
    else:
        print(__doc__)
