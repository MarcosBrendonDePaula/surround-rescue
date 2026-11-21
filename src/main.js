const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const EQ_FREQS = ["31", "63", "125", "250", "500", "1K", "2K", "4K", "8K", "16K"];
const FMT = {
  front_angle: (v) => `${v}°`, spatial: (v) => `${v}`, volume_db: (v) => `${v > 0 ? "+" : ""}${v} dB`,
  gain_db: (v) => `${v > 0 ? "+" : ""}${v} dB`,
};
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];

let snap = null; // { config, applied, eq_presets }
let timer = null;

const cfg = () => snap.config;
const cur = () => cfg().profiles.find((p) => p.name === cfg().active).settings;

function fillPct(el) {
  const p = ((el.value - el.min) / (el.max - el.min)) * 100;
  el.style.setProperty("--p", `${p}%`);
}

// ---------- render ----------
function render() {
  const s = cur();
  const c = cfg();

  // perfis
  const opts = c.profiles.map((p) => `<option${p.name === c.active ? " selected" : ""}>${esc(p.name)}</option>`).join("");
  $("#profile").innerHTML = opts;
  $("#rule-profile").innerHTML = c.profiles.map((p) => `<option>${esc(p.name)}</option>`).join("");
  $("#p-del").disabled = c.profiles.length < 2;

  // campos do perfil
  for (const el of $$("[data-s]")) {
    const k = el.dataset.s;
    if (el.type === "checkbox") el.checked = s[k];
    else { el.value = s[k]; fillPct(el); }
  }
  for (const o of $$("[data-o]")) {
    const k = o.dataset.o;
    o.textContent = FMT[k] ? FMT[k](s[k]) : s[k];
  }
  for (const f of $$("[data-dep]")) f.classList.toggle("off", !s[f.dataset.dep]);

  // microfone
  for (const el of $$("[data-m]")) {
    const k = el.dataset.m;
    if (el.type === "checkbox") el.checked = c.mic[k];
    else { el.value = c.mic[k]; fillPct(el); }
  }
  for (const o of $$("[data-mo]")) {
    const k = o.dataset.mo;
    o.textContent = FMT[k] ? FMT[k](c.mic[k]) : c.mic[k];
  }

  // equalizador
  $("#eq-preset").innerHTML =
    snap.eq_presets.map(([id, label]) => `<option value="${id}"${id === s.eq_preset ? " selected" : ""}>${label}</option>`).join("") +
    `<option value="personalizado"${s.eq_preset === "personalizado" ? " selected" : ""}>Personalizado</option>`;
  $$(".band input").forEach((el, i) => { el.value = s.eq[i]; fillPct(el); el.previousElementSibling.textContent = fmtDb(s.eq[i]); });
  $("#eq").classList.toggle("off", !s.eq_enabled);

  // regras
  $("#auto-switch").checked = c.auto_switch;
  $("#applied").textContent = snap.applied || c.active;
  $("#rules").innerHTML = c.app_rules.length
    ? c.app_rules.map((r, i) => `<tr><td>${esc(r.exe)}</td><td>${esc(r.profile)}</td><td><button data-del="${i}" title="Remover">✕</button></td></tr>`).join("")
    : `<tr><td colspan="3" class="desc">Nenhum programa ainda.</td></tr>`;

  document.body.classList.toggle("off", !s.enabled);
  $(".power-text").textContent = s.enabled ? "Surround ligado" : "Surround desligado";
  $$(".band input").forEach((el, i) => el.classList.toggle("neg", s.eq[i] < 0));
  drawStage(s);
  invoke("preview").then((t) => ($("#preview").textContent = t));
}

function fmtDb(v) { return `${v > 0 ? "+" : ""}${v}`; }
function esc(t) { return String(t).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]); }

// ---------- palco (calibração) ----------
const R = 112;
const NAMES = { C: "Centro", FL: "Frente esq.", FR: "Frente dir.", SL: "Lateral esq.", SR: "Lateral dir.", RL: "Trás esq.", RR: "Trás dir." };
const pos = (deg, r = R) => { const t = (deg * Math.PI) / 180; return [Math.sin(t) * r, -Math.cos(t) * r]; };

