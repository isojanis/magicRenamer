import { invoke } from "@tauri-apps/api/core";

// Check if running in Tauri desktop environment
const isTauriEnv = !!(window.__TAURI_INTERNALS__ || window.__TAURI__);

async function invokeTauri(cmd, args) {
  if (isTauriEnv) {
    return await invoke(cmd, args);
  }
  if (!["save_prompt_config", "test_ai_provider", "import_previous_settings"].includes(cmd)) {
    console.log(`[Tauri Mock invoke] ${cmd}:`, args);
  }

  // Mock responses for development preview outside Tauri runtime
  if (cmd === "select_folder") return "/mock";
  if (cmd === "scan_folder") {
    return [
      { name: "scan_001.pdf", path: "/mock/scan_001.pdf", extension: "pdf", size_bytes: 142000 },
      { name: "meeting_minutes_final.docx", path: "/mock/meeting_minutes_final.docx", extension: "docx", size_bytes: 84000 },
      { name: "invoice_aug_unpaid.pdf", path: "/mock/invoice_aug_unpaid.pdf", extension: "pdf", size_bytes: 92000 },
    ];
  }
  if (cmd === "extract_document_snippet") {
    return "INVOICE #98412\nDate: 2026-08-15\nBilled to: Acme Corp\nAmount: $1,250.00\nServices rendered: Q3 Security Audit & Review";
  }
  if (cmd === "build_directory_blueprint") {
    return "Selected folder: Mock invoices. 1 visible file; 1 already starts with YYYY-MM-DD. Files in this folder may be unrelated.";
  }
  if (cmd === "begin_processing_run") {
    return ++mockProcessingRunId;
  }
  if (cmd === "cancel_processing_run") {
    return true;
  }
  if (cmd === "suggest_filename_with_ollama") {
    return { new_name: "2026-08-15_AcmeCorp_Invoice.pdf", category: "Invoice", reason: "Matched the invoice naming convention." };
  }
  if (cmd === "rename_file") {
    return true;
  }
  if (cmd === "undo_rename_file") {
    return { restored_path: args.filePath, original_name: "original_file.pdf", source: "mock" };
  }
  if (cmd === "get_prompt_config") {
    return {
      system_prompt: "Use the document and directory context to suggest a concise filename that follows the directory's conventions. Return only valid JSON with string fields new_name, category, and reason. new_name must be a filename, not a path, and retain the target extension.",
      prompt_template: "Directory context:\n{folder_context}\n\nTarget extension: .{extension}\nDocument excerpt:\n\"\"\"\n{text_snippet}\n\"\"\"\nReturn JSON with new_name, category, and reason.",
      model: "qwen2.5-coder:7b",
      temperature: 0.2,
      folder_context_enabled: true,
      provider: "ollama",
      custom_base_url: "",
      configured_api_keys: { openai: false, gemini: false, anthropic: false, openrouter: false }
    };
  }
  if (cmd === "import_previous_settings") {
    return await invokeTauri("get_prompt_config");
  }
  if (cmd === "save_prompt_config") {
    const { api_key, clear_api_key, ...publicConfig } = args.config;
    return { ...publicConfig, configured_api_keys: { [publicConfig.provider]: Boolean(api_key) && !clear_api_key } };
  }
  if (cmd === "test_ai_provider") {
    return `Connected to ${args.config.provider} using ${args.config.model}.`;
  }
  return null;
}

// State
let mockProcessingRunId = 0;
let queue = [];
let activeIndex = -1;
let activeProcessingRun = null;
let activeFolderContext = null;
let folderContextForRun = true;

