const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const FIELDS = ["spatial", "width", "bass_db", "clarity_db", "treble_db", "volume_db"];
const UNIT = { spatial: "%", width: "%", bass_db: " dB", clarity_db: " dB", treble_db: " dB", volume_db: " dB" };
let settings = null;
let timer = null;

const $ = (id) => document.getElementById(id);

function render() {
  $("enabled").checked = settings.enabled;
  document.body.classList.toggle("off", !settings.enabled);
  for (const f of FIELDS) {
    const el = $(f);
    el.value = settings[f];
    el.nextElementSibling.textContent = `${settings[f]}${UNIT[f]}`;
  }
  document.querySelectorAll("#presets button").forEach((b) =>
    b.classList.toggle("active", b.dataset.p === settings.profile));
  invoke("preview", { settings }).then((t) => ($("preview").textContent = t));
}

async function save() {
  try {
    settings = await invoke("set_settings", { settings });
    $("error").hidden = true;
  } catch (e) {
    $("error").textContent = String(e);
    $("error").hidden = false;
  }
  render();
  refreshStatus();
}

function saveSoon() {
  clearTimeout(timer);
  timer = setTimeout(save, 150);
}

async function refreshStatus() {
  const s = await invoke("get_status");
  const items = [
    [s.apo_installed, "Equalizer APO instalado", "Equalizer APO não instalado", "bad"],
    [s.apo_on_kraken, "Efeito ativo no Kraken", s.kraken_found ? "Equalizer APO não está ligado no Kraken (abra o Configurator)" : "Kraken não encontrado", "bad"],
    [s.config_hooked, "Configuração conectada", "Configuração ainda não aplicada", "warn"],
    [!s.razer_service_running, "Razer Surround desligado", "Razer Surround ainda rodando: pode disputar o fone", "warn"],
  ];
  $("status").innerHTML = items
    .map(([ok, yes, no, lvl]) => `<div class="${ok ? "ok" : lvl}">${ok ? yes : no}</div>`)
    .join("");
}

async function init() {
  settings = await invoke("get_settings");
  render();
  refreshStatus();
  setInterval(refreshStatus, 5000);

  $("enabled").addEventListener("change", (e) => { settings.enabled = e.target.checked; save(); });
  for (const f of FIELDS) {
    $(f).addEventListener("input", (e) => {
      settings[f] = Number(e.target.value);
      settings.profile = "personalizado";
      render();
      saveSoon();
    });
  }
  document.querySelectorAll("#presets button").forEach((b) =>
    b.addEventListener("click", async () => {
      const p = await invoke("get_preset", { name: b.dataset.p });
      settings = { ...p, enabled: settings.enabled, volume_db: settings.volume_db };
      save();
    }));
  listen("settings-changed", (e) => { settings = e.payload; render(); });
}

init();