function drawStage(s) {
  const a = s.front_angle;
  const spk = [["C", 0, false], ["FL", -a, true], ["FR", a, true], ["SL", -90, false], ["SR", 90, false], ["RL", -150, false], ["RR", 150, false]];
  let svg = `<circle class="floor" r="${R + 30}"/>`;
  svg += `<circle class="orbit" r="${R * 0.45}"/><circle class="orbit main" r="${R}"/>`;
  for (let d = 0; d < 360; d += 15) {
    const [x1, y1] = pos(d, R + 22), [x2, y2] = pos(d, R + (d % 45 ? 26 : 30));
    svg += `<line class="tick" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}"/>`;
  }
  // feixe entre as duas caixas frontais (a "imagem" estéreo)
  const [lx, ly] = pos(-a), [rx, ry] = pos(a);
  svg += `<path class="beam" d="M0 0 L${lx} ${ly} A${R} ${R} 0 0 1 ${rx} ${ry} Z"/>`;
  svg += `<g id="waves"></g>`;
  for (const [id, deg, active] of spk) {
    const [x, y] = pos(deg);
    svg += `<g class="spkg" data-spk="${id}" transform="translate(${x} ${y}) rotate(${deg})">
      <title>${NAMES[id]}${active ? " — arraste para ajustar" : " — disponível no 7.1"}</title>
      <rect class="spk${active ? " on" : ""}" x="-11" y="-9" width="22" height="18" rx="5"/></g>`;
    const [tx, ty] = pos(deg, R - 24);
    svg += `<text class="lbl${active ? " on" : ""}" x="${tx}" y="${ty + 3}">${id}</text>`;
  }
  // cabeça vista de cima
  svg += `<ellipse class="head" cx="0" cy="2" rx="17" ry="20"/><ellipse class="head" cx="-18" cy="2" rx="4" ry="7"/><ellipse class="head" cx="18" cy="2" rx="4" ry="7"/>`;
  svg += `<path class="nose" d="M-5 -19 L0 -27 L5 -19 Z"/>`;
  svg += `<text class="angle" x="0" y="${R * 0.45 + 18}">${Math.round(a * 2)}° de abertura</text>`;
  $("#stage").innerHTML = svg;
}

// arrastar as caixas frontais muda o ângulo
function bindStageDrag() {
  const stage = $("#stage");
  let dragging = false;
  const toAngle = (ev) => {
    const pt = stage.createSVGPoint(); pt.x = ev.clientX; pt.y = ev.clientY;
    const p = pt.matrixTransform(stage.getScreenCTM().inverse());
    const deg = Math.abs((Math.atan2(p.x, -p.y) * 180) / Math.PI);
    return Math.round(Math.min(90, Math.max(10, deg)));
  };
  stage.addEventListener("pointerdown", (ev) => {
    const g = ev.target.closest("[data-spk]");
    if (!g || !["FL", "FR"].includes(g.dataset.spk)) return;
    dragging = true; stage.setPointerCapture(ev.pointerId);
  });
  stage.addEventListener("pointermove", (ev) => {
    if (!dragging) return;
    const v = toAngle(ev);
    if (v !== cur().front_angle) { cur().front_angle = v; render(); saveSoon(); }
  });
  const end = () => { dragging = false; };
  stage.addEventListener("pointerup", end);
  stage.addEventListener("pointercancel", end);
}

// ondas saindo das caixas que estão tocando o teste
function emitWaves(ids) {
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  const g = $("#waves"); if (!g) return;
  const a = cur().front_angle;
  const deg = { FL: -a, FR: a };
  for (const id of ids) {
    const [x, y] = pos(deg[id]);
    for (let k = 0; k < 3; k++) {
      const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
      c.setAttribute("class", "wave"); c.setAttribute("cx", x); c.setAttribute("cy", y); c.setAttribute("r", 12);
      g.appendChild(c);
      c.animate([{ r: 12, opacity: 0.9 }, { r: 70, opacity: 0 }], { duration: 900, delay: k * 260, easing: "ease-out", fill: "both" })
        .onfinish = () => c.remove();
    }
  }
}

// ---------- salvar ----------
async function save() {
  try {
    snap = await invoke("set_config", { config: cfg() });
    $("#error").hidden = true;
  } catch (e) {
    $("#error").textContent = String(e);
    $("#error").hidden = false;
  }
  render();
}
function saveSoon() { clearTimeout(timer); timer = setTimeout(save, 120); }

