// The wolfpack playground: the compiler runs in WebAssembly (`pkg/`, built from `crates/wolf_wasm`),
// and programs run as the generated TypeScript, transpiled in the browser and run in a worker.

import init, * as wolf from "./pkg/wolf_wasm.js";

const TYPESCRIPT = "https://cdn.jsdelivr.net/npm/typescript@5.9.3/lib/typescript.js";
const RUN_LIMIT_MS = 5000;
const STORAGE_KEY = "wolfpack-playground-source";
const MAIN_FILE = { ts: "src/main.ts", go: "main.go", rust: "src/main.rs" };

const $ = (id) => document.getElementById(id);
const code = $("code");
const gutter = $("gutter");
const view = $("view");
const status = $("status");
const fileSelect = $("file");
const runButton = $("run");

let current = "output";
let output = null; // the last result shown in the Output tab: a DOM fragment
let checkTimer = 0;
let lastFiles = { ts: null, go: null, rust: null };

// ----- Source and storage -------------------------------------------------------------------

function saveSource() {
  try {
    localStorage.setItem(STORAGE_KEY, code.value);
  } catch {
    // Storage may be unavailable; the source simply is not remembered.
  }
}

function sourceFromHash() {
  const match = location.hash.match(/^#code=([A-Za-z0-9_-]*)$/);
  if (!match) return null;
  try {
    const base64 = match[1].replace(/-/g, "+").replace(/_/g, "/");
    const bytes = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

function linkForSource() {
  const bytes = new TextEncoder().encode(code.value);
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  const base64 = btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return location.origin + location.pathname + "#code=" + base64;
}

async function loadExample(name) {
  const response = await fetch(`examples/${name}.wolf`);
  if (!response.ok) throw new Error(`cannot load the example ${name}`);
  return response.text();
}

// ----- The editor ---------------------------------------------------------------------------

function renderGutter(badLines) {
  const count = code.value.split("\n").length;
  const parts = [];
  for (let i = 1; i <= count; i++) {
    parts.push(badLines.has(i) ? `<span class="bad">${i}</span>` : String(i));
  }
  gutter.innerHTML = parts.join("\n");
  gutter.scrollTop = code.scrollTop;
}

code.addEventListener("scroll", () => {
  gutter.scrollTop = code.scrollTop;
});

code.addEventListener("keydown", (event) => {
  if (event.key === "Tab" && !event.shiftKey && !event.ctrlKey && !event.metaKey) {
    event.preventDefault();
    const { selectionStart: start, selectionEnd: end } = code;
    code.setRangeText("    ", start, end, "end");
    onEdit();
  } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    run();
  }
});

code.addEventListener("input", onEdit);

function onEdit() {
  saveSource();
  lastFiles = { ts: null, go: null, rust: null };
  clearTimeout(checkTimer);
  checkTimer = setTimeout(checkNow, 250);
}

// ----- Checking -----------------------------------------------------------------------------

function checkNow() {
  const result = JSON.parse(wolf.check(code.value));
  const bad = new Set();
  for (const d of result.diagnostics) {
    const label = d.labels.find((l) => l.primary && l.file === "src/main.wolf");
    if (label) bad.add(label.line_start);
  }
  renderGutter(bad);
  if (result.errors === 0) {
    setStatus("No errors. Press Run or Ctrl+Enter to run the program.", false);
  } else {
    const noun = result.errors === 1 ? "error" : "errors";
    setStatus(`${result.errors} ${noun}. The Output tab shows them.`, true);
    showDiagnostics(result.text);
  }
  if (current !== "output") showTarget(current);
  return result;
}

function setStatus(text, bad) {
  status.textContent = text;
  status.classList.toggle("bad", bad);
}

/** Diagnostics as text, with every `error[E0000]` code as a button that explains it. */
function diagnosticsFragment(text) {
  const fragment = document.createDocumentFragment();
  let last = 0;
  for (const match of text.matchAll(/error\[(E\d{4})\]/g)) {
    fragment.append(text.slice(last, match.index) + "error[");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "code-link";
    button.textContent = match[1];
    button.title = `Explain ${match[1]}`;
    button.addEventListener("click", () => showExplanation(match[1]));
    fragment.append(button, "]");
    last = match.index + match[0].length;
  }
  fragment.append(text.slice(last));
  return fragment;
}

function showDiagnostics(text) {
  output = diagnosticsFragment(text);
  if (current === "output") renderOutput();
}

function showExplanation(id) {
  const fragment = document.createDocumentFragment();
  fragment.append(wolf.explain(id) || `There is no error code ${id}.`);
  output = fragment;
  select("output");
}

// ----- Views --------------------------------------------------------------------------------

for (const tab of document.querySelectorAll(".tab")) {
  tab.addEventListener("click", () => select(tab.dataset.view));
}

function select(name) {
  current = name;
  for (const tab of document.querySelectorAll(".tab")) {
    tab.setAttribute("aria-selected", String(tab.dataset.view === name));
  }
  if (name === "output") {
    fileSelect.hidden = true;
    renderOutput();
  } else {
    showTarget(name);
  }
}

function renderOutput() {
  view.replaceChildren();
  if (output) {
    view.append(output.cloneNode(true));
    // Cloned buttons lose their listeners; attach them again.
    for (const button of view.querySelectorAll(".code-link")) {
      button.addEventListener("click", () => showExplanation(button.textContent));
    }
  } else {
    const note = document.createElement("p");
    note.className = "note";
    note.textContent =
      "Run the program to see its output here. The other tabs show the code it compiles to.";
    view.append(note);
  }
}

function build(target) {
  if (!lastFiles[target]) lastFiles[target] = JSON.parse(wolf.build(code.value, target));
  return lastFiles[target];
}

function showTarget(target) {
  const result = build(target);
  if (result.errors > 0) {
    fileSelect.hidden = true;
    view.replaceChildren(diagnosticsFragment(result.text));
    return;
  }
  const previous = fileSelect.value;
  fileSelect.replaceChildren(
    ...result.files.map((f) => new Option(f.path, f.path)),
  );
  const paths = result.files.map((f) => f.path);
  fileSelect.value = paths.includes(previous) ? previous : MAIN_FILE[target];
  fileSelect.hidden = false;
  showFile(result);
}

function showFile(result) {
  const file = result.files.find((f) => f.path === fileSelect.value);
  view.textContent = file ? file.contents : "";
}

fileSelect.addEventListener("change", () => {
  if (current !== "output") showFile(build(current));
});

// ----- Running ------------------------------------------------------------------------------

let typescript = null;

function loadTypeScript() {
  if (typescript) return typescript;
  typescript = new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = TYPESCRIPT;
    script.onload = () => resolve(globalThis.ts);
    script.onerror = () => {
      typescript = null;
      reject(new Error("TypeScript could not be loaded; check the network connection"));
    };
    document.head.append(script);
  });
  return typescript;
}