// DOM Elements
const dropZone = document.getElementById("drop-zone");
const fileQueueList = document.getElementById("file-queue-list");
const queueCount = document.getElementById("queue-count");
const inputOriginalName = document.getElementById("input-original-name");
const inputSuggestedName = document.getElementById("input-suggested-name");
const textSnippet = document.getElementById("text-snippet");
const activeStatus = document.getElementById("active-status");
const btnSelectFolder = document.getElementById("btn-select-folder");
const btnProcessAll = document.getElementById("btn-process-all");
const btnApproveAll = document.getElementById("btn-approve-all");
const btnApprove = document.getElementById("btn-approve");
const btnUndo = document.getElementById("btn-undo");
const btnSkip = document.getElementById("btn-skip");
const btnRegenAi = document.getElementById("btn-regen-ai");
const selectAiModel = document.getElementById("select-ai-model");
const previewContent = document.getElementById("preview-content");
const providerBadge = document.getElementById("provider-badge");

// Modal Elements
const btnPromptSettings = document.getElementById("btn-prompt-settings");
const promptModal = document.getElementById("prompt-modal");
const btnCloseModal = document.getElementById("btn-close-modal");
const btnCancelModal = document.getElementById("btn-cancel-modal");
const btnSaveModal = document.getElementById("btn-save-modal");
const btnResetPrompt = document.getElementById("btn-reset-prompt");
const btnImportSettings = document.getElementById("btn-import-settings");
const cfgSystemPrompt = document.getElementById("cfg-system-prompt");
const cfgPromptTemplate = document.getElementById("cfg-prompt-template");
const cfgModel = document.getElementById("cfg-model");
const cfgModelSuggestions = document.getElementById("cfg-model-suggestions");
const cfgProvider = document.getElementById("cfg-provider");
const cfgApiKey = document.getElementById("cfg-api-key");
const cfgClearApiKey = document.getElementById("cfg-clear-api-key");
const cfgKeyStatus = document.getElementById("cfg-key-status");
const cfgCustomEndpoint = document.getElementById("cfg-custom-endpoint");
const cloudCredentials = document.getElementById("cloud-credentials");
const endpointSetting = document.getElementById("endpoint-setting");
const btnTestProvider = document.getElementById("btn-test-provider");
const providerTestStatus = document.getElementById("provider-test-status");
const cfgTemperatureSlider = document.getElementById("cfg-temperature-slider");
const cfgTemperatureNum = document.getElementById("cfg-temperature-num");
const cfgFolderContextEnabled = document.getElementById("cfg-folder-context-enabled");

let currentConfig = null;
let modalProvider = "ollama";
const modelChoices = {
  ollama: ["qwen2.5-coder:7b", "qwen2.5-coder:14b", "deepseek-r1:7b", "llama3:8b"],
  openai: ["gpt-5.4-mini", "gpt-5.6-terra", "gpt-4o-mini"],
  gemini: ["gemini-2.5-flash", "gemini-2.5-pro"],
  anthropic: ["claude-haiku-5-5", "claude-sonnet-5-5", "claude-haiku-4-5"],
  openrouter: ["openrouter/auto", "openai/gpt-4o-mini"],
};
const providerNames = {
  ollama: "Ollama", openai: "OpenAI", gemini: "Gemini",
  anthropic: "Anthropic", openrouter: "OpenRouter",
};
let draftModels = {};
let draftEndpoints = {};

// Keyboard shortcuts: Enter to approve
window.addEventListener("keydown", (e) => {
  if (!promptModal.classList.contains("hidden")) return;
  if (e.key === "Enter" && !e.shiftKey && document.activeElement !== textSnippet) {
    e.preventDefault();
    if (activeIndex >= 0 && !btnApprove.disabled) {
      approveCurrent();
    }
  }
});

function getSelectedModel() {
  return selectAiModel.value || currentConfig?.model || "qwen2.5-coder:7b";
}

function escapeHtml(value) {
  return String(value ?? "").replace(/[&<>"']/g, (character) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "\"": "&quot;",
    "'": "&#39;",
  })[character]);
}

function updateCountsAndButtons() {
  queueCount.textContent = `${queue.length} files`;
  const readyCount = queue.filter((i) => i.status === "ready").length;
  btnApproveAll.disabled = readyCount === 0;
  btnApproveAll.textContent = readyCount > 0 ? `Approve All Ready (${readyCount})` : "Approve All Ready";
}

