// =============================================================================
// File: web/osai-3d.js
// Purpose:
//   Visual-only Three.js glass map for OSAI.
//   It listens to the same snapshot data that app.js already receives from Rust.
//
// Important:
//   This file does NOT call Rust directly.
//   app.js remains the controller and Rust API integration layer.
// =============================================================================

import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

const container = document.getElementById("osai3d");
const titleEl = document.getElementById("osai3dTitle");
const detailEl = document.getElementById("osai3dDetail");
const breakBtn = document.getElementById("breakSelected3dBtn");
const resetBtn = document.getElementById("reset3dBtn");
const askBtn = document.getElementById("askSelected3dBtn");

let scene;
let camera;
let renderer;
let controls;
let root;
let clock;
let raycaster;
let pointer;
let selected = null;
let lastSnapshot = null;
let clickable = [];
let shards = [];

const severityColor = {
  ok: 0x34d399,
  info: 0x7dd3fc,
  warn: 0xfacc15,
  critical: 0xfb7185,
};

if (container) {
  init();
  animate();

  window.addEventListener("osai:snapshot", (event) => {
    lastSnapshot = event.detail;
    rebuildFromSnapshot(lastSnapshot);
  });

  if (window.__lastOsaiSnapshot) {
    lastSnapshot = window.__lastOsaiSnapshot;
    rebuildFromSnapshot(lastSnapshot);
  }

  breakBtn?.addEventListener("click", () => {
    if (selected) shatterNode(selected);
  });

  resetBtn?.addEventListener("click", () => {
    if (lastSnapshot) rebuildFromSnapshot(lastSnapshot);
  });

  askBtn?.addEventListener("click", () => {
    if (!selected?.userData?.prompt) return;
    window.OSAI_UI?.ask?.(selected.userData.prompt);
  });
}

function init() {
  clock = new THREE.Clock();
  raycaster = new THREE.Raycaster();
  pointer = new THREE.Vector2();

  scene = new THREE.Scene();
  scene.fog = new THREE.Fog(0x08111f, 12, 34);

  camera = new THREE.PerspectiveCamera(50, 1, 0.1, 100);
  camera.position.set(0, 7.5, 13);

  renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.setClearColor(0x000000, 0);
  container.innerHTML = "";
  container.appendChild(renderer.domElement);

  controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.07;
  controls.minDistance = 7;
  controls.maxDistance = 26;
  controls.target.set(0, 0.5, 0);

  root = new THREE.Group();
  scene.add(root);

  scene.add(new THREE.AmbientLight(0xffffff, 0.55));

  const keyLight = new THREE.DirectionalLight(0xffffff, 1.25);
  keyLight.position.set(7, 10, 8);
  scene.add(keyLight);

  const cyanLight = new THREE.PointLight(0x7dd3fc, 2.5, 22);
  cyanLight.position.set(-6, 4, 5);
  scene.add(cyanLight);

  const dangerLight = new THREE.PointLight(0xfb7185, 1.6, 18);
  dangerLight.position.set(6, 3, -5);
  scene.add(dangerLight);

  addFloor();
  resize();

  window.addEventListener("resize", resize);
  renderer.domElement.addEventListener("pointermove", onPointerMove);
  renderer.domElement.addEventListener("click", onClick);
}

function resize() {
  if (!container || !renderer || !camera) return;
  const width = Math.max(1, container.clientWidth);
  const height = Math.max(1, container.clientHeight);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
  renderer.setSize(width, height, false);
}

function addFloor() {
  const grid = new THREE.GridHelper(18, 18, 0x34506f, 0x1a2942);
  grid.position.y = -2.1;
  grid.material.transparent = true;
  grid.material.opacity = 0.35;
  scene.add(grid);

  const ring = new THREE.Mesh(
    new THREE.TorusGeometry(4.8, 0.025, 12, 160),
    new THREE.MeshBasicMaterial({ color: 0x7dd3fc, transparent: true, opacity: 0.28 })
  );
  ring.rotation.x = Math.PI / 2;
  ring.position.y = -2.05;
  scene.add(ring);
}

