//! Fluxo de áudio (grafo) de um dispositivo.
//!
//! Regras simples:
//! - Cada nó SOMA tudo o que chega nele pelas setas; cada seta tem um ganho (volume).
//! - Nó de plugin processa essa soma; nó "mixer" só soma (serve para organizar o desenho).
//! - "in" é o som que vem do Windows; "out" é o que volta (antes do Surround Rescue + anti-estouro).
//! - Ligação em círculo é recusada: o fluxo inteiro vira "passa direto".

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const INPUT: &str = "in";
pub const OUTPUT: &str = "out";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NodeKind {
    Input,
    Output,
    Mixer,
    /// `plugin` = posição na lista `plugins` do arquivo de cadeia
    Plugin { plugin: usize },
    /// Envia o que chega aqui para outro dispositivo de saída ({guid}); também repassa adiante.
    Send { device: String },
    /// Tudo o que toca numa saída ({guid}), capturado depois da mistura (trigger "Ouvir uma saída")
    Listen { device: String },
    /// Som de um programa (captura por processo), ex.: "spotify.exe"
    App { exe: String },
    /// Som de um dispositivo de entrada ({guid}), ex.: outro microfone
    Receive { device: String },
    /// Bloco nativo (ganho, inverter, pan, mono, trocar L/R, atraso, filtros)
    Fx {
        fx: crate::blocks::Fx,
        #[serde(default)]
        value: f32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(flatten)]
    pub kind: NodeKind,
    /// Posição no editor (só visual)
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default = "one")]
    pub gain: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

impl Graph {
    /// Fluxo em fila a partir da lista de plugins (in → p0 → p1 → … → out).
    pub fn serial(n_plugins: usize) -> Graph {
        let mut g = Graph::default();
        g.nodes.push(Node { id: INPUT.into(), kind: NodeKind::Input, x: 40.0, y: 160.0 });
        let mut prev = INPUT.to_string();
        for i in 0..n_plugins {
            let id = format!("p{i}");
            g.nodes.push(Node { id: id.clone(), kind: NodeKind::Plugin { plugin: i }, x: 220.0 + 200.0 * i as f32, y: 160.0 });
            g.edges.push(Edge { from: prev, to: id.clone(), gain: 1.0 });
            prev = id;
        }
        g.nodes.push(Node { id: OUTPUT.into(), kind: NodeKind::Output, x: 220.0 + 200.0 * n_plugins as f32, y: 160.0 });
        g.edges.push(Edge { from: prev, to: OUTPUT.into(), gain: 1.0 });
        g
    }
}

/// Plano de execução: ordem dos nós e, para cada um, de onde vem o som.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Nós na ordem de cálculo (índices em `Graph::nodes`), sem "in"
    pub order: Vec<usize>,
    /// Para cada nó: lista de (nó de origem, ganho)
    pub inputs: Vec<Vec<(usize, f32)>>,
    pub input: usize,
    pub output: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    MissingInputOrOutput,
    Cycle,
}

/// Valida e ordena o grafo de um dispositivo (precisa das caixas "in" e "out").
pub fn plan(g: &Graph) -> Result<Plan, PlanError> {
    plan_with(g, true)
}

/// Valida e ordena um fluxo independente (triggers → saídas; "in"/"out" não existem).
pub fn plan_free(g: &Graph) -> Result<Plan, PlanError> {
    plan_with(g, false)
}