// ---------- status ----------
async function refreshStatus() {
  const s = await invoke("get_status");
  const label = (d) => `${d.name} (${d.device})`;
  const items = [
    [s.apo_installed, "Motor de áudio instalado", "Motor de áudio não instalado: reinstale o Surround Rescue", "bad"],
    [s.output?.apo, `Efeito ativo em: ${s.output ? label(s.output) : ""}`, "Nenhuma saída com o efeito ativo: escolha o fone acima e ative", "bad"],
    [s.input?.apo, `Microfone com melhorias: ${s.input ? label(s.input) : ""}`, "Nenhum microfone com o motor ativo", "warn"],
    [s.config_hooked, "Configuração aplicada", "Configuração ainda não aplicada", "warn"],
    [!s.razer_service_running, "Razer Surround antigo desligado", "Razer Surround antigo ainda rodando: pode disputar o fone", "warn"],
  ];
  $("#status").innerHTML = items.map(([ok, y, n, l]) => `<div class="${ok ? "ok" : l}">${esc(ok ? y : n)}</div>`).join("");
  $("#autostart").checked = s.autostart;
  $("#own-engine").checked = !!cfg().own_engine;
  renderDevices("#dev-out", "#dev-out-state", s.outputs, s.output, "output_device", "Render");
  renderDevices("#dev-in", "#dev-in-state", s.inputs, s.input, "input_device", "Capture");
}

function renderDevices(selSel, stateSel, list, chosen, key, flow) {
  const sel = $(selSel);
  const pinned = cfg()[key];
  sel.innerHTML = `<option value=""${pinned ? "" : " selected"}>Automático${!pinned && chosen ? ` — ${esc(chosen.name)}` : ""}</option>` +
    list.map((d) => `<option value="${d.id}"${pinned === d.id ? " selected" : ""}>${esc(`${d.name} (${d.device})`)}${d.apo ? " ✓" : ""}</option>`).join("");
  const target = list.find((d) => d.id === (pinned || chosen?.id));
  const box = $(stateSel);
  if (!target) { box.innerHTML = `<span class="tag">Nenhum dispositivo com o motor ativo.</span>`; return; }
  box.innerHTML = target.apo
    ? `<span class="tag on">● Efeito ativo neste dispositivo</span><button class="pill" data-engine="off">Desativar aqui</button>`
    : target.needs_configurator
      ? `<span class="tag">Este dispositivo usa um modo que só o Configurator do Equalizer APO consegue ativar.</span>`
      : `<span class="tag">Sem efeito neste dispositivo</span><button class="pill solid" data-engine="on">Ativar neste dispositivo</button>`;
  box.querySelector("[data-engine]")?.addEventListener("click", async (e) => {
    const on = e.target.dataset.engine === "on";
    e.target.disabled = true;
    e.target.textContent = on ? "Ativando… confirme a permissão" : "Desativando…";
    try {
      await invoke("set_device_engine", { flow, id: target.id, on });
      cfg()[key] = target.id;
      await save();
      $("#error").hidden = true;
    } catch (err) { $("#error").textContent = String(err); $("#error").hidden = false; }
    refreshStatus();
  });
}

// ---------- testes de som ----------
let actx = null;
function testTone(pan) {
  actx = actx || new AudioContext();
  const len = actx.sampleRate * 0.9;
  const buf = actx.createBuffer(1, len, actx.sampleRate);
  const d = buf.getChannelData(0);
  let b0 = 0, b1 = 0, b2 = 0;
  for (let i = 0; i < len; i++) { // ruído rosa simples com envelope
    const w = Math.random() * 2 - 1;
    b0 = 0.99765 * b0 + w * 0.099; b1 = 0.963 * b1 + w * 0.2965; b2 = 0.57 * b2 + w * 1.0526;
    const env = Math.min(1, i / 2000) * Math.min(1, (len - i) / 4000) * (Math.floor(i / (actx.sampleRate * 0.15)) % 2 ? 0.3 : 1);
    d[i] = (b0 + b1 + b2 + w * 0.1848) * 0.08 * env;
  }
  const src = actx.createBufferSource();
  src.buffer = buf;
  const p = actx.createStereoPanner();
  p.pan.value = pan;
  src.connect(p).connect(actx.destination);
  src.start();
}

