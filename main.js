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

// Event Listeners
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
