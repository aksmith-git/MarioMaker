// Page side of the web build (see core/crates/shell/src/web.rs).
//
// The owner's game data is copied from a folder they pick into this origin's private file
// system (OPFS, `mmk-data/`), so it stays in this browser between visits; nothing is uploaded
// anywhere. The wasm game reads it back through `mmkStorage.read(path)`.
//
// Accepted folders:
// - an asset cache made by `mmk import` (contains manifest.json and textures/index.json);
// - a folder with the extracted `romfs/` and `exefs/` of the owner's dump: imported here, in a
//   worker running the same importer (mmk-import), straight into the browser's storage.

import init, * as mmk from "./mmk-play.js";

const ROOT = "mmk-data";
// written by `mmk import` but not used by the portable shell yet (3D models): not copied
const SKIP = ["meshes/", "anims/"];
const WRITERS = 16;

const $ = (id) => document.getElementById(id);
const ui = {
  status: $("status"),
  progress: $("progress"),
  courses: $("courses"),
  forget: $("forget"),
  canvas: $("mmk"),
};

function status(text) {
  ui.status.textContent = text;
}

function progress(done, total) {
  ui.progress.hidden = total === 0;
  ui.progress.max = total || 1;
  ui.progress.value = done;
}

// ---- storage -------------------------------------------------------------------------------

async function opfs() {
  if (!navigator.storage?.getDirectory) throw new Error("this browser has no origin private file system");
  return navigator.storage.getDirectory();
}

// directory handles by path, so reads don't walk the tree every time
let dirs = new Map();

async function dirAt(path, create = false) {
  let d = dirs.get(path);
  if (d) return d;
  if (path === "") {
    d = await (await opfs()).getDirectoryHandle(ROOT, { create });
  } else {
    const cut = path.lastIndexOf("/");
    const parent = await dirAt(cut < 0 ? "" : path.slice(0, cut), create);
    d = await parent.getDirectoryHandle(path.slice(cut + 1), { create });
  }
  dirs.set(path, d);
  return d;
}

function split(path) {
  const cut = path.lastIndexOf("/");
  return cut < 0 ? ["", path] : [path.slice(0, cut), path.slice(cut + 1)];
}

async function readFile(path) {
  try {
    const [dir, name] = split(path);
    const file = await (await (await dirAt(dir)).getFileHandle(name)).getFile();
    return new Uint8Array(await file.arrayBuffer());
  } catch (e) {
    if (e.name === "NotFoundError" || e.name === "TypeMismatchError") return null;
    throw e;
  }
}

async function writeFile(path, data) {
  const [dir, name] = split(path);
  const handle = await (await dirAt(dir, true)).getFileHandle(name, { create: true });
  if (handle.createWritable) {
    const w = await handle.createWritable();
    await w.write(data);
    await w.close();
  } else {
    throw new Error("this browser can't write files to its storage from a page (createWritable)");
  }
}

async function removeData() {
  dirs = new Map();
  try {
    await (await opfs()).removeEntry(ROOT, { recursive: true });
  } catch (e) {
    if (e.name !== "NotFoundError") throw e;
  }
}

async function listDir(path) {
  const out = [];
  try {
    for await (const [name, h] of (await dirAt(path)).entries()) {
      if (h.kind === "file") out.push(name);
    }
  } catch (e) {
    if (e.name !== "NotFoundError") throw e;
  }
  return out.sort();
}

globalThis.mmkStorage = { read: readFile };

// ---- import ----------------------------------------------------------------------------------

// Run `jobs` (async functions) with `n` at a time; `tick` after each.
async function pool(jobs, n, tick) {
  let next = 0;
  let done = 0;
  const worker = async () => {
    while (next < jobs.length) {
      await jobs[next++]();
      tick(++done);
    }
  };
  await Promise.all(Array.from({ length: Math.min(n, jobs.length) }, worker));
}

// Picked files by path relative to the picked folder (its own name dropped).
function byPath(files) {
  const m = new Map();
  for (const f of files) {
    const rel = (f.webkitRelativePath || f.name).split("/").slice(1).join("/");
    m.set(rel || f.name, f);
  }
  return m;
}

// Where the asset cache is in the picked tree ("" = the folder itself, null = none).
function findCache(paths) {
  let best = null;
  for (const p of paths.keys()) {
    if (!p.endsWith("manifest.json")) continue;
    const dir = p.slice(0, p.length - "manifest.json".length);
    if (paths.has(dir + "textures/index.json") && (best === null || dir.length < best.length)) best = dir;
  }
  return best;
}

