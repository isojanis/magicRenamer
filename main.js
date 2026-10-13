import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

// Check if running in Tauri desktop environment
const isTauriEnv = !!(window.__TAURI_INTERNALS__ || window.__TAURI__);

async function invokeTauri(cmd, args) {
  if (isTauriEnv) {
    return await invoke(cmd, args);
  }
  console.log(`[Tauri Mock invoke] ${cmd}:`, args);

  // Mock responses for development preview outside Tauri runtime
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
  if (cmd === "suggest_filename_with_ollama") {
    return "2026-08-15_AcmeCorp_Invoice.pdf";
  }
  if (cmd === "rename_file") {
    return true;
  }
  if (cmd === "get_prompt_config") {
    return {
      system_prompt: "You are a specialized file-naming assistant. Analyze the document snippet and return ONLY a single standardized filename in format: YYYY-MM-DD_Topic.ext (or Topic.ext if no date found). Do not include quotes, explanation, or markdown. Use alphanumeric characters, hyphens, and underscores only.",
      prompt_template: "Target extension: .{extension}\nDocument content snippet:\n\"\"\"\n{text_snippet}\n\"\"\"\nSuggested filename:",
      model: "qwen2.5-coder:7b",
      temperature: 0.2
    };
  }
  if (cmd === "save_prompt_config") {
    return true;
  }
  return null;
}

// State
let queue = [];
let activeIndex = -1;
let isProcessingAll = false;

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
const btnSkip = document.getElementById("btn-skip");
const btnRegenAi = document.getElementById("btn-regen-ai");
const selectOllamaModel = document.getElementById("select-ollama-model");
const previewContent = document.getElementById("preview-content");

// Modal Elements
const btnPromptSettings = document.getElementById("btn-prompt-settings");
const promptModal = document.getElementById("prompt-modal");
const btnCloseModal = document.getElementById("btn-close-modal");
const btnCancelModal = document.getElementById("btn-cancel-modal");
const btnSaveModal = document.getElementById("btn-save-modal");
const btnResetPrompt = document.getElementById("btn-reset-prompt");
const cfgSystemPrompt = document.getElementById("cfg-system-prompt");
const cfgPromptTemplate = document.getElementById("cfg-prompt-template");
const cfgModel = document.getElementById("cfg-model");
const cfgTemperatureSlider = document.getElementById("cfg-temperature-slider");
const cfgTemperatureNum = document.getElementById("cfg-temperature-num");

let currentConfig = null;

// Keyboard shortcuts: Enter to approve
window.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey && document.activeElement !== textSnippet) {
    e.preventDefault();
    if (activeIndex >= 0 && !btnApprove.disabled) {
      approveCurrent();
    }
  }
});

