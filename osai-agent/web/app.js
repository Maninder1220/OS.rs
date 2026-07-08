// =============================================================================
// File: web/app.js
// Purpose:
//   Browser-side controller for dashboard polling, Ask OSAI, history, knowledge, plugins, and actions.
//
// Where this fits in OSAI:
//   Connects the HTML UI to the Rust REST API.
//
// Topics to know before editing:
//   Browser DOM APIs, fetch(), event handling, and the OSAI REST API.
//
// Important operational notes:
//   Frontend state should match API response shapes from src/main.rs and src/ask.rs.
// =============================================================================
const $ = (id) => document.getElementById(id);

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

function item(title, detail, extra = "") {
  return `<div class="item"><strong>${title}</strong><span>${detail}</span>${extra}</div>`;
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function chip(text, className = "") {
  return `<span class="chip ${className}">${text}</span>`;
}

function bar(value) {
  const safe = Math.max(0, Math.min(100, value || 0));
  return `<div class="progress"><div style="width:${safe}%"></div></div>`;
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

// All API calls pass through this helper so token prompting and error handling
// stay consistent across scan, history, Ask OSAI, and guarded action views.
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
        `${h.hostname} • findings ${h.finding_count} • warn ${h.warn_count} • critical ${h.critical_count}`
      )).join("")
    : item("No scan history", "A history record is created after each scan.");
}

async function askReasoning() {
  const question = $("reasonQuestion").value.trim() || "whats the update ?";
  $("reasonQuestion").value = question;
  if (!question) return;

  $("reasonOutput").innerHTML = item(
    "Working",
    aiRequested
      ? "Rust is planning the intent and building a focused FactPack first. AI will refine only if the reasoning layer is ready."
      : "Rust is planning the intent and building a focused FactPack. AI is off."
  );

  // /api/ask always returns deterministic Rust insight fields. When the AI
  // toggle is on and llama.cpp is ready, the answer may also be Qwen-refined.
  const data = await apiFetch("/api/ask", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ question, use_ai: aiRequested }),
  });

  lastAskData = { question, ...data };
  updateAiFromAsk(data);
  updatePinnedInsights(data.query_insights || []);
  renderImportantList(currentSnapshot);

  const insights = renderQueryInsights(data.query_insights || []);
  const inference = renderInferenceStatus(data.inference_status);

  $("reasonOutput").innerHTML = [
    inference,
    renderAskPlan(data.ask_plan, data.fact_pack_summary),
    insights,
    item("Answer", `<pre>${escapeHtml(data.answer)}</pre>`),
    renderFeedbackButtons(),
    item("Mode", escapeHtml(data.mode), `<div class="small">Model: ${escapeHtml(data.model)} • AI used: ${data.ai_used ? "yes" : "no"}</div>`),
    item("Context", `PostgreSQL: ${escapeHtml(data.postgres_status)} • Cognee: ${escapeHtml(data.cognee_status)} • llama.cpp: ${escapeHtml(data.llama_status)}`),
  ].join("");
  await loadCogneeLifecycle();
}

function renderAskPlan(plan, summary) {
  if (!plan || !summary) return "";
  const intents = (summary.intent_names || plan.intents || []).join(", ") || "unknown";
  const aiScope = summary.data_sent_to_ai || `${summary.fact_count || 0} facts, ${summary.metric_count || 0} metrics, ${summary.finding_count || 0} findings`;
  return item(
    "Rust AskPlan + FactPack",
    `Detected intent: ${escapeHtml(intents)}<br/>Data sent to AI: ${escapeHtml(aiScope)}`,
    `<div class="small">Cognee recall: ${plan.use_cognee ? "planned" : "skipped"} • Manual checks: ${summary.manual_check_count || 0} • ${escapeHtml(plan.planning_note || "")}</div>`
  );
}