function renderQueue() {
  fileQueueList.innerHTML = "";
  updateCountsAndButtons();

  queue.forEach((item, index) => {
    const li = document.createElement("li");
    li.className = `file-item ${index === activeIndex ? "active" : ""}`;
    li.innerHTML = `
      <div class="file-item-info">
        <span class="file-name" title="${escapeHtml(item.path)}">${escapeHtml(item.suggestedName || item.name)}</span>
        <small class="file-orig-sub">${escapeHtml(item.name)}</small>
      </div>
      <span class="status-badge ${escapeHtml(item.status)}">${escapeHtml(item.status)}</span>
    `;
    li.addEventListener("click", () => selectItem(index));
    fileQueueList.appendChild(li);
  });
}

function selectItem(index) {
  if (index < 0 || index >= queue.length) return;
  activeIndex = index;
  renderQueue();

  const item = queue[index];
  inputOriginalName.value = item.name;
  inputSuggestedName.value = item.suggestedName || "";
  textSnippet.value = item.snippet || "";
  activeStatus.textContent = item.status.toUpperCase();
  activeStatus.className = `status-badge ${item.status}`;

  // Enable/disable action buttons
  btnApprove.disabled = item.status === "renamed";
  if (btnUndo) {
    btnUndo.disabled = item.status !== "renamed";
  }

  // Update Right Column Preview
  updatePreview(item);
}

function updatePreview(item) {
  if (!item) {
    previewContent.innerHTML = `<div class="preview-empty">Select a file to inspect details</div>`;
    return;
  }

  const formatSize = (bytes) => {
    if (!bytes) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  };

  previewContent.innerHTML = `
    <div class="file-card">
      <div class="file-card-row"><strong>File:</strong> <span>${escapeHtml(item.name)}</span></div>
      <div class="file-card-row"><strong>Extension:</strong> <span>.${escapeHtml(item.extension)}</span></div>
      <div class="file-card-row"><strong>Size:</strong> <span>${formatSize(item.size_bytes)}</span></div>
      <div class="file-card-row"><strong>Path:</strong> <span class="path-break">${escapeHtml(item.path)}</span></div>
      <div class="file-card-row"><strong>Status:</strong> <span class="status-badge ${escapeHtml(item.status)}">${escapeHtml(item.status)}</span></div>
      ${item.category ? `<div class="file-card-row"><strong>Category:</strong> <span>${escapeHtml(item.category)}</span></div>` : ""}
      ${item.reason ? `<div class="file-card-row"><strong>AI rationale:</strong> <span>${escapeHtml(item.reason)}</span></div>` : ""}
      ${item.error ? `<div class="file-card-row"><strong>Issue:</strong> <span>${escapeHtml(item.error)}</span></div>` : ""}
    </div>
    <div class="preview-text-box">
      <h4>Document Preview Snippet</h4>
      <pre>${escapeHtml(item.snippet ? item.snippet : "(No snippet extracted yet. Run Process Queue to analyze)")}</pre>
    </div>
  `;
}

// Triage single file through extraction and the selected AI provider.
async function processFile(index, run = null) {
  const item = queue[index];
  if (!item || item.status === "renamed" || run?.cancelled) return;
  run?.inFlight.add(index);
  if (run) item.processingRun = run;

  try {
    item.status = "processing";
    item.error = "";
    item.suggestedName = "";
    item.category = "";
    item.reason = "";
    renderQueue();
    if (activeIndex === index) selectItem(index);

    const snippet = await invokeTauri("extract_document_snippet", { filePath: item.path });
    if (run?.cancelled || (run && item.processingRun !== run)) return;
    item.snippet = snippet;

    const suggested = await invokeTauri("suggest_filename_with_ollama", {
      textSnippet: snippet,
      extension: item.extension,
      model: getSelectedModel(),
      folderContext: folderContextForRun ? activeFolderContext : null,
      filePath: item.path,
      processingRunId: run?.id ?? null,
    });
    if (run?.cancelled || (run && item.processingRun !== run)) return;

    item.suggestedName = suggested.new_name || "";
    item.category = suggested.category || "";
    item.reason = suggested.reason || "";
    item.status = "ready";
  } catch (err) {
    if (!run?.cancelled && (!run || item.processingRun === run)) {
      console.error(`Failed processing ${item.name}:`, err);
      item.status = "failed";
      item.error = String(err);
    }
  } finally {
    run?.inFlight.delete(index);
    if (!run || item.processingRun === run) {
      if (run?.cancelled && item.status === "processing") item.status = "pending";
      if (run) delete item.processingRun;
      renderQueue();
      if (activeIndex === index) selectItem(index);
    }
  }
}

