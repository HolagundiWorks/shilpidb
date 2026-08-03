// ShilpiDB Desktop frontend. Plain JS, no build step — talks to the Rust
// side only through window.__TAURI__.core.invoke, which maps 1:1 onto the
// #[tauri::command] functions in gui/src-tauri/src/commands.rs.

const isTauri = typeof window.__TAURI__ !== "undefined";
const invoke = isTauri
  ? window.__TAURI__.core.invoke
  : async (cmd, args) => mockInvoke(cmd, args);

const el = (id) => document.getElementById(id);

const state = {
  connected: false,
  records: [], // [{id, min_x, min_y, max_x, max_y}]
  matchIds: null, // Set<u64> | null (null = no active query filter)
  selectedId: null,
};

function setStatus(text, kind) {
  const s = el("status");
  s.textContent = text;
  s.className = `status status-${kind}`;
}

function setConnected(connected) {
  state.connected = connected;
  el("connect-btn").disabled = connected;
  el("disconnect-btn").disabled = !connected;
  el("query-btn").disabled = !connected;
  el("insert-btn").disabled = !connected;
  el("save-btn").disabled = !connected;
  if (!connected) {
    state.records = [];
    state.matchIds = null;
    state.selectedId = null;
    renderTable();
    renderCanvas();
    renderDetail(null);
    el("stats-body").textContent = "Not connected.";
    el("stats-body").classList.remove("hidden");
  }
}

async function refreshAll() {
  const [records, stats] = await Promise.all([invoke("list_boxes"), invoke("stats")]);
  state.records = records;
  el("stats-body").textContent = `${stats.count} record(s) · cell size ${stats.cell_size.toFixed(3)}`;
  renderTable();
  renderCanvas();
}

function renderTable() {
  const body = el("records-body");
  body.innerHTML = "";
  for (const r of state.records) {
    const tr = document.createElement("tr");
    tr.dataset.id = r.id;
    if (state.matchIds && !state.matchIds.has(r.id)) {
      tr.style.opacity = "0.35";
    }
    if (state.matchIds && state.matchIds.has(r.id)) {
      tr.classList.add("match");
    }
    if (r.id === state.selectedId) {
      tr.classList.add("selected");
    }
    for (const v of [r.id, r.min_x, r.min_y, r.max_x, r.max_y]) {
      const td = document.createElement("td");
      td.textContent = v;
      tr.appendChild(td);
    }
    tr.addEventListener("click", () => selectRecord(r.id));
    body.appendChild(tr);
  }
}

function fitTransform(canvas, records) {
  if (records.length === 0) return null;
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const r of records) {
    minX = Math.min(minX, r.min_x);
    minY = Math.min(minY, r.min_y);
    maxX = Math.max(maxX, r.max_x);
    maxY = Math.max(maxY, r.max_y);
  }
  const spanX = Math.max(maxX - minX, 1e-6);
  const spanY = Math.max(maxY - minY, 1e-6);
  const pad = 24;
  const w = canvas.width - pad * 2;
  const h = canvas.height - pad * 2;
  const scale = Math.min(w / spanX, h / spanY);
  return { minX, minY, maxY, scale, pad };
}

function renderCanvas() {
  const canvas = el("canvas");
  const wrap = el("canvas-wrap");
  const dpr = window.devicePixelRatio || 1;
  canvas.width = wrap.clientWidth * dpr;
  canvas.height = wrap.clientHeight * dpr;
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, canvas.width, canvas.height);

  if (state.records.length === 0) {
    ctx.fillStyle = "#7f96b3";
    ctx.font = "13px sans-serif";
    ctx.fillText(
      state.connected ? "No records yet — insert one from the sidebar." : "Connect to a server to browse records.",
      16,
      24,
    );
    return;
  }

  const t = fitTransform(canvas, state.records);
  if (!t) return;
  // y is flipped so +y (geometry "up") renders upward on screen.
  const toScreen = (x, y) => [t.pad + (x - t.minX) * t.scale, canvas.height / dpr - t.pad - (y - t.minY) * t.scale];

  for (const r of state.records) {
    const [x0, y0] = toScreen(r.min_x, r.min_y);
    const [x1, y1] = toScreen(r.max_x, r.max_y);
    const x = Math.min(x0, x1);
    const y = Math.min(y0, y1);
    const w = Math.max(Math.abs(x1 - x0), 1);
    const h = Math.max(Math.abs(y1 - y0), 1);

    const isMatch = state.matchIds ? state.matchIds.has(r.id) : true;
    const isSelected = r.id === state.selectedId;

    ctx.fillStyle = isMatch ? "rgba(90, 200, 230, 0.28)" : "rgba(127, 150, 179, 0.08)";
    ctx.strokeStyle = isSelected ? "#ffffff" : isMatch ? "#5ac8e6" : "#3c5a8c";
    ctx.lineWidth = isSelected ? 2 : 1;
    ctx.fillRect(x, y, w, h);
    ctx.strokeRect(x, y, w, h);
  }
}

function renderDetail(detail) {
  const empty = el("detail-empty");
  const body = el("detail-body");
  const deleteBtn = el("delete-btn");
  if (!detail) {
    empty.classList.remove("hidden");
    body.classList.add("hidden");
    deleteBtn.classList.add("hidden");
    return;
  }
  empty.classList.add("hidden");
  body.classList.remove("hidden");
  deleteBtn.classList.remove("hidden");
  el("d-id").textContent = detail.id;
  el("d-bbox").textContent = `(${detail.min_x}, ${detail.min_y}) – (${detail.max_x}, ${detail.max_y})`;
  el("d-text").textContent = detail.payload_text || "(empty)";
  el("d-hex").textContent = detail.payload_hex || "(empty)";
}

