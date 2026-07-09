// =============================================================================
// File: web/app.js
// Purpose:
//   Browser-side controller for dashboard polling, Ask OSAI, history, knowledge,
//   plugins, actions, card closing, sidebar collapsing, and Keep eyes on pinning.
//
// Where this fits in OSAI:
//   Connects the HTML UI to the Rust REST API. No Rust API contract is changed.
// =============================================================================
const $ = (id) => document.getElementById(id);

const STORAGE_KEYS = {
  watch: "osaiKeepEyesOn.v1",
  hidden: "osaiHiddenSections.v1",
  sidebar: "osaiSidebarCollapsed.v1",
};

function bytes(value) {
  if (!Number.isFinite(value)) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit++;
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[unit]}`;
}

function pct(value) {
  if (!Number.isFinite(value)) return "0%";
  return `${value.toFixed(1)}%`;
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function compactText(value, max = 170) {
  const text = String(value ?? "").replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

function chip(text, className = "") {
  return `<span class="chip ${className}">${escapeHtml(text)}</span>`;
}

function bar(value) {
  const safe = Math.max(0, Math.min(100, value || 0));
  return `<div class="progress"><div style="width:${safe}%"></div></div>`;
}

function item(title, detail, extra = "") {
  return `<div class="item" data-watch-title="${escapeHtml(title)}">
    <strong>${title}</strong>
    <span>${detail}</span>
    ${extra}
  </div>`;
}

function card(title, metric, detail) {
  return `<div class="card metric-card" data-watch-title="${escapeHtml(title)}">
    <p>${escapeHtml(title)}</p>
    <div class="metric">${escapeHtml(metric)}</div>
    <p>${escapeHtml(detail)}</p>
  </div>`;
}

function authHeaders() {
  const token = localStorage.getItem("osaiToken");
  return token ? { "X-OSAI-Token": token } : {};
}

const quickQuestions = [
  ["whats the update ?", "Server update"],
  ["cpu core status", "CPU"],
  ["memory ram status", "Memory"],
  ["disk storage usage", "Storage"],
  ["network and open ports", "Ports"],
  ["top processes", "Processes"],
  ["services and databases", "Apps & DB"],
  ["current findings", "Findings"],
];

const optionalViews = [
  ["findings", "Findings"],
  ["compute", "Compute"],
  ["storage", "Storage"],
  ["network", "Network & Ports"],
  ["processes", "Top Processes"],
  ["apps", "Apps & DB"],
];

let aiRequested = false;
let aiState = "off";
let currentSnapshot = null;
let lastAskData = null;
const pinnedInsights = new Map();
let keepEyesItems = loadJson(STORAGE_KEYS.watch, []);
let hiddenSections = new Set(loadJson(STORAGE_KEYS.hidden, []));

function loadJson(key, fallback) {
  try {
    const raw = localStorage.getItem(key);
    return raw ? JSON.parse(raw) : fallback;
  } catch (_) {
    return fallback;
  }
}

function saveJson(key, value) {
  localStorage.setItem(key, JSON.stringify(value));
}

async function apiFetch(endpoint, options = {}) {
  const headers = {
    ...authHeaders(),
    ...(options.headers || {}),
  };
  const response = await fetch(endpoint, { ...options, headers });

  if (response.status === 401) {
    const token = window.prompt("OSAI dashboard token required");
    if (token) {
      localStorage.setItem("osaiToken", token);
      return apiFetch(endpoint, options);
    }
  }

  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `HTTP ${response.status}`);
  }

  return response.json();
}

async function loadSnapshot(force = false) {
  const endpoint = force ? "/api/scan" : "/api/snapshot";
  const data = await apiFetch(endpoint, { method: force ? "POST" : "GET" });
  render(data);
  await loadHistory();
}

async function loadHistory() {
  const history = await apiFetch("/api/history?limit=12");
  $("historyList").innerHTML = history.length
    ? history.map((h) => item(
        `${severity(h.highest_severity)} ${new Date(h.generated_at).toLocaleString()}`,
        `${escapeHtml(h.hostname)} • findings ${h.finding_count} • warn ${h.warn_count} • critical ${h.critical_count}`
      )).join("")
    : item("No scan history", "A history record is created after each scan.");
  decorateCards($("historyList"));
}

async function askReasoning() {
  const question = $("reasonQuestion").value.trim() || "whats the update ?";
  $("reasonQuestion").value = question;
  if (!question) return;

  $("reasonOutput").innerHTML = item(
    "Working",
    aiRequested
      ? "Rust is preparing a focused answer. AI will refine only if the reasoning layer is ready."
      : "Rust is preparing a deterministic answer. AI is off."
  );
  decorateCards($("reasonOutput"));

  const data = await apiFetch("/api/ask", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ question, use_ai: aiRequested }),
  });

  lastAskData = { question, ...data };
  updateAiFromAsk(data);
  updatePinnedInsights(data.query_insights || []);
  renderImportantList(currentSnapshot);

  const parts = [
    renderInferenceStatus(data.inference_status),
    renderQueryInsights(data.query_insights || []),
    item("Answer", `<pre>${escapeHtml(data.answer)}</pre>`),
    renderFeedbackButtons(),
  ].filter(Boolean);

  $("reasonOutput").innerHTML = parts.join("");
  decorateCards($("reasonOutput"));
  await loadCogneeLifecycle();
}

function renderFeedbackButtons() {
  return `<div class="item" data-watch-title="Improve Cognee memory">
    <strong>Improve Cognee memory</strong>
    <span>Mark whether this answer helped. OSAI will remember feedback and try Cognee improve.</span>
    <div class="feedback-row">
      <button data-feedback="helpful" type="button">Helpful</button>
      <button data-feedback="not helpful" type="button">Not helpful</button>
      <button data-feedback="needs more detail" type="button">Needs more detail</button>
      <button data-feedback="resolved" data-resolved="true" type="button">Resolved</button>
      <button data-feedback="still failing" type="button">Still failing</button>
    </div>
  </div>`;
}

async function sendMemoryFeedback(feedback, resolved = false) {
  if (!lastAskData) return;
  const result = await apiFetch("/api/cognee/feedback", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      question: lastAskData.question,
      answer: lastAskData.answer,
      feedback,
      resolved,
      note: `mode=${lastAskData.mode}; ai_used=${lastAskData.ai_used}`,
    }),
  });
  $("memoryLifecycleOutput").innerHTML = item("Feedback stored", escapeHtml(result.detail), `<div class="small">Dataset: ${escapeHtml(result.dataset)}</div>`);
  decorateCards($("memoryLifecycleOutput"));
}

async function loadCogneeLifecycle() {
  const status = await apiFetch("/api/cognee/lifecycle");
  $("memoryLifecycleOutput").innerHTML = [
    item("Lifecycle health", `${escapeHtml(status.health)} • ${escapeHtml(status.last_detail)}`),
    item("Dataset", escapeHtml(status.dataset), `<div class="small">API: ${escapeHtml(status.api_url)}</div>`),
    item("Operations", `Remember: ${escapeHtml(status.remember)} • Recall: ${escapeHtml(status.recall)}`),
    item("Improve / Forget", `${escapeHtml(status.improve)} • ${escapeHtml(status.forget)}`),
  ].join("");
  decorateCards($("memoryLifecycleOutput"));
}

async function forgetCogneeDataset() {
  const ok = window.confirm("Forget the configured Cognee dataset? Use this only for cleanup, stale memory, noisy memory, or secret-removal workflows.");
  if (!ok) return;
  const result = await apiFetch("/api/cognee/forget", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      confirm: true,
      reason: "operator requested forget from OSAI dashboard",
    }),
  });
  $("memoryLifecycleOutput").innerHTML = item("Forget result", escapeHtml(result.detail), `<div class="small">Dataset: ${escapeHtml(result.dataset)}</div>`);
  decorateCards($("memoryLifecycleOutput"));
}

function renderQueryInsights(insights) {
  const currentIds = new Set(insights.map((insight) => insight.id));
  const preserved = Array.from(pinnedInsights.values()).filter((insight) => !currentIds.has(insight.id));

  if (!insights.length && !preserved.length) {
    return item("Rust signal match", "No direct signal matched yet. Try CPU, memory, disk, ports, processes, services, Kubernetes, GitLab, or findings.");
  }

  const section = (title, items) => items.length
    ? `<div class="insight-section" data-watch-title="${escapeHtml(title)}">
        <div class="small section-label">${escapeHtml(title)}</div>
        <div class="insight-grid">${items.map(renderInsightCard).join("")}</div>
      </div>`
    : "";

  return [
    section("Still important", preserved),
    section("Current answer", insights),
  ].join("");
}

function renderInsightCard(insight) {
  const metrics = (insight.metrics || []).map((metric) => `
    <div class="insight-metric">
      <div>
        <strong>${escapeHtml(metric.label)}</strong>
        <span>${escapeHtml(metric.value)}${escapeHtml(metric.unit || "")}</span>
      </div>
      ${Number.isFinite(metric.percent) ? bar(metric.percent) : ""}
    </div>
  `).join("");
  const checks = renderManualChecks(insight.manual_checks || []);

  return `<div class="insight-card ${escapeHtml(insight.severity)}" data-watch-title="${escapeHtml(insight.label)}">
    <div class="insight-head">
      <strong>${escapeHtml(insight.label)}</strong>
      ${chip(insight.status, insight.severity)}
    </div>
    <div class="insight-labels">
      <button class="insight-query" data-query="${escapeHtml(deeperPrompt(insight.id))}" type="button">ask: ${escapeHtml(deeperPrompt(insight.id))}</button>
      ${chip(`signal: ${insight.id}`)}
    </div>
    <p>${escapeHtml(insight.summary)}</p>
    <div class="insight-metrics">${metrics}</div>
    ${checks}
    <div class="small">Recommendation: ${escapeHtml(insight.recommendation)}</div>
  </div>`;
}

function renderInferenceStatus(status) {
  if (!aiRequested || !status || status.ready || status.status === "disabled_by_user") return "";
  return `<div class="item inference-status compact-alert" data-watch-title="AI not available">
    <strong>${chip("AI not available", "warn")} Reasoning layer</strong>
    <span>${escapeHtml(status.status || "unavailable")}. Rust fallback answered the question.</span>
    <div class="small">Endpoint: ${escapeHtml(status.endpoint || "not configured")}</div>
  </div>`;
}

function renderManualChecks(commands) {
  if (!commands.length) return "";
  return `<details class="manual-checks">
    <summary>Safe manual checks</summary>
    ${commands.map((command) => `<code>${escapeHtml(command)}</code>`).join("")}
  </details>`;
}

function deeperPrompt(id) {
  const prompts = {
    server_overview: "whats the update ?",
    cpu_core: "cpu core status",
    memory: "memory ram status",
    storage: "disk storage usage",
    network_ports: "network and open ports",
    processes: "top processes",
    services_apps_databases: "services and databases",
    findings: "current findings",
    kubernetes: "kubernetes status",
    gitlab: "gitlab status",
  };
  return prompts[id] || id;
}

async function loadActions() {
  const actions = await apiFetch("/api/actions");
  $("actionsList").innerHTML = actions.length
    ? actions.map(renderAction).join("")
    : item("No actions", "Propose a read-only check or a repair action. Repair actions stay pending until approved.");
  decorateCards($("actionsList"));
}

function renderAction(action) {
  const cls = action.status === "blocked" || action.status === "failed" ? "danger" : action.status === "proposed" ? "warn" : "safe";
  const buttons = [
    action.status === "proposed" ? `<button data-approve="${escapeHtml(action.id)}" type="button">Approve</button>` : "",
    action.status === "approved" ? `<button data-run="${escapeHtml(action.id)}" type="button">Run</button>` : "",
  ].join(" ");
  const output = action.output
    ? `<pre>${escapeHtml(action.output.stdout || action.output.stderr || "No output")}</pre>`
    : "";

  return `<div class="item" data-watch-title="${escapeHtml(action.command)}">
    <strong>${chip(action.status, cls)} ${escapeHtml(action.command)} ${escapeHtml((action.args || []).join(" "))}</strong>
    <span>${escapeHtml(action.kind)} • ${escapeHtml(action.validation_message)}</span>
    <div class="small">Reason: ${escapeHtml(action.reason)}</div>
    <div class="actions inline-actions">${buttons}</div>
    ${output}
  </div>`;
}

async function proposeAction() {
  const reason = $("actionReason").value.trim() || "operator requested action";
  const command = $("actionCommand").value.trim();
  const args = $("actionArgs").value.trim().split(/\s+/).filter(Boolean);
  const kind = $("actionKind").value;

  if (!command) return;

  await apiFetch("/api/actions/propose", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ reason, command, args, kind }),
  });

  await loadActions();
}

async function approveAction(id) {
  await apiFetch(`/api/actions/${id}/approve`, { method: "POST" });
  await loadActions();
}

async function runAction(id) {
  await apiFetch(`/api/actions/${id}/run`, { method: "POST" });
  await loadActions();
}

function render(data) {
  currentSnapshot = data;
  updateVisualSeverity(data);
  publishOsaiSnapshot(data);
  applyHiddenSections();

  $("subtitle").textContent = `${data.host.hostname} • ${data.os.long_version} • scanned ${new Date(data.generated_at).toLocaleString()}`;

  const importantFindings = data.findings.filter((finding) => isImportantSeverity(finding.severity));

  $("overview").innerHTML = [
    card("Hostname", data.host.hostname, data.os.kernel_long_version),
    card("OS", data.os.long_version, `uptime ${data.host.uptime_seconds ? Math.floor(data.host.uptime_seconds / 3600) : 0}h`),
    card("Important", String(importantFindings.length), "warning or critical signals"),
  ].join("");
  decorateCards($("overview"));

  renderImportantList(data);

  $("findingsList").innerHTML = data.findings.length
    ? data.findings.map((f) => item(
        `${severity(f.severity)} ${escapeHtml(f.title)}`,
        escapeHtml(f.detail),
        `<div class="small">Rule: ${escapeHtml(f.rule_id || "legacy")} • Category: ${escapeHtml(f.category || "general")}</div>
         <div class="small">Recommendation: ${escapeHtml(f.recommendation || "Review manually.")}</div>`
      )).join("")
    : item("No findings", "The current read-only rules did not detect immediate warnings.");
  decorateCards($("findingsList"));

  $("cpuList").innerHTML = data.compute.cpus.map((cpu) => `
    <div class="card cpu-card" data-watch-title="${escapeHtml(cpu.name || "CPU")}">
      <strong>${escapeHtml(cpu.name || "CPU")}</strong>
      <p>${escapeHtml(cpu.brand || "Unknown brand")} • ${escapeHtml(cpu.frequency_mhz)} MHz</p>
      <div class="metric">${pct(cpu.usage_percent)}</div>
      ${bar(cpu.usage_percent)}
    </div>
  `).join("");
  decorateCards($("cpuList"));

  $("diskList").innerHTML = data.storage.map((disk) => item(
    escapeHtml(disk.mount_point),
    `${escapeHtml(disk.name)} • ${escapeHtml(disk.file_system)} • ${escapeHtml(disk.kind)}`,
    `<div class="small">${bytes(disk.total_bytes - disk.available_bytes)} used of ${bytes(disk.total_bytes)} • ${pct(disk.used_percent)}</div>${bar(disk.used_percent)}`
  )).join("");
  decorateCards($("diskList"));

  $("networkList").innerHTML = data.network.length
    ? data.network.map((net) => item(
        escapeHtml(net.interface),
        `${escapeHtml(net.operational_state)} • MAC ${escapeHtml(net.mac_address)}`,
        `<div class="small">RX ${bytes(net.total_received_bytes)} • TX ${bytes(net.total_transmitted_bytes)}</div>`
      )).join("")
    : item("No network interfaces", "No interfaces were returned by the scanner.");
  decorateCards($("networkList"));

  $("portList").innerHTML = data.listening_ports.length
    ? data.listening_ports.map((port) => chip(`${port.protocol}:${port.port}`, port.port < 1024 ? "warn" : "")).join("")
    : chip("no listening ports");

  $("processTable").innerHTML = data.top_processes.map((p) => `
    <tr>
      <td>${escapeHtml(p.pid)}</td>
      <td>${escapeHtml(p.name)}</td>
      <td>${escapeHtml(p.status)}</td>
      <td>${pct(p.cpu_usage_percent)}</td>
      <td>${bytes(p.memory_bytes)}</td>
    </tr>
  `).join("");

  $("servicesList").innerHTML = data.service_hints.length
    ? data.service_hints.map((x) => chip(`${x.name} · ${x.confidence}`)).join("")
    : chip("none detected");

  $("appsList").innerHTML = data.app_hints.length
    ? data.app_hints.map((x) => chip(`${x.name} · ${x.confidence}`)).join("")
    : chip("none detected");

  $("dbList").innerHTML = data.database_hints.length
    ? data.database_hints.map((x) => chip(`${x.name} · ${x.confidence}`, x.confidence === "low" ? "warn" : "")).join("")
    : chip("none detected");

  $("k8sSignals").innerHTML = data.kubernetes.signals.length
    ? [item("Summary", escapeHtml(data.kubernetes.summary || "Kubernetes detected.")), ...data.kubernetes.signals.map((x) => item(escapeHtml(x), "signal"))].join("")
    : item("Not detected", "No Kubernetes signals found.");

  $("gitlabSignals").innerHTML = data.gitlab.signals.length
    ? [item("Summary", escapeHtml(data.gitlab.summary || "GitLab detected.")), ...data.gitlab.signals.map((x) => item(escapeHtml(x), "signal"))].join("")
    : item("Not detected", "No GitLab signals found.");

  decorateCards($("apps"));
  decoratePanels();
  renderKeepEyesOn();
}

function severity(value) {
  if (value === "warn") return "WARN";
  if (value === "critical") return "CRITICAL";
  if (value === "ok") return "OK";
  return "INFO";
}

function isImportantSeverity(value) {
  return value === "warn" || value === "critical";
}

function highestSeverityFromSnapshot(data) {
  const values = [
    ...(data?.findings || []).map((finding) => finding.severity),
    ...Array.from(pinnedInsights.values()).map((insight) => insight.severity),
  ];
  if (values.includes("critical")) return "critical";
  if (values.includes("warn")) return "warn";
  return "ok";
}

function publishOsaiSnapshot(data) {
  window.__lastOsaiSnapshot = data;
  window.dispatchEvent(new CustomEvent("osai:snapshot", { detail: data }));
}

function updateVisualSeverity(data) {
  document.body.dataset.severity = highestSeverityFromSnapshot(data);
}

function updatePinnedInsights(insights) {
  for (const insight of insights) {
    if (isImportantSeverity(insight.severity)) {
      pinnedInsights.set(insight.id, insight);
    } else {
      pinnedInsights.delete(insight.id);
    }
  }
  if (currentSnapshot) {
    updateVisualSeverity(currentSnapshot);
    publishOsaiSnapshot(currentSnapshot);
  }
}

function renderImportantList(data) {
  if (!$("importantList")) return;
  const findings = (data?.findings || []).filter((finding) => isImportantSeverity(finding.severity));
  const pinned = Array.from(pinnedInsights.values());
  const findingItems = findings.map((finding) => item(
    `${severity(finding.severity)} ${escapeHtml(finding.title)}`,
    escapeHtml(finding.detail || "Important rule finding."),
    `<div class="small">Recommendation: ${escapeHtml(finding.recommendation || "Review manually.")}</div>`
  ));
  const pinnedItems = pinned.map((insight) => item(
    `${severity(insight.severity)} ${escapeHtml(insight.label)}`,
    escapeHtml(insight.summary),
    `<button class="insight-query" data-query="${escapeHtml(deeperPrompt(insight.id))}" type="button">ask again</button>`
  ));

  $("importantList").innerHTML = findingItems.concat(pinnedItems).length
    ? findingItems.concat(pinnedItems).join("")
    : item("No important signals", "No warning or critical server signals are active in this view.");
  decorateCards($("importantList"));
}

function updateAiButton() {
  const btn = $("aiToggleBtn");
  const hint = $("aiHint");
  const labelEl = $("aiToggleLabel");
  if (!btn || !hint || !labelEl) return;

  btn.className = `ai-toggle ${aiState}`;
  btn.setAttribute("aria-pressed", aiRequested ? "true" : "false");

  const labels = {
    off: ["AI off", "Rust-only mode. No llama/Qwen call will be made."],
    requested: ["AI requested", "Next Ask OSAI will use AI if the reasoning layer is ready."],
    ready: ["AI ready", "Last answer used llama/Qwen refinement."],
    unavailable: ["AI not used", "Rust fallback is active because AI was not ready or failed."],
  };
  const [label, detail] = labels[aiState] || labels.off;
  labelEl.textContent = label;
  hint.textContent = detail;
}

function updateAiFromAsk(data) {
  aiRequested = Boolean(data.ai_requested);
  if (!aiRequested) {
    aiState = "off";
  } else if (data.ai_used) {
    aiState = "ready";
  } else {
    aiState = "unavailable";
  }
  updateAiButton();
}

function renderQuickAsk() {
  $("quickAsk").innerHTML = quickQuestions
    .map(([query, label]) => `<button class="quick-chip" data-query="${escapeHtml(query)}" type="button">${escapeHtml(label)}</button>`)
    .join("");
}

function renderViewButtons() {
  $("viewButtons").innerHTML = optionalViews.map(([id, label]) => {
    const section = $(id);
    const hidden = section?.hidden ?? true;
    return `<button class="view-toggle" data-view="${escapeHtml(id)}" aria-pressed="${hidden ? "false" : "true"}" type="button">${hidden ? "Add" : "Hide"} ${escapeHtml(label)}</button>`;
  }).join("");
}

function toggleView(id) {
  const section = $(id);
  if (!section) return;
  section.hidden = !section.hidden;
  if (!section.hidden) hiddenSections.delete(id);
  saveHiddenSections();
  renderViewButtons();
  decoratePanels();
  if (!section.hidden) section.scrollIntoView({ behavior: "smooth", block: "start" });
}

function decoratePanels() {
  document.querySelectorAll("main > section.panel").forEach((panel) => {
    if (panel.id === "keepEyesOn") return;
    const title = panel.querySelector(":scope > .panel-title");
    if (!title || title.querySelector(".panel-tools")) return;
    const tools = document.createElement("div");
    tools.className = "panel-tools";
    tools.innerHTML = `
      <button class="watch-button" data-watch-card type="button">Keep eyes on</button>
      <button class="close-button" data-close-card type="button" aria-label="Close this card">×</button>
    `;
    title.appendChild(tools);
  });
}

function decorateCards(root = document) {
  const scope = root instanceof Element ? root : document;
  scope.querySelectorAll(".card, .item, .mini-card, .insight-card").forEach((node) => {
    if (node.querySelector(":scope > .card-controls")) return;
    const controls = document.createElement("div");
    controls.className = "card-controls";
    controls.innerHTML = `
      <button class="watch-button" data-watch-card type="button">Keep eyes on</button>
      <button class="close-button" data-close-card type="button" aria-label="Close this card">×</button>
    `;
    node.prepend(controls);
  });
}

function findCardFromButton(button) {
  return button.closest(".card, .item, .mini-card, .insight-card, section.panel");
}

function cardTitle(node) {
  return compactText(
    node.dataset.watchTitle ||
    node.querySelector("h3")?.textContent ||
    node.querySelector("h4")?.textContent ||
    node.querySelector("strong")?.textContent ||
    node.querySelector(".metric")?.textContent ||
    "Pinned card",
    90
  );
}

function cardDetail(node) {
  const clone = node.cloneNode(true);
  clone.querySelectorAll(".card-controls, .panel-tools, button, input, select, textarea").forEach((el) => el.remove());
  return compactText(clone.textContent, 260);
}

function addKeepEyesItemFromNode(node) {
  if (!node) return;
  const item = {
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    title: cardTitle(node),
    detail: cardDetail(node),
    source: node.id ? `#${node.id}` : "dashboard card",
    createdAt: new Date().toISOString(),
  };
  keepEyesItems.unshift(item);
  keepEyesItems = keepEyesItems.slice(0, 24);
  saveJson(STORAGE_KEYS.watch, keepEyesItems);
  renderKeepEyesOn();
}