// Where `romfs/` and `exefs/main` are ("" or "<dir>/"), or null.
function findDump(paths) {
  for (const p of paths.keys()) {
    if (p === "exefs/main" || p.endsWith("/exefs/main")) {
      const dir = p.slice(0, p.length - "exefs/main".length);
      for (const q of paths.keys()) if (q.startsWith(dir + "romfs/")) return dir;
    }
  }
  return null;
}

async function importCache(paths, prefix) {
  const files = [];
  let manifest = null;
  for (const [p, f] of paths) {
    if (!p.startsWith(prefix)) continue;
    const rel = p.slice(prefix.length);
    if (SKIP.some((s) => rel.startsWith(s)) || rel.split("/").some((c) => c.startsWith("."))) continue;
    if (rel === "manifest.json") manifest = f;
    else files.push([rel, f]);
  }
  const bytes = files.reduce((n, [, f]) => n + f.size, 0);
  status(`Copying game data into this browser: ${files.length} files, ${(bytes / 1e9).toFixed(2)} GB…`);
  await removeData();
  await pool(files.map(([rel, f]) => () => writeFile(rel, f)), WRITERS, (n) => progress(n, files.length));
  // the manifest last: its presence means a complete copy
  await writeFile("manifest.json", manifest);
}

async function importDump(paths, prefix) {
  const romfs = [];
  let main = null;
  for (const [p, f] of paths) {
    if (p.startsWith(prefix + "romfs/")) romfs.push([p.slice(prefix.length + 6), f]);
    else if (p === prefix + "exefs/main") main = f;
  }
  status(`Importing the dump in this browser (${romfs.length} RomFS files)…`);
  await removeData();
  const worker = new Worker(new URL("./mmk-import-worker.js", import.meta.url), { type: "module" });
  // the importer's output files, stored here (the worker can't wait for storage), WRITERS at a
  // time; the manifest last, as for a copied cache
  const queue = [];
  let active = 0;
  let manifest = null;
  let writeError = null;
  let drained = null;
  const pump = () => {
    while (active < WRITERS && queue.length) {
      const [path, bytes] = queue.shift();
      active++;
      writeFile(path, bytes)
        .catch((e) => (writeError ??= e))
        .finally(() => {
          active--;
          pump();
        });
    }
    if (!active && !queue.length && drained) drained();
  };
  let summary;
  try {
    summary = await new Promise((resolve, reject) => {
      worker.onerror = (e) => reject(new Error(e.message || "import worker failed"));
      worker.onmessage = ({ data }) => {
        if (data.write) {
          if (data.write === "manifest.json") manifest = data.bytes;
          else queue.push([data.write, data.bytes]);
          pump();
        } else if (data.progress) {
          const [stage, done, total] = data.progress;
          status(`Importing the dump in this browser: ${stage} (${done}/${total})…`);
          progress(done, total);
        } else if (data.done) {
          resolve(JSON.parse(data.done));
        } else if (data.error) {
          reject(new Error(data.error));
        }
      };
      worker.postMessage({ romfs, main });
    });
  } finally {
    worker.terminate();
  }
  status("Storing the last files…");
  await new Promise((resolve) => {
    drained = resolve;
    pump();
  });
  if (writeError) throw writeError;
  if (!manifest) throw new Error("the import wrote no manifest");
  await writeFile("manifest.json", manifest);
  console.log("imported in the browser:", summary);
}

async function importFolder(fileList) {
  const paths = byPath(fileList);
  const cache = findCache(paths);
  const dump = cache === null ? findDump(paths) : null;
  if (cache === null && dump === null) {
    throw new Error("that folder has no asset cache (manifest.json + textures/) and no romfs/ + exefs/main");
  }
  ui.forget.disabled = true;
  try {
    if (cache !== null) await importCache(paths, cache);
    else await importDump(paths, dump);
    await navigator.storage.persist?.();
  } finally {
    progress(0, 0);
  }
  await openData();
}

// ---- game data and courses ---------------------------------------------------------------------

async function openData() {
  if (!(await readFile("manifest.json"))) {
    status("No game data yet: placeholder art. Import game data to play your courses.");
    ui.forget.disabled = true;
    fillCourses([]);
    return false;
  }
  const info = JSON.parse(await mmk.open_assets());
  const courses = (await listDir("courses")).filter((n) => /\.(bcd|zlib|bin)$/.test(n));
  status(`Game data ${info.build}: ${info.textures} textures, ${info.sounds} sounds, ${courses.length} built-in courses (stored in this browser).`);
  ui.forget.disabled = false;
  fillCourses(courses);
  return true;
}

