// Navegador de plugins no estilo FX Browser: árvore de filtros à esquerda, tabela com busca à direita.
// Uso: openPluginBrowser((plugin) => { ... }) — plugin = { path, name, format, vendor, category, bits }.
// Usa PL, invoke, esc, baseName, $, $$ e plDirs/plAddDir de main.js.

const PB = { onPick: null, filter: "all", sort: "name", asc: true, sel: null, rows: [], scanning: false };

const pbGet = (k, d) => { try { return JSON.parse(localStorage.getItem(k) || d); } catch (e) { return JSON.parse(d); } };
const pbSet = (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch (e) {} };
const pbFavs = () => pbGet("sr-fav-plugins", "[]");
const pbRecent = () => pbGet("sr-recent-plugins", "[]");
const pbKey = (p) => String(p.path).toLowerCase();

async function pbScan(force) {
  if (PL.found.length && !force) return;
  PB.scanning = true;
  pbRender();
  try { PL.found = await invoke("scan_plugins", { extra: plDirs() }); } catch (e) { PL.found = []; }
  PB.scanning = false;
  pbRender();
}

/// Abre o navegador; `onPick` recebe o plugin escolhido (duplo clique, Enter ou "Adicionar").
function openPluginBrowser(onPick) {
  PB.onPick = onPick;
  PB.sel = null;
  $("#pb").hidden = false;
  $("#pb-search").value = "";
  $("#pb-dirform").hidden = true;
  pbRender();
  $("#pb-search").focus();
  pbScan(false);
}

function pbClose() {
  $("#pb").hidden = true;
  PB.onPick = null;
}

function pbPick(p) {
  if (!p || p.supported === false) return;
  const rec = pbRecent().filter((k) => k !== pbKey(p));
  rec.unshift(pbKey(p));
  pbSet("sr-recent-plugins", rec.slice(0, 20));
  const cb = PB.onPick;
  pbClose();
  cb?.(p);
}

/// Filtros da árvore: [id, rótulo, função de teste]
function pbFilters() {
  const all = PL.found;
  const favs = new Set(pbFavs()), rec = new Set(pbRecent());
  const by = (field) => [...new Set(all.map((p) => p[field] || "").filter(Boolean))].sort((a, b) => a.localeCompare(b, "pt-BR"));
  return [
    ["", [
      ["all", "Todos os plugins", () => true],
      ["fav", "★ Favoritos", (p) => favs.has(pbKey(p))],
      ["recent", "Recentes", (p) => rec.has(pbKey(p))],
    ]],
    ["Formato", [
      ["fmt:VST3", "VST3", (p) => p.format === "VST3"],
      ["fmt:VST2", "VST2 (64 bits)", (p) => p.format === "VST2" && p.bits !== 32],
      ["fmt:CLAP", "CLAP", (p) => p.format === "CLAP"],
      ["bits:32", "32 bits", (p) => p.bits === 32],
    ]],
    ["Tipo", by("category").map((c) => [`cat:${c}`, c, (p) => p.category === c])],
    ["Fabricante", by("vendor").map((v) => [`ven:${v}`, v, (p) => p.vendor === v])],
  ];
}