function renderFeedbackButtons() {
  return `<div class="item">
    <strong>Improve Cognee memory</strong>
    <span>Mark whether this answer helped. OSAI will remember feedback and try Cognee improve.</span>
    <div class="feedback-row">
      <button data-feedback="helpful">Helpful</button>
      <button data-feedback="not helpful">Not helpful</button>
      <button data-feedback="needs more detail">Needs more detail</button>
      <button data-feedback="resolved" data-resolved="true">Resolved</button>
      <button data-feedback="still failing">Still failing</button>
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
  $("memoryLifecycleOutput").innerHTML = item("Feedback stored", result.detail, `<div class="small">Dataset: ${escapeHtml(result.dataset)}</div>`);
}

async function loadCogneeLifecycle() {
  const status = await apiFetch("/api/cognee/lifecycle");
  $("memoryLifecycleOutput").innerHTML = [
    item("Lifecycle health", `${escapeHtml(status.health)} • ${escapeHtml(status.last_detail)}`),
    item("Dataset", escapeHtml(status.dataset), `<div class="small">API: ${escapeHtml(status.api_url)}</div>`),
    item("Operations", `Remember: ${escapeHtml(status.remember)} • Recall: ${escapeHtml(status.recall)}`),
    item("Improve / Forget", `${escapeHtml(status.improve)} • ${escapeHtml(status.forget)}`),
  ].join("");
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
  $("memoryLifecycleOutput").innerHTML = item("Forget result", result.detail, `<div class="small">Dataset: ${escapeHtml(result.dataset)}</div>`);
}

function renderQueryInsights(insights) {
  const currentIds = new Set(insights.map((insight) => insight.id));
  const preserved = Array.from(pinnedInsights.values()).filter((insight) => !currentIds.has(insight.id));

  if (!insights.length && !preserved.length) {
    return item("Rust signal match", "No direct signal matched yet. Try: whats the update, CPU status, memory status, disk usage, open ports, top processes, services, databases, Kubernetes, GitLab, or findings.");
  }

  const section = (title, items) => items.length
    ? `<div class="insight-section">
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

      return `<div class="insight-card ${escapeHtml(insight.severity)}">
        <div class="insight-head">
          <strong>${escapeHtml(insight.label)}</strong>
          ${chip(escapeHtml(insight.status), escapeHtml(insight.severity))}
        </div>
        <div class="insight-labels">
          <button class="insight-query" data-query="${escapeHtml(deeperPrompt(insight.id))}">ask: ${escapeHtml(deeperPrompt(insight.id))}</button>
          ${chip(`signal: ${escapeHtml(insight.id)}`)}
        </div>
        <p>${escapeHtml(insight.summary)}</p>
        <div class="insight-metrics">${metrics}</div>
        ${checks}
        <div class="small">Recommendation: ${escapeHtml(insight.recommendation)}</div>
      </div>`;
}

function renderInferenceStatus(status) {
  if (!status) return "";
  const disabled = status.status === "disabled_by_user";
  const cls = status.ready ? "ok" : disabled ? "info" : "warn";
  const checks = renderManualChecks(status.recommended_checks || []);
  return `<div class="item inference-status">
    <strong>${chip(status.ready ? "AI ready" : disabled ? "AI off" : "AI not ready", cls)} Inference / Reasoning Layer</strong>
    <span>${escapeHtml(status.status)} • ${escapeHtml(status.endpoint)} • model ${escapeHtml(status.model)}</span>
    <div class="small">Health: ${escapeHtml(status.health_url)}</div>
    <pre>${escapeHtml(status.detail || "No detail returned.")}</pre>
    ${checks}
  </div>`;
}

function renderManualChecks(commands) {
  if (!commands.length) return "";
  return `<div class="manual-checks">
    <div class="small">Safe manual checks</div>
    ${commands.map((command) => `<code>${escapeHtml(command)}</code>`).join("")}
  </div>`;
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
}