/** Transpiles the generated files to JavaScript modules behind blob URLs; the URL of `entry`. */
function modules(ts, files, entry) {
  const sources = new Map(files.map((f) => [f.path, f.contents]));
  const urls = new Map();
  const resolve = (from, spec) => {
    const parts = from.split("/").slice(0, -1);
    for (const part of spec.split("/")) {
      if (part === "..") parts.pop();
      else if (part !== ".") parts.push(part);
    }
    return parts.join("/").replace(/\.js$/, ".ts");
  };
  const urlFor = (path) => {
    if (urls.has(path)) return urls.get(path);
    const source = sources.get(path);
    if (source === undefined) throw new Error(`the generated module ${path} is missing`);
    let js = ts.transpileModule(source, {
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
    }).outputText;
    js = js.replace(
      /(\bfrom\s*|\bimport\s*)(["'])(\.{1,2}\/[^"']+)\2/g,
      (_, keyword, quote, spec) => `${keyword}${quote}${urlFor(resolve(path, spec))}${quote}`,
    );
    const url = URL.createObjectURL(new Blob([js], { type: "text/javascript" }));
    urls.set(path, url);
    return url;
  };
  return { entry: urlFor(entry), urls: [...urls.values()] };
}

const WORKER = `
const send = (stream, text) => self.postMessage({ stream, text });
self.process = {
  stdout: { write: (t) => (send("stdout", t), true) },
  stderr: { write: (t) => (send("stderr", t), true) },
  exitCode: undefined,
  argv: ["node", "main"],
  env: {},
  getBuiltinModule() {
    throw new Error("files, input and the environment are not available in the playground");
  },
};
self.onmessage = async (event) => {
  try {
    await import(event.data);
  } catch (error) {
    send("stderr", "error: " + (error && error.message ? error.message : String(error)) + "\\n");
    self.process.exitCode = self.process.exitCode ?? 101;
  }
  self.postMessage({ done: true, code: self.process.exitCode ?? 0 });
};
`;

async function run() {
  clearTimeout(checkTimer);
  const checked = checkNow();
  if (checked.errors > 0) {
    select("output");
    return;
  }
  const result = build("ts");
  if (result.errors > 0) {
    showDiagnostics(result.text);
    select("output");
    return;
  }
  runButton.disabled = true;
  const pre = document.createDocumentFragment();
  output = pre;
  select("output");
  try {
    setStatus("Loading TypeScript…", false);
    const ts = await loadTypeScript();
    setStatus("Running…", false);
    const { entry, urls } = modules(ts, result.files, "src/main.ts");
    const worker = new Worker(URL.createObjectURL(new Blob([WORKER], { type: "text/javascript" })), {
      type: "module",
    });
    const append = (text, className) => {
      const span = document.createElement("span");
      if (className) span.className = className;
      span.textContent = text;
      pre.append(span);
      if (current === "output") renderOutput();
    };
    const code = await new Promise((resolve) => {
      const timer = setTimeout(() => {
        worker.terminate();
        append(`stopped after ${RUN_LIMIT_MS / 1000} seconds\n`, "stderr");
        resolve(null);
      }, RUN_LIMIT_MS);
      worker.onmessage = ({ data }) => {
        if (data.done) {
          clearTimeout(timer);
          worker.terminate();
          resolve(data.code);
        } else {
          append(data.text, data.stream === "stderr" ? "stderr" : "");
        }
      };
      worker.postMessage(entry);
    });
    urls.forEach((url) => URL.revokeObjectURL(url));
    if (code === null) setStatus("The program ran too long and was stopped.", true);
    else if (code === 0) setStatus("The program finished.", false);
    else setStatus(`The program exited with code ${code}.`, true);
    if (pre.childNodes.length === 0 && code === 0) append("The program printed nothing.\n", "");
  } catch (error) {
    setStatus(error.message, true);
  } finally {
    runButton.disabled = false;
  }
}

// ----- Buttons ------------------------------------------------------------------------------

runButton.addEventListener("click", run);

$("format").addEventListener("click", () => {
  const result = JSON.parse(wolf.format(code.value));
  if (result.ok) {
    code.value = result.text;
    onEdit();
    checkNow();
    setStatus("Formatted.", false);
  } else {
    showDiagnostics(result.text);
    select("output");
    setStatus("The program has syntax errors, so it cannot be formatted.", true);
  }
});

$("share").addEventListener("click", async () => {
  const link = linkForSource();
  history.replaceState(null, "", link);
  try {
    await navigator.clipboard.writeText(link);
    setStatus("Link copied. It contains the whole program.", false);
  } catch {
    setStatus("The link is in the address bar. It contains the whole program.", false);
  }
});

$("example").addEventListener("change", async (event) => {
  code.value = await loadExample(event.target.value);
  output = null;
  history.replaceState(null, "", location.pathname);
  onEdit();
  checkNow();
  select("output");
});

// ----- Start --------------------------------------------------------------------------------

async function start() {
  await init();
  let source = sourceFromHash();
  if (source === null) {
    try {
      source = localStorage.getItem(STORAGE_KEY);
    } catch {
      source = null;
    }
  }
  code.value = source ?? (await loadExample("hello"));
  renderOutput();
  checkNow();
  document.title = `Wolfpack ${wolf.version()} playground`;
}

start().catch((error) => setStatus(`The compiler could not start: ${error.message}`, true));