async function selectRecord(id) {
  state.selectedId = id;
  renderTable();
  renderCanvas();
  try {
    const detail = await invoke("get_record", { id });
    renderDetail(detail);
  } catch (e) {
    renderDetail(null);
    reportError(e);
  }
}

function reportError(e) {
  const msg = typeof e === "string" ? e : e?.message || JSON.stringify(e);
  setStatus(msg, "error");
}

async function doConnect(host) {
  try {
    await invoke("connect", { host });
    setConnected(true);
    setStatus(`connected · ${host}`, "connected");
    await refreshAll();
  } catch (e) {
    setConnected(false);
    reportError(e);
  }
}

el("connect-btn").addEventListener("click", () => doConnect(el("host-input").value.trim()));

el("disconnect-btn").addEventListener("click", async () => {
  await invoke("disconnect");
  setConnected(false);
  setStatus("disconnected", "disconnected");
});

el("start-local-btn").addEventListener("click", async () => {
  const btn = el("start-local-btn");
  btn.disabled = true;
  setStatus("starting local shilpid…", "disconnected");
  try {
    const addr = await invoke("start_local_server", {});
    el("host-input").value = addr;
    await doConnect(addr);
  } catch (e) {
    reportError(e);
  } finally {
    btn.disabled = false;
  }
});

el("query-btn").addEventListener("click", async () => {
  try {
    const ids = await invoke("query_bbox", {
      minX: Number(el("q-minx").value),
      minY: Number(el("q-miny").value),
      maxX: Number(el("q-maxx").value),
      maxY: Number(el("q-maxy").value),
    });
    state.matchIds = new Set(ids);
    renderTable();
    renderCanvas();
  } catch (e) {
    reportError(e);
  }
});

el("clear-query-btn").addEventListener("click", () => {
  state.matchIds = null;
  renderTable();
  renderCanvas();
});

el("insert-btn").addEventListener("click", async () => {
  try {
    await invoke("insert_record", {
      id: Number(el("i-id").value),
      minX: Number(el("i-minx").value),
      minY: Number(el("i-miny").value),
      maxX: Number(el("i-maxx").value),
      maxY: Number(el("i-maxy").value),
      payload: el("i-payload").value,
    });
    await refreshAll();
  } catch (e) {
    reportError(e);
  }
});

el("delete-btn").addEventListener("click", async () => {
  if (state.selectedId === null) return;
  try {
    await invoke("delete_record", { id: state.selectedId });
    state.selectedId = null;
    renderDetail(null);
    await refreshAll();
  } catch (e) {
    reportError(e);
  }
});

el("save-btn").addEventListener("click", async () => {
  try {
    await invoke("save");
    setStatus("saved", "connected");
  } catch (e) {
    reportError(e);
  }
});

new ResizeObserver(() => renderCanvas()).observe(el("canvas-wrap"));

setConnected(false);
setStatus("disconnected", "disconnected");
renderCanvas();

// --- Browser-preview mock (no Tauri runtime) -------------------------------
// Lets the layout/interactions be checked in a plain browser tab without a
// live shilpid or the Tauri webview. Never used inside the real app.
function mockInvoke(cmd, args) {
  mockInvoke.db ??= new Map([
    [1, { id: 1, min_x: 0, min_y: 0, max_x: 10, max_y: 10, payload: "hello" }],
    [2, { id: 2, min_x: 20, min_y: 5, max_x: 28, max_y: 14, payload: "world" }],
    [3, { id: 3, min_x: -15, min_y: -8, max_x: -5, max_y: 2, payload: "mock record" }],
  ]);
  const db = mockInvoke.db;
  switch (cmd) {
    case "connect":
      return Promise.resolve();
    case "disconnect":
      return Promise.resolve();
    case "list_boxes":
      return Promise.resolve([...db.values()].map(({ payload, ...rest }) => rest));
    case "get_record": {
      const r = db.get(args.id);
      if (!r) return Promise.resolve(null);
      const bytes = new TextEncoder().encode(r.payload);
      return Promise.resolve({
        ...r,
        payload_text: r.payload,
        payload_hex: [...bytes].map((b) => b.toString(16).padStart(2, "0")).join(""),
      });
    }
    case "insert_record":
      db.set(args.id, { id: args.id, min_x: args.minX, min_y: args.minY, max_x: args.maxX, max_y: args.maxY, payload: args.payload });
      return Promise.resolve();
    case "delete_record": {
      const existed = db.delete(args.id);
      return Promise.resolve(existed);
    }
    case "query_bbox": {
      const hit = [...db.values()]
        .filter((r) => r.min_x <= args.maxX && r.max_x >= args.minX && r.min_y <= args.maxY && r.max_y >= args.minY)
        .map((r) => r.id);
      return Promise.resolve(hit);
    }
    case "stats":
      return Promise.resolve({ count: db.size, cell_size: 8 });
    case "save":
      return Promise.resolve();
    case "start_local_server":
      return Promise.resolve("127.0.0.1:7420");
    default:
      return Promise.reject(`mock: unhandled command '${cmd}'`);
  }
}
