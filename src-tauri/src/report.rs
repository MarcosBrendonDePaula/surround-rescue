//! "Relatar um problema": abre uma issue nova no GitHub já preenchida com dados de EXECUÇÃO.
//!
//! Vai: versão do app e do Windows, estado do motor em cada tipo de dispositivo, se o executor do
//! Fluxo está rodando e as últimas linhas dos logs do motor e do executor.
//! Não vai: configurações, perfis, nomes de programas, nome do usuário ou do computador, caminhos
//! pessoais (trocados por <usuário>), nomes de dispositivos.

const MAX_URL: usize = 7800; // o GitHub recusa links muito longos

fn data() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join("SurroundRescue")
}

/// Tira qualquer coisa que identifique a pessoa ou o PC.
pub fn redact(t: &str) -> String {
    let mut s = t.to_string();
    for var in ["USERPROFILE", "LOCALAPPDATA", "APPDATA"] {
        if let Ok(v) = std::env::var(var) {
            if v.len() > 3 {
                s = s.replace(&v, &format!("%{var}%"));
            }
        }
    }
    for var in ["USERNAME", "COMPUTERNAME", "USERDOMAIN"] {
        if let Ok(v) = std::env::var(var) {
            if v.len() > 2 {
                s = s.replace(&v, &format!("<{}>", var.to_lowercase()));
            }
        }
    }
    // qualquer C:\Users\<nome>\ que tenha sobrado
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find(r"\Users\") {
        out.push_str(&rest[..i + 7]);
        rest = &rest[i + 7..];
        let end = rest.find(['\\', '/', '"', ' ']).unwrap_or(rest.len());
        out.push_str("<usuário>");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Últimas `n` linhas úteis de um log (sem as linhas repetitivas de diagnóstico).
fn tail(path: &std::path::Path, n: usize) -> String {
    let Ok(t) = std::fs::read(path) else { return "(sem log)".into() };
    let t = String::from_utf8_lossy(&t[t.len().saturating_sub(200_000)..]).to_string();
    let lines: Vec<&str> = t.lines().filter(|l| !l.contains("QueryInterface") && !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn windows_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let k = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    match k {
        Ok(k) => {
            let name: String = k.get_value("ProductName").unwrap_or_default();
            let disp: String = k.get_value("DisplayVersion").unwrap_or_default();
            let build: String = k.get_value("CurrentBuild").unwrap_or_default();
            format!("{name} {disp} (build {build})")
        }
        Err(_) => "?".into(),
    }
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

/// Nomes que a pessoa deu aos fluxos aparecem no log como `fluxo "nome"`: troca por `<nome>`.
fn hide_flow_names(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(i) = rest.find("fluxo \"") {
        out.push_str(&rest[..i + 7]);
        rest = &rest[i + 7..];
        match rest.find('"') {
            Some(j) => {
                out.push_str("<nome>");
                rest = &rest[j..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Textos da issue em português ou inglês (segue o idioma escolhido no app).
fn tr(en: bool, pt: &'static str, en_text: &'static str) -> &'static str {
    if en { en_text } else { pt }
}

/// Texto da issue (só execução). `lang`: "en" ou "pt".
pub fn body(app_version: &str, own_engine: bool, host_running: bool, lines: usize, lang: &str) -> String {
    let en = lang == "en";
    let devs = crate::engine::list();
    let count = |flow: &str, f: &dyn Fn(&crate::engine::DevEngine) -> bool| devs.iter().filter(|d| d.flow == flow && f(d)).count();
    let dll = std::path::Path::new(r"C:\Program Files\SurroundRescue\apo\sr_apo.dll");
    let dll_state = match std::fs::metadata(dll) {
        Ok(m) => format!("{} ({} KB)", tr(en, "presente", "present"), m.len() / 1024),
        Err(_) => tr(en, "ausente", "missing").into(),
    };
    let install_err = std::fs::read_to_string(data().join(r"engine\install-error.txt")).unwrap_or_default();
    let stats = std::fs::read_to_string(data().join("stats.json")).unwrap_or_default();
    let mut b = String::new();
    b.push_str(tr(
        en,
        "## O que aconteceu\n\n<!-- Descreva o problema e como chegar nele. Se quiser, arraste prints aqui. -->\n\n## Execução\n\n",
        "## What happened\n\n<!-- Describe the problem and how to reproduce it. Feel free to drag screenshots here. -->\n\n## Runtime\n\n",
    ));
    b.push_str(&format!("- Surround Rescue {app_version}\n- {}\n", windows_version()));
    let engine = if own_engine { tr(en, "sim", "yes") } else { tr(en, "não (Equalizer APO)", "no (Equalizer APO)") };
    b.push_str(&format!("- {}: {engine}\n- {}: {dll_state}\n", tr(en, "Motor próprio em uso", "Built-in engine in use"), tr(en, "DLL do motor", "Engine DLL")));
    b.push_str(&format!(
        "- {}: {} / {} · {}: {} / {}\n",
        tr(en, "Saídas conectadas / com o motor", "Outputs connected / with engine"),
        count("Render", &|d| d.connected),
        count("Render", &|d| d.connected && d.engine),
        tr(en, "Entradas conectadas / com o motor", "Inputs connected / with engine"),
        count("Capture", &|d| d.connected),
        count("Capture", &|d| d.connected && d.engine)
    ));
    let host = if host_running { tr(en, "rodando", "running") } else { tr(en, "parado", "stopped") };
    b.push_str(&format!("- {}: {host}\n", tr(en, "Executor do Fluxo", "Flow host")));
    if !install_err.trim().is_empty() {
        b.push_str(&format!("- {}: `{}`\n", tr(en, "Último erro de instalação", "Last install error"), install_err.trim()));
    }
    if !stats.trim().is_empty() {
        b.push_str(&format!("\n<details><summary>{}</summary>\n\n```json\n{}\n```\n</details>\n", tr(en, "Medições do executor", "Host measurements"), stats.trim()));
    }
    // os logs em si ficam como o programa escreveu (é o que o desenvolvedor procura)
    let host_log = hide_flow_names(&tail(&data().join("host-log.txt"), lines));
    let apo_log = tail(&data().join(r"engine\apo-log.txt"), lines);
    b.push_str(&format!("\n<details><summary>{}</summary>\n\n```\n{host_log}\n```\n</details>\n", tr(en, "Log do executor (fim)", "Host log (tail)")));
    b.push_str(&format!("\n<details><summary>{}</summary>\n\n```\n{apo_log}\n```\n</details>\n", tr(en, "Log do motor (fim)", "Engine log (tail)")));
    redact(&b)
}

/// Abre a página de issue nova já preenchida. Logs longos são encurtados para caber no link.
pub fn open(app_version: &str, own_engine: bool, host_running: bool, lang: &str) -> Result<(), String> {
    let title = format!("[{app_version}] ");
    let base = format!("https://github.com/{}/issues/new?title={}&body=", crate::update::REPO, enc(&title));
    // menos linhas de log até o link caber
    let mut lines = 30;
    let url = loop {
        let u = format!("{base}{}", enc(&body(app_version, own_engine, host_running, lines, lang)));
        if u.len() <= MAX_URL || lines <= 2 {
            break u;
        }
        lines = lines * 2 / 3;
    };
    use std::os::windows::process::CommandExt;
    std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", &url])
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tira_nome_de_usuario_dos_caminhos() {
        let t = redact(r"abriu C:\Users\fulano\AppData\x.dll e C:\Users\Outra Pessoa\y");
        assert!(!t.contains("fulano"), "{t}");
        assert!(t.contains(r"C:\Users\<usuário>\AppData"), "{t}");
    }

    /// Mostra o texto que iria para a issue neste PC (cargo test -- --ignored --nocapture mostra_relatorio)
    #[test]
    #[ignore]
    fn mostra_relatorio() {
        println!("{}", body("teste", true, true, 30, "pt"));
        println!("{}", body("test", true, true, 30, "en"));
    }

    #[test]
    fn esconde_nomes_de_fluxos() {
        let t = hide_flow_names("fluxo \"Spotify do João\": iniciando\nfluxo \"x\": parado");
        assert_eq!(t, "fluxo \"<nome>\": iniciando\nfluxo \"<nome>\": parado");
    }

    #[test]
    fn codifica_para_link() {
        assert_eq!(enc("a b/ç"), "a%20b%2F%C3%A7");
    }
}