function addManualKeepEyesItem() {
  const input = $("keepEyesInput");
  const text = input.value.trim();
  if (!text) return;
  keepEyesItems.unshift({
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    title: text,
    detail: "Manual operator watch item.",
    source: "manual",
    createdAt: new Date().toISOString(),
  });
  input.value = "";
  keepEyesItems = keepEyesItems.slice(0, 24);
  saveJson(STORAGE_KEYS.watch, keepEyesItems);
  renderKeepEyesOn();
}

function removeKeepEyesItem(id) {
  keepEyesItems = keepEyesItems.filter((item) => item.id !== id);
  saveJson(STORAGE_KEYS.watch, keepEyesItems);
  renderKeepEyesOn();
}

function renderKeepEyesOn() {
  const list = $("keepEyesList");
  const hint = $("keepEyesHint");
  if (!list) return;
  hint.hidden = keepEyesItems.length > 0;
  list.innerHTML = keepEyesItems.map((entry) => `
    <article class="watch-card" data-watch-id="${escapeHtml(entry.id)}">
      <div class="watch-card-top">
        <span class="pill info">WATCH</span>
        <button class="close-button" data-remove-watch="${escapeHtml(entry.id)}" type="button" aria-label="Remove watch item">×</button>
      </div>
      <h4>${escapeHtml(entry.title)}</h4>
      <p>${escapeHtml(entry.detail)}</p>
      <div class="watch-meta">${escapeHtml(entry.source)} • ${new Date(entry.createdAt).toLocaleString()}</div>
    </article>
  `).join("");
}