function getSelectedModel() {
  return selectOllamaModel.value || "qwen2.5-coder:7b";
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
        <span class="file-name" title="${item.path}">${item.suggestedName || item.name}</span>
        <small class="file-orig-sub">${item.name}</small>
      </div>
      <span class="status-badge ${item.status}">${item.status}</span>
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
      <div class="file-card-row"><strong>File:</strong> <span>${item.name}</span></div>
      <div class="file-card-row"><strong>Extension:</strong> <span>.${item.extension}</span></div>
      <div class="file-card-row"><strong>Size:</strong> <span>${formatSize(item.size_bytes)}</span></div>
      <div class="file-card-row"><strong>Path:</strong> <span class="path-break">${item.path}</span></div>
      <div class="file-card-row"><strong>Status:</strong> <span class="status-badge ${item.status}">${item.status}</span></div>
    </div>
    <div class="preview-text-box">
      <h4>Document Preview Snippet</h4>
      <pre>${item.snippet ? item.snippet : "(No snippet extracted yet. Run Process Queue to analyze)"}</pre>
    </div>
  `;
}

// Triage single file through extract and Ollama suggestion
async function processFile(index) {
  const item = queue[index];
  if (!item || item.status === "renamed") return;

  try {
    item.status = "processing";
    renderQueue();
    if (activeIndex === index) selectItem(index);

    // Step 1: Extract snippet
    const snippet = await invokeTauri("extract_document_snippet", { filePath: item.path });
    item.snippet = snippet;

    // Step 2: Suggest filename with Ollama
    const suggested = await invokeTauri("suggest_filename_with_ollama", {
      textSnippet: snippet,
      extension: item.extension,
      model: getSelectedModel(),
    });

    item.suggestedName = suggested;
    item.status = "ready";
  } catch (err) {
    console.error(`Failed processing ${item.name}:`, err);
    item.status = "failed";
    item.error = String(err);
  } finally {
    renderQueue();
    if (activeIndex === index) selectItem(index);
  }
}

// Concurrently process all pending files using Promise.allSettled()
async function processAllPending() {
  if (isProcessingAll) return;
  isProcessingAll = true;
  btnProcessAll.disabled = true;

  const pendingIndices = queue
    .map((item, idx) => (item.status === "pending" || item.status === "failed" ? idx : -1))
    .filter((idx) => idx !== -1);

  // Concurrency batch size
  const concurrency = 3;
  for (let i = 0; i < pendingIndices.length; i += concurrency) {
    const chunk = pendingIndices.slice(i, i + concurrency);
    await Promise.allSettled(chunk.map((idx) => processFile(idx)));
  }

  isProcessingAll = false;
  btnProcessAll.disabled = false;
  updateCountsAndButtons();
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
  let selectedFolder = null;

  if (isTauriEnv) {
    try {
      selectedFolder = await openDialog({
        directory: true,
        multiple: false,
        title: "Select Folder to Triage",
      });
    } catch (err) {
      console.error("Failed opening native dialog:", err);
    }
  } else {
    // Fallback prompt for demo/mock mode in standard browser
    selectedFolder = prompt("Enter directory path to scan:", "/home/isojanis/development/magicRenamer");
  }

  if (!selectedFolder) return;

  try {
    const files = await invokeTauri("scan_folder", { folderPath: selectedFolder });
    if (Array.isArray(files)) {
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
async function loadConfig() {
  try {
    const cfg = await invokeTauri("get_prompt_config");
    if (cfg) {
      currentConfig = cfg;
      if (selectOllamaModel && cfg.model) {
        selectOllamaModel.value = cfg.model;
      }
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

function openPromptModal() {
  if (!currentConfig) return;
  cfgSystemPrompt.value = currentConfig.system_prompt || "";
  cfgPromptTemplate.value = currentConfig.prompt_template || "";
  cfgModel.value = currentConfig.model || "qwen2.5-coder:7b";
  syncTemperature(currentConfig.temperature !== undefined ? currentConfig.temperature : 0.2);
  promptModal.classList.remove("hidden");
}

function closePromptModal() {
  promptModal.classList.add("hidden");
}

async function savePromptModal() {
  const tempVal = parseFloat(cfgTemperatureNum.value);
  const updated = {
    system_prompt: cfgSystemPrompt.value.trim(),
    prompt_template: cfgPromptTemplate.value.trim(),
    model: cfgModel.value.trim() || "qwen2.5-coder:7b",
    temperature: !isNaN(tempVal) ? Math.max(0.0, Math.min(1.0, tempVal)) : 0.2,
  };

  try {
    await invokeTauri("save_prompt_config", { config: updated });
    currentConfig = updated;
    if (selectOllamaModel) {
      selectOllamaModel.value = updated.model;
    }
    closePromptModal();
    alert("Configuration saved to config.json!");
  } catch (err) {
    console.error("Failed saving prompt config:", err);
    alert(`Failed to save config: ${err}`);
  }
}

function resetDefaultPrompts() {
  cfgSystemPrompt.value = "You are a specialized file-naming assistant. Analyze the document snippet and return ONLY a single standardized filename in format: YYYY-MM-DD_Topic.ext (or Topic.ext if no date found). Do not include quotes, explanation, or markdown. Use alphanumeric characters, hyphens, and underscores only.";
  cfgPromptTemplate.value = "Target extension: .{extension}\nDocument content snippet:\n\"\"\"\n{text_snippet}\n\"\"\"\nSuggested filename:";
  cfgModel.value = "qwen2.5-coder:7b";
  syncTemperature(0.2);
}

// Event Listeners
btnPromptSettings.addEventListener("click", openPromptModal);
btnCloseModal.addEventListener("click", closePromptModal);
btnCancelModal.addEventListener("click", closePromptModal);
btnSaveModal.addEventListener("click", savePromptModal);
btnResetPrompt.addEventListener("click", resetDefaultPrompts);

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
