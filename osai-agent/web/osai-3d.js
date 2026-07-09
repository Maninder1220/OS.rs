// =============================================================================
// File: web/osai-3d.js
// Purpose:
//   The previous visual map has been removed from the main UI.
//   This file is now a small frontend-only visual companion for the quiet
//   "Keep eyes on" workflow. It does not call Rust and it does not change data.
// =============================================================================
(function () {
  const root = document.documentElement;
  const body = document.body;

  function highestSeverity(snapshot) {
    const severities = (snapshot?.findings || []).map((finding) => finding.severity);
    if (severities.includes("critical")) return "critical";
    if (severities.includes("warn")) return "warn";
    return "ok";
  }

  function updateVisualPulse(snapshot) {
    const severity = highestSeverity(snapshot);
    body.dataset.severity = severity;
    root.style.setProperty("--osai-pulse", severity === "critical" ? ".9" : severity === "warn" ? ".55" : ".25");
  }

  window.addEventListener("osai:snapshot", (event) => {
    updateVisualPulse(event.detail);
  });

  if (window.__lastOsaiSnapshot) {
    updateVisualPulse(window.__lastOsaiSnapshot);
  }
})();
