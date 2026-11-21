//! Surround Rescue como plugin VST3/CLAP: o mesmo motor `sr-dsp` do app,
//! para usar na DAW (Reaper etc.) em ASIO, com latência zero somada pelo plugin.

use nih_plug::prelude::*;
mod editor;

use nih_plug_egui::{create_egui_editor, EguiState};
use sr_dsp::{Processor, Settings};
use std::sync::Arc;

#[derive(Params)]
pub struct SrParams {
    /// Tamanho da janela, salvo junto com os ajustes
    #[persist = "editor-state"]
    editor_state: Arc<EguiState>,
    #[id = "on"]
    enabled: BoolParam,
    #[id = "angle"]
    front_angle: FloatParam,
    #[id = "spatial"]
    spatial: FloatParam,
    #[id = "wide"]
    stereo_enhanced: BoolParam,
    #[id = "bass"]
    bass: FloatParam,
    #[id = "norm"]
    normalization: FloatParam,
    #[id = "pres"]
    presence: FloatParam,
    #[id = "vvol"]
    voice: FloatParam,
    #[id = "vol"]
    volume: FloatParam,
    #[id = "eq0"] eq0: FloatParam,
    #[id = "eq1"] eq1: FloatParam,
    #[id = "eq2"] eq2: FloatParam,
    #[id = "eq3"] eq3: FloatParam,
    #[id = "eq4"] eq4: FloatParam,
    #[id = "eq5"] eq5: FloatParam,
    #[id = "eq6"] eq6: FloatParam,
    #[id = "eq7"] eq7: FloatParam,
    #[id = "eq8"] eq8: FloatParam,
    #[id = "eq9"] eq9: FloatParam,
}

fn pct(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 100.0 }).with_step_size(1.0)
}

fn eq(name: &str) -> FloatParam {
    FloatParam::new(name, 0.0, FloatRange::Linear { min: -12.0, max: 12.0 }).with_step_size(0.5).with_unit(" dB")
}

impl Default for SrParams {
    fn default() -> Self {
        let d = Settings::default();
        Self {
            editor_state: EguiState::from_size(editor::SIZE.0, editor::SIZE.1),
            enabled: BoolParam::new("Surround ligado", true),
            front_angle: FloatParam::new("Abertura das caixas", d.front_angle as f32, FloatRange::Linear { min: 10.0, max: 90.0 })
                .with_step_size(1.0)
                .with_unit("°"),
            spatial: pct("Intensidade do surround", d.spatial as f32),
            stereo_enhanced: BoolParam::new("Estéreo melhorado", d.stereo_enhanced),
            bass: pct("Reforço de graves", d.bass_level as f32),
            normalization: pct("Normalização", 0.0),
            presence: FloatParam::new("Clareza: presença", 0.0, FloatRange::Linear { min: -100.0, max: 100.0 }).with_step_size(1.0),
            voice: FloatParam::new("Clareza: volume das vozes", 0.0, FloatRange::Linear { min: -100.0, max: 100.0 }).with_step_size(1.0),
            volume: FloatParam::new("Volume", 0.0, FloatRange::Linear { min: -30.0, max: 6.0 }).with_step_size(0.5).with_unit(" dB"),
            eq0: eq("EQ 31 Hz"),
            eq1: eq("EQ 63 Hz"),
            eq2: eq("EQ 125 Hz"),
            eq3: eq("EQ 250 Hz"),
            eq4: eq("EQ 500 Hz"),
            eq5: eq("EQ 1 kHz"),
            eq6: eq("EQ 2 kHz"),
            eq7: eq("EQ 4 kHz"),
            eq8: eq("EQ 8 kHz"),
            eq9: eq("EQ 16 kHz"),
        }
    }
}