/// Ordenação topológica de Kahn. Setas para nós inexistentes são ignoradas.
fn plan_with(g: &Graph, require_io: bool) -> Result<Plan, PlanError> {
    let idx: HashMap<&str, usize> = g.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let (input, output) = match (idx.get(INPUT), idx.get(OUTPUT)) {
        (Some(&i), Some(&o)) => (i, o),
        _ if require_io => return Err(PlanError::MissingInputOrOutput),
        // fluxo independente: índices fora do grafo (nenhum nó é "in"/"out")
        _ => (usize::MAX, usize::MAX),
    };
    let n = g.nodes.len();
    let mut inputs: Vec<Vec<(usize, f32)>> = vec![vec![]; n];
    let mut outs: Vec<Vec<usize>> = vec![vec![]; n];
    let mut indeg = vec![0usize; n];
    for e in &g.edges {
        let (Some(&a), Some(&b)) = (idx.get(e.from.as_str()), idx.get(e.to.as_str())) else { continue };
        if b == input || a == output || !e.gain.is_finite() {
            continue; // nada entra no "in" e nada sai do "out"
        }
        inputs[b].push((a, e.gain.clamp(0.0, 4.0)));
        outs[a].push(b);
        indeg[b] += 1;
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut order = vec![];
    while let Some(i) = queue.pop() {
        order.push(i);
        for &j in &outs[i] {
            indeg[j] -= 1;
            if indeg[j] == 0 {
                queue.push(j);
            }
        }
    }
    if order.len() != n {
        return Err(PlanError::Cycle);
    }
    order.retain(|&i| i != input);
    let _ = require_io;
    Ok(Plan { order, inputs, input, output })
}

/// O que cada nó faz com o som (o executor liga isso aos plugins de verdade).
pub trait Processor {
    /// Processa em L/R separados; retorna false se não processou (o som do nó segue como está).
    fn process(&mut self, node: usize, left: &mut [f32], right: &mut [f32]) -> bool;
}

/// Executa o plano. `bufs` tem um par L/R por nó, já com capacidade suficiente (sem alocação).
pub fn run(plan: &Plan, bufs: &mut [[Vec<f32>; 2]], frames: usize, proc_: &mut impl Processor) {
    for &i in &plan.order {
        // soma das entradas (com ganho) no buffer do nó
        let (mut l, mut r) = (std::mem::take(&mut bufs[i][0]), std::mem::take(&mut bufs[i][1]));
        l.clear();
        l.resize(frames, 0.0);
        r.clear();
        r.resize(frames, 0.0);
        for &(src, gain) in &plan.inputs[i] {
            for k in 0..frames {
                l[k] += bufs[src][0][k] * gain;
                r[k] += bufs[src][1][k] * gain;
            }
        }
        if i != plan.output {
            proc_.process(i, &mut l, &mut r);
        }
        bufs[i] = [l, r];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plugin de teste: multiplica pelo fator do nó.
    struct Mul(HashMap<usize, f32>);
    impl Processor for Mul {
        fn process(&mut self, node: usize, l: &mut [f32], r: &mut [f32]) -> bool {
            if let Some(f) = self.0.get(&node) {
                l.iter_mut().chain(r.iter_mut()).for_each(|x| *x *= f);
                return true;
            }
            false
        }
    }

    fn exec(g: &Graph, muls: &[(&str, f32)], input: f32) -> Option<f32> {
        let p = plan(g).ok()?;
        let by_id = |id: &str| g.nodes.iter().position(|n| n.id == id).unwrap();
        let mut m = Mul(muls.iter().map(|(id, f)| (by_id(id), *f)).collect());
        let mut bufs: Vec<[Vec<f32>; 2]> = (0..g.nodes.len()).map(|_| [Vec::with_capacity(8), Vec::with_capacity(8)]).collect();
        bufs[p.input] = [vec![input; 4], vec![input; 4]];
        run(&p, &mut bufs, 4, &mut m);
        Some(bufs[p.output][0][0])
    }

    fn node(id: &str, kind: NodeKind) -> Node {
        Node { id: id.into(), kind, x: 0.0, y: 0.0 }
    }

    fn edge(a: &str, b: &str, g: f32) -> Edge {
        Edge { from: a.into(), to: b.into(), gain: g }
    }

    #[test]
    fn serial_applies_in_order() {
        let g = Graph::serial(2);
        assert_eq!(exec(&g, &[("p0", 2.0), ("p1", 3.0)], 1.0), Some(6.0));
    }

    #[test]
    fn empty_serial_is_passthrough() {
        assert_eq!(exec(&Graph::serial(0), &[], 0.5), Some(0.5));
    }

    #[test]
    fn parallel_branches_are_mixed_with_gains() {
        // in → A(x2) ─0.7→ mix ; in → B(x10) ─0.3→ mix ; mix → out
        let g = Graph {
            nodes: vec![
                node("in", NodeKind::Input),
                node("a", NodeKind::Plugin { plugin: 0 }),
                node("b", NodeKind::Plugin { plugin: 1 }),
                node("mix", NodeKind::Mixer),
                node("out", NodeKind::Output),
            ],
            edges: vec![edge("in", "a", 1.0), edge("in", "b", 1.0), edge("a", "mix", 0.7), edge("b", "mix", 0.3), edge("mix", "out", 1.0)],
        };
        let v = exec(&g, &[("a", 2.0), ("b", 10.0)], 1.0).unwrap();
        assert!((v - (2.0 * 0.7 + 10.0 * 0.3)).abs() < 1e-5, "{v}");
    }

    #[test]
    fn dry_wet_mix_without_mixer_node() {
        // som original + reverb somados direto na saída
        let g = Graph {
            nodes: vec![node("in", NodeKind::Input), node("rev", NodeKind::Plugin { plugin: 0 }), node("out", NodeKind::Output)],
            edges: vec![edge("in", "out", 1.0), edge("in", "rev", 1.0), edge("rev", "out", 0.5)],
        };
        assert_eq!(exec(&g, &[("rev", 4.0)], 1.0), Some(3.0));
    }

    #[test]
    fn cycle_is_rejected() {
        let g = Graph {
            nodes: vec![node("in", NodeKind::Input), node("a", NodeKind::Mixer), node("b", NodeKind::Mixer), node("out", NodeKind::Output)],
            edges: vec![edge("in", "a", 1.0), edge("a", "b", 1.0), edge("b", "a", 1.0), edge("b", "out", 1.0)],
        };
        assert_eq!(plan(&g), Err(PlanError::Cycle));
    }

    #[test]
    fn missing_ends_and_bad_edges() {
        let g = Graph { nodes: vec![node("in", NodeKind::Input)], edges: vec![] };
        assert_eq!(plan(&g), Err(PlanError::MissingInputOrOutput));
        // seta para nó inexistente e seta saindo de "out" são ignoradas
        let mut g = Graph::serial(1);
        g.edges.push(edge("p0", "fantasma", 1.0));
        g.edges.push(edge("out", "p0", 1.0));
        assert_eq!(exec(&g, &[("p0", 2.0)], 1.0), Some(2.0));
    }

    #[test]
    fn unconnected_output_is_silence() {
        let g = Graph { nodes: vec![node("in", NodeKind::Input), node("out", NodeKind::Output)], edges: vec![] };
        assert_eq!(exec(&g, &[], 1.0), Some(0.0));
    }

    #[test]
    fn send_node_passes_sum_through() {
        let g = Graph {
            nodes: vec![node("in", NodeKind::Input), node("s", NodeKind::Send { device: "{x}".into() }), node("out", NodeKind::Output)],
            edges: vec![edge("in", "s", 0.5), edge("s", "out", 1.0)],
        };
        assert_eq!(exec(&g, &[], 1.0), Some(0.5));
        let t = serde_json::to_string(&g).unwrap();
        assert!(t.contains(r#""kind":"send","device":"{x}""#), "{t}");
    }

    #[test]
    fn fx_node_json() {
        let n: Node = serde_json::from_str(r#"{"id":"g","kind":"fx","fx":"gain","value":-6,"x":0,"y":0}"#).unwrap();
        assert_eq!(n.kind, NodeKind::Fx { fx: crate::blocks::Fx::Gain, value: -6.0 });
    }

    #[test]
    fn free_flow_without_in_out() {
        // trigger (programa) → ganho → envio, sem as caixas fixas
        let g = Graph {
            nodes: vec![
                node("a", NodeKind::App { exe: "x.exe".into() }),
                node("m", NodeKind::Mixer),
                node("s", NodeKind::Send { device: "{d}".into() }),
            ],
            edges: vec![edge("a", "m", 0.5), edge("m", "s", 1.0)],
        };
        assert_eq!(plan(&g), Err(PlanError::MissingInputOrOutput));
        let p = plan_free(&g).unwrap();
        assert_eq!(p.order.len(), 3);
        let pos = |id: &str| p.order.iter().position(|&i| g.nodes[i].id == id).unwrap();
        assert!(pos("a") < pos("m") && pos("m") < pos("s"));
    }

    #[test]
    fn json_shape() {
        let g = Graph::serial(1);
        let t = serde_json::to_string(&g).unwrap();
        assert!(t.contains(r#""kind":"plugin","plugin":0"#), "{t}");
        assert_eq!(serde_json::from_str::<Graph>(&t).unwrap(), g);
    }
}