function updateProcessButton() {
  btnProcessAll.disabled = Boolean(activeProcessingRun?.cancelled);
  btnProcessAll.classList.toggle("stop-processing", Boolean(activeProcessingRun));
  btnProcessAll.textContent = activeProcessingRun
    ? activeProcessingRun.cancelled ? "Stopping…" : "Stop Processing"
    : "Process Queue";
  btnProcessAll.title = activeProcessingRun
    ? "Stop starting new files and cancel active AI requests"
    : "Run AI snippet extraction and suggestions for pending files";
  btnSelectFolder.disabled = Boolean(activeProcessingRun);
  btnRegenAi.disabled = Boolean(activeProcessingRun);
  btnPromptSettings.disabled = Boolean(activeProcessingRun);
}

async function requestBackendStop(run) {
  if (run.id === null || run.stopSent) return;
  run.stopSent = true;
  try {
    await invokeTauri("cancel_processing_run", { processingRunId: run.id });
  } catch (err) {
    console.error("Failed to cancel active AI requests:", err);
    alert("Queue stopped, but active AI requests may continue until they finish.");
  }
}

async function stopProcessingRun(run) {
  if (run.cancelled) return;
  run.cancelled = true;
  for (const index of run.inFlight) {
    const item = queue[index];
    if (item?.processingRun === run) {
      if (item.status === "processing") item.status = "pending";
      item.error = "";
      delete item.processingRun;
    }
  }
  run.resolveCancelled();
  renderQueue();
  if (activeIndex >= 0) selectItem(activeIndex);
  updateProcessButton();
  await requestBackendStop(run);
}

// Process at most three files at a time; the same button stops the run.
async function processAllPending() {
  if (activeProcessingRun) {
    await stopProcessingRun(activeProcessingRun);
    return;
  }
  const pendingIndices = queue
    .map((item, idx) => (item.status === "pending" || item.status === "failed" ? idx : -1))
    .filter((idx) => idx !== -1);
  if (pendingIndices.length === 0) return;

  let resolveCancelled;
  const cancelledPromise = new Promise((resolve) => { resolveCancelled = resolve; });
  const run = { id: null, cancelled: false, stopSent: false, inFlight: new Set(), cancelledPromise, resolveCancelled };
  activeProcessingRun = run;
  updateProcessButton();
  try {
    run.id = await invokeTauri("begin_processing_run");
    if (!Number.isSafeInteger(run.id) || run.id <= 0) {
      throw new Error("Could not start a cancellable processing run.");
    }
    if (run.cancelled) {
      await requestBackendStop(run);
      return;
    }
    const concurrency = 3;
    for (let i = 0; i < pendingIndices.length && !run.cancelled; i += concurrency) {
      const chunk = pendingIndices.slice(i, i + concurrency);
      const batch = Promise.allSettled(chunk.map((idx) => processFile(idx, run)));
      await Promise.race([batch, run.cancelledPromise]);
    }
  } catch (err) {
    if (!run.cancelled) {
      console.error("Failed to start queue processing:", err);
      alert(`Could not process queue: ${err}`);
    }
  } finally {
    if (activeProcessingRun === run) activeProcessingRun = null;
    updateProcessButton();
    updateCountsAndButtons();
  }
}