function rebuildFromSnapshot(snapshot) {
  selected = null;
  clickable = [];
  shards.forEach((s) => scene.remove(s.mesh));
  shards = [];

  root.clear();

  const nodes = snapshotToNodes(snapshot);
  const center = nodes[0];
  const outer = nodes.slice(1);

  const host = createGlassNode(center, new THREE.SphereGeometry(1.05, 40, 24));
  host.position.set(0, 0.3, 0);
  root.add(host);
  clickable.push(host);

  outer.forEach((node, index) => {
    const angle = (index / outer.length) * Math.PI * 2;
    const radius = 4.2 + (index % 2) * 1.25;
    const y = index % 3 === 0 ? 1.05 : index % 3 === 1 ? -0.6 : 0.15;

    const geometry = geometryForNode(node.kind);
    const mesh = createGlassNode(node, geometry);
    mesh.position.set(Math.cos(angle) * radius, y, Math.sin(angle) * radius);
    mesh.rotation.y = -angle;
    root.add(mesh);
    clickable.push(mesh);

    addLink(host.position, mesh.position, node.severity);
  });

  updateHud(null);
}

function snapshotToNodes(snapshot) {
  const findings = Array.isArray(snapshot?.findings) ? snapshot.findings : [];
  const criticalCount = findings.filter((f) => f.severity === "critical").length;
  const warnCount = findings.filter((f) => f.severity === "warn").length;

  const hostSeverity = criticalCount > 0 ? "critical" : warnCount > 0 ? "warn" : "ok";
  const hostname = snapshot?.host?.hostname || "OSAI Host";
  const os = snapshot?.os?.long_version || "Unknown OS";

  const cpuList = snapshot?.compute?.cpus || [];
  const cpuMax = maxNumber(cpuList.map((c) => c.usage_percent));
  const diskMax = maxNumber((snapshot?.storage || []).map((d) => d.used_percent));
  const ports = snapshot?.listening_ports || [];
  const processes = snapshot?.top_processes || [];

  const services = snapshot?.service_hints || [];
  const apps = snapshot?.app_hints || [];
  const dbs = snapshot?.database_hints || [];
  const k8s = snapshot?.kubernetes?.signals || [];
  const gitlab = snapshot?.gitlab?.signals || [];

  const nodes = [
    {
      id: "host",
      kind: "host",
      label: hostname,
      detail: `${os}. Findings: ${findings.length}, warn: ${warnCount}, critical: ${criticalCount}`,
      severity: hostSeverity,
      prompt: "whats the update ?",
      value: findings.length,
    },
    {
      id: "cpu",
      kind: "cpu",
      label: "CPU",
      detail: `${cpuList.length || 0} logical CPU entries. Max usage ${formatPercent(cpuMax)}.`,
      severity: metricSeverity(cpuMax, 85, 95),
      prompt: "cpu core status",
      value: cpuMax,
    },
    {
      id: "storage",
      kind: "disk",
      label: "Storage",
      detail: `${(snapshot?.storage || []).length} mount points. Highest usage ${formatPercent(diskMax)}.`,
      severity: metricSeverity(diskMax, 80, 92),
      prompt: "disk storage usage",
      value: diskMax,
    },
    {
      id: "network",
      kind: "network",
      label: "Network",
      detail: `${(snapshot?.network || []).length} interfaces. ${ports.length} listening ports.`,
      severity: ports.some((p) => Number(p.port) < 1024) ? "warn" : "info",
      prompt: "network and open ports",
      value: ports.length,
    },
    {
      id: "processes",
      kind: "process",
      label: "Processes",
      detail: `${processes.length} top processes returned by Rust snapshot.`,
      severity: processes.some((p) => Number(p.cpu_usage_percent) > 90) ? "critical" : processes.some((p) => Number(p.cpu_usage_percent) > 70) ? "warn" : "info",
      prompt: "top processes",
      value: processes.length,
    },
    {
      id: "services",
      kind: "service",
      label: "Services",
      detail: `${services.length} services, ${apps.length} apps, ${dbs.length} databases detected.`,
      severity: dbs.some((d) => d.confidence === "low") ? "warn" : "ok",
      prompt: "services and databases",
      value: services.length + apps.length + dbs.length,
    },
    {
      id: "findings",
      kind: "finding",
      label: "Findings",
      detail: criticalCount
        ? `${criticalCount} critical finding(s). Glass is cracked.`
        : warnCount
          ? `${warnCount} warning finding(s). Glass is blinking.`
          : "No warning or critical findings in this snapshot.",
      severity: criticalCount ? "critical" : warnCount ? "warn" : "ok",
      prompt: "current findings",
      value: findings.length,
    },
    {
      id: "kubernetes",
      kind: "k8s",
      label: "Kubernetes",
      detail: k8s.length ? `${k8s.length} Kubernetes signal(s) detected.` : "No Kubernetes signals detected.",
      severity: k8s.length ? "info" : "ok",
      prompt: "kubernetes status",
      value: k8s.length,
    },
    {
      id: "gitlab",
      kind: "gitlab",
      label: "GitLab",
      detail: gitlab.length ? `${gitlab.length} GitLab signal(s) detected.` : "No GitLab signals detected.",
      severity: gitlab.length ? "info" : "ok",
      prompt: "gitlab status",
      value: gitlab.length,
    },
  ];

  return nodes;
}

