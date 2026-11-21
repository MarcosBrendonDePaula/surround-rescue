"""Roda a interface de verdade (src/) num navegador sem o Tauri, com respostas falsas no lugar do app.

Serve para enxergar a tela sem mexer no app aberto: gera um HTML com um `window.__TAURI__` de mentira
(dados reais de config, fluxos e níveis do ProgramData) e um roteiro de cliques; o Edge sem janela
tira a foto ou devolve o DOM.

Uso: python tools/ui-harness.py <saida-dir> [roteiro.js]
"""

import json
import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
PD = os.path.join(os.environ.get("ProgramData", r"C:\ProgramData"), "SurroundRescue")


def read(p, default=None):
    try:
        with open(p, encoding="utf-8-sig") as f:
            return json.load(f)
    except Exception:
        return default


def main():
    out = sys.argv[1]
    script = open(sys.argv[2], encoding="utf-8").read() if len(sys.argv) > 2 else ""
    os.makedirs(out, exist_ok=True)
    cfg = read(os.path.join(os.environ["APPDATA"], "surround-rescue", "config.json"), {})
    chains = {}
    cdir = os.path.join(PD, "chains")
    for f in os.listdir(cdir) if os.path.isdir(cdir) else []:
        if f.endswith(".json"):
            chains[f[:-5]] = read(os.path.join(cdir, f), {})
    levels = read(os.path.join(PD, "levels.json"), {})
    dev = lambda i, n, d: {"id": i, "flow": "", "name": n, "device": d, "apo": True, "needs_configurator": False, "connected": True}
    outs = [dev("{e59c3ff3-fcd2-454f-9738-b402bc3a0801}", "Fone de ouvido do headset", "Razer Kraken 7.1 Chroma"),
            dev("{15a6a387-82c2-419b-b0b0-b472feaaa4e6}", "Alto-falantes", "Realtek(R) Audio")]
    ins = [dev("{b773f106-c861-46c0-85ec-b5d91581a674}", "Microfone do Headset", "Razer Kraken 7.1 Chroma"),
           dev("{41bd579e-21ab-4c7a-a0c6-67df31653825}", "Microfone", "REDRAGON Live Camera Audio")]
    data = {
        "get_snapshot": {"config": cfg, "applied": "", "eq_presets": []},
        "get_status": {"outputs": outs, "inputs": ins, "output": outs[0], "input": ins[0], "apo_installed": True,
                       "config_hooked": True, "razer_service_running": False, "autostart": False},
        "chains": chains,
        "levels": levels,
    }
    stub = """<script>
const __D = %s;
const __log = [];
window.__TAURI__ = {
  core: { invoke: async (cmd, a) => {
    __log.push(cmd);
    if (cmd === "get_chain") return __D.chains[a.endpoint] || __D.chains[String(a.endpoint).toLowerCase()] || { plugins: [] };
    if (cmd === "get_levels") {
      // níveis reais com variação, para os medidores mexerem
      const o = {}; for (const [k, v] of Object.entries(__D.levels)) { o[k] = {}; for (const [n, x] of Object.entries(v)) o[k][n] = Array.isArray(x) ? x.map((y) => Math.min(1, y * (8 + 4 * Math.random()))) : Math.min(1, x * (8 + 4 * Math.random())); }
      return o;
    }
    if (cmd in __D) return JSON.parse(JSON.stringify(__D[cmd]));
    if (["list_flows", "scan_plugins", "audio_apps", "engine_devices", "recent_apps"].includes(cmd)) return [];
    if (cmd === "get_stats") return { flows: [] };
    if (cmd === "plugin_env") return { bridge32: true };
    if (cmd === "host_status") return { found: true, running: true };
    if (cmd === "check_update") return { current: "0.5.0", latest: "0.5.0", available: false, notes: "" };
    return null;
  } },
  event: { listen: async () => () => {} },
};
window.addEventListener("error", (e) => __log.push("ERRO: " + e.message + " @ " + (e.error && e.error.stack || "").split("\\n").slice(1,4).join(" | ")));
window.addEventListener("unhandledrejection", (e) => __log.push("ERRO: " + (e.reason && e.reason.message || e.reason)));
</script>""" % json.dumps(data)
    html = open(os.path.join(ROOT, "src", "index.html"), encoding="utf-8").read()
    base = "file:///" + os.path.join(ROOT, "src").replace("\\", "/") + "/"
    html = html.replace("<head>", f'<head><base href="{base}">' + stub, 1)
    html = html.replace("</body>", f"<script>{script}</script></body>", 1)
    with open(os.path.join(out, "ui.html"), "w", encoding="utf-8") as f:
        f.write(html)
    print(os.path.join(out, "ui.html"))


if __name__ == "__main__":
    main()