async function approveCurrent() {
  if (activeIndex < 0 || activeIndex >= queue.length) return;
  const item = queue[activeIndex];
  const newName = inputSuggestedName.value.trim();
  if (!newName) return;

  try {
    const lastSlash = item.path.lastIndexOf("/");
    const dir = lastSlash !== -1 ? item.path.substring(0, lastSlash + 1) : "";
    const newPath = dir + newName;

    const actualNewPath = await invokeTauri("rename_file", { oldPath: item.path, newPath });
    const finalPath = actualNewPath || newPath;
    const finalName = finalPath.substring(finalPath.lastIndexOf("/") + 1);

    item.status = "renamed";
    item.name = finalName;
    item.path = finalPath;

    // Advance to next ready file
    const nextIndex = queue.findIndex((f, idx) => idx > activeIndex && f.status === "ready");
    if (nextIndex !== -1) {
      selectItem(nextIndex);
    } else {
      renderQueue();
      selectItem(activeIndex);
    }
  } catch (err) {
    console.error("Rename failed:", err);
    alert(`Rename failed: ${err}`);
  }
}

async function undoCurrent() {
  if (activeIndex < 0 || activeIndex >= queue.length) return;
  const item = queue[activeIndex];
  if (item.status !== "renamed") return;

  try {
    const res = await invokeTauri("undo_rename_file", { filePath: item.path });
    if (res && res.restored_path) {
      item.path = res.restored_path;
      item.name = res.original_name;
      item.status = "ready"; // Mark ready so user can re-evaluate or rename again if desired
      renderQueue();
      selectItem(activeIndex);
    }
  } catch (err) {
    console.error("Undo failed:", err);
    alert(`Undo failed: ${err}`);
  }
}


async function approveAllReady() {
  const readyItems = queue.filter((i) => i.status === "ready");
  for (const item of readyItems) {
    const lastSlash = item.path.lastIndexOf("/");
    const dir = lastSlash !== -1 ? item.path.substring(0, lastSlash + 1) : "";
    const newPath = dir + (item.suggestedName || item.name);
    try {
      const actualNewPath = await invokeTauri("rename_file", { oldPath: item.path, newPath });
      const finalPath = actualNewPath || newPath;
      const finalName = finalPath.substring(finalPath.lastIndexOf("/") + 1);
      item.status = "renamed";
      item.name = finalName;
      item.path = finalPath;
    } catch (err) {
      console.error(`Failed renaming ${item.name}:`, err);
      item.status = "failed";
    }
  }
  renderQueue();
  if (activeIndex >= 0) selectItem(activeIndex);
}

// Folder Selection Handler
async function chooseFolder() {
  if (activeProcessingRun) return;
  let selectedFolder = null;

  if (isTauriEnv) {
    try {
      selectedFolder = await invokeTauri("select_folder");
    } catch (err) {
      console.error("Failed opening native dialog:", err);
    }
  } else {
    // Fallback prompt for demo/mock mode in standard browser
    selectedFolder = prompt("Enter directory path to scan:", "/home/isojanis/development/magicRenamer");
  }

  if (!selectedFolder || activeProcessingRun) return;

  try {
    const files = await invokeTauri("scan_folder", { folderPath: selectedFolder });
    if (Array.isArray(files) && !activeProcessingRun) {
      folderContextForRun = Boolean(currentConfig?.folder_context_enabled ?? true);
      if (folderContextForRun && files.length > 500) {
        const keepContext = window.confirm(`Large folder detected (${files.length} files). Context checks nearby files for each document. Press OK to continue with context, or Cancel to disable context for this run.`);
        if (!keepContext) folderContextForRun = false;
      }
      activeFolderContext = folderContextForRun
        ? await invokeTauri("build_directory_blueprint", { folderPath: selectedFolder })
        : null;
      queue = files.map((f) => ({
        ...f,
        status: "pending",
        suggestedName: "",
        snippet: "",
      }));
      activeIndex = queue.length > 0 ? 0 : -1;
      renderQueue();
      if (activeIndex >= 0) selectItem(0);
    }
  } catch (err) {
    console.error("Failed scanning folder:", err);
    alert(`Failed scanning folder: ${err}`);
  }
}