function createGlassNode(node, geometry) {
  const severity = normalizeSeverity(node.severity);
  const color = severityColor[severity] || severityColor.info;
  const material = new THREE.MeshPhysicalMaterial({
    color,
    emissive: color,
    emissiveIntensity: severity === "critical" ? 0.45 : severity === "warn" ? 0.28 : 0.12,
    metalness: 0.05,
    roughness: 0.08,
    transmission: 0.55,
    thickness: 0.8,
    transparent: true,
    opacity: severity === "critical" ? 0.58 : 0.45,
    clearcoat: 1,
    clearcoatRoughness: 0.05,
  });

  const mesh = new THREE.Mesh(geometry, material);
  mesh.userData = {
    ...node,
    baseScale: 1,
    baseColor: color,
    severity,
    cracked: severity === "critical" || severity === "warn",
    broken: false,
  };

  const edges = new THREE.LineSegments(
    new THREE.EdgesGeometry(geometry),
    new THREE.LineBasicMaterial({
      color,
      transparent: true,
      opacity: severity === "critical" ? 0.85 : 0.45,
    })
  );
  mesh.add(edges);

  const label = makeLabel(node.label, color);
  label.position.set(0, -1.25, 0);
  mesh.add(label);

  if (severity === "warn" || severity === "critical") {
    mesh.add(makeCracks(severity));
  }

  return mesh;
}

function geometryForNode(kind) {
  switch (kind) {
    case "cpu":
      return new THREE.CylinderGeometry(0.72, 0.72, 0.7, 24);
    case "disk":
      return new THREE.CylinderGeometry(0.9, 0.9, 0.35, 36);
    case "network":
      return new THREE.TorusKnotGeometry(0.55, 0.18, 80, 12);
    case "process":
      return new THREE.IcosahedronGeometry(0.78, 1);
    case "finding":
      return new THREE.OctahedronGeometry(0.9, 0);
    case "k8s":
      return new THREE.DodecahedronGeometry(0.8, 0);
    case "gitlab":
      return new THREE.ConeGeometry(0.82, 1.15, 4);
    default:
      return new THREE.BoxGeometry(1.25, 1.25, 1.25);
  }
}

function addLink(from, to, severity) {
  const color = severityColor[normalizeSeverity(severity)] || severityColor.info;
  const geometry = new THREE.BufferGeometry().setFromPoints([
    new THREE.Vector3(from.x, from.y, from.z),
    new THREE.Vector3(to.x, to.y, to.z),
  ]);
  const line = new THREE.Line(
    geometry,
    new THREE.LineBasicMaterial({
      color,
      transparent: true,
      opacity: 0.32,
    })
  );
  root.add(line);
}