function fillCourses(names) {
  ui.courses.replaceChildren(new Option("Demo strip", ""));
  const groups = new Map();
  for (const n of names) {
    const g = n.replace(/[_-]?\d.*$/, "") || "other";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(n);
  }
  for (const [g, list] of groups) {
    const og = document.createElement("optgroup");
    og.label = `Built-in: ${g}`;
    for (const n of list) og.append(new Option(n.replace(/\.[^.]+$/, ""), `builtin:${n}`));
    ui.courses.append(og);
  }
}

// Show what plays in the course list (`value`: a list entry, or a file's name).
function showCourse(value, fileName) {
  ui.courses.querySelector("option[data-file]")?.remove();
  if (fileName) {
    const o = new Option(`File: ${fileName}`, value);
    o.dataset.file = "1";
    ui.courses.append(o);
  }
  ui.courses.value = value;
}

async function play(bytes, label) {
  const info = JSON.parse(await mmk.play_course(bytes));
  status(`Playing ${label}${info.name ? ` “${info.name}”` : ""} (${info.style}, theme ${info.theme}).`);
  ui.canvas.focus();
  return info;
}

async function playBuiltin(name) {
  const bytes = await readFile(`courses/${name}`);
  if (!bytes) throw new Error(`no built-in course ${name}`);
  const info = await play(bytes, name.replace(/\.[^.]+$/, ""));
  showCourse(`builtin:${name}`);
  return info;
}

async function guard(f) {
  try {
    return await f();
  } catch (e) {
    console.error(e);
    status(`Error: ${e.message || e}`);
  }
}

// ---- start -----------------------------------------------------------------------------------

await init();

// browsers only start sound after a user gesture
const unlock = () => {
  try {
    mmk.unlock_audio();
  } catch (e) {
    console.warn("sound:", e);
  }
};
addEventListener("pointerdown", unlock, { capture: true });
addEventListener("keydown", unlock, { capture: true });

$("import").onclick = () => $("folder").click();
$("folder").onchange = (e) => guard(() => importFolder(e.target.files));
$("open").onclick = () => $("file").click();
$("file").onchange = (e) =>
  guard(async () => {
    const f = e.target.files[0];
    e.target.value = "";
    if (f) {
      await play(new Uint8Array(await f.arrayBuffer()), f.name);
      showCourse("file", f.name);
    }
  });
ui.courses.onchange = () =>
  guard(() => {
    const v = ui.courses.value;
    if (v === "file") return; // the opened file (already playing)
    return v.startsWith("builtin:") ? playBuiltin(v.slice(8)) : play(new Uint8Array(), "the demo strip");
  });
ui.forget.onclick = () =>
  guard(async () => {
    await removeData();
    mmk.close_assets();
    await openData();
  });

// ---- online play -----------------------------------------------------------------------------
// Everyone picks the same course, then joins the same room on a course server's relay (its
// WebSocket port, the HTTP port + 1); whoever is first waits for the others.

function netJoin(relay, room, player, players) {
  mmk.net_join(relay, room, player, players);
  status(`Online: room “${room}”, player ${player} of ${players}. Waiting for the others…`);
  ui.canvas.focus();
}

$("online").onclick = () =>
  guard(() => {
    const relay = prompt("Relay (the course server's WebSocket):", localStorage.getItem("mmk-relay") || "ws://localhost:8641");
    if (!relay) return;
    const room = prompt("Room name (1–16 characters, the same for everyone):", "SMCRAFT");
    if (!room) return;
    const players = Number(prompt("How many players (2–4)?", "2")) || 2;
    const player = Number(prompt(`Which player are you (1–${players})?`, "1")) || 1;
    try {
      localStorage.setItem("mmk-relay", relay);
    } catch {}
    netJoin(relay, room, player, players);
  });

// for scripted checks (and the console)
globalThis.mmkPage = { importFolder, openData, playBuiltin, play, readFile, removeData, netJoin };

await guard(openData);
// ?relay=ws://host:8641&room=NAME&player=1&players=2 joins at once (after ?course=builtin name)
{
  const q = new URLSearchParams(location.search);
  if (q.get("course")) await guard(() => playBuiltin(q.get("course")));
  if (q.get("relay") && q.get("room"))
    guard(() => netJoin(q.get("relay"), q.get("room"), Number(q.get("player")) || 1, Number(q.get("players")) || 2));
}
ui.canvas.focus();