// Prompt Config Functions
function fillModelOptions(target, provider, activeModel) {
  target.innerHTML = "";
  const models = [...(modelChoices[provider] || [])];
  if (activeModel && !models.includes(activeModel)) models.push(activeModel);
  for (const model of models) {
    const option = document.createElement("option");
    option.value = model;
    option.textContent = model;
    target.appendChild(option);
  }
  if (target === selectAiModel) target.value = activeModel || models[0] || "";
}

function applyActiveProvider(cfg) {
  const provider = modelChoices[cfg.provider] ? cfg.provider : "ollama";
  fillModelOptions(selectAiModel, provider, cfg.model);
  let endpoint = null;
  if (cfg.custom_base_url) {
    try { endpoint = new URL(cfg.custom_base_url); } catch { /* Backend rejects invalid endpoints. */ }
  }
  const ollamaIsRemote = provider === "ollama" && Boolean(cfg.custom_base_url)
    && (!endpoint || !["localhost", "127.0.0.1", "[::1]"].includes(endpoint.hostname));
  providerBadge.textContent = provider === "ollama"
    ? `Ollama · ${ollamaIsRemote ? "Remote" : "Local"}`
    : `${providerNames[provider]} · Cloud`;
  providerBadge.classList.toggle("cloud", provider !== "ollama" || Boolean(ollamaIsRemote));
  providerBadge.title = ollamaIsRemote
    ? endpoint
      ? `Document excerpts and naming context are sent to ${endpoint.origin} for AI suggestions.`
      : "Custom Ollama endpoint is invalid. Check it in Settings."
    : provider === "ollama"
      ? "Document excerpts stay on this computer for AI suggestions."
      : `Document excerpts and naming context are sent to ${endpoint?.origin || providerNames[provider]} for AI suggestions.`;
}

async function loadConfig() {
  try {
    const cfg = await invokeTauri("get_prompt_config");
    if (cfg) {
      currentConfig = cfg;
      applyActiveProvider(cfg);
    }
  } catch (err) {
    console.error("Failed to load prompt config:", err);
  }
}

function syncTemperature(val) {
  let num = parseFloat(val);
  if (isNaN(num)) num = 0.2;
  num = Math.max(0.0, Math.min(1.0, num));
  const formatted = num.toFixed(2);
  cfgTemperatureSlider.value = formatted;
  cfgTemperatureNum.value = formatted;
}

cfgTemperatureSlider.addEventListener("input", (e) => {
  cfgTemperatureNum.value = parseFloat(e.target.value).toFixed(2);
});

cfgTemperatureNum.addEventListener("input", (e) => {
  let val = parseFloat(e.target.value);
  if (!isNaN(val)) {
    val = Math.max(0.0, Math.min(1.0, val));
    cfgTemperatureSlider.value = val.toFixed(2);
  }
});

cfgTemperatureNum.addEventListener("blur", (e) => {
  syncTemperature(e.target.value);
});

function showProviderFields() {
  const provider = cfgProvider.value;
  fillModelOptions(cfgModelSuggestions, provider, "");
  cloudCredentials.classList.toggle("hidden", provider === "ollama");
  endpointSetting.classList.toggle("hidden", !["ollama", "openai", "openrouter"].includes(provider));
  cfgKeyStatus.textContent = currentConfig?.configured_api_keys?.[provider]
    ? "A key is configured. Leave this blank to keep it."
    : "No key saved. An environment variable can also supply one.";
  cfgApiKey.value = "";
  cfgClearApiKey.checked = false;
  providerTestStatus.textContent = "";
}

cfgProvider.addEventListener("change", () => {
  draftModels[modalProvider] = cfgModel.value.trim();
  draftEndpoints[modalProvider] = cfgCustomEndpoint.value.trim();
  modalProvider = cfgProvider.value;
  cfgModel.value = draftModels[modalProvider] || modelChoices[modalProvider]?.[0] || "";
  cfgCustomEndpoint.value = draftEndpoints[modalProvider] || "";
  showProviderFields();
});

