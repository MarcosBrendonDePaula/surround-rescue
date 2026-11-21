// Editor de fluxo: caixas (nós) e setas (ligações) sobre os mesmos dados da aba Plugins.
// Usa PL, invoke, esc, baseName, $, $$ e plLoadDevices/plLoadChain/plScan de main.js.

const NODE_W = 160;
const NODE_H = 56;
const FL = { sel: null, drag: null, wire: null, timer: null, apps: [] };

// blocos nativos: nome, unidade, faixa e valor inicial
const FX = {
  gain: { name: "Ganho", unit: "dB", min: -60, max: 24, step: 0.5, def: 0 },
  invert: { name: "Inverter", hint: "somado = subtrai" },
  pan: { name: "Pan", unit: "", min: -100, max: 100, step: 1, def: 0 },
  mono: { name: "Mono", hint: "L e R viram um só" },
  swap: { name: "Trocar L/R", hint: "Inverte os lados" },
  delay: { name: "Atraso", unit: "ms", min: 0, max: 500, step: 1, def: 50 },
  lowpass: { name: "Passa-baixa", unit: "Hz", min: 200, max: 20000, step: 50, def: 8000 },
  highpass: { name: "Passa-alta", unit: "Hz", min: 20, max: 2000, step: 5, def: 120 },
};

function serialGraph(n) {
  const nodes = [{ id: "in", kind: "input", x: 30, y: 180 }];
  const edges = [];
  let prev = "in";
  for (let i = 0; i < n; i++) {
    nodes.push({ id: `p${i}`, kind: "plugin", plugin: i, x: 230 + 200 * i, y: 180 });
    edges.push({ from: prev, to: `p${i}`, gain: 1 });
    prev = `p${i}`;
  }
  nodes.push({ id: "out", kind: "output", x: 230 + 200 * n, y: 180 });
  edges.push({ from: prev, to: "out", gain: 1 });
  return { nodes, edges };
}

/// O fluxo é uma fila simples (a lista consegue representar)?
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
  if (!PL.chain.graph) PL.chain.graph = serialGraph(PL.chain.plugins.length);
  return PL.chain.graph;
}

function hasCycle(g) {
  const out = {};
  g.edges.forEach((e) => (out[e.from] = out[e.from] || []).push(e.to));
  const state = {};
  const visit = (id) => {
    if (state[id] === 1) return true;
    if (state[id] === 2) return false;
    state[id] = 1;
    const cyc = (out[id] || []).some(visit);
    state[id] = 2;
    return cyc;
  };
  return g.nodes.some((n) => visit(n.id));
}

function nodeText(n) {
  if (n.kind === "input") return ["Windows", "som de todos os programas"];
  if (n.kind === "output") return ["Saída", "Surround Rescue + anti-estouro"];
  if (n.kind === "mixer") return ["Mixer", "soma o que chega"];
  if (n.kind === "app") return [`Programa: ${n.exe || "?"}`, "som só deste programa"];
  if (n.kind === "receive") {
    const d = PL.devices.find((x) => x.id === n.device);
    return [`Receber: ${d ? d.name : "dispositivo"}`, d ? d.device : "escolha a entrada"];
  }
  if (n.kind === "fx") {
    const f = FX[n.fx] || { name: n.fx };
    return [f.name, f.unit !== undefined ? `${n.value} ${f.unit}`.trim() : f.hint || "bloco nativo"];
  }
  if (n.kind === "send") {
    const d = PL.devices.find((x) => x.id === n.device);
    return [`Enviar: ${d ? d.name : "dispositivo"}`, d ? `${d.device} · +~30 ms` : "escolha o dispositivo"];
  }
  const p = PL.chain.plugins[n.plugin];
  return [p ? baseName(p.path) : "(plugin removido)", p && !p.enabled ? "desligado" : "plugin VST3"];
}