function makeCracks(severity) {
  const group = new THREE.Group();
  const color = severity === "critical" ? 0xffffff : 0xfff7aa;
  const material = new THREE.LineBasicMaterial({
    color,
    transparent: true,
    opacity: severity === "critical" ? 0.95 : 0.55,
  });

  const crackSets = [
    [[-0.45, 0.38, 0.74], [-0.1, 0.12, 0.78], [0.18, 0.35, 0.76]],
    [[-0.1, 0.12, 0.78], [0.08, -0.18, 0.80], [0.42, -0.35, 0.77]],
    [[0.08, -0.18, 0.80], [-0.28, -0.42, 0.76]],
    [[-0.1, 0.12, 0.78], [-0.5, -0.08, 0.75]],
  ];

  crackSets.forEach((points) => {
    const geometry = new THREE.BufferGeometry().setFromPoints(
      points.map(([x, y, z]) => new THREE.Vector3(x, y, z))
    );
    group.add(new THREE.Line(geometry, material));
  });

  group.userData.isCrack = true;
  return group;
}

function makeLabel(text, color) {
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  canvas.width = 512;
  canvas.height = 128;

  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.font = "700 42px Inter, Segoe UI, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = "rgba(8, 17, 31, .72)";
  roundRect(ctx, 24, 24, 464, 80, 30);
  ctx.fill();

  ctx.strokeStyle = `#${color.toString(16).padStart(6, "0")}`;
  ctx.lineWidth = 3;
  roundRect(ctx, 24, 24, 464, 80, 30);
  ctx.stroke();

  ctx.fillStyle = "#eef3ff";
  ctx.fillText(String(text).slice(0, 18), 256, 66);

  const texture = new THREE.CanvasTexture(canvas);
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: texture,
      transparent: true,
      depthWrite: false,
    })
  );
  sprite.scale.set(2.2, 0.55, 1);
  return sprite;
}

function roundRect(ctx, x, y, width, height, radius) {
  ctx.beginPath();
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + width, y, x + width, y + height, radius);
  ctx.arcTo(x + width, y + height, x, y + height, radius);
  ctx.arcTo(x, y + height, x, y, radius);
  ctx.arcTo(x, y, x + width, y, radius);
  ctx.closePath();
}

function onPointerMove(event) {
  const rect = renderer.domElement.getBoundingClientRect();
  pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
  pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
}

function onClick() {
  raycaster.setFromCamera(pointer, camera);
  const hits = raycaster.intersectObjects(clickable, false);
  if (!hits.length) {
    selectNode(null);
    return;
  }
  selectNode(hits[0].object);
}

function selectNode(mesh) {
  selected = mesh;
  clickable.forEach((obj) => {
    obj.userData.selected = obj === mesh;
  });
  updateHud(mesh);
}

function updateHud(mesh) {
  if (!titleEl || !detailEl) return;

  if (!mesh) {
    titleEl.textContent = "No node selected";
    detailEl.textContent = "Click any glass node. Low severity glows, warnings blink, critical nodes crack, and Break selected shatters the visual object only.";
    return;
  }

  const severity = normalizeSeverity(mesh.userData.severity);
  const mode =
    severity === "critical"
      ? "critical cracked glass"
      : severity === "warn"
        ? "warning blinking glass"
        : "low glow glass";

  titleEl.textContent = `${mesh.userData.label} · ${severity.toUpperCase()}`;
  detailEl.textContent = `${mesh.userData.detail} Visual mode: ${mode}. Ask selected sends "${mesh.userData.prompt}" into the existing Ask OSAI UI.`;
}

