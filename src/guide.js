// Guia de início: abre sozinho na primeira vez e pelo menu ("Guia de início").
// Passos: escolher o fone e o microfone → instalar o motor neles → calibrar ouvindo → o que mais dá para fazer.
(() => {
  const DONE_KEY = "sr-guide-done";
  const G = { step: 0, status: null, engines: [] };
  const q = (s) => document.querySelector(s);
  const h = (t) => String(t).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

  const STEPS = [
    { title: "Escolha o fone e o microfone", body: stepDevices },
    { title: "Instale o motor neles", body: stepEngine },
    { title: "Calibre o surround ouvindo", body: stepCalib },
    { title: "Pronto", body: stepDone },
  ];

  function chosen(list, key, auto) {
    const id = snap?.config?.[key];
    return list.find((d) => d.id === id) || auto || list[0];
  }

  function stepDevices() {
    const s = G.status;
    if (!s) return `<p class="desc">Lendo os dispositivos…</p>`;
    const opt = (list, cur) => list.map((d) => `<option value="${h(d.id)}"${cur && cur.id === d.id ? " selected" : ""}>${h(d.name)} (${h(d.device)})</option>`).join("");
    const out = chosen(s.outputs, "output_device", s.output), inp = chosen(s.inputs, "input_device", s.input);
    return `<p class="desc">O surround, o equalizador e o resto agem no fone escolhido aqui. As melhorias de voz agem no microfone.</p>
      <label class="gd-field"><span>Fone / saída de som</span><select id="gd-out">${opt(s.outputs, out)}</select></label>
      <label class="gd-field"><span>Microfone</span><select id="gd-in">${opt(s.inputs, inp)}</select></label>`;
  }

  function stepEngine() {
    const s = G.status;
    const out = chosen(s.outputs, "output_device", s.output), inp = chosen(s.inputs, "input_device", s.input);
    const has = (flow, d) => d && G.engines.some((e) => e.flow === flow && e.id === d.id && e.engine);
    const row = (label, d, ok) => `<li class="${ok ? "ok" : ""}"><span class="gd-dot"></span><b>${h(label)}</b> ${h(d ? d.name : "—")}<small>${ok ? "motor instalado" : "sem o motor"}</small></li>`;
    const both = has("Render", out) && (!inp || has("Capture", inp));
    return `<p class="desc">O Windows só deixa um efeito agir no som de um dispositivo se ele estiver instalado nele. Vai aparecer um pedido de administrador, e o som de tudo para por alguns segundos.</p>
      <ul class="gd-list">${row("Fone", out, has("Render", out))}${row("Microfone", inp, has("Capture", inp))}</ul>
      ${both ? `<p class="gd-ok">Tudo certo: o motor já está nos dois.</p>` : `<button class="pill solid" id="gd-install">Instalar no fone e no microfone</button>`}
      <p class="desc" id="gd-msg"></p>`;
  }

  // caixas virtuais: [id, índice no motor, ângulo usado (ou null = centro), lado]
  const CAL = [["FL", 0, "front_angle", -1], ["FR", 1, "front_angle", 1], ["C", 2, null, 0], ["SL", 3, "side_angle", -1],
    ["SR", 4, "side_angle", 1], ["RL", 5, "rear_angle", -1], ["RR", 6, "rear_angle", 1]];
  const CAL_NAMES = { FL: "Frente esq.", FR: "Frente dir.", C: "Centro", SL: "Lateral esq.", SR: "Lateral dir.", RL: "Trás esq.", RR: "Trás dir." };
  let playing = -1;

  function stepCalib() {
    const s = cur();
    const slider = (k, label, min, max) => `<label class="gd-slider"><span>${label}</span><output id="gd-o-${k}">${s[k]}°</output>
      <input type="range" min="${min}" max="${max}" step="1" value="${s[k]}" data-gd="${k}"></label>`;
    return `<p class="desc">Clique numa caixa: uma nota toca só nela. Se o som não parecer vir de onde a caixa está, mexa no ângulo daquele par até parecer.</p>
      <div class="gd-cal">
        <div class="gd-ring" id="gd-ring">${CAL.map(([id, i]) => `<button class="gd-spk" data-gd-spk="${i}" title="${CAL_NAMES[id]}">${id}</button>`).join("")}<span class="gd-head"></span></div>
        <div class="gd-sliders">
          ${slider("front_angle", "Frente", 10, 80)}${slider("side_angle", "Laterais", 60, 120)}${slider("rear_angle", "Trás", 110, 175)}
          <div class="gd-row"><button class="pill solid" id="gd-round">Dar a volta</button><button class="pill" id="gd-stop">Parar</button></div>
        </div>
      </div>`;
  }

  function placeSpeakers() {
    const s = cur();
    document.querySelectorAll("[data-gd-spk]").forEach((b) => {
      const [, , k, side] = CAL[Number(b.dataset.gdSpk)];
      const deg = k ? side * s[k] : 0;
      const t = (deg * Math.PI) / 180, r = 42;
      b.style.left = `${50 + Math.sin(t) * r}%`;
      b.style.top = `${50 - Math.cos(t) * r}%`;
      b.classList.toggle("on", Number(b.dataset.gdSpk) === playing);
    });
  }

  function play(i) {
    playing = i;
    invoke("engine_test", { speaker: i }).catch(() => {});
    placeSpeakers();
  }

  function stepDone() {
    const tip = (t, d) => `<li><b>${t}</b><span>${d}</span></li>`;
    return `<ul class="gd-tips">
      ${tip("Som", "surround, sala virtual, graves, normalização e clareza de voz")}
      ${tip("Equalizador", "10 faixas e predefinições (jogo, filme, música)")}
      ${tip("Microfone", "redução de ruído, porta de ruído e ganho")}
      ${tip("Fluxo", "plugins VST, mesa de som, mandar som de um programa para o microfone")}
      ${tip("Perfis", "um ajuste por jogo ou programa, trocado sozinho")}
    </ul>
    <p class="desc">O app continua rodando perto do relógio quando você fecha a janela. Este guia fica no menu, em "Guia de início".</p>`;
  }

  function render() {
    const st = STEPS[G.step];
    q("#gd-steps").innerHTML = STEPS.map((s, i) => `<li class="${i === G.step ? "on" : i < G.step ? "done" : ""}"><span>${i + 1}</span>${h(s.title)}</li>`).join("");
    q("#gd-title").textContent = st.title;
    q("#gd-body").innerHTML = st.body();
    q("#gd-back").hidden = G.step === 0;
    q("#gd-next").textContent = G.step === STEPS.length - 1 ? "Começar a usar" : "Continuar";
    bindStep();
  }

  function bindStep() {
    q("#gd-install")?.addEventListener("click", install);
    q("#gd-round")?.addEventListener("click", () => play(7));
    q("#gd-stop")?.addEventListener("click", () => play(-1));
    document.querySelectorAll("[data-gd-spk]").forEach((b) => b.addEventListener("click", () => play(playing === Number(b.dataset.gdSpk) ? -1 : Number(b.dataset.gdSpk))));
    document.querySelectorAll("[data-gd]").forEach((el) => el.addEventListener("input", () => {
      const k = el.dataset.gd;
      cur()[k] = Number(el.value);
      if (typeof fillPct === "function") fillPct(el);
      q(`#gd-o-${k}`).textContent = `${el.value}°`;
      placeSpeakers();
      saveSoon();
      // mexeu num par: toca uma caixa dele para conferir
      const i = CAL.findIndex(([, , key, side]) => key === k && side === 1);
      if (playing !== i && playing !== 7) play(i);
    }));
    if (q("#gd-ring")) placeSpeakers();
    if (typeof fillPct === "function") document.querySelectorAll("[data-gd]").forEach(fillPct);
  }

  async function saveDevices() {
    const o = q("#gd-out"), i = q("#gd-in");
    if (!o) return;
    snap.config.output_device = o.value || null;
    snap.config.input_device = i?.value || null;
    try { { const r = await invoke("set_config", { config: snap.config }); if (r && r.config) snap = r; } } catch (e) { /* segue: Configurações mostra o erro */ }
  }

  async function install() {
    const s = G.status;
    const out = chosen(s.outputs, "output_device", s.output), inp = chosen(s.inputs, "input_device", s.input);
    const items = [];
    if (out) items.push(["Render", out.id]);
    if (inp) items.push(["Capture", inp.id]);
    q("#gd-install").disabled = true;
    q("#gd-msg").textContent = "Instalando… aceite o pedido de administrador.";
    try {
      await invoke("set_engine", { on: true, items });
      // o motor próprio passa a valer (o Equalizer APO fica em espera)
      snap.config.own_engine = true;
      { const r = await invoke("set_config", { config: snap.config }); if (r && r.config) snap = r; }
    } catch (e) {
      q("#gd-msg").textContent = `Não deu certo: ${e}`;
    }
    await load();
    render();
  }

  async function load() {
    try { G.status = await invoke("get_status"); } catch (e) { G.status = { outputs: [], inputs: [] }; }
    try { G.engines = await invoke("engine_devices"); } catch (e) { G.engines = []; }
  }

  async function open() {
    G.step = 0;
    q("#guide").hidden = false;
    render();
    await load();
    render();
  }

  function close() {
    play(-1);
    q("#guide").hidden = true;
    try { localStorage.setItem(DONE_KEY, "1"); } catch (e) { /* sem armazenamento: abre de novo na próxima vez */ }
  }

  document.addEventListener("DOMContentLoaded", () => {});
  window.addEventListener("load", () => {
    q("#gd-next").addEventListener("click", async () => {
      if (G.step === 0) await saveDevices();
      if (G.step === 2) play(-1);
      if (G.step === STEPS.length - 1) return close();
      G.step++;
      render();
    });
    q("#gd-back").addEventListener("click", () => { if (G.step === 2) play(-1); G.step = Math.max(0, G.step - 1); render(); });
    q("#gd-close").addEventListener("click", close);
    q("#guide-open").addEventListener("click", open);
    let done = false;
    try { done = localStorage.getItem(DONE_KEY) === "1"; } catch (e) { /* trata como primeira vez */ }
    if (!done) setTimeout(() => { if (typeof snap !== "undefined" && snap) open(); }, 600);
  });
})();