// ---------- eventos ----------
function bind() {
  $$("#tabs button").forEach((b) => b.addEventListener("click", () => {
    $$("#tabs button").forEach((x) => x.classList.toggle("active", x === b));
    $$(".tab").forEach((t) => t.classList.toggle("active", t.id === `tab-${b.dataset.tab}`));
    if (b.dataset.tab === "configuracoes") refreshStatus();
    if (b.dataset.tab === "perfis") invoke("recent_apps").then((a) => ($("#recent-apps").innerHTML = a.map((x) => `<option value="${esc(x)}">`).join("")));
  }));

  for (const el of $$("[data-s]")) {
    el.addEventListener(el.type === "checkbox" ? "change" : "input", () => {
      cur()[el.dataset.s] = el.type === "checkbox" ? el.checked : Number(el.value);
      render(); saveSoon();
    });
  }
  for (const el of $$("[data-m]")) {
    el.addEventListener(el.type === "checkbox" ? "change" : "input", () => {
      cfg().mic[el.dataset.m] = el.type === "checkbox" ? el.checked : Number(el.value);
      render(); saveSoon();
    });
  }

  // equalizador
  $("#eq").innerHTML = EQ_FREQS.map((f, i) =>
    `<div class="band"><output></output><input type="range" min="-12" max="12" step="0.5" data-band="${i}"><span>${f}</span></div>`).join("");
  $$(".band input").forEach((el) => el.addEventListener("input", () => {
    cur().eq[Number(el.dataset.band)] = Number(el.value);
    cur().eq_preset = "personalizado";
    render(); saveSoon();
  }));
  $("#eq-preset").addEventListener("change", (e) => {
    const p = snap.eq_presets.find(([id]) => id === e.target.value);
    if (p) { cur().eq = [...p[2]]; cur().eq_preset = p[0]; save(); }
  });

  // perfis
  $("#profile").addEventListener("change", (e) => { cfg().active = e.target.value; save(); });
  $("#p-new").addEventListener("click", () => {
    let n = 1; while (cfg().profiles.some((p) => p.name === `Perfil ${n}`)) n++;
    cfg().profiles.push({ name: `Perfil ${n}`, settings: structuredClone(cur()) });
    cfg().active = `Perfil ${n}`;
    save();
  });
  $("#p-rename").addEventListener("click", () => {
    const sel = $("#profile");
    const input = document.createElement("input");
    input.type = "text"; input.value = cfg().active;
    sel.replaceWith(input); input.focus(); input.select();
    const done = (commit) => {
      const name = input.value.trim();
      if (commit && name && !cfg().profiles.some((p) => p.name === name)) {
        const old = cfg().active;
        cfg().profiles.find((p) => p.name === old).name = name;
        cfg().app_rules.forEach((r) => { if (r.profile === old) r.profile = name; });
        cfg().active = name;
      }
      input.replaceWith(sel);
      commit ? save() : render();
    };
    input.addEventListener("keydown", (e) => { if (e.key === "Enter") done(true); if (e.key === "Escape") done(false); });
    input.addEventListener("blur", () => done(true), { once: true });
  });
  $("#p-del").addEventListener("click", () => {
    if (cfg().profiles.length < 2) return;
    const old = cfg().active;
    cfg().profiles = cfg().profiles.filter((p) => p.name !== old);
    cfg().app_rules = cfg().app_rules.filter((r) => r.profile !== old);
    cfg().active = cfg().profiles[0].name;
    save();
  });

  // regras por programa
  $("#auto-switch").addEventListener("change", (e) => { cfg().auto_switch = e.target.checked; save(); });
  $("#rule-add").addEventListener("click", () => {
    const exe = $("#rule-exe").value.trim().toLowerCase();
    if (!exe) return;
    cfg().app_rules = cfg().app_rules.filter((r) => r.exe !== exe);
    cfg().app_rules.push({ exe, profile: $("#rule-profile").value });
    $("#rule-exe").value = "";
    save();
  });
  $("#rules").addEventListener("click", (e) => {
    const i = e.target.dataset.del;
    if (i !== undefined) { cfg().app_rules.splice(Number(i), 1); save(); }
  });

  // configurações
  $("#own-engine").addEventListener("change", (e) => { cfg().own_engine = e.target.checked; save(); });
  $("#dev-out").addEventListener("change", (e) => { cfg().output_device = e.target.value || null; save().then(refreshStatus); });
  $("#dev-in").addEventListener("change", (e) => { cfg().input_device = e.target.value || null; save().then(refreshStatus); });
  $("#autostart").addEventListener("change", async (e) => {
    try { await invoke("set_autostart", { on: e.target.checked }); } catch (err) { $("#error").textContent = String(err); $("#error").hidden = false; }
    refreshStatus();
  });

  // testes
  $("#test-left").addEventListener("click", () => { testTone(-1); emitWaves(["FL"]); });
  $("#test-right").addEventListener("click", () => { testTone(1); emitWaves(["FR"]); });
  $("#test-both").addEventListener("click", () => { testTone(0); emitWaves(["FL", "FR"]); });
  bindStageDrag();

  listen("snapshot", (e) => { snap = e.payload; render(); });
}