function renderAction(action) {
  const cls = action.status === "blocked" || action.status === "failed" ? "danger" : action.status === "proposed" ? "warn" : "safe";
  const buttons = [
    action.status === "proposed" ? `<button data-approve="${action.id}">Approve</button>` : "",
    action.status === "approved" ? `<button data-run="${action.id}">Run</button>` : "",
  ].join(" ");
  const output = action.output
    ? `<pre>${action.output.stdout || action.output.stderr || "No output"}</pre>`
    : "";

  return `<div class="item">
    <strong>${chip(action.status, cls)} ${action.command} ${(action.args || []).join(" ")}</strong>
    <span>${action.kind} • ${action.validation_message}</span>
    <div class="small">Reason: ${action.reason}</div>
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
  $("subtitle").textContent = `${data.host.hostname} • ${data.os.long_version} • scanned ${new Date(data.generated_at).toLocaleString()}`;

  const importantFindings = data.findings.filter((finding) => isImportantSeverity(finding.severity));

  $("overview").innerHTML = [
    card("Hostname", data.host.hostname, data.os.kernel_long_version),
    card("OS", data.os.long_version, `uptime ${data.host.uptime_seconds ? Math.floor(data.host.uptime_seconds / 3600) : 0}h`),
    card("Important", String(importantFindings.length), "warning or critical signals"),
  ].join("");

  renderImportantList(data);

  $("findingsList").innerHTML = data.findings.length
    ? data.findings.map((f) => item(
        `${severity(f.severity)} ${f.title}`,
        f.detail,
        `<div class="small">Rule: ${f.rule_id || "legacy"} • Category: ${f.category || "general"}</div>
         <div class="small">Recommendation: ${f.recommendation || "Review manually."}</div>`
      )).join("")
    : item("No findings", "The current read-only rules did not detect immediate warnings.");

  $("cpuList").innerHTML = data.compute.cpus.map((cpu) => `
    <div class="card">
      <strong>${cpu.name || "CPU"}</strong>
      <p>${cpu.brand || "Unknown brand"} • ${cpu.frequency_mhz} MHz</p>
      <div class="metric">${pct(cpu.usage_percent)}</div>
      ${bar(cpu.usage_percent)}
    </div>
  `).join("");

  $("diskList").innerHTML = data.storage.map((disk) => item(
    disk.mount_point,
    `${disk.name} • ${disk.file_system} • ${disk.kind}`,
    `<div class="small">${bytes(disk.total_bytes - disk.available_bytes)} used of ${bytes(disk.total_bytes)} • ${pct(disk.used_percent)}</div>${bar(disk.used_percent)}`
  )).join("");

  $("networkList").innerHTML = data.network.length
    ? data.network.map((net) => item(
        net.interface,
        `${net.operational_state} • MAC ${net.mac_address}`,
        `<div class="small">RX ${bytes(net.total_received_bytes)} • TX ${bytes(net.total_transmitted_bytes)}</div>`
      )).join("")
    : item("No network interfaces", "No interfaces were returned by the scanner.");

  $("portList").innerHTML = data.listening_ports.length
    ? data.listening_ports.map((port) => chip(`${port.protocol}:${port.port}`, port.port < 1024 ? "warn" : "")).join("")
    : chip("no listening ports");

  $("processTable").innerHTML = data.top_processes.map((p) => `
    <tr>
      <td>${p.pid}</td>
      <td>${p.name}</td>
      <td>${p.status}</td>
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
    ? [item("Summary", data.kubernetes.summary || "Kubernetes detected."), ...data.kubernetes.signals.map((x) => item(x, "signal"))].join("")
    : item("Not detected", "No Kubernetes signals found.");

  $("gitlabSignals").innerHTML = data.gitlab.signals.length
    ? [item("Summary", data.gitlab.summary || "GitLab detected."), ...data.gitlab.signals.map((x) => item(x, "signal"))].join("")
    : item("Not detected", "No GitLab signals found.");
}

function card(title, metric, detail) {
  return `
    <div class="card">
      <p>${title}</p>
      <div class="metric">${metric}</div>
      <p>${detail}</p>
    </div>
  `;
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

function updatePinnedInsights(insights) {
  for (const insight of insights) {
    if (isImportantSeverity(insight.severity)) {
      pinnedInsights.set(insight.id, insight);
    } else {
      pinnedInsights.delete(insight.id);
    }
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
    `<button class="insight-query" data-query="${escapeHtml(deeperPrompt(insight.id))}">ask again</button>`
  ));

  $("importantList").innerHTML = findingItems.concat(pinnedItems).length
    ? findingItems.concat(pinnedItems).join("")
    : item("No important signals", "No warning or critical server signals are active in this view.");
}

function updateAiButton() {
  const btn = $("aiToggleBtn");
  const hint = $("aiHint");
  const labelEl = $("aiToggleLabel");
  if (!btn || !hint || !labelEl) return;
  // The toggle only requests model refinement. The Rust API still performs a
  // health check and falls back to deterministic answers if llama.cpp is busy.
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
    .map(([query, label]) => `<button class="quick-chip" data-query="${escapeHtml(query)}">${escapeHtml(label)}</button>`)
    .join("");
}

function renderViewButtons() {
  $("viewButtons").innerHTML = optionalViews.map(([id, label]) => {
    const hidden = $(id)?.hidden ?? true;
    return `<button class="view-toggle" data-view="${escapeHtml(id)}" aria-pressed="${hidden ? "false" : "true"}">${hidden ? "Add" : "Hide"} ${escapeHtml(label)}</button>`;
  }).join("");
}

function toggleView(id) {
  const section = $(id);
  if (!section) return;
  section.hidden = !section.hidden;
  renderViewButtons();
  if (!section.hidden) section.scrollIntoView({ behavior: "smooth", block: "start" });
}

$("refreshBtn").addEventListener("click", () => loadSnapshot(true).catch(showError));
$("reasonBtn").addEventListener("click", () => askReasoning().catch(showError));
$("aiToggleBtn").addEventListener("click", () => {
  aiRequested = !aiRequested;
  aiState = aiRequested ? "requested" : "off";
  updateAiButton();
});
$("memoryRefreshBtn").addEventListener("click", () => loadCogneeLifecycle().catch(showError));
$("forgetDatasetBtn").addEventListener("click", () => forgetCogneeDataset().catch(showError));
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
$("proposeActionBtn").addEventListener("click", () => proposeAction().catch(showError));
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

function showError(err) {
  console.error(err);
  $("subtitle").textContent = `Error: ${err.message}`;
}

loadSnapshot(false).catch(showError);
loadActions().catch(showError);
loadCogneeLifecycle().catch(showError);
renderQuickAsk();
renderViewButtons();
updateAiButton();
