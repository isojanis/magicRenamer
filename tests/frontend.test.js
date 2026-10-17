import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const appSource = readFileSync(new URL("../main.js", import.meta.url), "utf8")
  .replace(/^import\s+.*?;\s*$/gm, "");
const ids = [...readFileSync(new URL("../index.html", import.meta.url), "utf8").matchAll(/\bid="([^"]+)"/g)]
  .map((match) => match[1]);

class FakeClassList {
  values = new Set();
  add(value) { this.values.add(value); }
  remove(value) { this.values.delete(value); }
  contains(value) { return this.values.has(value); }
}

class FakeElement {
  constructor(id = "") {
    this.id = id;
    this.value = "";
    this.checked = false;
    this.disabled = false;
    this.textContent = "";
    this.className = "";
    this.classList = new FakeClassList();
    this.style = {};
    this.children = [];
    this.listeners = new Map();
    this._innerHTML = "";
  }
  set innerHTML(value) {
    this._innerHTML = String(value);
    if (this._innerHTML === "") this.children = [];
  }
  get innerHTML() { return this._innerHTML; }
  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) || [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }
  removeEventListener(type, listener) {
    this.listeners.set(type, (this.listeners.get(type) || []).filter((item) => item !== listener));
  }
  appendChild(child) {
    this.children.push(child);
    this._innerHTML += child.innerHTML;
  }
  getBoundingClientRect() { return { width: Number.parseFloat(this.style.width) || 300 }; }
  async dispatch(type, init = {}) {
    if (this.disabled && type === "click") return;
    const event = {
      target: init.target || this,
      key: init.key,
      shiftKey: Boolean(init.shiftKey),
      clientX: init.clientX || 0,
      preventDefault() { this.defaultPrevented = true; },
      ...init,
    };
    const results = (this.listeners.get(type) || []).map((listener) => listener(event));
    await Promise.all(results.map((value) => Promise.resolve(value)));
    return event;
  }
  click() { return this.dispatch("click"); }
}

class FakeWindow extends FakeElement {
  confirmCalls = [];
  confirmResult = true;
  confirm(message) {
    this.confirmCalls.push(message);
    return this.confirmResult;
  }
}

function fixture(name, index = 0) {
  return {
    name,
    path: `/docs/${name}`,
    extension: name.split(".").at(-1).toLowerCase(),
    size_bytes: 1200 + index,
  };
}

function createUi({ files = [fixture("report.pdf")], behavior = {}, confirmResult = true } = {}) {
  const elements = new Map(ids.map((id) => [id, new FakeElement(id)]));
  elements.get("prompt-modal").classList.add("hidden");
  const document = {
    body: new FakeElement("body"),
    activeElement: null,
    getElementById(id) {
      if (!elements.has(id)) elements.set(id, new FakeElement(id));
      return elements.get(id);
    },
    createElement() { return new FakeElement(); },
  };
  document.activeElement = document.body;
  const window = new FakeWindow();
  window.__TAURI_INTERNALS__ = {};
  window.confirmResult = confirmResult;
  const storage = new Map();
  const localStorage = {
    getItem(key) { return storage.has(key) ? storage.get(key) : null; },
    setItem(key, value) { storage.set(key, String(value)); },
  };
  const calls = [];
  const alerts = [];
  const config = {
    system_prompt: "test system",
    prompt_template: "{text_snippet} {extension} {folder_context}",
    model: "test-model",
    temperature: 0.2,
    folder_context_enabled: true,
  };
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (behavior[command]) return behavior[command](args, calls);
    if (command === "get_prompt_config") return config;
    if (command === "scan_folder") return files;
    if (command === "build_directory_blueprint") return "local folder context";
    if (command === "extract_document_snippet") return "Report\nOrganization: Example Lab";
    if (command === "suggest_filename_with_ollama") {
      return { new_name: `Example_Lab_Report.${args.extension}`, category: "Report", reason: "Matched source text" };
    }
    if (command === "rename_file") return args.newPath;
    if (command === "undo_rename_file") {
      return { restored_path: args.filePath, original_name: "original.pdf", source: "manifest" };
    }
    if (command === "save_prompt_config") return true;
    return null;
  };
  const context = {
    window,
    document,
    localStorage,
    invoke,
    openDialog: async () => "/docs",
    alert: (message) => alerts.push(String(message)),
    prompt: () => "/docs",
    console: { log() {}, error() {} },
  };
  vm.runInNewContext(appSource, context, { filename: "main.js" });
  const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
  return { document, elements, window, calls, alerts, storage, flush };
}