function openPromptModal() {
  if (!currentConfig) return;
  cfgSystemPrompt.value = currentConfig.system_prompt || "";
  cfgPromptTemplate.value = currentConfig.prompt_template || "";
  modalProvider = modelChoices[currentConfig.provider] ? currentConfig.provider : "ollama";
  cfgProvider.value = modalProvider;
  draftModels = { [modalProvider]: currentConfig.model || modelChoices[modalProvider][0] };
  draftEndpoints = { [modalProvider]: currentConfig.custom_base_url || "" };
  cfgModel.value = draftModels[modalProvider];
  cfgCustomEndpoint.value = draftEndpoints[modalProvider];
  syncTemperature(currentConfig.temperature !== undefined ? currentConfig.temperature : 0.2);
  cfgFolderContextEnabled.checked = currentConfig.folder_context_enabled !== false;
  showProviderFields();
  promptModal.classList.remove("hidden");
}

function closePromptModal() {
  promptModal.classList.add("hidden");
}

function buildConfigUpdate() {
  const tempVal = parseFloat(cfgTemperatureNum.value);
  return {
    system_prompt: cfgSystemPrompt.value.trim(),
    prompt_template: cfgPromptTemplate.value.trim(),
    model: cfgModel.value.trim() || modelChoices[cfgProvider.value]?.[0] || "",
    temperature: !isNaN(tempVal) ? Math.max(0.0, Math.min(1.0, tempVal)) : 0.2,
    folder_context_enabled: cfgFolderContextEnabled.checked,
    provider: cfgProvider.value,
    custom_base_url: endpointSetting.classList.contains("hidden") ? "" : cfgCustomEndpoint.value.trim(),
    api_key: cfgApiKey.value.trim(),
    clear_api_key: cfgClearApiKey.checked,
  };
}

async function importPreviousSettings() {
  try {
    btnImportSettings.disabled = true;
    const imported = await invokeTauri("import_previous_settings");
    if (!imported) return;
    currentConfig = imported;
    applyActiveProvider(imported);
    openPromptModal();
    providerTestStatus.textContent = "Settings imported. Review them, then remove the old API key file.";
  } catch (err) {
    providerTestStatus.textContent = `Import failed: ${String(err)}`;
  } finally {
    btnImportSettings.disabled = false;
  }
}

async function savePromptModal() {
  const updated = buildConfigUpdate();
  try {
    const saved = await invokeTauri("save_prompt_config", { config: updated });
    currentConfig = saved && typeof saved === "object" ? saved : {
      ...updated,
      configured_api_keys: {
        ...(currentConfig?.configured_api_keys || {}),
        [updated.provider]: updated.clear_api_key ? false : Boolean(updated.api_key) || Boolean(currentConfig?.configured_api_keys?.[updated.provider]),
      },
    };
    applyActiveProvider(currentConfig);
    cfgApiKey.value = "";
    closePromptModal();
  } catch (err) {
    console.error("Failed saving prompt config:", err);
    providerTestStatus.textContent = `Save failed: ${String(err)}`;
  }
}

async function testProviderConnection() {
  const draft = buildConfigUpdate();
  btnTestProvider.disabled = true;
  providerTestStatus.textContent = "Checking connection…";
  try {
    providerTestStatus.textContent = await invokeTauri("test_ai_provider", { config: draft });
  } catch (err) {
    providerTestStatus.textContent = `Connection failed: ${String(err)}`;
  } finally {
    btnTestProvider.disabled = false;
  }
}

function resetDefaultPrompts() {
  cfgSystemPrompt.value = "Use the document and directory context to suggest a concise filename that follows the directory's conventions. Return only valid JSON with string fields new_name, category, and reason. new_name must be a filename, not a path, and retain the target extension.";
  cfgPromptTemplate.value = "Directory context:\n{folder_context}\n\nTarget extension: .{extension}\nDocument excerpt:\n\"\"\"\n{text_snippet}\n\"\"\"\nReturn JSON with new_name, category, and reason.";
  cfgModel.value = modelChoices[cfgProvider.value]?.[0] || "qwen2.5-coder:7b";
  syncTemperature(0.2);
  cfgFolderContextEnabled.checked = true;
}