impl SrParams {
    fn settings(&self) -> Settings {
        let eq = [&self.eq0, &self.eq1, &self.eq2, &self.eq3, &self.eq4, &self.eq5, &self.eq6, &self.eq7, &self.eq8, &self.eq9];
        let mut e = [0.0f64; 10];
        for (o, p) in e.iter_mut().zip(eq) {
            *o = p.value() as f64;
        }
        let bass = self.bass.value() as f64;
        let norm = self.normalization.value() as f64;
        let (pres, voice) = (self.presence.value() as f64, self.voice.value() as f64);
        Settings {
            enabled: self.enabled.value(),
            front_angle: self.front_angle.value() as f64,
            spatial: self.spatial.value() as f64,
            stereo_enhanced: self.stereo_enhanced.value(),
            bass_enabled: bass > 0.0,
            bass_level: bass,
            normalization_enabled: norm > 0.0,
            normalization_level: norm,
            clarity_enabled: pres != 0.0 || voice != 0.0,
            clarity_presence: pres,
            clarity_volume: voice,
            volume_db: self.volume.value() as f64,
            eq_enabled: true,
            eq_preset: String::new(),
            eq: e,
        }
    }

    /// Soma barata dos valores para detectar mudança sem alocar.
    fn fingerprint(&self) -> f32 {
        let eq = [&self.eq0, &self.eq1, &self.eq2, &self.eq3, &self.eq4, &self.eq5, &self.eq6, &self.eq7, &self.eq8, &self.eq9];
        let mut h = self.enabled.value() as u8 as f32 * 1.0e6 + self.stereo_enhanced.value() as u8 as f32 * 3.0e5;
        for (i, p) in [&self.front_angle, &self.spatial, &self.bass, &self.normalization, &self.presence, &self.voice, &self.volume]
            .into_iter()
            .chain(eq)
            .enumerate()
        {
            h += p.value() * (i as f32 * 7.13 + 1.0);
        }
        h
    }
}

struct SurroundRescue {
    params: Arc<SrParams>,
    dsp: Option<Processor>,
    last: f32,
}

impl Default for SurroundRescue {
    fn default() -> Self {
        Self { params: Arc::new(SrParams::default()), dsp: None, last: f32::NAN }
    }
}

impl Plugin for SurroundRescue {
    const NAME: &'static str = "Surround Rescue";
    const VENDOR: &'static str = "Surround Rescue";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        create_egui_editor(self.params.editor_state.clone(), (), |ctx, _| editor::setup(ctx), move |ctx, setter, _| editor::draw(ctx, setter, &params))
    }

    fn initialize(&mut self, _: &AudioIOLayout, cfg: &BufferConfig, ctx: &mut impl InitContext<Self>) -> bool {
        // alocação só aqui, fora do processamento em tempo real
        let mut p = Processor::new(cfg.sample_rate as f64);
        p.set(&self.params.settings());
        // a DAW compensa a antecipação do limitador (1,5 ms)
        ctx.set_latency_samples(p.latency() as u32);
        self.dsp = Some(p);
        self.last = self.params.fingerprint();
        true
    }

    fn process(&mut self, buffer: &mut Buffer, _: &mut AuxiliaryBuffers, _: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let Some(dsp) = self.dsp.as_mut() else { return ProcessStatus::Normal };
        let fp = self.params.fingerprint();
        if fp != self.last {
            self.last = fp;
            // `settings()` cria uma String vazia (sem alocação no heap), seguro em tempo real
            dsp.set(&self.params.settings());
        }
        let ch = buffer.as_slice();
        if let [l, r] = ch {
            dsp.process_stereo(l, r);
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for SurroundRescue {
    const CLAP_ID: &'static str = "dev.surround-rescue.plugin";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Surround virtual, EQ e anti-estouro com latência zero");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo, ClapFeature::Equalizer, ClapFeature::Surround];
}

impl Vst3Plugin for SurroundRescue {
    const VST3_CLASS_ID: [u8; 16] = *b"SurroundRescue01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Spatial, Vst3SubCategory::Eq];
}

nih_export_clap!(SurroundRescue);
nih_export_vst3!(SurroundRescue);