function pbRender() {
  // árvore
  const groups = pbFilters();
  let test = () => true;
  let tree = "";
  for (const [title, items] of groups) {
    if (!items.length) continue;
    if (title) tree += `<h4>${esc(title)}</h4>`;
    for (const [id, label, fn] of items) {
      const n = PL.found.filter(fn).length;
      if (title && n === 0 && !id.startsWith("fmt:")) continue;
      if (PB.filter === id) test = fn;
      tree += `<button data-f="${esc(id)}" class="${PB.filter === id ? "on" : ""}"><span>${esc(label)}</span><small>${n}</small></button>`;
    }
  }
  $("#pb-tree").innerHTML = tree;

  // tabela
  const q = $("#pb-search").value.trim().toLowerCase();
  const favs = new Set(pbFavs());
  PB.rows = PL.found
    .filter(test)
    .filter((p) => !q || `${p.name} ${p.vendor} ${p.category} ${p.format}`.toLowerCase().includes(q))
    .sort((a, b) => {
      const x = String(a[PB.sort] || ""), y = String(b[PB.sort] || "");
      return (PB.asc ? 1 : -1) * x.localeCompare(y, "pt-BR", { sensitivity: "base" });
    });
  $$(".pb-table th[data-sort]").forEach((th) => {
    th.classList.toggle("sorted", th.dataset.sort === PB.sort);
    th.classList.toggle("asc", th.dataset.sort === PB.sort && PB.asc);
  });
  $("#pb-rows").innerHTML = PB.scanning && !PL.found.length
    ? `<tr><td colspan="5" class="pb-empty">Procurando plugins…</td></tr>`
    : PB.rows.length
      ? PB.rows.map((p, i) => `<tr data-i="${i}" class="${PB.sel === pbKey(p) ? "sel" : ""}${p.supported === false ? " off" : ""}" title="${esc(p.path)}">
          <td><button class="star${favs.has(pbKey(p)) ? " on" : ""}" data-star="${i}" title="Favorito">★</button></td>
          <td>${esc(p.name || baseName(p.path))}</td>
          <td><span class="pb-fmt">${esc(p.format || "VST3")}${p.bits === 32 ? " 32" : ""}</span></td>
          <td>${esc(p.vendor || "—")}</td>
          <td>${esc(p.category || "")}${p.supported === false ? " · ainda não suportado" : ""}</td></tr>`).join("")
      : `<tr><td colspan="5" class="pb-empty">Nenhum plugin aqui. Use "Adicionar pasta…" para incluir outra pasta.</td></tr>`;
  const sel = PB.rows.find((p) => pbKey(p) === PB.sel);
  $("#pb-path").textContent = sel ? sel.path : `${PB.rows.length} de ${PL.found.length} plugins`;
  $("#pb-add").disabled = !sel || sel.supported === false;
}

function pbBind() {
  $("#pb-close").addEventListener("click", pbClose);
  $("#pb").addEventListener("click", (e) => { if (e.target.id === "pb") pbClose(); });
  $("#pb-search").addEventListener("input", pbRender);
  $("#pb-tree").addEventListener("click", (e) => {
    const b = e.target.closest("[data-f]");
    if (b) { PB.filter = b.dataset.f; pbRender(); }
  });
  $$(".pb-table th[data-sort]").forEach((th) => th.addEventListener("click", () => {
    PB.asc = PB.sort === th.dataset.sort ? !PB.asc : true;
    PB.sort = th.dataset.sort;
    pbRender();
  }));
  $("#pb-rows").addEventListener("click", (e) => {
    const star = e.target.closest("[data-star]");
    if (star) {
      const k = pbKey(PB.rows[Number(star.dataset.star)]);
      const f = pbFavs();
      pbSet("sr-fav-plugins", f.includes(k) ? f.filter((x) => x !== k) : [...f, k]);
      return pbRender();
    }
    const tr = e.target.closest("tr[data-i]");
    if (tr) { PB.sel = pbKey(PB.rows[Number(tr.dataset.i)]); pbRender(); }
  });
  $("#pb-rows").addEventListener("dblclick", (e) => {
    const tr = e.target.closest("tr[data-i]");
    if (tr) pbPick(PB.rows[Number(tr.dataset.i)]);
  });
  $("#pb-add").addEventListener("click", () => pbPick(PB.rows.find((p) => pbKey(p) === PB.sel)));
  $("#pb-rescan").addEventListener("click", () => pbScan(true));
  $("#pb-dir").addEventListener("click", () => { $("#pb-dirform").hidden = false; $("#pb-dirpath").focus(); });
  $("#pb-dirok").addEventListener("click", async () => {
    const d = $("#pb-dirpath").value.trim().replace(/^"|"$/g, "");
    if (!d) return;
    plAddDir(d);
    $("#pb-dirform").hidden = true;
    await pbScan(true);
    PB.filter = "all";
    $("#pb-search").value = "";
    pbRender();
  });
  document.addEventListener("keydown", (e) => {
    if ($("#pb").hidden) return;
    if (e.key === "Escape") pbClose();
    if (e.key === "Enter" && document.activeElement?.id !== "pb-dirpath") {
      pbPick(PB.rows.find((p) => pbKey(p) === PB.sel) || (PB.rows.length === 1 ? PB.rows[0] : null));
    }
    // setas mudam a seleção
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const i = PB.rows.findIndex((p) => pbKey(p) === PB.sel);
      const n = Math.max(0, Math.min(PB.rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (PB.rows[n]) { PB.sel = pbKey(PB.rows[n]); pbRender(); }
    }
  });
}
pbBind();
