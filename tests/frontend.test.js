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
  toggle(value, force) {
    const add = force === undefined ? !this.values.has(value) : force;
    if (add) this.values.add(value);
    else this.values.delete(value);
    return add;
  }
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

function createUi({ files = [fixture("report.pdf")], behavior = {}, confirmResult = true, dialogResult = "/docs" } = {}) {
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
  let nextProcessingRunId = 0;
  const config = {
    system_prompt: "test system",
    prompt_template: "{text_snippet} {extension} {folder_context}",
    model: "test-model",
    temperature: 0.2,
    folder_context_enabled: true,
    provider: "ollama",
    custom_base_url: "",
    configured_api_keys: { openai: false, gemini: false, anthropic: false, openrouter: false },
  };
  const invoke = async (command, args) => {
    calls.push({ command, args });
    if (behavior[command]) return behavior[command](args, calls);
    if (command === "get_prompt_config") return config;
    if (command === "scan_folder") return files;
    if (command === "build_directory_blueprint") return "local folder context";
    if (command === "extract_document_snippet") return "Report\nOrganization: Example Lab";
    if (command === "begin_processing_run") return ++nextProcessingRunId;
    if (command === "cancel_processing_run") return true;
    if (command === "suggest_filename_with_ollama") {
      return { new_name: `Example_Lab_Report.${args.extension}`, category: "Report", reason: "Matched source text" };
    }
    if (command === "rename_file") return args.newPath;
    if (command === "undo_rename_file") {
      return { restored_path: args.filePath, original_name: "original.pdf", source: "manifest" };
    }
    if (command === "save_prompt_config") return { ...args.config, configured_api_keys: config.configured_api_keys };
    return null;
  };
  const context = {
    window,
    document,
    localStorage,
    URL,
    invoke,
    openDialog: async () => dialogResult,
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
  ui.elements.get("select-ai-model").value = "qwen2.5-coder:14b";
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
  assert.equal(ui.elements.get("select-ai-model").value, "custom-model");
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

test("cloud provider settings select a model, keep keys masked, and show the data-sharing badge", async () => {
  const ui = createUi({
    behavior: {
      save_prompt_config: ({ config }) => ({
        ...config,
        api_key: undefined,
        configured_api_keys: { openai: true, gemini: false, anthropic: false, openrouter: false },
      }),
      test_ai_provider: ({ config }) => `Connected to ${config.provider} using ${config.model}.`,
    },
  });
  await ui.flush();
  await ui.elements.get("btn-prompt-settings").click();
  const provider = ui.elements.get("cfg-provider");
  provider.value = "openai";
  await provider.dispatch("change");
  assert.equal(ui.elements.get("cfg-model").value, "gpt-5.4-mini");
  assert.equal(ui.elements.get("cloud-credentials").classList.contains("hidden"), false);
  assert.match(readFileSync(new URL("../index.html", import.meta.url), "utf8"), /type="password" id="cfg-api-key"/);
  ui.elements.get("cfg-api-key").value = "new-secret";
  await ui.elements.get("btn-test-provider").click();
  const connection = ui.calls.find((call) => call.command === "test_ai_provider");
  assert.equal(connection.args.config.api_key, "new-secret");
  assert.match(ui.elements.get("provider-test-status").textContent, /Connected to openai/);
  await ui.elements.get("btn-save-modal").click();
  assert.equal(ui.elements.get("provider-badge").textContent, "OpenAI · Cloud");
  assert.match(ui.elements.get("provider-badge").title, /document excerpts and naming context/i);
  assert.equal(ui.elements.get("select-ai-model").value, "gpt-5.4-mini");
  assert.equal(ui.elements.get("cfg-api-key").value, "");
  await ui.elements.get("btn-prompt-settings").click();
  assert.match(ui.elements.get("cfg-key-status").textContent, /key is configured/);
  assert.equal(ui.elements.get("cfg-api-key").value, "");
});

test("switching providers resets the endpoint draft and connection errors stay in the modal", async () => {
  const ui = createUi({
    behavior: {
      test_ai_provider: () => { throw new Error("HTTP 401"); },
    },
  });
  await ui.flush();
  await ui.elements.get("btn-prompt-settings").click();
  ui.elements.get("cfg-custom-endpoint").value = "http://localhost:11434/api/generate";
  const provider = ui.elements.get("cfg-provider");
  provider.value = "gemini";
  await provider.dispatch("change");
  assert.equal(ui.elements.get("cfg-model").value, "gemini-2.5-flash");
  assert.equal(ui.elements.get("endpoint-setting").classList.contains("hidden"), true);
  assert.equal(ui.elements.get("cfg-custom-endpoint").value, "");
  await ui.elements.get("btn-test-provider").click();
  assert.match(ui.elements.get("provider-test-status").textContent, /HTTP 401/);
  assert.equal(ui.elements.get("btn-test-provider").disabled, false);
  provider.value = "ollama";
  await provider.dispatch("change");
  assert.equal(ui.elements.get("cfg-custom-endpoint").value, "http://localhost:11434/api/generate");
  assert.equal(ui.elements.get("cloud-credentials").classList.contains("hidden"), true);
});

function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

test("Process Queue stops immediately and ignores late extraction after a resumed run", async () => {
  const gates = [];
  let holdExtraction = true;
  const ui = createUi({
    files: Array.from({ length: 5 }, (_, index) => fixture(`pending-${index}.txt`, index)),
    behavior: {
      extract_document_snippet: () => {
        if (!holdExtraction) return "Report\nOrganization: Example Lab";
        const gate = deferred();
        gates.push(gate);
        return gate.promise;
      },
    },
  });
  await ui.flush();
  await chooseFolder(ui);
  const firstRun = ui.elements.get("btn-process-all").click();
  await ui.flush();
  const button = ui.elements.get("btn-process-all");
  assert.equal(button.textContent, "Stop Processing");
  assert.equal(button.disabled, false);
  assert.equal(gates.length, 3);

  await button.click();
  await firstRun;
  assert.equal(button.textContent, "Process Queue");
  assert.equal(button.disabled, false);
  assert.equal(ui.calls.filter((call) => call.command === "cancel_processing_run").length, 1);
  assert.equal(ui.calls.filter((call) => call.command === "suggest_filename_with_ollama").length, 0);
  assert.equal((ui.elements.get("file-queue-list").innerHTML.match(/status-badge pending/g) || []).length, 5);

  holdExtraction = false;
  await button.click();
  assert.equal(ui.calls.filter((call) => call.command === "begin_processing_run").length, 2);
  assert.equal(ui.calls.filter((call) => call.command === "suggest_filename_with_ollama").length, 5);
  assert.equal((ui.elements.get("file-queue-list").innerHTML.match(/status-badge ready/g) || []).length, 5);
  for (const gate of gates) gate.resolve("Late extraction result");
  await ui.flush();
  assert.equal(ui.calls.filter((call) => call.command === "suggest_filename_with_ollama").length, 5);
  assert.equal((ui.elements.get("file-queue-list").innerHTML.match(/status-badge ready/g) || []).length, 5);
});

test("stopping an in-flight AI batch discards late suggestions and starts no later batch", async () => {
  const gates = [];
  const ui = createUi({
    files: Array.from({ length: 5 }, (_, index) => fixture(`pending-${index}.pdf`, index)),
    behavior: {
      suggest_filename_with_ollama: () => {
        const gate = deferred();
        gates.push(gate);
        return gate.promise;
      },
    },
  });
  await ui.flush();
  await chooseFolder(ui);
  const firstRun = ui.elements.get("btn-process-all").click();
  await ui.flush();
  assert.equal(gates.length, 3);
  await ui.elements.get("btn-process-all").click();
  const began = ui.calls.find((call) => call.command === "begin_processing_run");
  const cancelled = ui.calls.find((call) => call.command === "cancel_processing_run");
  assert.equal(cancelled.args.processingRunId, 1);
  assert.ok(began);
  assert.ok(ui.calls.filter((call) => call.command === "suggest_filename_with_ollama")
    .every((call) => call.args.processingRunId === 1));
  for (const gate of gates) {
    gate.resolve({ new_name: "Late_Suggestion.pdf", category: "Report", reason: "late" });
  }
  await firstRun;
  assert.equal(ui.calls.filter((call) => call.command === "suggest_filename_with_ollama").length, 3);
  assert.equal((ui.elements.get("file-queue-list").innerHTML.match(/status-badge ready/g) || []).length, 0);
  assert.equal((ui.elements.get("file-queue-list").innerHTML.match(/status-badge failed/g) || []).length, 0);
  assert.equal(ui.elements.get("btn-process-all").textContent, "Process Queue");
});

test("stopping before the backend assigns a run ID still cancels that run", async () => {
  const begin = deferred();
  const ui = createUi({
    behavior: { begin_processing_run: () => begin.promise },
  });
  await ui.flush();
  await chooseFolder(ui);
  const firstRun = ui.elements.get("btn-process-all").click();
  await ui.flush();
  assert.equal(ui.elements.get("btn-process-all").textContent, "Stop Processing");
  await ui.elements.get("btn-process-all").click();
  assert.equal(ui.calls.filter((call) => call.command === "extract_document_snippet").length, 0);
  begin.resolve(17);
  await firstRun;
  assert.equal(ui.calls.find((call) => call.command === "cancel_processing_run").args.processingRunId, 17);
  assert.equal(ui.elements.get("btn-process-all").textContent, "Process Queue");
});

test("remote Ollama endpoint shows document sharing instead of local badge", async () => {
  const ui = createUi({
    behavior: {
      get_prompt_config: () => ({
        system_prompt: "", prompt_template: "", model: "qwen2.5-coder:7b",
        temperature: 0.2, folder_context_enabled: true, provider: "ollama",
        custom_base_url: "https://ollama.example.test/api/generate",
        configured_api_keys: { openai: false, gemini: false, anthropic: false, openrouter: false },
      }),
    },
  });
  await ui.flush();
  const badge = ui.elements.get("provider-badge");
  assert.equal(badge.textContent, "Ollama · Remote");
  assert.equal(badge.classList.contains("cloud"), true);
  assert.match(badge.title, /https:\/\/ollama\.example\.test/);
});


test("imports previous settings through explicit file selection", async () => {
  const imported = {
    system_prompt: "imported system", prompt_template: "{text_snippet}",
    model: "imported-model", temperature: 0.4, folder_context_enabled: false,
    provider: "openai", custom_base_url: "",
    configured_api_keys: { openai: true, gemini: false, anthropic: false, openrouter: false },
  };
  const ui = createUi({
    dialogResult: "/old/config.json",
    behavior: { import_prompt_config: () => imported },
  });
  await ui.flush();
  await ui.elements.get("btn-prompt-settings").click();
  await ui.elements.get("btn-import-settings").click();
  assert.equal(ui.calls.find((call) => call.command === "import_prompt_config").args.filePath, "/old/config.json");
  assert.equal(ui.elements.get("cfg-system-prompt").value, "imported system");
  assert.equal(ui.elements.get("cfg-provider").value, "openai");
  assert.equal(ui.elements.get("provider-badge").textContent, "OpenAI · Cloud");
  assert.match(ui.elements.get("provider-test-status").textContent, /Settings imported/);
});


test("custom cloud endpoints do not show an invalid Ollama warning", async () => {
  for (const provider of ["openai", "openrouter"]) {
    const ui = createUi({ behavior: { get_prompt_config: () => ({
      provider, model: "test-model", custom_base_url: "https://api.example.test/v1/chat/completions",
    }) } });
    await ui.flush();
    const badge = ui.elements.get("provider-badge");
    assert.match(badge.textContent, /Cloud/);
    assert.doesNotMatch(badge.title, /Ollama|invalid/);
    assert.match(badge.title, /https:\/\/api\.example\.test/);
  }
});


test("cancelling settings import keeps the current draft and makes no import request", async () => {
  const ui = createUi({ dialogResult: null });
  await ui.flush();
  await ui.elements.get("btn-prompt-settings").click();
  ui.elements.get("cfg-model").value = "unsaved-draft";
  const badgeBefore = ui.elements.get("provider-badge").textContent;
  await ui.elements.get("btn-import-settings").click();
  assert.equal(ui.calls.some((call) => call.command === "import_prompt_config"), false);
  assert.equal(ui.elements.get("cfg-model").value, "unsaved-draft");
  assert.equal(ui.elements.get("provider-badge").textContent, badgeBefore);
  assert.equal(ui.elements.get("btn-import-settings").disabled, false);
});

test("failed settings import preserves the draft and allows a retry", async () => {
  const ui = createUi({ dialogResult: "/old/config.json", behavior: {
    import_prompt_config: () => { throw new Error("Previous settings file is not valid JSON."); },
  } });
  await ui.flush();
  await ui.elements.get("btn-prompt-settings").click();
  ui.elements.get("cfg-model").value = "unsaved-draft";
  const badgeBefore = ui.elements.get("provider-badge").textContent;
  await ui.elements.get("btn-import-settings").click();
  assert.match(ui.elements.get("provider-test-status").textContent, /Import failed.*not valid JSON/);
  assert.equal(ui.elements.get("cfg-model").value, "unsaved-draft");
  assert.equal(ui.elements.get("provider-badge").textContent, badgeBefore);
  assert.equal(ui.elements.get("prompt-modal").classList.contains("hidden"), false);
  assert.equal(ui.elements.get("btn-import-settings").disabled, false);
});

test("Ollama privacy badge distinguishes loopback from lookalike remote hosts", async () => {
  for (const [endpoint, local] of [
    ["", true], ["http://localhost:11434/api/generate", true],
    ["http://127.0.0.1:11434/api/generate", true], ["http://[::1]:11434/api/generate", true],
    ["https://localhost.example.test/api/generate", false],
  ]) {
    const ui = createUi({ behavior: { get_prompt_config: () => ({
      provider: "ollama", model: "test-model", custom_base_url: endpoint,
    }) } });
    await ui.flush();
    const badge = ui.elements.get("provider-badge");
    assert.equal(badge.textContent, `Ollama · ${local ? "Local" : "Remote"}`, endpoint);
    assert.equal(badge.classList.contains("cloud"), !local, endpoint);
    if (local) assert.match(badge.title, /stay on this computer/);
    else assert.match(badge.title, /https:\/\/localhost\.example\.test/);
  }
});