async function chooseFolder(ui) {
  await ui.elements.get("btn-select-folder").click();
}

async function processQueue(ui) {
  await ui.elements.get("btn-process-all").click();
}

test("loads a folder, processes files, and sends selected model and local context", async () => {
  const ui = createUi({ files: [fixture("report.pdf"), fixture("notes.txt", 1)] });
  await ui.flush();
  ui.elements.get("select-ollama-model").value = "qwen2.5-coder:14b";
  await chooseFolder(ui);
  assert.equal(ui.elements.get("queue-count").textContent, "2 files");
  assert.equal(ui.elements.get("btn-approve-all").disabled, true);

  await processQueue(ui);
  assert.equal(ui.elements.get("btn-approve-all").disabled, false);
  assert.equal(ui.elements.get("btn-approve-all").textContent, "Approve All Ready (2)");
  assert.match(ui.elements.get("file-queue-list").innerHTML, /ready/);
  const suggestionCalls = ui.calls.filter((call) => call.command === "suggest_filename_with_ollama");
  assert.equal(suggestionCalls.length, 2);
  assert.ok(suggestionCalls.every((call) => call.args.model === "qwen2.5-coder:14b"));
  assert.ok(suggestionCalls.every((call) => call.args.folderContext === "local folder context"));
});

test("single approval uses the edited name, adopts collision result, advances, and can undo", async () => {
  const files = [fixture("first.pdf"), fixture("second.pdf", 1)];
  const ui = createUi({
    files,
    behavior: {
      rename_file: (args) => args.oldPath.endsWith("first.pdf") ? "/docs/edited_1.pdf" : args.newPath,
      undo_rename_file: () => ({ restored_path: "/docs/first.pdf", original_name: "first.pdf", source: "xattr" }),
    },
  });
  await ui.flush();
  await chooseFolder(ui);
  await processQueue(ui);
  const suggestion = ui.elements.get("input-suggested-name");
  suggestion.value = "edited.pdf";
  await ui.elements.get("btn-approve").click();

  const renameCall = ui.calls.find((call) => call.command === "rename_file");
  assert.equal(renameCall.args.newPath, "/docs/edited.pdf");
  assert.equal(ui.elements.get("input-original-name").value, "second.pdf");
  assert.match(ui.elements.get("file-queue-list").innerHTML, /edited_1\.pdf/);

  await ui.elements.get("file-queue-list").children[0].click();
  assert.equal(ui.elements.get("btn-undo").disabled, false);
  await ui.elements.get("btn-undo").click();
  assert.equal(ui.calls.filter((call) => call.command === "undo_rename_file").length, 1);
  assert.equal(ui.elements.get("input-original-name").value, "first.pdf");
  assert.match(ui.elements.get("active-status").textContent, /READY/);
});

test("batch approval skips failed items and preserves per-file failures", async () => {
  const files = [fixture("a.pdf"), fixture("b.pdf", 1), fixture("c.pdf", 2)];
  const ui = createUi({
    files,
    behavior: {
      extract_document_snippet: (args) => {
        if (args.filePath.endsWith("b.pdf")) throw new Error("bad document");
        return "Report\nExample Lab";
      },
      rename_file: (args) => {
        if (args.oldPath.endsWith("c.pdf")) throw new Error("destination denied");
        return args.newPath;
      },
    },
  });
  await ui.flush();
  await chooseFolder(ui);
  await processQueue(ui);
  assert.equal(ui.elements.get("btn-approve-all").textContent, "Approve All Ready (2)");
  await ui.elements.get("btn-approve-all").click();
  const renameCalls = ui.calls.filter((call) => call.command === "rename_file");
  assert.equal(renameCalls.length, 2);
  assert.ok(renameCalls.every((call) => !call.args.oldPath.endsWith("b.pdf")));
  const html = ui.elements.get("file-queue-list").innerHTML;
  assert.match(html, /renamed/);
  assert.match(html, /failed/);
});

test("large folders can disable context for the current run", async () => {
  const files = Array.from({ length: 501 }, (_, index) => fixture(`document-${index}.txt`, index));
  const ui = createUi({ files, confirmResult: false });
  await ui.flush();
  await chooseFolder(ui);
  assert.equal(ui.window.confirmCalls.length, 1);
  assert.match(ui.window.confirmCalls[0], /501 files/);
  assert.equal(ui.calls.filter((call) => call.command === "build_directory_blueprint").length, 0);
  await ui.elements.get("btn-regen-ai").click();
  await ui.flush();
  const suggestionCalls = ui.calls.filter((call) => call.command === "suggest_filename_with_ollama");
  assert.equal(suggestionCalls.length, 1);
  assert.equal(suggestionCalls[0].args.folderContext, null);
});

