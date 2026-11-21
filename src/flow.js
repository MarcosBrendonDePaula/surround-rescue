// Editor de fluxo no estilo "editor de nós": tela infinita (arrastar/zoom), cartões por tipo,
// "+" na saída de cada caixa, ferramentas ao passar o mouse e catálogo lateral com busca.
// Mesmo formato salvo de antes (graph.nodes / graph.edges). Usa PL, invoke, esc, baseName,
// fillPct, $, $$ e plLoadDevices/plLoadChain de main.js.

const W = 220, H = 64;
/// Documento sendo editado: o fluxo do dispositivo (PL.chain) ou um fluxo independente.
const doc = () => (FL.mode === "flow" ? FL.flow : PL.chain);
/// Quem é o "dono" do fluxo: {guid} do dispositivo ou id do fluxo independente.
const owner = () => (FL.mode === "flow" ? FL.flowId : PL.device);
const FL = {
  sel: null,          // { node: id } | { edge: índice }
  drag: null,         // arrastando caixa
  pan: null,          // arrastando o fundo
  wire: null,         // puxando seta
  view: { x: 40, y: 40, z: 1 },
  timer: null,
  apps: [],
  pending: null,      // de onde o catálogo foi aberto: { from } | { edge }
  scanning: false,
  mode: "device",     // "device" | "flow"
  flowId: null,
  flow: null,         // { name, enabled, when: { exe }, plugins, graph }
  flows: [],          // lista resumida para a barra lateral
  stats: {},          // medições do executor por dono (id do dispositivo/fluxo)
};

// ---------- medições (CPU, tempos, atrasos) ----------
const fmt = (n, d = 1) => Number(n || 0).toLocaleString("pt-BR", { minimumFractionDigits: d, maximumFractionDigits: d });

async function flPollStats() {
  if (!$("#tab-fluxo").classList.contains("active")) return;
  try {
    const st = await invoke("get_stats");
    const fresh = Date.now() - (st.updated_ms ? 0 : 0);
    FL.stats = Object.fromEntries((st.flows || []).map((f) => [f.id.toLowerCase(), f]));
  } catch (e) { FL.stats = {}; }
  flStatsRender();
  flSideStats();
}

function linkName(l) {
  const d = PL.devices.find((x) => x.id.toLowerCase() === String(l.target).toLowerCase());
  return d ? devFull(d) : l.target;
}

function flStatsRender() {
  const s = FL.stats[String(owner() || "").toLowerCase()];
  const box = $("#fl-stats");
  if (!s || !s.running) {
    box.innerHTML = s === undefined ? "" : `<span class="fl-stat">Parado (sem som passando ou condição não atendida)</span>`;
    return;
  }
  const cpuCls = s.cpu_pct > 50 ? "bad" : s.cpu_pct > 20 ? "warn" : "";
  let h = `<span class="fl-stat ${cpuCls}" title="Parte de cada bloco de 10 ms gasta processando este fluxo"><b>${fmt(s.cpu_pct, 2)}%</b>CPU</span>`;
  h += `<span class="fl-stat" title="Tempo médio por bloco; entre parênteses, o pior do último segundo"><b>${fmt(s.avg_ms, 3)} ms</b>por bloco (pior ${fmt(s.worst_ms, 2)})</span>`;
  h += `<span class="fl-stat" title="${s.kind === "device" ? "Roda dentro do bloco do Windows: não soma atraso" : "Relógio próprio de 10 ms"}"><b>+${fmt(s.inline_latency_ms, 0)} ms</b>no caminho principal</span>`;
  if (s.misses) h += `<span class="fl-stat bad" title="Blocos que passaram sem efeito por atraso"><b>${s.misses}</b>blocos atrasados</span>`;
  for (const l of s.links || []) {
    const cls = !l.connected ? "warn" : l.glitches ? "warn" : "";
    const what = l.kind === "send" ? "envio" : "captura";
    h += `<span class="fl-stat ${cls}" title="Fila ${fmt(l.queue_ms)} ms · correção de relógio ${fmt(l.drift_pct, 3)}% · ${l.glitches} falhas"><b>${l.connected ? `${fmt(l.latency_ms, 0)} ms` : "—"}</b>${what}: ${esc(trunc(linkName(l), 22))}${l.connected ? "" : " (esperando)"}</span>`;
  }
  box.innerHTML = h;
}

function flSideStats() {
  $$(".fl-row").forEach((r) => {
    const id = (r.dataset.dev || r.dataset.flow || "").toLowerCase();
    const s = FL.stats[id];
    let c = r.querySelector(".cpu");
    if (!c) { c = document.createElement("span"); c.className = "cpu"; r.appendChild(c); }
    c.textContent = s && s.running ? `${fmt(s.cpu_pct, 1)}%` : "";
  });
}
setInterval(flPollStats, 1000);

// blocos nativos: nome, unidade, faixa e valor inicial
// Ícones de traço (grade 24), no lugar de emojis: iguais em qualquer Windows e na cor de cada tipo.
const ICONS = {
  pc: "M3 5h18v11H3zM8 20h8M12 16v4",
  out: "M4 9h4l5-4v14l-5-4H4zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12",
  mic: "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21",
  tomic: "M10 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM3.5 11a6.5 6.5 0 0 0 11.2 4.5M10 17.5V21M18 7v7M14.5 10.5h7",
  app: "M3 5h18v14H3zM3 9h18M10 12l5 2.5-5 2.5z",
  listen: "M4 15v-3a8 8 0 0 1 16 0v3M4 15h3v5H4zM17 15h3v5h-3z",
  send: "M14 4h6v6M20 4l-9 9M18 14v5H5V6h5",
  mixer: "M6 4v16M12 4v16M18 4v16M4 14h4M10 8h4M16 16h4",
  vst: "M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4",
  star: "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z",
  gain: "M4 19h16V5zM4 19l16-14",
  invert: "M3 12c3-6 6-6 9 0s6 6 9 0M3 12c3 6 6 6 9 0s6-6 9 0",
  pan: "M4 12h16M4 12l3-3M4 12l3 3M20 12l-3-3M20 12l-3 3M12 8v8",
  mono: "M5 5l7 7 7-7M12 12v8",
  swap: "M4 8h15l-3-3M20 16H5l3 3",
  delay: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2",
  lowpass: "M3 8h9c3 0 4 3 5 6s2 4 4 4",
  highpass: "M3 18c2 0 3-1 4-4s2-6 5-6h9",
  unknown: "M9 9a3 3 0 1 1 4 2.8c-.7.3-1 1-1 1.7V14M12 18h.01",
};
const iconPath = (k) => ICONS[k] || ICONS.unknown;
/// Ícone para HTML (catálogo, mesa de som).
const iconHtml = (k) => `<svg class="ico" viewBox="0 0 24 24" aria-hidden="true"><path d="${iconPath(k)}"/></svg>`;

const FX = {
  gain: { name: "Ganho", unit: "dB", min: -60, max: 24, step: 0.5, def: 0, glyph: "gain", desc: "Aumenta ou diminui o volume" },
  invert: { name: "Inverter", glyph: "invert", desc: "Inverte a fase. Somado com o original, subtrai", hint: "somado = subtrai" },
  pan: { name: "Pan", unit: "", min: -100, max: 100, step: 1, def: 0, glyph: "pan", desc: "Joga o som para um lado" },
  mono: { name: "Mono", glyph: "mono", desc: "Junta esquerda e direita", hint: "L e R viram um só" },
  swap: { name: "Trocar L/R", glyph: "swap", desc: "Inverte os lados", hint: "inverte os lados" },
  delay: { name: "Atraso", unit: "ms", min: 0, max: 500, step: 1, def: 50, glyph: "delay", desc: "Atrasa o som (eco, alinhar caminhos)" },
  lowpass: { name: "Passa-baixa", unit: "Hz", min: 200, max: 20000, step: 50, def: 8000, glyph: "lowpass", desc: "Corta os agudos" },
  highpass: { name: "Passa-alta", unit: "Hz", min: 20, max: 2000, step: 5, def: 120, glyph: "highpass", desc: "Corta os graves" },
};