function closeCard(node) {
  if (!node || node.id === "keepEyesOn") return;
  if (node.matches("section.panel")) {
    node.hidden = true;
    if (node.id) hiddenSections.add(node.id);
    saveHiddenSections();
    renderViewButtons();
    return;
  }
  node.remove();
}

function saveHiddenSections() {
  saveJson(STORAGE_KEYS.hidden, Array.from(hiddenSections));
}

function applyHiddenSections() {
  hiddenSections.forEach((id) => {
    const section = $(id);
    if (section && section.id !== "keepEyesOn") section.hidden = true;
  });
}

function restoreHiddenCards() {
  hiddenSections.clear();
  saveHiddenSections();
  document.querySelectorAll("main > section.panel").forEach((section) => {
    if (!section.classList.contains("optional-panel")) section.hidden = false;
  });
  renderViewButtons();
  decoratePanels();
}

function initSidebarState() {
  const collapsed = localStorage.getItem(STORAGE_KEYS.sidebar) === "true";
  document.body.classList.toggle("sidebar-collapsed", collapsed);
  const btn = $("sidebarToggle");
  if (btn) btn.setAttribute("aria-expanded", collapsed ? "false" : "true");
}

function toggleSidebar() {
  const collapsed = !document.body.classList.contains("sidebar-collapsed");
  document.body.classList.toggle("sidebar-collapsed", collapsed);
  localStorage.setItem(STORAGE_KEYS.sidebar, String(collapsed));
  $("sidebarToggle")?.setAttribute("aria-expanded", collapsed ? "false" : "true");
}