test("the large-folder warning starts above 500 files", async () => {
  const files = Array.from({ length: 500 }, (_, index) => fixture(`document-${index}.txt`, index));
  const ui = createUi({ files });
  await ui.flush();
  await chooseFolder(ui);
  assert.equal(ui.window.confirmCalls.length, 0);
  assert.equal(ui.calls.filter((call) => call.command === "build_directory_blueprint").length, 1);
});

test("queue processing limits concurrent work to three files", async () => {
  let active = 0;
  let maximumActive = 0;
  const files = Array.from({ length: 7 }, (_, index) => fixture(`document-${index}.txt`, index));
  const ui = createUi({
    files,
    behavior: {
      extract_document_snippet: async () => {
        active += 1;
        maximumActive = Math.max(maximumActive, active);
        await new Promise((resolve) => setTimeout(resolve, 5));
        active -= 1;
        return "Report\nExample Lab";
      },
    },
  });
  await ui.flush();
  await chooseFolder(ui);
  await processQueue(ui);
  assert.equal(maximumActive, 3);
  assert.equal(ui.calls.filter((call) => call.command === "suggest_filename_with_ollama").length, 7);
});

test("Enter approves the active suggestion while Shift+Enter does not", async () => {
  const ui = createUi();
  await ui.flush();
  await chooseFolder(ui);
  await processQueue(ui);
  ui.elements.get("input-suggested-name").value = "approved.pdf";
  await ui.window.dispatch("keydown", { key: "Enter", shiftKey: true });
  await ui.flush();
  assert.equal(ui.calls.filter((call) => call.command === "rename_file").length, 0);
  await ui.elements.get("btn-prompt-settings").click();
  await ui.window.dispatch("keydown", { key: "Enter", shiftKey: false });
  await ui.flush();
  assert.equal(ui.calls.filter((call) => call.command === "rename_file").length, 0);
  await ui.elements.get("btn-close-modal").click();
  await ui.window.dispatch("keydown", { key: "Enter", shiftKey: false });
  await ui.flush();
  assert.equal(ui.calls.filter((call) => call.command === "rename_file").length, 1);
});

test("settings clamp temperature and persist model and context toggle", async () => {
  const ui = createUi();
  await ui.flush();
  await ui.elements.get("btn-prompt-settings").click();
  assert.equal(ui.elements.get("prompt-modal").classList.contains("hidden"), false);
  ui.elements.get("cfg-model").value = "custom-model";
  ui.elements.get("cfg-temperature-num").value = "1.8";
  await ui.elements.get("cfg-temperature-num").dispatch("blur");
  ui.elements.get("cfg-folder-context-enabled").checked = false;
  await ui.elements.get("btn-save-modal").click();

  const save = ui.calls.find((call) => call.command === "save_prompt_config");
  assert.equal(save.args.config.model, "custom-model");
  assert.equal(save.args.config.temperature, 1);
  assert.equal(save.args.config.folder_context_enabled, false);
  assert.equal(ui.elements.get("select-ollama-model").value, "custom-model");
  assert.equal(ui.elements.get("prompt-modal").classList.contains("hidden"), true);
});

test("document names, paths, snippets, and model text render as escaped text", async () => {
  const hostileName = '<img src=x onerror="run()">.txt';
  const ui = createUi({
    files: [fixture(hostileName)],
    behavior: {
      extract_document_snippet: () => "<script>alert('document')</script>",
      suggest_filename_with_ollama: () => ({
        new_name: "safe.txt",
        category: "<b>Category</b>",
        reason: "<svg onload=run()>",
      }),
    },
  });
  await ui.flush();
  await chooseFolder(ui);
  await processQueue(ui);
  const queueHtml = ui.elements.get("file-queue-list").innerHTML;
  const previewHtml = ui.elements.get("preview-content").innerHTML;
  assert.ok(queueHtml.includes("&lt;img"));
  assert.ok(!queueHtml.includes("<img"));
  assert.ok(previewHtml.includes("&lt;script&gt;"));
  assert.ok(previewHtml.includes("&lt;b&gt;Category&lt;/b&gt;"));
  assert.ok(previewHtml.includes("&lt;svg"));
  assert.ok(!previewHtml.includes("<script>"));
  assert.ok(!previewHtml.includes("<svg"));
});
