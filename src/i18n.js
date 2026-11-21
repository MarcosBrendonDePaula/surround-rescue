// Traduções da interface.
//
// O português é o texto de origem (está no HTML e no JavaScript). Este arquivo troca, na tela, cada
// texto em português pelo do idioma escolhido, inclusive o que aparece depois (painéis, listas,
// mensagens), observando as mudanças na página. Textos com valores no meio usam os PADRÕES.
//
// Para adicionar um idioma: crie um dicionário como o EN abaixo e registre em LANGS.
(() => {
  const EN = {
    // menu, barra de título, janela
    "Som": "Sound", "Equalizador": "Equalizer", "Microfone": "Microphone", "Fluxo": "Flow", "Perfis": "Profiles",
    "Configurações": "Settings", "Plugins": "Plugins", "Perfil": "Profile", "Guia de início": "Getting started",
    "Minimizar": "Minimize", "Maximizar": "Maximize", "Fechar (continua perto do relógio)": "Close (keeps running in the tray)",
    "Novo perfil (cópia do atual)": "New profile (copy of the current one)", "Renomear perfil": "Rename profile", "Excluir perfil": "Delete profile",
    "Liga ou desliga todo o processamento do fone": "Turns all headphone processing on or off",
    "Surround ligado": "Surround on", "Surround desligado": "Surround off", "Atualizar": "Update",
    // aba Som
    "Campo de som": "Sound field", "Tom": "Tone", "Caixas da frente": "Front speakers", "Caixas laterais": "Side speakers",
    "Caixas de trás": "Rear speakers", "Intensidade do surround": "Surround intensity", "Sala virtual": "Virtual room",
    "Um eco curto, como o de um quarto, que tira o som de dentro da cabeça. Desligue para um som mais seco.":
      "A short echo, like a small room's, that moves the sound out of your head. Turn it off for a drier sound.",
    "Dar a volta": "Go around", "Parar o teste": "Stop test", "Parar": "Stop",
    "Clique numa caixa para ouvir o teste. Arraste até o som parecer vir de lá.": "Click a speaker to hear the test. Drag it until the sound seems to come from there.",
    "Estéreo melhorado": "Stereo widening", "Abre o campo estéreo para o som parecer maior.": "Widens the stereo field so the sound feels bigger.",
    "Reforço de graves": "Bass boost", "Mais peso em explosões, música e motores.": "More weight in explosions, music and engines.",
    "Quantidade": "Amount", "Normalização de som": "Loudness leveling",
    "Aproxima sons baixos e altos: diálogo baixo sobe, explosão desce.": "Brings quiet and loud sounds closer: quiet dialogue goes up, explosions go down.",
    "Clareza de voz": "Voice clarity", "Destaca as vozes de quem fala com você.": "Brings out the voices of people talking to you.",
    "Presença": "Presence", "Volume das vozes": "Voice volume", "Volume geral": "Master volume",
    "Nunca estoura: o app desconta sozinho o reforço de graves e os efeitos.": "Never clips: the app compensates for the bass boost and effects on its own.",
    "Centro": "Center", "Frente esq.": "Front left", "Frente dir.": "Front right", "Lateral esq.": "Side left",
    "Lateral dir.": "Side right", "Trás esq.": "Rear left", "Trás dir.": "Rear right",
    // equalizador
    "Ativo": "On", "Predefinição do equalizador": "Equalizer preset", "Padrão": "Default", "Jogo (FPS)": "Game (FPS)",
    "Filme": "Movie", "Música": "Music", "Voz": "Voice", "Reforço de agudos": "Treble boost", "Personalizado": "Custom",
    // microfone
    "Vale para o microfone escolhido em Configurações, em todos os programas: Discord, jogos e gravações.":
      "Applies to the microphone chosen in Settings, in every program: Discord, games and recordings.",
    "Melhorias do microfone": "Microphone enhancements", "Reforço (+10 dB)": "Boost (+10 dB)", "Filtro de ruídos graves": "Rumble filter",
    "Corta ventilador, batidas na mesa e zumbido abaixo de 90 Hz.": "Cuts fans, desk bumps and hum below 90 Hz.",
    "Ganho": "Gain", "Nitidez da voz": "Voice sharpness", "Redução de ruído de fundo": "Background noise reduction",
    "Aprende o ruído constante (ventilador, ar-condicionado, chiado) e tira enquanto você fala. Soma 11 ms de atraso.":
      "Learns steady noise (fans, air conditioning, hiss) and removes it while you talk. Adds 11 ms of delay.",
    "Quanto tirar": "How much to remove", "Porta de ruído": "Noise gate",
    "Fecha o microfone quando você não está falando (teclado, respiração, barulho da sala).": "Closes the microphone when you are not talking (keyboard, breathing, room noise).",
    "Abre acima de": "Opens above",
    // fluxo
    "Dispositivos": "Devices", "O som que passa por eles. Efeito sem atraso.": "The sound that goes through them. No added delay.",
    "Meus fluxos": "My flows", "Começam em triggers (programas, microfones) e enviam para onde quiser.": "Start from triggers (programs, microphones) and send anywhere you want.",
    "+ Novo fluxo": "+ New flow", "Novo fluxo": "New flow", "Fluxo de áudio": "Audio flow", "Nome do fluxo": "Flow name", "Ligado": "On",
    "Desfaz o desenho e volta a uma fila simples": "Undoes the drawing and goes back to a simple chain", "Organizar em fila": "Arrange as a chain",
    "Excluir fluxo": "Delete flow", "+ Adicionar": "+ Add", "Rodar": "Run", "sempre que estiver ligado": "whenever it is on",
    "só quando este programa estiver aberto:": "only while this program is open:", "Ajustar à tela": "Fit to screen", "Aproximar": "Zoom in", "Afastar": "Zoom out",
    "Arraste o fundo para mover · roda do mouse para zoom · clique no": "Drag the background to move · mouse wheel to zoom · click the",
    "de uma caixa para ligar a próxima": "on a box to connect the next one", "Adicionar caixa": "Add box", "Fechar": "Close",
    "Buscar": "Search", "Buscar (ex.: reverb, ganho, spotify)": "Search (e.g. reverb, gain, spotify)",
    "Entradas": "Inputs", "Efeitos nativos": "Built-in effects", "Saídas": "Outputs", "Utilitários": "Utilities",
    "Mandar para o microfone": "Send to microphone", "Misturar na voz": "Mix into voice", "Mixer": "Mixer", "soma o que chega": "sums what comes in",
    "Mesa de som": "Mixing desk", "Master": "Master", "Mudo": "Mute", "Solo (só as entradas em solo tocam)": "Solo (only soloed inputs play)",
    "Duplo clique no fader volta a 0 dB; no pan, volta ao centro. S = solo: só as entradas em solo tocam.":
      "Double-click the fader to reset to 0 dB; on pan, back to center. S = solo: only soloed inputs play.",
    "Esquerdo e direito · claro: o que entra · cor: depois do fader e do pan": "Left and right · light: what comes in · color: after fader and pan",
    "Windows": "Windows", "som de todos os programas": "sound from every program", "Saída": "Output", "Saída do microfone": "Microphone output",
    "o que o Discord e os outros programas recebem": "what Discord and other programs receive", "o que o microfone capta": "what the microphone picks up",
    "Surround Rescue + anti-estouro": "Surround Rescue + anti-clipping", "Programa": "Program", "som só deste programa": "sound from this program only",
    "Ouvir saída": "Listen to output", "Saída a ouvir": "Output to listen to", "escolha a saída": "choose the output", "Entrada": "Input",
    "escolha a entrada": "choose the input", "escolha o microfone": "choose the microphone", "escolha o destino": "choose the destination",
    "Enviar": "Send", "Dispositivo de destino": "Destination device", "Microfone de destino": "Destination microphone",
    "Aumenta ou diminui o volume": "Raises or lowers the volume", "Inverter": "Invert", "Inverte a fase. Somado com o original, subtrai": "Inverts the phase. Added to the original, it cancels",
    "somado = subtrai": "added = cancels", "Pan": "Pan", "Joga o som para um lado": "Moves the sound to one side", "Mono": "Mono",
    "Junta esquerda e direita": "Merges left and right", "L e R viram um só": "L and R become one", "Trocar L/R": "Swap L/R", "Inverte os lados": "Swaps the sides",
    "inverte os lados": "swaps the sides", "Atraso": "Delay", "Atrasa o som (eco, alinhar caminhos)": "Delays the sound (echo, aligning paths)",
    "Passa-baixa": "Low-pass", "Corta os agudos": "Cuts the treble", "Passa-alta": "High-pass", "Corta os graves": "Cuts the bass",
    "Volume": "Volume", "Ligação": "Connection", "Remover ligação": "Remove connection", "Remover caixa": "Remove box", "Esta caixa é fixa.": "This box is fixed.",
    "Escolher plugin…": "Choose plugin…", "Escolha um plugin para esta caixa.": "Choose a plugin for this box.", "Adicionar outro plugin…": "Add another plugin…",
    "Escolha o plugin (VST3, VST2, CLAP); dá para pôr vários em fila": "Choose the plugin (VST3, VST2, CLAP); you can chain several",
    "Abrir janela do plugin": "Open plugin window", "Abrir a janela do plugin": "Open the plugin window", "Tirar do rack": "Remove from rack", "Tirar da caixa": "Remove from box",
    "Subir": "Move up", "Descer": "Move down", "Ligar": "Turn on", "Desligar": "Turn off", "Ligar ou desligar este plugin": "Turn this plugin on or off",
    "Capturar programa…": "Capture a program…", "Programa (.exe)": "Program (.exe)", "Um player, o navegador, um jogo — escolha o .exe depois": "A player, the browser, a game — pick the .exe later",
    "Adicionar e ligar": "Add and connect", "Soma vários caminhos": "Sums several paths", "bloco nativo": "built-in block", "Sem nome": "Untitled",
    "Nenhum fluxo ainda.": "No flows yet.", "Não salvou": "Not saved", "VST · desligado": "VST · off", "nenhum plugin": "no plugins",
    "Essa ligação faria o som andar em círculo.": "That connection would make the sound go in circles.",
    "Essa ligação faria o som andar em círculo. Ligue para uma caixa mais à frente.": "That connection would make the sound go in circles. Connect to a box further ahead.",
    "Parado (sem som passando ou condição não atendida)": "Stopped (no sound flowing or condition not met)", "Relógio próprio de 10 ms": "Own 10 ms clock",
    "Roda dentro do bloco do Windows: não soma atraso": "Runs inside the Windows block: adds no delay", "CPU": "CPU", "blocos atrasados": "late blocks",
    "Parte de cada bloco de 10 ms gasta processando este fluxo": "Share of each 10 ms block spent processing this flow",
    "Tempo médio por bloco; entre parênteses, o pior do último segundo": "Average time per block; in parentheses, the worst in the last second",
    "Blocos que passaram sem efeito por atraso": "Blocks that went through unprocessed because they were late",
    "sem motor": "no engine", "motor ativo": "engine active", "no caminho principal": "on the main path",
    "surround, equalizador e anti-estouro (sempre por último)": "surround, equalizer and anti-clipping (always last)",
    "O som que chega aqui também toca nesse dispositivo, convertido para o formato dele. Fica uns 30 ms atrás do principal.":
      "The sound arriving here also plays on that device, converted to its format. It runs about 30 ms behind the main one.",
    "O som que chega nesta caixa é somado à voz desse microfone: qualquer programa que grava dele (Discord, OBS, jogo) recebe os dois juntos. O microfone precisa estar com o motor instalado (Configurações). Para o volume, clique na seta que entra aqui.":
      "The sound arriving at this box is added to that microphone's voice: any program recording it (Discord, OBS, a game) gets both together. The microphone needs the engine installed (Settings). For the volume, click the arrow coming in here.",
    "A captura é uma cópia: o programa continua tocando onde estava. Para ouvir só a versão processada, aponte o programa para outra saída nas configurações de som do Windows.":
      "The capture is a copy: the program keeps playing where it was. To hear only the processed version, point the program to another output in the Windows sound settings.",
    "Pega tudo o que está tocando nessa saída (depois da mistura do Windows). Não envie de volta para a mesma saída: o executor recusa, para não virar microfonia.":
      "Takes everything playing on that output (after the Windows mix). Don't send it back to the same output: the host refuses, to avoid feedback.",
    "A janela do próprio plugin abre separada (é dela que vêm os ajustes). Os ajustes ficam salvos com o fluxo.":
      "The plugin's own window opens separately (that's where its settings are). Settings are saved with the flow.",
    "O som passa pelos plugins de cima para baixo.": "The sound goes through the plugins from top to bottom.",
    "Fluxo com caminhos paralelos: a ordem e as ligações são editadas na aba Fluxo.": "Flow with parallel paths: order and connections are edited in the Flow tab.",
    "Nenhum plugin ainda. O som passa direto.": "No plugins yet. The sound goes straight through.", "Nenhum plugin ainda. Adicione um abaixo.": "No plugins yet. Add one below.",
    "Nenhum programa ainda.": "No programs yet.", "Arraste para mudar a ordem": "Drag to reorder",
    // navegador de plugins
    "Adicionar plugin": "Add plugin", "Adicionar plugin…": "Add plugin…", "Filtrar (nome, fabricante, tipo)": "Filter (name, vendor, type)",
    "Filtrar plugins": "Filter plugins", "Fechar (Esc)": "Close (Esc)", "Filtros": "Filters", "Nome": "Name", "Formato": "Format", "Fabricante": "Vendor", "Tipo": "Type",
    "Adicionar pasta…": "Add folder…", "Procurar de novo": "Scan again", "Adicionar": "Add", "Procurar nesta pasta": "Scan this folder", "Pasta de plugins": "Plugin folder",
    "Todos os plugins": "All plugins", "★ Favoritos": "★ Favorites", "Recentes": "Recent", "Favorito": "Favorite", "Procurando plugins…": "Scanning plugins…", "Procurando…": "Scanning…",
    "Nenhum plugin aqui. Use \"Adicionar pasta…\" para incluir outra pasta.": "No plugins here. Use \"Add folder…\" to include another folder.",
    "Nenhum plugin VST2 ou VST3 nessa pasta.": "No VST2 or VST3 plugins in that folder.", "32 bits": "32-bit", "32 bits (ainda não suportado)": "32-bit (not supported yet)",
    "· ainda não suportado": "· not supported yet",
    // perfis
    "Perfis por programa": "Profiles per program", "Trocar o perfil sozinho conforme o programa em uso": "Switch the profile automatically based on the program in use",
    "Programas fora da lista usam o perfil escolhido no topo. Em uso agora:": "Programs not on the list use the profile chosen at the top. In use now:",
    "Perfil do programa": "Program profile", "Adicionar programa": "Add program", "Remover": "Remove",
    "Deixe o programa aberto e volte aqui: ele aparece nas sugestões.": "Leave the program open and come back here: it shows up in the suggestions.",
    // configurações
    "Fone / saída de som": "Headphones / sound output", "Dispositivo de saída": "Output device", "Estado do sistema": "System status",
    "Motor e inicialização": "Engine and startup", "Usar o motor próprio (em teste)": "Use the built-in engine (beta)",
    "Troca o Equalizer APO pelo motor do Surround Rescue, que também roda os plugins VST. Precisa do motor instalado no dispositivo.":
      "Replaces Equalizer APO with the Surround Rescue engine, which also runs VST plugins. Needs the engine installed on the device.",
    "Iniciar com o Windows": "Start with Windows", "Abre escondido, perto do relógio.": "Starts hidden, in the tray.", "Ver configuração gerada": "Show generated configuration",
    "Atualização": "Updates", "Procurar atualização": "Check for updates", "Atualizar agora": "Update now", "Motor nos dispositivos": "Engine on devices",
    "O Windows só deixa um efeito agir no som de um fone ou microfone se ele estiver instalado nesse dispositivo. Instale onde quiser usar o Fluxo (inclusive para tocar sons no microfone). Cada mudança pede administrador uma vez e o som de tudo para por alguns segundos.":
      "Windows only lets an effect act on a headset's or microphone's sound if it is installed on that device. Install it wherever you want to use Flow (including playing sounds into the microphone). Each change asks for administrator once and all sound stops for a few seconds.",
    "Carregando dispositivos…": "Loading devices…", "Instalar em todos": "Install on all", "Remover de todos": "Remove from all", "Pastas de plugins": "Plugin folders",
    "Onde o app procura VST3, VST2 e CLAP. As pastas padrão do Windows já entram sozinhas; adicione aqui as outras.":
      "Where the app looks for VST3, VST2 and CLAP. The standard Windows folders are included automatically; add any others here.",
    "Adicionar pasta": "Add folder", "Pastas padrão": "Standard folders", "Idioma": "Language", "Automático (idioma do Windows)": "Automatic (Windows language)",
    "Nenhuma pasta sua ainda. Adicione abaixo a pasta onde seus plugins estão instalados.": "None of your folders yet. Add the folder where your plugins are installed below.",
    "Clique em Procurar de novo para contar os plugins.": "Click Scan again to count the plugins.",
    "Executor de 32 bits instalado: plugins VST2 de 32 bits funcionam.": "32-bit host installed: 32-bit VST2 plugins work.",
    "Executor de 32 bits não encontrado: plugins de 32 bits não vão carregar.": "32-bit host not found: 32-bit plugins won't load.",
    "O executor de VST não foi encontrado nesta instalação.": "The VST host was not found in this installation.",
    "Motor de áudio instalado": "Audio engine installed", "Motor de áudio não instalado: reinstale o Surround Rescue": "Audio engine not installed: reinstall Surround Rescue",
    "Nenhuma saída com o efeito ativo: escolha o fone acima e ative": "No output with the effect active: choose your headphones above and turn it on",
    "Nenhum microfone com o motor ativo": "No microphone with the engine active", "Configuração aplicada": "Configuration applied",
    "Configuração ainda não aplicada": "Configuration not applied yet", "Razer Surround antigo desligado": "Old Razer Surround turned off",
    "Razer Surround antigo ainda rodando: pode disputar o fone": "Old Razer Surround still running: it may fight over the headset",
    "Nenhum dispositivo com o motor ativo.": "No device with the engine active.", "● Efeito ativo neste dispositivo": "● Effect active on this device",
    "Desativar aqui": "Turn off here", "Sem efeito neste dispositivo": "No effect on this device", "Ativar neste dispositivo": "Turn on for this device",
    "Este dispositivo usa um modo que só o Configurator do Equalizer APO consegue ativar.": "This device uses a mode that only the Equalizer APO Configurator can enable.",
    "Ativando… confirme a permissão": "Turning on… confirm the permission", "Desativando…": "Turning off…", "Instalar": "Install", "● instalado": "● installed",
    "não instalado": "not installed", "Nenhum dispositivo de áudio encontrado.": "No audio devices found.",
    "Instalando… aceite o pedido de administrador. O som volta em alguns segundos.": "Installing… accept the administrator prompt. Sound comes back in a few seconds.",
    "Removendo… aceite o pedido de administrador.": "Removing… accept the administrator prompt.",
    "Pronto: motor instalado.": "Done: engine installed.", "Pronto: motor removido e o efeito original devolvido.": "Done: engine removed and the original effect restored.",
    "Procurando…": "Checking…",
    // guia de início
    "Escolha o fone e o microfone": "Choose your headphones and microphone", "Instale o motor neles": "Install the engine on them",
    "Calibre o surround ouvindo": "Calibrate the surround by ear", "Pronto": "Done", "Continuar": "Continue", "Voltar": "Back", "Começar a usar": "Start using it",
    "Fechar o guia": "Close the guide", "Lendo os dispositivos…": "Reading devices…",
    "O surround, o equalizador e o resto agem no fone escolhido aqui. As melhorias de voz agem no microfone.":
      "Surround, equalizer and the rest act on the headphones chosen here. Voice enhancements act on the microphone.",
    "O Windows só deixa um efeito agir no som de um dispositivo se ele estiver instalado nele. Vai aparecer um pedido de administrador, e o som de tudo para por alguns segundos.":
      "Windows only lets an effect act on a device's sound if it is installed on it. An administrator prompt will appear, and all sound stops for a few seconds.",
    "Fone": "Headphones", "motor instalado": "engine installed", "sem o motor": "no engine", "Tudo certo: o motor já está nos dois.": "All set: the engine is on both.",
    "Instalar no fone e no microfone": "Install on headphones and microphone", "Instalando… aceite o pedido de administrador.": "Installing… accept the administrator prompt.",
    "Cada cabeça é diferente. Toque o teste e veja se o som parece vir de cada lugar em volta de você.": "Every head is different. Play the test and check whether the sound seems to come from each place around you.",
    "Se alguma caixa parecer fora do lugar, depois ajuste na aba": "If a speaker seems out of place, adjust it later in the", ": clique numa caixa para ouvir e arraste até o som vir de lá.": " tab: click a speaker to hear it and drag until the sound comes from there.",
    "surround, sala virtual, graves, normalização e clareza de voz": "surround, virtual room, bass, leveling and voice clarity",
    "10 faixas e predefinições (jogo, filme, música)": "10 bands and presets (game, movie, music)", "redução de ruído, porta de ruído e ganho": "noise reduction, noise gate and gain",
    "plugins VST, mesa de som, mandar som de um programa para o microfone": "VST plugins, mixing desk, sending a program's sound into the microphone",
    "um ajuste por jogo ou programa, trocado sozinho": "one setup per game or program, switched automatically",
    "O app continua rodando perto do relógio quando você fecha a janela. Este guia fica no menu, em \"Guia de início\".":
      "The app keeps running in the tray when you close the window. This guide stays in the menu, under \"Getting started\".",
  };

  // textos com valores no meio
  const EN_PATTERNS = [
    [/^Versão (\S+) disponível \(instalada: (.+)\)\.$/, "Version $1 available (installed: $2)."],
    [/^Versão instalada: (.+?)\. Está atualizado\.$/, "Installed version: $1. You're up to date."],
    [/^Versão instalada: (.+?)\. Não deu para verificar agora \((.*)\)\.$/, "Installed version: $1. Couldn't check right now ($2)."],
    [/^Versão instalada: (.+)$/, "Installed version: $1"],
    [/^Baixando a versão (.+?)… o app fecha, instala \(aceite o pedido de administrador\) e abre de novo\.$/, "Downloading version $1… the app closes, installs (accept the administrator prompt) and opens again."],
    [/^Atualizar para (.+)$/, "Update to $1"],
    [/^Versão (.+)$/, "Version $1"],
    [/^Não deu certo: (.+)$/s, "It didn't work: $1"],
    [/^Não deu para atualizar: (.+)$/s, "Couldn't update: $1"],
    [/^(\d+)° de abertura$/, "$1° spread"],
    [/^(\d+) entradas?$/, (m, n) => `${n} input${n === "1" ? "" : "s"}`],
    [/^Salvo ✓ (.+)$/, "Saved ✓ $1"],
    [/^Efeito ativo em: (.+)$/, "Effect active on: $1"],
    [/^Microfone com melhorias: (.+)$/, "Microphone with enhancements: $1"],
    [/^Automático( — .+)?$/, "Automatic$1"],
    [/^(.+) \((\d+)°\) — clique para ouvir(, arraste para ajustar)?$/, (m, n, d, drag) => `${tr(n)} (${d}°) — click to listen${drag ? ", drag to adjust" : ""}`],
    [/^(\d+) de (\d+) plugins$/, "$1 of $2 plugins"],
  ];

  const LANGS = { en: { dict: EN, patterns: EN_PATTERNS, html: "en" } };
  const KEY = "sr-lang";
  let pref = "auto";
  try { pref = localStorage.getItem(KEY) || "auto"; } catch (e) { /* sem armazenamento: automático */ }
  const sys = (navigator.language || "pt").toLowerCase().startsWith("pt") ? "pt" : "en";
  const lang = pref === "auto" ? sys : pref;
  const L = LANGS[lang];

  function tr(t) {
    if (!L) return t;
    const k = t.trim();
    if (!k) return t;
    let v = L.dict[k];
    if (v === undefined) {
      for (const [re, rep] of L.patterns) {
        if (re.test(k)) { v = k.replace(re, rep); break; }
      }
    }
    if (v === undefined) return t;
    // mantém os espaços em volta (o HTML depende deles entre palavras e tags)
    return t.replace(k, v);
  }
  window.srT = tr;

  const ATTRS = ["title", "placeholder", "aria-label"];
  function translate(root) {
    if (!L || !root) return;
    if (root.nodeType === 3) {
      if (root.parentNode?.closest?.("[data-no-i18n]")) return;
      const v = tr(root.nodeValue);
      if (v !== root.nodeValue) root.nodeValue = v;
      return;
    }
    if (root.nodeType !== 1) return;
    if (root.matches?.("script, style, pre, textarea, input[type=text]")) return;
    for (const a of ATTRS) {
      if (root.hasAttribute?.(a)) {
        const v = tr(root.getAttribute(a));
        if (v !== root.getAttribute(a)) root.setAttribute(a, v);
      }
    }
    const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
    let n;
    while ((n = w.nextNode())) {
      if (n.nodeType === 3) {
        const p = n.parentNode;
        if (p && (p.nodeName === "SCRIPT" || p.nodeName === "STYLE" || p.nodeName === "PRE")) continue;
        // nomes que a pessoa deu (perfis, fluxos) ficam como estão
        if (p && p.closest && p.closest("[data-no-i18n]")) continue;
        const v = tr(n.nodeValue);
        if (v !== n.nodeValue) n.nodeValue = v;
      } else {
        for (const a of ATTRS) {
          if (n.hasAttribute(a)) {
            const v = tr(n.getAttribute(a));
            if (v !== n.getAttribute(a)) n.setAttribute(a, v);
          }
        }
      }
    }
  }

  function start() {
    // seletor de idioma em Configurações
    const host = document.getElementById("lang-pick");
    if (host) {
      host.value = pref;
      host.addEventListener("change", () => {
        try { localStorage.setItem(KEY, host.value); } catch (e) { /* ignora */ }
        location.reload();
      });
    }
    if (!L) return;
    document.documentElement.lang = L.html;
    translate(document.body);
    new MutationObserver((muts) => {
      for (const m of muts) {
        if (m.type === "characterData") translate(m.target);
        else if (m.type === "attributes") translate(m.target);
        else m.addedNodes.forEach(translate);
      }
    }).observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ATTRS });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