// ---------- dados ----------
function serialGraph(n) {
  const nodes = [{ id: "in", kind: "input", x: 0, y: 160 }];
  const edges = [];
  let prev = "in";
  for (let i = 0; i < n; i++) {
    nodes.push({ id: `p${i}`, kind: "plugin", plugin: i, x: 300 * (i + 1), y: 160 });
    edges.push({ from: prev, to: `p${i}`, gain: 1 });
    prev = `p${i}`;
  }
  nodes.push({ id: "out", kind: "output", x: 300 * (n + 1), y: 160 });
  edges.push({ from: prev, to: "out", gain: 1 });
  return { nodes, edges };
}

/// O fluxo é uma fila simples (a lista da aba Plugins consegue representar)?
function isSerial(g, n) {
  if (!g) return true;
  if (g.nodes.some((x) => !["input", "output", "plugin"].includes(x.kind)) || g.edges.length !== n + 1) return false;
  let cur = "in";
  for (let i = 0; i <= n; i++) {
    const outs = g.edges.filter((e) => e.from === cur);
    if (outs.length !== 1 || outs[0].gain !== 1) return false;
    const next = g.nodes.find((x) => x.id === outs[0].to);
    if (!next) return false;
    if (i < n && !(next.kind === "plugin" && next.plugin === i)) return false;
    cur = next.id;
  }
  return cur === "out";
}

function graph() {
  const d = doc();
  if (!d.graph) d.graph = FL.mode === "flow" ? { nodes: [], edges: [] } : serialGraph(d.plugins.length);
  return d.graph;
}

function hasCycle(g) {
  const out = {};
  g.edges.forEach((e) => (out[e.from] = out[e.from] || []).push(e.to));
  const st = {};
  const visit = (id) => {
    if (st[id] === 1) return true;
    if (st[id] === 2) return false;
    st[id] = 1;
    const c = (out[id] || []).some(visit);
    st[id] = 2;
    return c;
  };
  return g.nodes.some((n) => visit(n.id));
}

const uid = (p) => `${p}${Date.now().toString(36)}${Math.floor(Math.random() * 99)}`;
const devName = (id) => PL.devices.find((x) => x.id === id);
const devFull = (d) => (d.device ? `${d.name} (${d.device})` : d.name);

/// Título, subtítulo e ícone de cada caixa.
function nodeInfo(n) {
  switch (n.kind) {
    case "input": {
      const d = devName(owner());
      return d?.kind === "Microfone" ? [`Voz: ${d.device || d.name}`, "o que o microfone capta", "mic"] : ["Windows", "som de todos os programas", "pc"];
    }
    case "output": {
      const d = devName(owner());
      return d?.kind === "Microfone" ? ["Saída do microfone", "o que o Discord e os outros programas recebem", "out"] : ["Saída", d ? `toca em ${d.device || d.name}` : "Surround Rescue + anti-estouro", "out"];
    }
    case "mixer": return ["Mixer", "soma o que chega", "mixer"];
    case "app": return [n.exe || "Programa", "som só deste programa", "app"];
    case "listen": { const d = devName(n.device); return [d ? `Ouvir: ${d.device || d.name}` : "Ouvir saída", d ? `${d.name} · tudo o que toca nela` : "escolha a saída", "listen"]; }
    case "receive": { const d = devName(n.device); return [d ? (d.device || d.name) : "Entrada", d ? d.name : "escolha a entrada", "mic"]; }
    case "tomic": { const d = devName(n.device); return [d ? `Para o mic: ${d.device || d.name}` : "Mandar para o microfone", d ? `${d.name} · somado à voz` : "escolha o microfone", "tomic"]; }
    case "send": { const d = devName(n.device); return [d ? `Enviar: ${d.device || d.name}` : "Enviar", d ? `${d.name} · ~30 ms depois` : "escolha o destino", "send"]; }
    case "fx": { const f = FX[n.fx] || { name: n.fx, glyph: "unknown" }; return [f.name, f.unit !== undefined ? `${n.value} ${f.unit}`.trim() : f.hint || "bloco nativo", f.glyph]; }
    case "rack": {
      const list = (n.plugins || []).map((i) => doc().plugins[i]).filter(Boolean);
      if (list.length === 1) { const p = list[0]; return [plName(p), p.enabled ? `VST · ${plFormat(p)}` : "VST · desligado", "vst"]; }
      const names = list.map((p) => (p.enabled ? "" : "⏻") + plName(p));
      return ["VST", names.length ? names.join(" → ") : "vazio: clique para escolher o plugin", "vst"];
    }
    case "plugin": {
      const p = doc().plugins[n.plugin];
      return [p ? baseName(p.path) : "(plugin removido)", p && !p.enabled ? "desligado" : /\.dll$/i.test(p?.path || "") ? "plugin VST2" : /\.clap$/i.test(p?.path || "") ? "plugin CLAP" : "plugin VST3", "vst"];
    }
  }
  return [n.kind, "", "unknown"];
}