async function init() {
  snap = await invoke("get_snapshot");
  bind();
  render();
  refreshStatus();
}

init();

// ---------- plugins (cadeia de VST3 por dispositivo) ----------
const PL = { device: null, chain: { plugins: [] }, found: [], devices: [] };
const baseName = (p) => String(p).split(/[\\/]/).pop().replace(/\.vst3$/i, "");

async function plLoadDevices() {
  const s = await invoke("get_status");
  PL.devices = [
    ...s.outputs.map((d) => ({ ...d, kind: "Saída" })),
    ...s.inputs.map((d) => ({ ...d, kind: "Microfone" })),
  ];
  if (!PL.device) PL.device = s.output?.id || PL.devices[0]?.id || null;
  $("#pl-device").innerHTML = PL.devices
    .map((d) => `<option value="${d.id}"${d.id === PL.device ? " selected" : ""}>${esc(`${d.kind}: ${d.name} (${d.device})`)}</option>`)
    .join("");
}

async function plLoadChain() {
  if (!PL.device) return;
  try { PL.chain = await invoke("get_chain", { endpoint: PL.device }); } catch (e) { PL.chain = { plugins: [] }; }
  plRender();
}

async function plScan() {
  $("#pl-found").innerHTML = `<option>Procurando plugins…</option>`;
  try { PL.found = await invoke("scan_plugins"); } catch (e) { PL.found = []; $("#error").textContent = String(e); $("#error").hidden = false; }
  $("#pl-found").innerHTML = PL.found.length
    ? PL.found.map((f, i) => `<option value="${i}">${esc(f.name || baseName(f.path))}</option>`).join("")
    : `<option value="">Nenhum VST3 encontrado nas pastas padrão</option>`;
}

/// Lista só edita a estrutura quando o fluxo é uma fila simples (aí o desenho vira fila de novo).
function plListEditable() {
  return typeof isSerial !== "function" || isSerial(PL.chain.graph, PL.chain.plugins.length);
}
function plListEdit() {
  if (!plListEditable()) {
    $("#error").textContent = "Este dispositivo tem um fluxo com caminhos paralelos. Edite pela aba Fluxo (ou use 'Organizar em fila' lá).";
    $("#error").hidden = false;
    return false;
  }
  PL.chain.graph = null;
  return true;
}

function plRender() {
  const list = PL.chain.plugins;
  const rows = list.map((p, i) => `
    <li class="${p.enabled ? "" : "off"}" draggable="true" data-pl-idx="${i}">
      <span class="grip" title="Arraste para mudar a ordem" aria-hidden="true">⋮⋮</span>
      <label class="toggle" title="Ligar ou desligar este plugin"><input type="checkbox" data-pl-on="${i}"${p.enabled ? " checked" : ""}><span></span></label>
      <div><div class="pname">${esc(baseName(p.path))}</div><div class="ppath">${esc(p.path)}</div></div>
      <div class="pactions">
        <button data-pl-up="${i}" title="Subir"${i === 0 ? " disabled" : ""}>↑</button>
        <button data-pl-down="${i}" title="Descer"${i === list.length - 1 ? " disabled" : ""}>↓</button>
        <button data-pl-win="${i}" title="Abrir a janela do plugin"${p.enabled ? "" : " disabled"}>▣</button>
        <button data-pl-del="${i}" title="Remover">×</button>
      </div>
    </li>`).join("");
  const locked = !plListEditable();
  $("#pl-chain").classList.toggle("locked", locked);
  $("#pl-chain").innerHTML = (locked ? `<li class="fixed empty-row"><span></span><span class="chain-locked">Fluxo com caminhos paralelos: a ordem e as ligações são editadas na aba Fluxo.</span></li>` : "") + (rows || `<li class="fixed empty-row"><span></span><span class="empty">Nenhum plugin ainda. Adicione um abaixo.</span></li>`) +
    `<li class="fixed"><span></span><div><div class="pname">Surround Rescue</div><div class="ppath">surround, equalizador e anti-estouro (sempre por último)</div></div></li>`;
  invoke("host_status").then((h) => {
    $("#pl-engine").textContent = !h.found
      ? "O executor de VST não foi encontrado nesta instalação."
      : `Executor de VST ${h.running ? "rodando" : "parado (sobe sozinho ao adicionar um plugin)"}. Os plugins só tocam em dispositivos com o motor próprio do Surround Rescue (em teste).`;
  });
}