function shatterNode(mesh) {
  if (!mesh || mesh.userData.broken) return;

  mesh.userData.broken = true;
  mesh.visible = false;

  const color = mesh.userData.baseColor || severityColor.info;
  const origin = mesh.getWorldPosition(new THREE.Vector3());
  const severity = normalizeSeverity(mesh.userData.severity);
  const count = severity === "critical" ? 44 : severity === "warn" ? 30 : 20;

  for (let i = 0; i < count; i++) {
    const shard = new THREE.Mesh(
      new THREE.TetrahedronGeometry(0.08 + Math.random() * 0.16, 0),
      new THREE.MeshPhysicalMaterial({
        color,
        emissive: color,
        emissiveIntensity: severity === "critical" ? 0.55 : 0.25,
        roughness: 0.05,
        transmission: 0.35,
        transparent: true,
        opacity: 0.55,
      })
    );

    shard.position.copy(origin);
    shard.rotation.set(Math.random() * Math.PI, Math.random() * Math.PI, Math.random() * Math.PI);

    const velocity = new THREE.Vector3(
      (Math.random() - 0.5) * 4.5,
      Math.random() * 3.2 + 0.7,
      (Math.random() - 0.5) * 4.5
    );

    scene.add(shard);
    shards.push({
      mesh: shard,
      velocity,
      spin: new THREE.Vector3(Math.random() * 4, Math.random() * 4, Math.random() * 4),
      ttl: 1.8 + Math.random() * 0.8,
      age: 0,
    });
  }
}

function animate() {
  requestAnimationFrame(animate);

  const delta = Math.min(clock.getDelta(), 0.05);
  const time = clock.elapsedTime;

  controls?.update();

  if (root) {
    root.rotation.y += delta * 0.06;
  }

  clickable.forEach((obj, index) => {
    if (obj.userData.broken) return;

    const severity = normalizeSeverity(obj.userData.severity);
    const selectedBoost = obj.userData.selected ? 1.18 : 1;
    const blink =
      severity === "critical"
        ? 1 + Math.sin(time * 9 + index) * 0.12
        : severity === "warn"
          ? 1 + Math.sin(time * 4.2 + index) * 0.07
          : 1 + Math.sin(time * 1.2 + index) * 0.025;

    obj.scale.setScalar(selectedBoost * blink);

    if (obj.material?.emissiveIntensity !== undefined) {
      const base =
        severity === "critical"
          ? 0.5
          : severity === "warn"
            ? 0.28
            : severity === "info"
              ? 0.18
              : 0.12;
      obj.material.emissiveIntensity = base + Math.abs(Math.sin(time * (severity === "critical" ? 8 : 3))) * base;
    }

    if (severity === "critical") {
      obj.rotation.z = Math.sin(time * 18 + index) * 0.015;
    }
  });

  updateShards(delta);
  renderer?.render(scene, camera);
}

function updateShards(delta) {
  for (let i = shards.length - 1; i >= 0; i--) {
    const shard = shards[i];
    shard.age += delta;
    shard.velocity.y -= 4.6 * delta;
    shard.mesh.position.addScaledVector(shard.velocity, delta);
    shard.mesh.rotation.x += shard.spin.x * delta;
    shard.mesh.rotation.y += shard.spin.y * delta;
    shard.mesh.rotation.z += shard.spin.z * delta;

    const fade = Math.max(0, 1 - shard.age / shard.ttl);
    if (shard.mesh.material) shard.mesh.material.opacity = 0.55 * fade;

    if (shard.age >= shard.ttl) {
      scene.remove(shard.mesh);
      shard.mesh.geometry.dispose();
      shard.mesh.material.dispose();
      shards.splice(i, 1);
    }
  }
}

function maxNumber(values) {
  const nums = values.map(Number).filter(Number.isFinite);
  return nums.length ? Math.max(...nums) : 0;
}

function metricSeverity(value, warnAt, criticalAt) {
  const n = Number(value) || 0;
  if (n >= criticalAt) return "critical";
  if (n >= warnAt) return "warn";
  return "ok";
}

function normalizeSeverity(value) {
  if (value === "critical") return "critical";
  if (value === "warn") return "warn";
  if (value === "ok") return "ok";
  return "info";
}

function formatPercent(value) {
  return `${(Number(value) || 0).toFixed(1)}%`;
}