// Event Listeners
btnPromptSettings.addEventListener("click", openPromptModal);
btnCloseModal.addEventListener("click", closePromptModal);
btnCancelModal.addEventListener("click", closePromptModal);
btnSaveModal.addEventListener("click", savePromptModal);
btnResetPrompt.addEventListener("click", resetDefaultPrompts);
btnImportSettings.addEventListener("click", importPreviousSettings);
btnTestProvider.addEventListener("click", testProviderConnection);

promptModal.addEventListener("click", (e) => {
  if (e.target === promptModal) closePromptModal();
});

// Column Resizing Logic
const paneLeft = document.getElementById("pane-left");
const paneRight = document.getElementById("pane-right");
const resizerLeft = document.getElementById("resizer-left");
const resizerRight = document.getElementById("resizer-right");

// Restore saved widths if present
const savedLeftWidth = localStorage.getItem("magic_renamer_left_width");
const savedRightWidth = localStorage.getItem("magic_renamer_right_width");
if (savedLeftWidth && paneLeft) paneLeft.style.width = `${savedLeftWidth}px`;
if (savedRightWidth && paneRight) paneRight.style.width = `${savedRightWidth}px`;

function setupResizer(resizer, targetPane, isRightSide) {
  if (!resizer || !targetPane) return;

  let startX = 0;
  let startWidth = 0;

  const onMouseMove = (e) => {
    const delta = e.clientX - startX;
    let newWidth = isRightSide ? startWidth - delta : startWidth + delta;

    // Boundary constraints
    const minWidth = isRightSide ? 220 : 200;
    const maxWidth = isRightSide ? 650 : 600;
    newWidth = Math.max(minWidth, Math.min(maxWidth, newWidth));

    targetPane.style.width = `${newWidth}px`;
    localStorage.setItem(isRightSide ? "magic_renamer_right_width" : "magic_renamer_left_width", newWidth);
  };

  const onMouseUp = () => {
    resizer.classList.remove("resizing");
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
    window.removeEventListener("mousemove", onMouseMove);
    window.removeEventListener("mouseup", onMouseUp);
  };

  resizer.addEventListener("mousedown", (e) => {
    e.preventDefault();
    startX = e.clientX;
    startWidth = targetPane.getBoundingClientRect().width;
    resizer.classList.add("resizing");
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  });
}

setupResizer(resizerLeft, paneLeft, false);
setupResizer(resizerRight, paneRight, true);

// Load config at initialization
loadConfig();

btnSelectFolder.addEventListener("click", chooseFolder);
btnProcessAll.addEventListener("click", processAllPending);
btnApprove.addEventListener("click", approveCurrent);
if (btnUndo) btnUndo.addEventListener("click", undoCurrent);
btnApproveAll.addEventListener("click", approveAllReady);
btnSkip.addEventListener("click", () => {
  if (activeIndex < queue.length - 1) {
    selectItem(activeIndex + 1);
  }
});

btnRegenAi.addEventListener("click", () => {
  if (activeIndex >= 0) {
    processFile(activeIndex);
  }
});

// Drag & drop support
dropZone.addEventListener("click", chooseFolder);
dropZone.addEventListener("dragover", (e) => {
  e.preventDefault();
  dropZone.classList.add("dragover");
});
dropZone.addEventListener("dragleave", () => {
  dropZone.classList.remove("dragover");
});
dropZone.addEventListener("drop", (e) => {
  e.preventDefault();
  dropZone.classList.remove("dragover");
  chooseFolder();
});

// Initial mock load only when running outside Tauri desktop environment
if (!isTauriEnv) {
  invokeTauri("scan_folder", { folderPath: "/mock" }).then((files) => {
    if (files && queue.length === 0) {
      queue = files.map((f) => ({
        ...f,
        status: "pending",
        suggestedName: "",
        snippet: "",
      }));
      activeIndex = 0;
      renderQueue();
      selectItem(0);
    }
  });
}