function showError(err) {
  console.error(err);
  $("subtitle").textContent = `Error: ${err.message}`;
}

function initEvents() {
  $("refreshBtn").addEventListener("click", () => loadSnapshot(true).catch(showError));
  $("reasonBtn").addEventListener("click", () => askReasoning().catch(showError));
  $("aiToggleBtn").addEventListener("click", () => {
    aiRequested = !aiRequested;
    aiState = aiRequested ? "requested" : "off";
    updateAiButton();
  });
  $("memoryRefreshBtn").addEventListener("click", () => loadCogneeLifecycle().catch(showError));
  $("forgetDatasetBtn").addEventListener("click", () => forgetCogneeDataset().catch(showError));
  $("proposeActionBtn").addEventListener("click", () => proposeAction().catch(showError));
  $("addKeepEyesBtn").addEventListener("click", addManualKeepEyesItem);
  $("keepEyesInput").addEventListener("keydown", (event) => {
    if (event.key === "Enter") addManualKeepEyesItem();
  });
  $("clearKeepEyesBtn").addEventListener("click", () => {
    keepEyesItems = [];
    saveJson(STORAGE_KEYS.watch, keepEyesItems);
    renderKeepEyesOn();
  });
  $("restoreCardsBtn").addEventListener("click", restoreHiddenCards);
  $("sidebarToggle").addEventListener("click", toggleSidebar);

  $("quickAsk").addEventListener("click", (event) => {
    const query = event.target.getAttribute("data-query");
    if (!query) return;
    $("reasonQuestion").value = query;
    askReasoning().catch(showError);
  });
  $("viewButtons").addEventListener("click", (event) => {
    const id = event.target.getAttribute("data-view");
    if (id) toggleView(id);
  });
  $("actionsList").addEventListener("click", (event) => {
    const approveId = event.target.getAttribute("data-approve");
    const runId = event.target.getAttribute("data-run");
    if (approveId) approveAction(approveId).catch(showError);
    if (runId) runAction(runId).catch(showError);
  });
  $("reasonOutput").addEventListener("click", (event) => {
    const feedback = event.target.getAttribute("data-feedback");
    if (feedback) {
      sendMemoryFeedback(feedback, event.target.getAttribute("data-resolved") === "true").catch(showError);
      return;
    }
    const query = event.target.getAttribute("data-query");
    if (!query) return;
    $("reasonQuestion").value = query;
    askReasoning().catch(showError);
  });
  $("importantList").addEventListener("click", (event) => {
    const query = event.target.getAttribute("data-query");
    if (!query) return;
    $("reasonQuestion").value = query;
    askReasoning().catch(showError);
  });
  $("keepEyesList").addEventListener("click", (event) => {
    const id = event.target.getAttribute("data-remove-watch");
    if (id) removeKeepEyesItem(id);
  });
  document.addEventListener("click", (event) => {
    const watchBtn = event.target.closest("[data-watch-card]");
    if (watchBtn) {
      event.preventDefault();
      addKeepEyesItemFromNode(findCardFromButton(watchBtn));
      return;
    }
    const closeBtn = event.target.closest("[data-close-card]");
    if (closeBtn) {
      event.preventDefault();
      closeCard(findCardFromButton(closeBtn));
    }
  });
}

function initNavHighlighting() {
  const navLinks = Array.from(document.querySelectorAll("nav a[href^='#']"));
  const navTargets = navLinks
    .map((link) => document.querySelector(link.getAttribute("href")))
    .filter(Boolean);
  if (navTargets.length && "IntersectionObserver" in window) {
    const observer = new IntersectionObserver((entries) => {
      const visible = entries
        .filter((entry) => entry.isIntersecting)
        .sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0];
      if (!visible) return;
      navLinks.forEach((link) => link.classList.toggle("active", link.getAttribute("href") === `#${visible.target.id}`));
    }, { rootMargin: "-30% 0px -58% 0px", threshold: [0.05, 0.2, 0.5] });
    navTargets.forEach((target) => observer.observe(target));
  }
}

initSidebarState();
initEvents();
initNavHighlighting();
renderQuickAsk();
renderViewButtons();
renderKeepEyesOn();
decoratePanels();
updateAiButton();
loadSnapshot(false).catch(showError);
loadActions().catch(showError);
loadCogneeLifecycle().catch(showError);