async function plSave() {
  try {
    await invoke("set_chain", { endpoint: PL.device, chain: PL.chain });
    $("#error").hidden = true;
  } catch (e) { $("#error").textContent = String(e); $("#error").hidden = false; }
  plRender();
}

function plBind() {
  $("#pl-device").addEventListener("change", (e) => { PL.device = e.target.value; plLoadChain(); });
  $("#pl-rescan").addEventListener("click", plScan);
  $("#pl-add").addEventListener("click", () => {
    const f = PL.found[Number($("#pl-found").value)];
    if (!f) return;
    if (!plListEdit()) return;
    PL.chain.plugins.push({ path: f.path, enabled: true });
    plSave();
  });
  $("#pl-add-path").addEventListener("click", () => {
    const p = $("#pl-path").value.trim().replace(/^"|"$/g, "");
    if (!/\.vst3$/i.test(p)) { $("#error").textContent = "Informe o caminho de um arquivo ou pasta .vst3."; $("#error").hidden = false; return; }
    if (!plListEdit()) return;
    PL.chain.plugins.push({ path: p, enabled: true });
    $("#pl-path").value = "";
    plSave();
  });
  $("#pl-chain").addEventListener("click", (e) => {
    const t = e.target.closest("button");
    if (!t) return;
    const L = PL.chain.plugins;
    if (t.dataset.plWin !== undefined) {
      invoke("open_plugin_window", { endpoint: PL.device, index: Number(t.dataset.plWin) })
        .catch((err) => { $("#error").textContent = String(err); $("#error").hidden = false; });
      return;
    }
    const i = Number(t.dataset.plUp ?? t.dataset.plDown ?? t.dataset.plDel);
    if (!plListEdit()) return;
    if (t.dataset.plUp !== undefined && i > 0) [L[i - 1], L[i]] = [L[i], L[i - 1]];
    else if (t.dataset.plDown !== undefined && i < L.length - 1) [L[i + 1], L[i]] = [L[i], L[i + 1]];
    else if (t.dataset.plDel !== undefined) L.splice(i, 1);
    else return;
    plSave();
  });
  // arrastar e soltar para mudar a ordem
  let dragFrom = null;
  const chainEl = $("#pl-chain");
  chainEl.addEventListener("dragstart", (e) => {
    const li = e.target.closest("li[data-pl-idx]");
    if (!li) return;
    dragFrom = Number(li.dataset.plIdx);
    li.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
  });
  chainEl.addEventListener("dragover", (e) => {
    const li = e.target.closest("li[data-pl-idx]");
    if (dragFrom === null || !li) return;
    e.preventDefault();
    $$("#pl-chain li").forEach((x) => x.classList.toggle("drop-here", x === li));
  });
  chainEl.addEventListener("drop", (e) => {
    const li = e.target.closest("li[data-pl-idx]");
    if (dragFrom === null || !li) return;
    e.preventDefault();
    const to = Number(li.dataset.plIdx);
    if (to !== dragFrom) {
      if (!plListEdit()) return;
      const [moved] = PL.chain.plugins.splice(dragFrom, 1);
      PL.chain.plugins.splice(to, 0, moved);
      plSave();
    }
  });
  chainEl.addEventListener("dragend", () => {
    dragFrom = null;
    $$("#pl-chain li").forEach((x) => x.classList.remove("dragging", "drop-here"));
  });
  $("#pl-chain").addEventListener("change", (e) => {
    const i = e.target.dataset.plOn;
    if (i === undefined) return;
    PL.chain.plugins[Number(i)].enabled = e.target.checked;
    plSave();
  });
  // carrega ao abrir a aba
  $$("#tabs button").forEach((b) => b.addEventListener("click", async () => {
    if (b.dataset.tab !== "plugins") return;
    await plLoadDevices();
    await plLoadChain();
    if (!PL.found.length) plScan();
  }));
}
plBind();