const portIn = (n) => [n.x, n.y + NODE_H / 2];
const portOut = (n) => [n.x + NODE_W, n.y + NODE_H / 2];
const curve = ([x1, y1], [x2, y2]) => {
  const dx = Math.max(40, Math.abs(x2 - x1) / 2);
  return `M${x1} ${y1} C${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
};

function flRender() {
  const g = graph();
  const svg = $("#fl-canvas");
  // escala 1:1 (tamanho real); passou da área visível, a caixa rola
  const area = svg.parentElement;
  const maxX = Math.max(area.clientWidth, ...g.nodes.map((n) => n.x + NODE_W + 40));
  const maxY = Math.max(area.clientHeight, ...g.nodes.map((n) => n.y + NODE_H + 40));
  svg.setAttribute("viewBox", `0 0 ${maxX} ${maxY}`);
  svg.setAttribute("width", maxX);
  svg.setAttribute("height", maxY);
  const byId = Object.fromEntries(g.nodes.map((n) => [n.id, n]));
  let s = "";
  g.edges.forEach((e, i) => {
    const a = byId[e.from], b = byId[e.to];
    if (!a || !b) return;
    const d = curve(portOut(a), portIn(b));
    const sel = FL.sel?.edge === i;
    s += `<path class="edge${sel ? " sel" : ""}${e.gain < 0.999 ? " weak" : ""}" d="${d}"/>`;
    s += `<path class="hit" data-edge="${i}" d="${d}"><title>Clique para ajustar ou remover a ligação</title></path>`;
    if (Math.abs(e.gain - 1) > 0.001) {
      const [x1, y1] = portOut(a), [x2, y2] = portIn(b);
      s += `<text class="gain" x="${(x1 + x2) / 2}" y="${(y1 + y2) / 2 - 8}">${Math.round(e.gain * 100)}%</text>`;
    }
  });
  for (const n of g.nodes) {
    const [t, sub] = nodeText(n);
    const p = n.kind === "plugin" ? PL.chain.plugins[n.plugin] : null;
    const cls = `node ${n.kind}${FL.sel?.node === n.id ? " sel" : ""}${p && !p.enabled ? " off" : ""}`;
    s += `<g class="${cls}" data-node="${esc(n.id)}" transform="translate(${n.x} ${n.y})">
      <rect class="box" width="${NODE_W}" height="${NODE_H}" rx="10"/>
      <text class="title" x="14" y="23">${esc(t.length > 18 ? t.slice(0, 17) + "…" : t)}</text>
      <text class="sub" x="14" y="41">${esc(sub)}</text>
      ${!["input", "app", "receive"].includes(n.kind) ? `<circle class="port in" data-port-in="${esc(n.id)}" cx="0" cy="${NODE_H / 2}" r="7"/>` : ""}
      ${n.kind !== "output" ? `<circle class="port out" data-port-out="${esc(n.id)}" cx="${NODE_W}" cy="${NODE_H / 2}" r="7"/>` : ""}
    </g>`;
  }
  if (FL.wire) s += `<path class="wire" d="${curve(FL.wire.from, FL.wire.to)}"/>`;
  svg.innerHTML = s;
  flPanel();
}

function flPanel() {
  const g = graph();
  const box = $("#fl-panel");
  if (FL.sel?.edge !== undefined) {
    const e = g.edges[FL.sel.edge];
    if (!e) { FL.sel = null; return flPanel(); }
    const name = (id) => nodeText(g.nodes.find((n) => n.id === id) || {})[0];
    box.innerHTML = `<h2>Ligação</h2><p class="desc">${esc(name(e.from))} → ${esc(name(e.to))}</p>
      <div class="field"><div class="flabel"><span>Volume</span><output id="fl-gain-v">${Math.round(e.gain * 100)}%</output></div>
      <input type="range" id="fl-gain" min="0" max="200" step="1" value="${Math.round(e.gain * 100)}"></div>
      <button class="pill" id="fl-del-edge">Remover ligação</button>`;
    const r = $("#fl-gain");
    fillPct(r);
    r.addEventListener("input", () => { e.gain = Number(r.value) / 100; $("#fl-gain-v").textContent = `${r.value}%`; fillPct(r); flSaveSoon(); flRenderEdgesOnly(); });
    $("#fl-del-edge").addEventListener("click", () => { g.edges.splice(FL.sel.edge, 1); FL.sel = null; flChanged(); });
  } else if (FL.sel?.node) {
    const n = g.nodes.find((x) => x.id === FL.sel.node);
    if (!n) { FL.sel = null; return flPanel(); }
    const [t, sub] = nodeText(n);
    let h = `<h2>${esc(t)}</h2><p class="desc">${esc(sub)}</p>`;
    if (n.kind === "plugin") {
      const p = PL.chain.plugins[n.plugin];
      h += `<button class="pill solid" id="fl-open">Abrir janela do plugin</button>
        <button class="pill" id="fl-toggle">${p?.enabled ? "Desligar" : "Ligar"}</button>`;
    }
    if (n.kind === "send") {
      const outs = PL.devices.filter((d) => d.kind === "Saída" && d.id !== PL.device);
      h += `<div class="field"><div class="flabel"><span>Dispositivo de destino</span></div>
        <select id="fl-send-dev" style="width:100%">${outs.map((d) => `<option value="${d.id}"${d.id === n.device ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}</option>`).join("")}</select></div>
        <p class="desc">O som que chega aqui também toca nesse dispositivo, convertido para o formato dele. Fica uns 30 ms atrás do principal.</p>`;
    }
    if (n.kind === "app") {
      h += `<div class="field"><div class="flabel"><span>Programa (.exe)</span></div>
        <input type="text" id="fl-app-exe" value="${esc(n.exe || "")}" list="fl-app-list" style="width:100%">
        <datalist id="fl-app-list">${FL.apps.map((a) => `<option value="${esc(a)}">`).join("")}</datalist></div>
        <p class="desc">Sugestões: programas tocando som agora. A captura é uma cópia: o programa continua tocando onde estava. Para ouvir só a versão processada, aponte o programa para outra saída nas configurações de som do Windows.</p>`;
    }
    if (n.kind === "receive") {
      const ins = PL.devices.filter((d) => d.kind === "Microfone" && d.id !== PL.device);
      h += `<div class="field"><div class="flabel"><span>Entrada</span></div>
        <select id="fl-recv-dev" style="width:100%">${ins.map((d) => `<option value="${d.id}"${d.id === n.device ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}</option>`).join("")}</select></div>`;
    }
    if (n.kind === "fx") {
      const f = FX[n.fx];
      if (f && f.unit !== undefined) {
        h += `<div class="field"><div class="flabel"><span>${esc(f.name)}</span><output id="fl-fx-v">${n.value} ${f.unit}</output></div>
          <input type="range" id="fl-fx-val" min="${f.min}" max="${f.max}" step="${f.step}" value="${n.value}"></div>`;
      } else if (f) h += `<p class="desc">${esc(f.hint)}</p>`;
    }
    if (!["input", "output"].includes(n.kind)) h += `<button class="pill" id="fl-del-node">Remover caixa</button>`;
    if (n.kind === "input" || n.kind === "output") h += `<p class="desc">Esta caixa é fixa.</p>`;
    box.innerHTML = h;
    $("#fl-open")?.addEventListener("click", () => flOpen(n));
    $("#fl-toggle")?.addEventListener("click", () => { const p = PL.chain.plugins[n.plugin]; p.enabled = !p.enabled; flChanged(); });
    $("#fl-del-node")?.addEventListener("click", () => flRemoveNode(n));
    $("#fl-send-dev")?.addEventListener("change", (e) => { n.device = e.target.value; flChanged(); });
    $("#fl-recv-dev")?.addEventListener("change", (e) => { n.device = e.target.value; flChanged(); });
    $("#fl-app-exe")?.addEventListener("change", (e) => { n.exe = e.target.value.trim().toLowerCase(); flChanged(); });
    const fv = $("#fl-fx-val");
    if (fv) {
      fillPct(fv);
      fv.addEventListener("input", () => { n.value = Number(fv.value); $("#fl-fx-v").textContent = `${n.value} ${FX[n.fx].unit}`; fillPct(fv); flSaveSoon(); flRenderEdgesOnly(); });
    }
  } else {
    box.innerHTML = `<h2>Dicas</h2><p class="desc">Arraste as caixas para organizar. Puxe da bolinha da direita para a da esquerda de outra caixa para ligar. Clique numa seta para mudar o volume dela.</p>
      <p class="desc">Exemplo: ligue o Windows direto na Saída e também num reverb, e o reverb na Saída com 30%. Você ouve o som original com um pouco de reverb.</p>`;
  }
}

function flRenderEdgesOnly() {
  // re-render barato durante o arrasto do volume
  const keep = FL.sel;
  flRender();
  FL.sel = keep;
}

function flOpen(n) {
  invoke("open_plugin_window", { endpoint: PL.device, index: n.plugin }).catch((err) => { $("#error").textContent = String(err); $("#error").hidden = false; });
}

function flRemoveNode(n) {
  const g = graph();
  g.edges = g.edges.filter((e) => e.from !== n.id && e.to !== n.id);
  g.nodes = g.nodes.filter((x) => x.id !== n.id);
  if (n.kind === "plugin") {
    PL.chain.plugins.splice(n.plugin, 1);
    g.nodes.forEach((x) => { if (x.kind === "plugin" && x.plugin > n.plugin) x.plugin -= 1; });
  }
  FL.sel = null;
  flChanged();
}

function flChanged() {
  flRender();
  flSaveSoon();
}

function flSaveSoon() {
  clearTimeout(FL.timer);
  FL.timer = setTimeout(async () => {
    try {
      await invoke("set_chain", { endpoint: PL.device, chain: PL.chain });
      $("#error").hidden = true;
      const t = new Date().toLocaleTimeString("pt-BR");
      $("#fl-saved").textContent = `Salvo ✓ às ${t}`;
    } catch (e) {
      $("#error").textContent = String(e); $("#error").hidden = false;
      $("#fl-saved").textContent = "Não foi possível salvar (veja a mensagem em vermelho)";
    }
  }, 250);
}

function svgPoint(ev) {
  const svg = $("#fl-canvas");
  const pt = svg.createSVGPoint();
  pt.x = ev.clientX; pt.y = ev.clientY;
  const p = pt.matrixTransform(svg.getScreenCTM().inverse());
  return [p.x, p.y];
}

function flBind() {
  const svg = $("#fl-canvas");
  svg.addEventListener("pointerdown", (ev) => {
    const g = graph();
    const t = ev.target;
    if (t.dataset.portOut) {
      const n = g.nodes.find((x) => x.id === t.dataset.portOut);
      FL.wire = { id: n.id, from: portOut(n), to: svgPoint(ev) };
      svg.setPointerCapture(ev.pointerId);
      return;
    }
    const nodeEl = t.closest("[data-node]");
    if (nodeEl && !t.dataset.portIn) {
      const n = g.nodes.find((x) => x.id === nodeEl.dataset.node);
      const [px, py] = svgPoint(ev);
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
    FL.sel = null;
    flRender();
  });
  svg.addEventListener("pointermove", (ev) => {
    if (FL.drag) {
      const [px, py] = svgPoint(ev);
      FL.drag.n.x = Math.max(0, Math.round((px - FL.drag.dx) / 10) * 10);
      FL.drag.n.y = Math.max(0, Math.round((py - FL.drag.dy) / 10) * 10);
      FL.drag.moved = true;
      flRender();
    } else if (FL.wire) {
      FL.wire.to = svgPoint(ev);
      flRender();
      // destaca a entrada sob o cursor
      const el = document.elementFromPoint(ev.clientX, ev.clientY);
      if (el?.dataset?.portIn) el.classList.add("target");
    }
  });
  svg.addEventListener("pointerup", (ev) => {
    if (FL.drag) {
      if (FL.drag.moved) flSaveSoon();
      FL.drag = null;
    }
    if (FL.wire) {
      const el = document.elementFromPoint(ev.clientX, ev.clientY);
      const to = el?.dataset?.portIn;
      const g = graph();
      if (to && to !== FL.wire.id && !g.edges.some((e) => e.from === FL.wire.id && e.to === to)) {
        g.edges.push({ from: FL.wire.id, to, gain: 1 });
        if (hasCycle(g)) {
          g.edges.pop();
          $("#error").textContent = "Essa ligação faria o som andar em círculo. Ligue para uma caixa mais à frente.";
          $("#error").hidden = false;
        } else {
          FL.sel = { edge: g.edges.length - 1 };
          flSaveSoon();
        }
      }
      FL.wire = null;
      flRender();
    }
  });
  svg.addEventListener("dblclick", (ev) => {
    const nodeEl = ev.target.closest("[data-node]");
    const n = nodeEl && graph().nodes.find((x) => x.id === nodeEl.dataset.node);
    if (n?.kind === "plugin") flOpen(n);
  });
  document.addEventListener("keydown", (ev) => {
    if (!$("#tab-fluxo").classList.contains("active") || ev.key !== "Delete" || !FL.sel) return;
    const g = graph();
    if (FL.sel.edge !== undefined) { g.edges.splice(FL.sel.edge, 1); FL.sel = null; flChanged(); }
    else if (FL.sel.node) {
      const n = g.nodes.find((x) => x.id === FL.sel.node);
      if (n && (n.kind === "plugin" || n.kind === "mixer")) flRemoveNode(n);
    }
  });

  $("#fl-add").addEventListener("click", () => {
    const f = PL.found[Number($("#fl-found").value)];
    if (!f) return;
    const g = graph();
    PL.chain.plugins.push({ path: f.path, enabled: true });
    const id = `p${Date.now().toString(36)}`;
    g.nodes.push({ id, kind: "plugin", plugin: PL.chain.plugins.length - 1, x: 260, y: 40 + 70 * (g.nodes.length % 5) });
    FL.sel = { node: id };
    flChanged();
  });
  $("#fl-mixer").addEventListener("click", () => {
    const g = graph();
    const id = `m${Date.now().toString(36)}`;
    g.nodes.push({ id, kind: "mixer", x: 420, y: 320 });
    FL.sel = { node: id };
    flChanged();
  });
  $("#fl-send").addEventListener("click", () => {
    const g = graph();
    const target = PL.devices.find((d) => d.kind === "Saída" && d.id !== PL.device);
    if (!target) { $("#error").textContent = "Não há outro dispositivo de saída para enviar."; $("#error").hidden = false; return; }
    const id = `s${Date.now().toString(36)}`;
    g.nodes.push({ id, kind: "send", device: target.id, x: 440, y: 40 });
    FL.sel = { node: id };
    flChanged();
  });
  const addNode = (node) => {
    const g = graph();
    g.nodes.push({ x: 40, y: 300, ...node });
    FL.sel = { node: node.id };
    flChanged();
  };
  $("#fl-app").addEventListener("click", async () => {
    try { FL.apps = await invoke("audio_apps"); } catch (e) { FL.apps = []; }
    addNode({ id: `a${Date.now().toString(36)}`, kind: "app", exe: FL.apps[0] || "" });
  });
  $("#fl-recv").addEventListener("click", () => {
    const d = PL.devices.find((x) => x.kind === "Microfone" && x.id !== PL.device);
    if (!d) { $("#error").textContent = "Não há outra entrada de áudio para receber."; $("#error").hidden = false; return; }
    addNode({ id: `r${Date.now().toString(36)}`, kind: "receive", device: d.id });
  });
  $("#fl-addfx").addEventListener("click", () => {
    const fx = $("#fl-fx").value;
    addNode({ id: `f${Date.now().toString(36)}`, kind: "fx", fx, value: FX[fx].def ?? 0, x: 240, y: 300 });
  });
  $("#fl-serial").addEventListener("click", () => {
    PL.chain.graph = serialGraph(PL.chain.plugins.length);
    FL.sel = null;
    flChanged();
  });
  $("#fl-device").addEventListener("change", async (e) => {
    PL.device = e.target.value;
    await plLoadChain();
    FL.sel = null;
    flRender();
  });
  $$("#tabs button").forEach((b) => b.addEventListener("click", async () => {
    if (b.dataset.tab !== "fluxo") return;
    await plLoadDevices();
    $("#fl-device").innerHTML = $("#pl-device").innerHTML;
    $("#fl-device").value = PL.device;
    await plLoadChain();
    if (!PL.found.length) await plScan();
    $("#fl-found").innerHTML = $("#pl-found").innerHTML;
    flRender();
  }));
}
flBind();