// ---------- desenho ----------
const portIn = (n) => [n.x, n.y + H / 2];
const portOut = (n) => [n.x + W, n.y + H / 2];
const curve = ([x1, y1], [x2, y2]) => {
  const dx = Math.max(50, Math.abs(x2 - x1) / 2);
  return `M${x1} ${y1} C${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
};
const trunc = (t, n) => (t.length > n ? t.slice(0, n - 1) + "…" : t);
const btn = (x, y, label, attrs, cls = "") => `<g class="btn ${cls}" ${attrs} transform="translate(${x} ${y})"><circle r="12"/><text>${label}</text></g>`;

function flRender() {
  const g = graph();
  const svg = $("#fl-canvas");
  const v = FL.view;
  const byId = Object.fromEntries(g.nodes.map((n) => [n.id, n]));
  const hasOut = new Set(g.edges.map((e) => e.from));
  let s = `<g transform="translate(${v.x} ${v.y}) scale(${v.z})">`;

  g.edges.forEach((e, i) => {
    const a = byId[e.from], b = byId[e.to];
    if (!a || !b) return;
    const p1 = portOut(a), p2 = portIn(b);
    const d = curve(p1, p2);
    const sel = FL.sel?.edge === i;
    const mx = (p1[0] + p2[0]) / 2, my = (p1[1] + p2[1]) / 2;
    s += `<g class="edge-g${sel ? " sel" : ""}">
      <path class="edge${e.mute ? " muted" : " live"}${sel ? " sel" : ""}" d="${d}"/>
      <path class="hit" data-edge="${i}" d="${d}"/>
      ${Math.abs(e.gain - 1) > 0.001 ? `<text class="gain" x="${mx}" y="${my - 20}">${Math.round(e.gain * 100)}%</text>` : ""}
      <g class="edge-tools">${btn(mx - 15, my, "+", `data-edge-add="${i}"`)}${btn(mx + 15, my, "🗑", `data-edge-del="${i}"`, "danger")}</g>
    </g>`;
  });

  for (const n of g.nodes) {
    const [title, sub, glyph] = nodeInfo(n);
    const p = n.kind === "plugin" ? doc().plugins[n.plugin] : null;
    const cls = `node k-${n.kind}${FL.sel?.node === n.id ? " sel" : ""}${p && !p.enabled ? " off" : ""}`;
    const tools = [];
    if (n.kind === "plugin") {
      tools.push([`▣`, `data-tool="open" data-id="${esc(n.id)}"`, ""]);
      tools.push([`⏻`, `data-tool="toggle" data-id="${esc(n.id)}"`, ""]);
    }
    if (!["input", "output"].includes(n.kind)) tools.push([`×`, `data-tool="del" data-id="${esc(n.id)}"`, "danger"]);
    const toolsSvg = tools.map(([l, a, c], k) => btn(W - 14 - 28 * (tools.length - 1 - k), -18, l, a, c)).join("");
    s += `<g class="${cls}" data-node="${esc(n.id)}" transform="translate(${n.x} ${n.y})">
      <rect class="card" width="${W}" height="${H}" rx="12"/>
      <rect class="stripe" x="0" y="12" width="4" height="${H - 24}" rx="2"/>
      <rect class="badge" x="14" y="12" width="40" height="40" rx="10"/>
      <svg class="glyph" x="23" y="21" width="22" height="22" viewBox="0 0 24 24"><path d="${iconPath(glyph)}"/></svg>
      <text class="title" x="66" y="28">${esc(trunc(title, 20))}</text>
      <text class="sub" x="66" y="46">${esc(trunc(sub, 26))}</text>
      <rect class="lvl-bg" x="66" y="${H - 9}" width="${W - 84}" height="3" rx="1.5"/>
      <rect class="lvl" data-lvl="${esc(n.id)}" x="66" y="${H - 9}" width="0" height="3" rx="1.5"/>
      ${!["input", "app", "receive", "listen"].includes(n.kind) ? `<circle class="port in" data-port-in="${esc(n.id)}" cx="0" cy="${H / 2}" r="7"/>` : ""}
      ${n.kind !== "output" ? `<circle class="port out" data-port-out="${esc(n.id)}" cx="${W}" cy="${H / 2}" r="7"/>` : ""}
      ${n.kind !== "output" && !hasOut.has(n.id) ? `<g class="plus" data-plus="${esc(n.id)}"><line x1="${W + 7}" y1="${H / 2}" x2="${W + 34}" y2="${H / 2}"/><rect x="${W + 34}" y="${H / 2 - 13}" width="26" height="26" rx="7"/><text x="${W + 47}" y="${H / 2}">+</text></g>` : ""}
      <g class="tools">${toolsSvg}</g>
    </g>`;
  }
  if (FL.wire) s += `<path class="wire" d="${curve(FL.wire.from, FL.wire.to)}"/>`;
  s += "</g>";
  svg.innerHTML = s;
  $("#fl-hint").hidden = g.nodes.length > 3;
}

// ---------- painel de propriedades ----------
function flClosePanel() {
  FL.open = false;
  clearInterval(FL.meterTimer);
  $("#fl-panel").hidden = true;
}

function flPanel() {
  const g = graph();
  const box = $("#fl-panel");
  if (!FL.sel || !FL.open) { box.hidden = true; clearInterval(FL.meterTimer); return; }
  $("#fl-creator").hidden = true;
  box.hidden = false;
  let h = "";
  if (FL.sel.edge !== undefined) {
    const e = g.edges[FL.sel.edge];
    if (!e) { FL.sel = null; return flPanel(); }
    const name = (id) => nodeInfo(g.nodes.find((n) => n.id === id) || { kind: "?" })[0];
    h = `<div class="fl-drawer-head"><h2>Ligação</h2><button class="icon" data-close>×</button></div>
      <p class="desc">${esc(name(e.from))} → ${esc(name(e.to))}</p>
      <div class="field"><div class="flabel"><span>Volume</span><output id="fl-gain-v">${Math.round(e.gain * 100)}%</output></div>
      <input type="range" id="fl-gain" min="0" max="200" step="1" value="${Math.round(e.gain * 100)}"></div>
      <button class="pill" id="fl-del-edge">Remover ligação</button>`;
    box.innerHTML = h;
    const r = $("#fl-gain");
    fillPct(r);
    r.addEventListener("input", () => { e.gain = Number(r.value) / 100; $("#fl-gain-v").textContent = `${r.value}%`; fillPct(r); flSaveSoon(); flRender(); });
    $("#fl-del-edge").addEventListener("click", () => { g.edges.splice(FL.sel.edge, 1); FL.sel = null; flChanged(); flPanel(); });
  } else {
    const n = g.nodes.find((x) => x.id === FL.sel.node);
    if (!n) { FL.sel = null; return flPanel(); }
    const [t, sub, gl] = nodeInfo(n);
    if (FL.rkNode !== n.id) { FL.rkNode = n.id; FL.rkSel = 0; }
    h = `<div class="fl-drawer-head"><div class="fl-modal-title"><span class="strip-ic big k-${n.kind}">${iconHtml(gl)}</span><div><h2>${esc(t)}</h2><p class="desc">${esc(sub)}</p></div></div><button class="icon" data-close title="Fechar (Esc)">×</button></div>`;
    if (n.kind === "plugin") {
      const p = doc().plugins[n.plugin];
      h += `<button class="pill solid" id="fl-open">Abrir janela do plugin</button><button class="pill" id="fl-toggle">${p?.enabled ? "Desligar" : "Ligar"}</button>`;
    }
    if (n.kind === "rack") {
      const list = n.plugins || (n.plugins = []);
      const sel = Math.min(FL.rkSel ?? 0, Math.max(0, list.length - 1));
      FL.rkSel = sel;
      const sp = doc().plugins[list[sel]];
      h += `<div class="rk"><div class="rk-list">${list.length ? list.map((pi, i) => {
        const p = doc().plugins[pi];
        if (!p) return "";
        return `<div class="rk-row${p.enabled ? "" : " off"}${i === sel ? " sel" : ""}" data-rk="${i}">
          <span class="rk-n">${i + 1}</span>
          <span class="rk-name" title="${esc(p.path)}">${esc(plName(p))}<small>${esc(plFormat(p))}</small></span>
          <button class="icon" data-rk-up="${i}" title="Subir"${i === 0 ? " disabled" : ""}>▲</button>
          <button class="icon" data-rk-down="${i}" title="Descer"${i === list.length - 1 ? " disabled" : ""}>▼</button>
          <button class="icon" data-rk-on="${i}" title="${p.enabled ? "Desligar" : "Ligar"}">⏻</button>
          <button class="icon" data-rk-win="${i}" title="Abrir janela do plugin">⧉</button>
          <button class="icon" data-rk-del="${i}" title="Tirar do rack">×</button>
        </div>`;
      }).join("") : `<p class="desc">Nenhum plugin ainda. O som passa direto.</p>`}
        <button class="pill solid" id="rk-add">${list.length ? "Adicionar outro plugin…" : "Escolher plugin…"}</button></div>
        <div class="rk-detail">${sp ? `<div class="rk-big">${iconHtml("vst")}<div><h3>${esc(plName(sp))}</h3><p class="desc">${esc(plFormat(sp))} · ${sp.enabled ? "ligado" : "desligado"}</p></div></div>
          <p class="rk-path" title="${esc(sp.path)}">${esc(sp.path)}</p>
          <div class="rk-actions"><button class="pill solid" data-rk-win="${sel}">Abrir a janela do plugin</button>
          <button class="pill" data-rk-on="${sel}">${sp.enabled ? "Desligar (bypass)" : "Ligar"}</button>
          <button class="pill" data-rk-del="${sel}">Tirar da caixa</button></div>
          <p class="desc">A janela do próprio plugin abre separada (é dela que vêm os ajustes). Os ajustes ficam salvos com o fluxo.</p>`
          : `<p class="desc">Escolha um plugin para esta caixa.</p>`}</div></div>
        ${list.length > 1 ? `<p class="desc">O som passa pelos plugins de cima para baixo.</p>` : ""}`;
    }
    if (n.kind === "send") {
      const outs = PL.devices.filter((d) => d.kind === "Saída" && d.id !== owner());
      h += `<div class="field"><div class="flabel"><span>Dispositivo de destino</span></div>
        <select id="fl-send-dev">${outs.map((d) => `<option value="${d.id}"${d.id === n.device ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}</option>`).join("")}</select></div>
        <p class="desc">O som que chega aqui também toca nesse dispositivo, convertido para o formato dele. Fica uns 30 ms atrás do principal.</p>`;
    }
    if (n.kind === "tomic") {
      const mics = PL.devices.filter((d) => d.kind === "Microfone" && d.id !== owner());
      h += `<div class="field"><div class="flabel"><span>Microfone de destino</span></div>
        <select id="fl-tomic-dev">${mics.map((d) => `<option value="${d.id}"${d.id === n.device ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}</option>`).join("")}</select></div>
        <p class="desc">O som que chega nesta caixa é somado à voz desse microfone: qualquer programa que grava dele (Discord, OBS, jogo) recebe os dois juntos. O microfone precisa estar com o motor instalado (Configurações). Para o volume, clique na seta que entra aqui.</p>`;
    }
    if (n.kind === "app") {
      h += `<div class="field"><div class="flabel"><span>Programa (.exe)</span></div>
        <input type="text" id="fl-app-exe" value="${esc(n.exe || "")}" list="fl-app-list">
        <datalist id="fl-app-list">${FL.apps.map((a) => `<option value="${esc(a)}">`).join("")}</datalist></div>
        <p class="desc">A captura é uma cópia: o programa continua tocando onde estava. Para ouvir só a versão processada, aponte o programa para outra saída nas configurações de som do Windows.</p>`;
    }
    if (n.kind === "listen") {
      const outs2 = PL.devices.filter((d) => d.kind === "Saída" && d.id !== owner());
      h += `<div class="field"><div class="flabel"><span>Saída a ouvir</span></div>
        <select id="fl-listen-dev">${outs2.map((d) => `<option value="${d.id}"${d.id === n.device ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}</option>`).join("")}</select></div>
        <p class="desc">Pega tudo o que está tocando nessa saída (depois da mistura do Windows). Não envie de volta para a mesma saída: o executor recusa, para não virar microfonia.</p>`;
    }
    if (n.kind === "receive") {
      const ins = PL.devices.filter((d) => d.kind === "Microfone" && d.id !== owner());
      h += `<div class="field"><div class="flabel"><span>Entrada</span></div>
        <select id="fl-recv-dev">${ins.map((d) => `<option value="${d.id}"${d.id === n.device ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}</option>`).join("")}</select></div>`;
    }
    if (n.kind === "fx") {
      const f = FX[n.fx];
      if (f && f.unit !== undefined) {
        h += `<div class="field"><div class="flabel"><span>${esc(f.name)}</span><output id="fl-fx-v">${n.value} ${f.unit}</output></div>
          <input type="range" id="fl-fx-val" min="${f.min}" max="${f.max}" step="${f.step}" value="${n.value}"></div>`;
      } else if (f) h += `<p class="desc">${esc(f.desc)}</p>`;
    }
    const ins = g.edges.map((e, i) => [e, i]).filter(([e]) => e.to === n.id);
    if (ins.length && ["mixer", "output", "send", "tomic"].includes(n.kind)) {
      h += `<div class="flabel" style="margin-top:6px"><span>Mesa de som</span><span class="desc" style="margin:0">${ins.length} entrada${ins.length > 1 ? "s" : ""}</span></div>
        <div class="mx">${ins.map(([e, i]) => mixStrip(g, e, i)).join("")}${masterStrip(n)}</div>
        <p class="desc">Duplo clique no fader volta a 0 dB; no pan, volta ao centro. S = solo: só as entradas em solo tocam.</p>`;
    }
    if (["input", "output"].includes(n.kind)) h += `<p class="desc">Esta caixa é fixa.</p>`;
    else h += `<button class="pill" id="fl-del-node">Remover caixa</button>`;
    box.innerHTML = h;
    $("#fl-open")?.addEventListener("click", () => flOpen(n));
    if (n.kind === "rack") {
      $("#rk-add")?.addEventListener("click", () => openPluginBrowser((p) => {
        doc().plugins.push({ path: p.path, enabled: true });
        n.plugins.push(doc().plugins.length - 1);
        flChanged();
        flPanel();
      }));
      box.querySelectorAll(".rk-row").forEach((row) => row.addEventListener("click", (ev) => {
        if (ev.target.closest("button")) return;
        FL.rkSel = Number(row.dataset.rk);
        flPanel();
      }));
      box.querySelectorAll("[data-rk-up],[data-rk-down],[data-rk-on],[data-rk-win],[data-rk-del]").forEach((b) => b.addEventListener("click", () => {
        const d = b.dataset, l = n.plugins;
        if (d.rkUp !== undefined) { const i = +d.rkUp; [l[i - 1], l[i]] = [l[i], l[i - 1]]; FL.rkSel = i - 1; }
        else if (d.rkDown !== undefined) { const i = +d.rkDown; [l[i + 1], l[i]] = [l[i], l[i + 1]]; FL.rkSel = i + 1; }
        else if (d.rkOn !== undefined) { const p = doc().plugins[l[+d.rkOn]]; p.enabled = !p.enabled; }
        else if (d.rkWin !== undefined) { return flOpen({ plugin: l[+d.rkWin] }); }
        else if (d.rkDel !== undefined) { const pi = l[+d.rkDel]; l.splice(+d.rkDel, 1); dropPlugin(pi); }
        flChanged();
        flPanel();
      }));
    }
    $("#fl-toggle")?.addEventListener("click", () => { const p = doc().plugins[n.plugin]; p.enabled = !p.enabled; flChanged(); flPanel(); });
    $("#fl-del-node")?.addEventListener("click", () => flRemoveNode(n));
    $("#fl-send-dev")?.addEventListener("change", (e) => { n.device = e.target.value; flChanged(); });
    $("#fl-tomic-dev")?.addEventListener("change", (e) => { n.device = e.target.value; flChanged(); });
    $("#fl-recv-dev")?.addEventListener("change", (e) => { n.device = e.target.value; flChanged(); });
    $("#fl-listen-dev")?.addEventListener("change", (e) => { n.device = e.target.value; flChanged(); });
    $("#fl-app-exe")?.addEventListener("change", (e) => { n.exe = e.target.value.trim().toLowerCase(); flChanged(); });
    bindMixer(box, g);
    startMeters(box, g);
    const fv = $("#fl-fx-val");
    if (fv) {
      fillPct(fv);
      fv.addEventListener("input", () => { n.value = Number(fv.value); $("#fl-fx-v").textContent = `${n.value} ${FX[n.fx].unit}`; fillPct(fv); flSaveSoon(); flRender(); });
    }
  }
  box.querySelector("[data-close]")?.addEventListener("click", (ev) => { ev.stopPropagation(); flClosePanel(); flRender(); });
}

// ---------- mesa de som ----------
const toDb = (g) => (g <= 0.001 ? -60 : Math.max(-60, Math.min(6, 20 * Math.log10(g))));
const fromDb = (db) => (db <= -59.9 ? 0 : 10 ** (db / 20));
const dbText = (g) => (g <= 0.001 ? "−∞" : `${toDb(g) > 0 ? "+" : ""}${fmt(toDb(g), 1)}`);

const panText = (p) => (Math.abs(p) < 0.005 ? "C" : `${p < 0 ? "E" : "D"} ${Math.round(Math.abs(p) * 100)}`);
function mixStrip(g, e, i) {
  const src = g.nodes.find((x) => x.id === e.from);
  const [name, , glyph] = src ? nodeInfo(src) : ["?", "", "unknown"];
  const pan = e.pan || 0;
  return `<div class="strip${e.mute ? " muted" : ""}${e.solo ? " soloed" : ""}" data-edge-i="${i}" data-src="${esc(e.from)}">
    <span class="strip-ic">${iconHtml(glyph)}</span>
    <span class="strip-name" title="${esc(name)}">${esc(trunc(name, 12))}</span>
    <div class="strip-pan"><input type="range" class="pan" min="-100" max="100" step="1" value="${Math.round(pan * 100)}" aria-label="Pan de ${esc(name)}"><output>${panText(pan)}</output></div>
    <div class="strip-body">
      <div class="scale" aria-hidden="true">${[6, 0, -6, -12, -24, -48].map((d) => `<span style="bottom:${((d + 60) / 66) * 100}%">${d > 0 ? "+" + d : d}</span>`).join("")}</div>
      <input type="range" class="fader" min="-60" max="6" step="0.5" value="${toDb(e.gain)}" aria-label="Volume de ${esc(name)}">
      <div class="meter" title="Claro: o que entra · Cor: depois do fader" aria-hidden="true"><i class="pre"></i><i class="post"></i><b></b></div>
    </div>
    <output class="strip-db">${dbText(e.gain)}</output>
    <div class="strip-ms"><button class="strip-mute" title="Mudo">M</button><button class="strip-solo" title="Solo (só as entradas em solo tocam)">S</button></div>
  </div>`;
}
/// Canal master: nível do que sai da caixa.
function masterStrip(n) {
  return `<div class="strip master" data-master="${esc(n.id)}">
    <span class="strip-ic">${iconHtml("out")}</span>
    <span class="strip-name">Master</span>
    <div class="strip-pan"></div>
    <div class="strip-body"><div class="scale" aria-hidden="true">${[6, 0, -6, -12, -24, -48].map((d) => `<span style="bottom:${((d + 60) / 66) * 100}%">${d > 0 ? "+" + d : d}</span>`).join("")}</div><div class="meter wide" aria-hidden="true"><i class="pre"></i><i class="post"></i><b></b></div></div>
    <output class="strip-db strip-peak">−∞</output>
    <div class="strip-ms"></div>
  </div>`;
}
/// Medidores: pede os níveis ao executor enquanto a mesa está aberta.
const meterPct = (v) => Math.max(0, Math.min(100, ((20 * Math.log10(Math.max(v, 1e-6)) + 60) / 66) * 100));
/// `pre` = o que entra (antes do fader); `v` = o que passa (depois do fader).
function meterSet(el, v, pre = v) {
  if (!el) return;
  const pct = meterPct(v);
  const hold = Math.max(pct, (Number(el.dataset.hold) || 0) - 1.2);
  el.dataset.hold = hold;
  el.querySelector(".pre").style.height = `${meterPct(pre)}%`;
  el.querySelector(".post").style.height = `${pct}%`;
  el.querySelector("b").style.bottom = `${hold}%`;
  el.classList.toggle("hot", v >= 0.89);
}
function startMeters(box, g) {
  clearInterval(FL.meterTimer);
  if (!box.querySelector(".meter")) return;
  FL.meterTimer = setInterval(async () => {
    if (box.hidden || !document.body.contains(box)) return clearInterval(FL.meterTimer);
    let lv;
    try { const all = await invoke("get_levels"); lv = all[owner()] || all[String(owner()).toLowerCase()] || {}; } catch (e) { return; }
    box.querySelectorAll(".strip[data-edge-i]").forEach((st) => {
      const e = g.edges[Number(st.dataset.edgeI)];
      if (!e) return;
      const pre = lv[st.dataset.src] || 0;
      meterSet(st.querySelector(".meter"), pre * (e.mute ? 0 : e.gain), pre);
    });
    box.querySelectorAll(".strip[data-master]").forEach((st) => {
      const v = lv[st.dataset.master] || 0;
      meterSet(st.querySelector(".meter"), v);
      st.querySelector(".strip-peak").textContent = v > 1e-6 ? `${fmt(20 * Math.log10(v), 1)}` : "−∞";
    });
  }, 70);
}

function bindMixer(box, g) {
  box.querySelectorAll(".strip[data-edge-i]").forEach((st) => {
    const e = g.edges[Number(st.dataset.edgeI)];
    const f = st.querySelector(".fader");
    const out = st.querySelector(".strip-db");
    fillPct(f);
    f.addEventListener("input", () => {
      e.gain = fromDb(Number(f.value));
      out.textContent = dbText(e.gain);
      fillPct(f);
      flSaveSoon();
      flRender();
    });
    // duplo clique no fader: volta a 0 dB
    f.addEventListener("dblclick", () => { e.gain = 1; f.value = 0; out.textContent = dbText(1); fillPct(f); flSaveSoon(); flRender(); });
    st.querySelector(".strip-mute").addEventListener("click", () => {
      e.mute = !e.mute;
      st.classList.toggle("muted", e.mute);
      flSaveSoon();
      flRender();
    });
    st.querySelector(".strip-solo").addEventListener("click", () => {
      e.solo = !e.solo;
      st.classList.toggle("soloed", e.solo);
      flSaveSoon();
    });
    const pan = st.querySelector(".pan"), po = st.querySelector(".strip-pan output");
    pan.addEventListener("input", () => { e.pan = Number(pan.value) / 100; po.textContent = panText(e.pan); flSaveSoon(); });
    pan.addEventListener("dblclick", () => { e.pan = 0; pan.value = 0; po.textContent = "C"; flSaveSoon(); });
  });
}

// ---------- catálogo ----------
async function flOpenCreator(pending) {
  FL.pending = pending || null;
  FL.sel = null;
  $("#fl-panel").hidden = true;
  $("#fl-creator").hidden = false;
  $("#fl-creator-title").textContent = pending ? "Adicionar e ligar" : "Adicionar caixa";
  $("#fl-search").value = "";
  flRender();
  flCatalog();
  $("#fl-search").focus();
  // buscas lentas só aqui, uma vez, em segundo plano (não travam a tela)
  if (!PL.found.length && !FL.scanning) {
    FL.scanning = true;
    flCatalog();
    try { PL.found = await invoke("scan_plugins", { extra: plDirs() }); } catch (e) { PL.found = []; }
    FL.scanning = false;
    if (!$("#fl-creator").hidden) flCatalog();
  }
  invoke("audio_apps").then((a) => { FL.apps = a; if (!$("#fl-creator").hidden) flCatalog(); }).catch(() => {});
}

function catalogItems() {
  const outs = PL.devices.filter((d) => d.kind === "Saída" && d.id !== owner());
  const ins = PL.devices.filter((d) => d.kind === "Microfone" && d.id !== owner());
  // fluxo de um microfone: as entradas são o que vai ser misturado na voz (o Discord recebe junto)
  const onMic = devName(owner())?.kind === "Microfone";
  return [
    [onMic ? "Misturar na voz" : "Entradas", [
      ...FL.apps.map((a) => ({ k: "app", ic: "app", t: a, d: "Som só deste programa (tocando agora)", node: { kind: "app", exe: a } })),
      { k: "app", ic: "app", t: "Capturar programa…", d: "Um player, o navegador, um jogo — escolha o .exe depois", node: { kind: "app", exe: "" } },
      ...ins.map((d) => ({ k: "app", ic: "mic", t: d.device || d.name, d: `Receber de: ${d.name}`, node: { kind: "receive", device: d.id } })),
      ...outs.map((d) => ({ k: "app", ic: "listen", t: `Ouvir: ${d.device || d.name}`, d: `${d.name} · tudo o que toca nesta saída`, node: { kind: "listen", device: d.id } })),
    ]],
    ["Efeitos nativos", Object.entries(FX).map(([id, f]) => ({ k: "fx", ic: f.glyph, t: f.name, d: f.desc, node: { kind: "fx", fx: id, value: f.def ?? 0 } }))],
    ["VST", [
      { k: "plugin", ic: "vst", t: "VST", d: "Escolha o plugin (VST3, VST2, CLAP); dá para pôr vários em fila", browser: true },
      ...PL.found.filter((f) => f.supported !== false && pbFavs().includes(pbKey(f)))
        .map((f) => ({ k: "plugin", ic: "star", t: f.name || baseName(f.path), d: `favorito · ${f.format || "VST3"}`, plugin: f.path })),
    ]],
    ["Saídas", outs.map((d) => ({ k: "send", ic: "send", t: `Enviar: ${d.device || d.name}`, d: d.name, node: { kind: "send", device: d.id } }))],
    ["Mandar para o microfone", PL.devices.filter((d) => d.kind === "Microfone" && d.id !== owner())
      .map((d) => ({ k: "send", ic: "tomic", t: `Para o mic: ${d.device || d.name}`, d: `${d.name} · o Discord recebe junto com a voz`, node: { kind: "tomic", device: d.id } }))],
    ["Utilitários", [{ k: "mixer", ic: "mixer", t: "Mixer", d: "Soma vários caminhos", node: { kind: "mixer" } }]],
  ];
}

function flCatalog() {
  const q = $("#fl-search").value.trim().toLowerCase();
  let html = "";
  let idx = 0;
  FL.catalog = [];
  for (const [cat, items] of catalogItems()) {
    const vis = items.filter((it) => !q || `${it.t} ${it.d} ${cat}`.toLowerCase().includes(q));
    if (!vis.length) continue;
    html += `<h3>${esc(cat)}</h3>`;
    for (const it of vis) {
      FL.catalog.push(it);
      html += `<button class="fl-item k-${it.k}" data-cat="${idx++}"><span class="ic">${iconHtml(it.ic)}</span><span><b>${esc(trunc(it.t, 30))}</b><small>${esc(it.d)}</small></span></button>`;
    }
  }
  $("#fl-catalog").innerHTML = html || `<p class="desc">Nada encontrado para "${esc(q)}".</p>`;
}

/// Cria a caixa escolhida no catálogo, já ligada quando veio de um "+".
function flCreate(it) {
  if (it?.browser) {
    // abre o navegador mantendo de onde o catálogo veio (+ de uma caixa, meio de uma seta)
    const pending = FL.pending;
    $("#fl-creator").hidden = true;
    return openPluginBrowser((p) => { FL.pending = pending; flCreate({ plugin: p.path }); });
  }
  if (!it || (!it.node && !it.plugin)) return;
  const g = graph();
  let node;
  if (it.plugin) {
    doc().plugins.push({ path: it.plugin, enabled: true });
    node = { id: uid("v"), kind: "rack", plugins: [doc().plugins.length - 1] };
  } else {
    node = { id: uid(it.node.kind[0]), ...it.node };
  }
  const p = FL.pending;
  if (p?.from) {
    const a = g.nodes.find((x) => x.id === p.from);
    Object.assign(node, { x: a.x + W + 80, y: a.y });
    g.nodes.push(node);
    // entradas (programa/dispositivo) não recebem seta: só aparecem soltas
    if (!["app", "receive", "listen"].includes(node.kind)) g.edges.push({ from: p.from, to: node.id, gain: 1 });
  } else if (p?.edge !== undefined) {
    // insere no meio da seta: A → novo → B
    const e = g.edges[p.edge];
    const a = g.nodes.find((x) => x.id === e.from), b = g.nodes.find((x) => x.id === e.to);
    Object.assign(node, { x: Math.round((a.x + b.x) / 2), y: Math.round((a.y + b.y) / 2) + 100 });
    g.nodes.push(node);
    if (!["app", "receive", "listen"].includes(node.kind)) g.edges.splice(p.edge, 1, { from: e.from, to: node.id, gain: e.gain }, { from: node.id, to: e.to, gain: 1 });
  } else {
    // no centro da área visível
    const r = $("#fl-canvas").getBoundingClientRect(), v = FL.view;
    Object.assign(node, { x: Math.round((r.width / 2 - v.x) / v.z - W / 2), y: Math.round((r.height / 2 - v.y) / v.z - H / 2) });
    g.nodes.push(node);
  }
  // entrada nova (programa, outro microfone, ouvir saída) já sai ligada na Saída deste dispositivo
  if (["app", "receive", "listen"].includes(node.kind) && g.nodes.some((x) => x.id === "out") && !g.edges.some((e) => e.from === node.id)) {
    g.edges.push({ from: node.id, to: "out", gain: 1 });
  }
  if (hasCycle(g)) {
    g.nodes.pop();
    return flError("Essa ligação faria o som andar em círculo.");
  }
  $("#fl-creator").hidden = true;
  FL.pending = null;
  FL.sel = { node: node.id };
  FL.open = true;
  flChanged();
  flPanel();
}

// ---------- ações ----------
function flError(msg) { $("#error").textContent = msg; $("#error").hidden = false; }

function flOpen(n) {
  invoke("open_plugin_window", { endpoint: owner(), index: n.plugin }).catch((e) => flError(String(e)));
}

/// Tira um plugin da lista do arquivo e corrige as posições usadas pelas caixas e racks.
function dropPlugin(pi) {
  doc().plugins.splice(pi, 1);
  graph().nodes.forEach((x) => {
    if (x.kind === "plugin" && x.plugin > pi) x.plugin -= 1;
    if (x.kind === "rack") x.plugins = (x.plugins || []).filter((i) => i !== pi).map((i) => (i > pi ? i - 1 : i));
  });
}
const plName = (p) => (PL.found.find((f) => pbKey(f) === String(p.path).toLowerCase())?.name) || baseName(p.path);
const plFormat = (p) => (/\.dll$/i.test(p.path) ? "VST2" : /\.clap$/i.test(p.path) ? "CLAP" : "VST3");

function flRemoveNode(n) {
  const g = graph();
  g.edges = g.edges.filter((e) => e.from !== n.id && e.to !== n.id);
  g.nodes = g.nodes.filter((x) => x.id !== n.id);
  if (n.kind === "plugin") dropPlugin(n.plugin);
  if (n.kind === "rack") [...(n.plugins || [])].sort((a, b) => b - a).forEach(dropPlugin);
  FL.sel = null;
  flChanged();
  flPanel();
}

function flChanged() { flRender(); flSaveSoon(); }

function flSaveSoon() {
  clearTimeout(FL.timer);
  FL.timer = setTimeout(async () => {
    try {
      if (FL.mode === "flow") {
        await invoke("set_flow", { id: FL.flowId, flow: FL.flow });
        flSideRender();
      } else {
        await invoke("set_chain", { endpoint: PL.device, chain: PL.chain });
      }
      $("#error").hidden = true;
      $("#fl-saved").textContent = `Salvo ✓ ${new Date().toLocaleTimeString("pt-BR")}`;
    } catch (e) {
      flError(String(e));
      $("#fl-saved").textContent = "Não salvou";
    }
  }, 250);
}

/// Ponto do mouse em coordenadas do mundo (desfaz arrasto e zoom da tela).
function worldPoint(ev) {
  const r = $("#fl-canvas").getBoundingClientRect(), v = FL.view;
  return [(ev.clientX - r.left - v.x) / v.z, (ev.clientY - r.top - v.y) / v.z];
}

function flZoom(factor, cx, cy) {
  const v = FL.view;
  const r = $("#fl-canvas").getBoundingClientRect();
  cx ??= r.width / 2;
  cy ??= r.height / 2;
  const z = Math.min(2, Math.max(0.3, v.z * factor));
  v.x = cx - ((cx - v.x) * z) / v.z;
  v.y = cy - ((cy - v.y) * z) / v.z;
  v.z = z;
  flRender();
}

function flFit() {
  const g = graph();
  const r = $("#fl-canvas").getBoundingClientRect();
  if (!g.nodes.length || !r.width) return;
  const xs = g.nodes.map((n) => n.x), ys = g.nodes.map((n) => n.y);
  const minX = Math.min(...xs) - 60, maxX = Math.max(...xs) + W + 90, minY = Math.min(...ys) - 60, maxY = Math.max(...ys) + H + 60;
  const z = Math.min(1.2, Math.max(0.3, Math.min(r.width / (maxX - minX), r.height / (maxY - minY))));
  FL.view = { z, x: (r.width - (maxX - minX) * z) / 2 - minX * z, y: (r.height - (maxY - minY) * z) / 2 - minY * z };
  flRender();
}

// ---------- eventos ----------
function flBind() {
  document.body.appendChild($("#fl-panel"));
  $("#fl-panel").addEventListener("click", (ev) => { if (ev.target.id === "fl-panel") ev.stopPropagation(); });
  $("#fl-canvas").addEventListener("dblclick", (ev) => {
    const t = ev.target;
    if (t.closest(".btn") || t.dataset.portIn || t.dataset.portOut) return;
    const nodeEl = t.closest("[data-node]");
    if (nodeEl) FL.sel = { node: nodeEl.dataset.node };
    else if (t.dataset.edge !== undefined) FL.sel = { edge: Number(t.dataset.edge) };
    else return;
    FL.open = true;
    flRender();
    flPanel();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("#fl-panel").hidden && $("#pb").hidden) { flClosePanel(); flRender(); }
  });
  const svg = $("#fl-canvas");
  svg.addEventListener("pointerdown", (ev) => {
    const g = graph();
    const t = ev.target;
    const tool = t.closest("[data-tool]"), plus = t.closest("[data-plus]");
    const eAdd = t.closest("[data-edge-add]"), eDel = t.closest("[data-edge-del]");
    if (tool) {
      const n = g.nodes.find((x) => x.id === tool.dataset.id);
      if (tool.dataset.tool === "open") flOpen(n);
      if (tool.dataset.tool === "toggle") { const p = doc().plugins[n.plugin]; p.enabled = !p.enabled; flChanged(); }
      if (tool.dataset.tool === "del") flRemoveNode(n);
      return;
    }
    if (plus) return flOpenCreator({ from: plus.dataset.plus });
    if (eAdd) return flOpenCreator({ edge: Number(eAdd.dataset.edgeAdd) });
    if (eDel) { g.edges.splice(Number(eDel.dataset.edgeDel), 1); FL.sel = null; flChanged(); flPanel(); return; }
    if (t.dataset.portOut) {
      const n = g.nodes.find((x) => x.id === t.dataset.portOut);
      FL.wire = { id: n.id, from: portOut(n), to: worldPoint(ev) };
      svg.setPointerCapture(ev.pointerId);
      return;
    }
    const nodeEl = t.closest("[data-node]");
    if (nodeEl && !t.dataset.portIn) {
      const n = g.nodes.find((x) => x.id === nodeEl.dataset.node);
      const [px, py] = worldPoint(ev);
      FL.drag = { n, dx: px - n.x, dy: py - n.y, moved: false };
      FL.sel = { node: n.id };
      svg.setPointerCapture(ev.pointerId);
      flRender();
      return;
    }
    if (t.dataset.edge !== undefined) {
      FL.sel = { edge: Number(t.dataset.edge) };
      flRender();
      return;
    }
    // fundo: arrastar a tela
    FL.pan = { x: ev.clientX, y: ev.clientY, vx: FL.view.x, vy: FL.view.y, moved: false };
    svg.classList.add("panning");
    svg.setPointerCapture(ev.pointerId);
  });
  svg.addEventListener("pointermove", (ev) => {
    if (FL.drag) {
      const [px, py] = worldPoint(ev);
      FL.drag.n.x = Math.round((px - FL.drag.dx) / 10) * 10;
      FL.drag.n.y = Math.round((py - FL.drag.dy) / 10) * 10;
      FL.drag.moved = true;
      flRender();
    } else if (FL.wire) {
      FL.wire.to = worldPoint(ev);
      flRender();
      const el = document.elementFromPoint(ev.clientX, ev.clientY);
      if (el?.dataset?.portIn) el.classList.add("target");
    } else if (FL.pan) {
      FL.view.x = FL.pan.vx + ev.clientX - FL.pan.x;
      FL.view.y = FL.pan.vy + ev.clientY - FL.pan.y;
      FL.pan.moved = true;
      flRender();
    }
  });
  svg.addEventListener("pointerup", (ev) => {
    if (FL.drag) { if (FL.drag.moved) flSaveSoon(); FL.drag = null; }
    if (FL.pan) {
      if (!FL.pan.moved && FL.sel) { FL.sel = null; flRender(); flPanel(); } // clique no fundo: deseleciona
      FL.pan = null;
      svg.classList.remove("panning");
    }
    if (FL.wire) {
      const el = document.elementFromPoint(ev.clientX, ev.clientY);
      const to = el?.dataset?.portIn;
      const g = graph();
      const from = FL.wire.id;
      FL.wire = null;
      if (to && to !== from && !g.edges.some((e) => e.from === from && e.to === to)) {
        g.edges.push({ from, to, gain: 1 });
        if (hasCycle(g)) { g.edges.pop(); flError("Essa ligação faria o som andar em círculo. Ligue para uma caixa mais à frente."); }
        else { FL.sel = { edge: g.edges.length - 1 }; flSaveSoon(); flPanel(); }
        flRender();
      } else if (!to) {
        // soltou no vazio: abre o catálogo para criar a próxima caixa já ligada
        flRender();
        flOpenCreator({ from });
      } else flRender();
    }
  });
  svg.addEventListener("wheel", (ev) => {
    ev.preventDefault();
    const r = svg.getBoundingClientRect();
    flZoom(ev.deltaY < 0 ? 1.1 : 1 / 1.1, ev.clientX - r.left, ev.clientY - r.top);
  }, { passive: false });
  svg.addEventListener("dblclick", (ev) => {
    const nodeEl = ev.target.closest("[data-node]");
    const n = nodeEl && graph().nodes.find((x) => x.id === nodeEl.dataset.node);
    if (n?.kind === "plugin") flOpen(n);
  });
  document.addEventListener("keydown", (ev) => {
    if (!$("#tab-fluxo").classList.contains("active") || ["INPUT", "SELECT"].includes(document.activeElement?.tagName)) return;
    if (ev.key === "Escape") { $("#fl-creator").hidden = true; FL.sel = null; flRender(); flPanel(); }
    if (ev.key !== "Delete" || !FL.sel) return;
    const g = graph();
    if (FL.sel.edge !== undefined) { g.edges.splice(FL.sel.edge, 1); FL.sel = null; flChanged(); flPanel(); }
    else {
      const n = g.nodes.find((x) => x.id === FL.sel.node);
      if (n && !["input", "output"].includes(n.kind)) flRemoveNode(n);
    }
  });

  $("#fl-fit").addEventListener("click", flFit);
  $("#fl-zin").addEventListener("click", () => flZoom(1.2));
  $("#fl-zout").addEventListener("click", () => flZoom(1 / 1.2));
  $("#fl-open-creator").addEventListener("click", () => flOpenCreator(null));
  $("#fl-creator-close").addEventListener("click", () => { $("#fl-creator").hidden = true; FL.pending = null; });
  $("#fl-search").addEventListener("input", flCatalog);
  $("#fl-search").addEventListener("keydown", (ev) => { if (ev.key === "Enter") flCreate(FL.catalog?.[0]); });
  $("#fl-catalog").addEventListener("click", (ev) => {
    const b = ev.target.closest("[data-cat]");
    if (b) flCreate(FL.catalog[Number(b.dataset.cat)]);
  });
  $("#fl-serial").addEventListener("click", () => { doc().graph = serialGraph(doc().plugins.length); FL.sel = null; flChanged(); flPanel(); flFit(); });
  $("#fl-new-flow").addEventListener("click", flNewFlow);
  $("#fl-delete").addEventListener("click", flDeleteFlow);
  $("#fl-name").addEventListener("change", (e) => { FL.flow.name = e.target.value.trim() || "Sem nome"; flSaveSoon(); });
  $("#fl-enabled").addEventListener("change", (e) => { FL.flow.enabled = e.target.checked; flSaveSoon(); });
  $("#fl-when-mode").addEventListener("change", (e) => {
    FL.flow.when = e.target.value === "exe" ? { exe: $("#fl-when-exe").value.trim().toLowerCase() } : {};
    $("#fl-when-exe").hidden = e.target.value !== "exe";
    flSaveSoon();
  });
  $("#fl-when-exe").addEventListener("change", (e) => { FL.flow.when = { exe: e.target.value.trim().toLowerCase() }; flSaveSoon(); });
  $("#fl-side-dev").addEventListener("click", (e) => { const r = e.target.closest("[data-dev]"); if (r) flSelectDevice(r.dataset.dev); });
  $("#fl-side-flows").addEventListener("click", (e) => { const r = e.target.closest("[data-flow]"); if (r) flSelectFlow(r.dataset.flow); });
  // abrir a aba: só dados rápidos (dispositivos e lista de fluxos); plugins são buscados ao abrir o catálogo
  $$("#tabs button").forEach((b) => b.addEventListener("click", async () => {
    if (b.dataset.tab !== "fluxo") return;
    await plLoadDevices();
    FL.flows = await invoke("list_flows").catch(() => []);
    if (FL.mode === "flow" && FL.flowId) await flSelectFlow(FL.flowId);
    else await flSelectDevice(PL.device);
  }));
}

// ---------- barra lateral ----------
function flSideRender() {
  $("#fl-side-dev").innerHTML = PL.devices
    .map((d) => `<button class="fl-row${FL.mode === "device" && d.id === PL.device ? " active" : ""}" data-dev="${d.id}">
      <span class="dot${d.apo && d.connected !== false ? " on" : ""}" title="${d.apo ? "motor ativo" : "sem motor"}"></span>
      <span class="t">${esc(d.device || d.name)}<small>${esc(d.kind)} · ${esc(d.name)}</small></span></button>`)
    .join("");
  const cur = FL.flows.map((f) => (FL.mode === "flow" && f.id === FL.flowId && FL.flow ? { ...f, name: FL.flow.name, enabled: FL.flow.enabled } : f));
  $("#fl-side-flows").innerHTML = cur.length
    ? cur.map((f) => `<button class="fl-row${FL.mode === "flow" && f.id === FL.flowId ? " active" : ""}" data-flow="${esc(f.id)}">
        <span class="dot${f.enabled ? " on" : ""}" title="${f.enabled ? "ligado" : "desligado"}"></span><span class="t">${esc(f.name)}</span></button>`).join("")
    : `<p class="fl-side-hint">Nenhum fluxo ainda.</p>`;
}

function flHeader() {
  const flow = FL.mode === "flow";
  $("#fl-title").hidden = flow;
  $("#fl-name").hidden = !flow;
  $("#fl-enabled-wrap").hidden = !flow;
  $("#fl-delete").hidden = !flow;
  $("#fl-serial").hidden = flow;
  $("#fl-when").hidden = !flow;
  if (flow) {
    $("#fl-name").value = FL.flow.name || "";
    $("#fl-enabled").checked = !!FL.flow.enabled;
    const exe = FL.flow.when?.exe || "";
    $("#fl-when-mode").value = exe ? "exe" : "always";
    $("#fl-when-exe").hidden = !exe;
    $("#fl-when-exe").value = exe;
    invoke("audio_apps").then((a) => ($("#fl-when-list").innerHTML = a.map((x) => `<option value="${esc(x)}">`).join(""))).catch(() => {});
  } else {
    const d = PL.devices.find((x) => x.id === PL.device);
    $("#fl-title").textContent = d ? `${d.kind}: ${devFull(d)}` : "Fluxo de áudio";
  }
}

async function flSelectDevice(id) {
  FL.mode = "device";
  FL.flowId = null;
  FL.flow = null;
  if (id) PL.device = id;
  await plLoadChain();
  FL.sel = null;
  flHeader();
  flSideRender();
  flPanel();
  flRender();
  requestAnimationFrame(flFit);
}

async function flSelectFlow(id) {
  try {
    const f = await invoke("get_flow", { id });
    FL.mode = "flow";
    FL.flowId = id;
    FL.flow = { name: f.name || id, enabled: !!f.enabled, when: f.when || {}, plugins: f.plugins || [], graph: f.graph || { nodes: [], edges: [] } };
  } catch (e) { return flError(String(e)); }
  FL.sel = null;
  flHeader();
  flSideRender();
  flPanel();
  flRender();
  requestAnimationFrame(flFit);
}

async function flNewFlow() {
  const id = `fluxo-${Date.now().toString(36)}`;
  const flow = { name: "Novo fluxo", enabled: false, when: {}, plugins: [], graph: { nodes: [], edges: [] } };
  try { await invoke("set_flow", { id, flow }); } catch (e) { return flError(String(e)); }
  FL.flows = await invoke("list_flows").catch(() => []);
  await flSelectFlow(id);
  flOpenCreator(null); // já abre o catálogo para escolher o primeiro trigger
}

async function flDeleteFlow() {
  if (FL.mode !== "flow") return;
  try { await invoke("delete_flow", { id: FL.flowId }); } catch (e) { return flError(String(e)); }
  FL.flows = await invoke("list_flows").catch(() => []);
  await flSelectDevice(PL.device);
}

flBind();

// ---------- níveis ao vivo nas caixas ----------
setInterval(async () => {
  const tab = document.getElementById("tab-fluxo");
  if (!tab || !tab.classList.contains("active") || document.hidden) return;
  let lv;
  try { const all = await invoke("get_levels"); lv = all[owner()] || all[String(owner()).toLowerCase()]; } catch (e) { return; }
  const w = W - 84;
  document.querySelectorAll("#fl-canvas .lvl").forEach((r) => {
    const v = lv ? lv[r.dataset.lvl] || 0 : 0;
    r.setAttribute("width", ((meterPct(v) / 100) * w).toFixed(1));
    r.classList.toggle("hot", v >= 0.89);
  });
}, 100);
