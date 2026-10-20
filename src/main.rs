//! dimtune — reproductor de música ultraminimalista.
//! Modo oscuro fijo, sin animaciones, sin red, sin telemetría.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Color32, ColorImage, Key, RichText, TextureHandle, TextureOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

const EXTENSIONES: [&str; 5] = ["mp3", "flac", "ogg", "wav", "m4a"];
const FILA_ALTO: f32 = 22.0;
const LADO_PORTADA: f32 = 148.0;

// ───────────────────────────── utilidades ─────────────────────────────

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn dir_config_base() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

fn ruta_config() -> PathBuf {
    dir_config_base().join("dimtune").join("config")
}

/// Carpeta de música del usuario (XDG), p. ej. ~/Música en Debian en español.
/// Si no hay una carpeta válida, usa ~/Music aunque no exista: escanear todo
/// el home por defecto sería demasiado caro y sorprendente.
fn carpeta_musica_por_defecto() -> PathBuf {
    let casa = home();
    let dirs = dir_config_base().join("user-dirs.dirs");
    if let Ok(txt) = std::fs::read_to_string(dirs) {
        for linea in txt.lines() {
            if let Some(v) = linea.trim().strip_prefix("XDG_MUSIC_DIR=") {
                let v = v
                    .trim()
                    .trim_matches('"')
                    .replace("$HOME", &casa.to_string_lossy());
                let p = PathBuf::from(v);
                if p.is_dir() && p != casa {
                    return p;
                }
            }
        }
    }
    casa.join("Music")
}

struct Config {
    carpeta: Option<PathBuf>,
    aleatorio: bool,
    volumen: f32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            carpeta: None,
            aleatorio: false,
            volumen: 0.8,
        }
    }
}

impl Config {
    fn cargar() -> Self {
        std::fs::read_to_string(ruta_config())
            .map(|txt| Self::desde_texto(&txt))
            .unwrap_or_default()
    }

    fn desde_texto(txt: &str) -> Self {
        let mut c = Self::default();
        for l in txt.lines() {
            if let Some((k, v)) = l.split_once('=') {
                match k.trim() {
                    "carpeta" if !v.trim().is_empty() => c.carpeta = Some(PathBuf::from(v.trim())),
                    "aleatorio" => c.aleatorio = v.trim() == "1",
                    "volumen" => {
                        if let Ok(x) = v.trim().parse::<f32>()
                            && x.is_finite()
                        {
                            c.volumen = x.clamp(0.0, 1.0);
                        }
                    }
                    _ => {}
                }
            }
        }
        c
    }

    fn guardar(&self) {
        let ruta = ruta_config();
        if let Some(dir) = ruta.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let carpeta = self
            .carpeta
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let txt = format!(
            "carpeta={}\naleatorio={}\nvolumen={:.2}\n",
            carpeta,
            if self.aleatorio { 1 } else { 0 },
            self.volumen
        );
        // Escribir en un temporal y renombrar evita dejar el archivo a medias.
        let temporal = ruta.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&temporal, txt).is_ok() {
            if std::fs::rename(&temporal, &ruta).is_err() {
                let _ = std::fs::remove_file(&temporal);
            }
        } else {
            let _ = std::fs::remove_file(&temporal);
        }
    }
}

fn escanear(dir: &Path, salida: &mut Vec<PathBuf>, profundidad: u32) {
    if profundidad > 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            escanear(&p, salida, profundidad + 1);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| EXTENSIONES.contains(&x.to_ascii_lowercase().as_str()))
        {
            salida.push(p);
        }
    }
}

fn formatear(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, seg) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{seg:02}")
    } else {
        format!("{m}:{seg:02}")
    }
}

/// Generador pseudoaleatorio mínimo (xorshift64), sin dependencias.
struct Azar(u64);

impl Azar {
    fn nuevo() -> Self {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Azar(n | 1)
    }
    fn siguiente(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn mezclar(&mut self, v: &mut [usize]) {
        for i in (1..v.len()).rev() {
            let j = (self.siguiente() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

// ─────────────────────── metadatos y portada (hilo) ───────────────────────

struct Meta {
    generacion: u64,
    titulo: Option<String>,
    artista: Option<String>,
    album: Option<String>,
    duracion: Option<Duration>,
    portada: Option<ColorImage>,
}

struct ResultadoEscaneo {
    generacion: u64,
    carpeta: PathBuf,
    pistas: Vec<PathBuf>,
}

fn decodificar_portada(bytes: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.thumbnail(320, 320).to_rgba8();
    let tam = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(tam, img.as_raw()))
}

fn portada_de_carpeta(archivo: &Path) -> Option<ColorImage> {
    let dir = archivo.parent()?;
    let entradas: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    for nombre in ["cover", "folder", "front", "album"] {
        for ext in ["jpg", "jpeg", "png"] {
            for p in &entradas {
                let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Some(extension) = p.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !stem.eq_ignore_ascii_case(nombre) || !extension.eq_ignore_ascii_case(ext) {
                    continue;
                }
                if let Some(img) = std::fs::read(p).ok().and_then(|b| decodificar_portada(&b)) {
                    return Some(img);
                }
            }
        }
    }
    None
}

fn leer_meta(generacion: u64, ruta: &Path) -> Meta {
    let mut m = Meta {
        generacion,
        titulo: None,
        artista: None,
        album: None,
        duracion: None,
        portada: None,
    };
    if let Ok(tf) = lofty::read_from_path(ruta) {
        let d = tf.properties().duration();
        if !d.is_zero() {
            m.duracion = Some(d);
        }
        if let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) {
            m.titulo = tag
                .title()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            m.artista = tag
                .artist()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            m.album = tag
                .album()
                .map(|s| s.into_owned())
                .filter(|s| !s.trim().is_empty());
            m.portada = tag
                .pictures()
                .iter()
                .find_map(|p| decodificar_portada(p.data()));
        }
    }
    if m.portada.is_none() {
        m.portada = portada_de_carpeta(ruta);
    }
    m
}

// ───────────────────────────────── app ─────────────────────────────────

struct App {
    config: Config,
    // Audio (el sink debe vivir mientras suene algo)
    sink: Option<MixerDeviceSink>,
    player: Option<Player>,
    // Lista
    carpeta: Option<PathBuf>,
    pistas: Vec<PathBuf>,
    orden: Vec<usize>,
    pos: usize,
    actual: Option<usize>,
    azar: Azar,
    // Info de la pista actual
    generacion: u64,
    meta: Option<Meta>,
    portada: Option<TextureHandle>,
    // Interfaz
    buscando_carpeta: bool,
    cargando: bool,
    rx_carpeta: Receiver<PathBuf>,
    tx_carpeta: Sender<PathBuf>,
    rx_escaneo: Receiver<ResultadoEscaneo>,
    tx_escaneo: Sender<ResultadoEscaneo>,
    generacion_escaneo: u64,
    rx_meta: Receiver<Meta>,
    tx_meta: Sender<Meta>,
    arrastre: Option<f32>,
    ir_a_actual: bool,
    fallos_seguidos: usize,
    saltar_siguiente: bool,
    estado: String,
}

impl App {
    fn nueva(cc: &eframe::CreationContext<'_>) -> Self {
        aplicar_estilo(&cc.egui_ctx);
        let config = Config::cargar();
        let (tx_carpeta, rx_carpeta) = mpsc::channel();
        let (tx_escaneo, rx_escaneo) = mpsc::channel();
        let (tx_meta, rx_meta) = mpsc::channel();
        let mut app = App {
            sink: None,
            player: None,
            carpeta: None,
            pistas: Vec::new(),
            orden: Vec::new(),
            pos: 0,
            actual: None,
            azar: Azar::nuevo(),
            generacion: 0,
            meta: None,
            portada: None,
            buscando_carpeta: false,
            cargando: false,
            rx_carpeta,
            tx_carpeta,
            rx_escaneo,
            tx_escaneo,
            generacion_escaneo: 0,
            rx_meta,
            tx_meta,
            arrastre: None,
            ir_a_actual: false,
            fallos_seguidos: 0,
            saltar_siguiente: false,
            estado: String::new(),
            config,
        };
        let inicial = app
            .config
            .carpeta
            .clone()
            .filter(|p| p.is_dir())
            .unwrap_or_else(carpeta_musica_por_defecto);
        app.cargar_carpeta(inicial, &cc.egui_ctx);
        app
    }

    fn cargar_carpeta(&mut self, dir: PathBuf, ctx: &egui::Context) {
        self.detener();
        self.generacion_escaneo = self.generacion_escaneo.wrapping_add(1);
        let generacion = self.generacion_escaneo;
        self.pistas.clear();
        self.orden.clear();
        self.pos = 0;
        self.ir_a_actual = false;
        self.estado.clear();
        self.cargando = true;
        self.carpeta = Some(dir.clone());
        self.config.carpeta = Some(dir.clone());
        self.config.guardar();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title("dimtune".into()));
        let tx = self.tx_escaneo.clone();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let mut v = Vec::new();
            escanear(&dir, &mut v, 0);
            v.sort_by_cached_key(|p| p.to_string_lossy().to_lowercase());
            let _ = tx.send(ResultadoEscaneo {
                generacion,
                carpeta: dir,
                pistas: v,
            });
            c.request_repaint();
        });
    }

    fn reconstruir_orden(&mut self, primero: Option<usize>) {
        let mut o: Vec<usize> = (0..self.pistas.len()).collect();
        if self.config.aleatorio {
            self.azar.mezclar(&mut o);
            if let Some(p) = primero
                && let Some(i) = o.iter().position(|&x| x == p)
            {
                o.swap(0, i);
            }
        }
        self.orden = o;
        self.pos = match primero {
            Some(p) => self.orden.iter().position(|&x| x == p).unwrap_or(0),
            None => 0,
        };
    }

    fn detener(&mut self) {
        self.player = None;
        self.actual = None;
        self.meta = None;
        self.portada = None;
        self.arrastre = None;
        self.ir_a_actual = false;
        self.fallos_seguidos = 0;
        self.saltar_siguiente = false;
        self.generacion = self.generacion.wrapping_add(1);
    }

    fn asegurar_salida(&mut self) -> bool {
        if self.sink.is_none() {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut s) => {
                    s.log_on_drop(false);
                    self.sink = Some(s);
                }
                Err(e) => {
                    self.estado = format!("No se pudo abrir la salida de audio: {e}");
                    return false;
                }
            }
        }
        true
    }

    fn reproducir_indice(&mut self, idx: usize, ctx: &egui::Context) {
        let Some(ruta) = self.pistas.get(idx).cloned() else {
            return;
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} — dimtune",
            nombre_pista(&ruta)
        )));
        self.generacion = self.generacion.wrapping_add(1);
        let generacion = self.generacion;
        self.player = None;
        self.actual = Some(idx);
        self.meta = None;
        self.portada = None;
        self.arrastre = None;
        self.saltar_siguiente = false;
        self.estado.clear();
        if !self.asegurar_salida() {
            return;
        }
        let decoder = File::open(&ruta)
            .map_err(|e| e.to_string())
            .and_then(|f| Decoder::try_from(f).map_err(|e| e.to_string()));
        let decoder = match decoder {
            Ok(d) => d,
            Err(e) => {
                self.estado = format!("No se pudo reproducir {}: {e}", nombre_pista(&ruta));
                self.fallos_seguidos += 1;
                self.saltar_siguiente = true;
                ctx.request_repaint();
                return;
            }
        };
        self.fallos_seguidos = 0;
        let Some(sink) = self.sink.as_ref() else {
            return;
        };
        let player = Player::connect_new(sink.mixer());
        player.set_volume(self.config.volumen);
        player.append(decoder);
        // El anterior (si existía) se detiene al soltarse.
        self.player = Some(player);
        self.ir_a_actual = true;
        let (tx, c) = (self.tx_meta.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(leer_meta(generacion, &ruta));
            c.request_repaint();
        });
    }

    fn reproducir_en_pos(&mut self, pos: usize, ctx: &egui::Context) {
        if self.orden.is_empty() {
            return;
        }
        self.pos = pos % self.orden.len();
        let idx = self.orden[self.pos];
        self.reproducir_indice(idx, ctx);
    }

    fn siguiente(&mut self, ctx: &egui::Context) {
        if self.orden.is_empty() {
            return;
        }
        let mut p = self.pos + 1;
        if p >= self.orden.len() {
            p = 0;
            if self.config.aleatorio {
                let actual = self.actual;
                self.reconstruir_orden(None);
                // Evita repetir la misma canción justo al reiniciar la ronda.
                if self.orden.len() > 1 && actual == Some(self.orden[0]) {
                    self.orden.swap(0, 1);
                }
            }
        }
        self.reproducir_en_pos(p, ctx);
    }

    fn anterior(&mut self, ctx: &egui::Context) {
        if self.orden.is_empty() {
            return;
        }
        // Si ya avanzó unos segundos, vuelve al inicio de la pista.
        if let Some(pl) = &self.player
            && pl.get_pos() > Duration::from_secs(4)
        {
            let _ = pl.try_seek(Duration::ZERO);
            return;
        }
        let p = if self.pos == 0 {
            self.orden.len() - 1
        } else {
            self.pos - 1
        };
        self.reproducir_en_pos(p, ctx);
    }

    fn alternar_pausa(&mut self, ctx: &egui::Context) {
        match &self.player {
            Some(p) => {
                if p.is_paused() {
                    p.play();
                } else {
                    p.pause();
                }
            }
            None => {
                if !self.orden.is_empty() {
                    self.fallos_seguidos = 0;
                    self.saltar_siguiente = false;
                    if let Some(i) = self.actual {
                        self.reproducir_indice(i, ctx);
                    } else {
                        self.reproducir_en_pos(self.pos, ctx);
                    }
                }
            }
        }
    }

    fn elegir_carpeta(&mut self, ctx: &egui::Context) {
        if self.buscando_carpeta {
            return;
        }
        self.buscando_carpeta = true;
        let tx = self.tx_carpeta.clone();
        let c = ctx.clone();
        let inicio = self
            .carpeta
            .clone()
            .unwrap_or_else(carpeta_musica_por_defecto);
        std::thread::spawn(move || {
            let r = rfd::FileDialog::new()
                .set_title("Elegir carpeta de música")
                .set_directory(inicio)
                .pick_folder();
            // Siempre responde (incluso si se cancela) para liberar el botón.
            let _ = tx.send(r.unwrap_or_default());
            c.request_repaint();
        });
    }
}

fn nombre_pista(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ─────────────────────────────── estilo ───────────────────────────────

fn aplicar_estilo(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut v = egui::Visuals::dark();
    let fondo = Color32::from_rgb(0x12, 0x12, 0x13);
    let panel = Color32::from_rgb(0x18, 0x18, 0x1a);
    let texto = Color32::from_rgb(0xc8, 0xc8, 0xc8);
    v.panel_fill = fondo;
    v.window_fill = fondo;
    v.extreme_bg_color = Color32::from_rgb(0x0d, 0x0d, 0x0e);
    v.override_text_color = Some(texto);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.selection.bg_fill = Color32::from_rgb(0x2b, 0x3a, 0x4a);
    v.selection.stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(0xdd, 0xdd, 0xdd));
    v.widgets.noninteractive.bg_fill = panel;
    v.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0_f32, Color32::from_rgb(0x26, 0x26, 0x29));
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x22, 0x22, 0x25);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x22, 0x22, 0x25);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x32);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2e, 0x2e, 0x32);
    v.widgets.active.bg_fill = Color32::from_rgb(0x38, 0x38, 0x3d);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x3d);
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.animation_time = 0.0; // sin animaciones
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.interaction.tooltip_delay = 0.6;
        s.scroll_animation = egui::style::ScrollAnimation::none();
        for f in s.text_styles.values_mut() {
            f.size = (f.size * 1.08).round();
        }
    });
}

// ──────────────────────────────── ui ────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // — eventos de hilos —
        while let Ok(p) = self.rx_carpeta.try_recv() {
            self.buscando_carpeta = false;
            if p.is_dir() {
                self.cargar_carpeta(p, ctx);
            }
        }
        while let Ok(r) = self.rx_escaneo.try_recv() {
            if r.generacion == self.generacion_escaneo {
                self.pistas = r.pistas;
                self.carpeta = Some(r.carpeta.clone());
                self.config.carpeta = Some(r.carpeta);
                self.reconstruir_orden(None);
                self.cargando = false;
                self.ir_a_actual = false;
                self.estado = if self.pistas.is_empty() {
                    "No se encontró música en esta carpeta".into()
                } else {
                    String::new()
                };
            }
        }
        // Un fallo de decodificación se salta en el siguiente frame para no
        // encadenar llamadas recursivas ni congelar la interfaz.
        if self.saltar_siguiente {
            self.saltar_siguiente = false;
            if self.fallos_seguidos < self.pistas.len().max(1) {
                self.siguiente(ctx);
            }
        }
        while let Ok(m) = self.rx_meta.try_recv() {
            if m.generacion == self.generacion {
                self.portada = m
                    .portada
                    .as_ref()
                    .map(|img| ctx.load_texture("portada", img.clone(), TextureOptions::LINEAR));
                self.meta = Some(m);
            }
        }

        // — fin de pista → siguiente automáticamente —
        let terminada = self.player.as_ref().is_some_and(|p| p.empty());
        if terminada {
            self.siguiente(ctx);
        }

        // — atajos —
        if !ctx.wants_keyboard_input() {
            let (espacio, der, izq) = ctx.input(|i| {
                (
                    i.key_pressed(Key::Space),
                    i.key_pressed(Key::ArrowRight),
                    i.key_pressed(Key::ArrowLeft),
                )
            });
            if espacio {
                self.alternar_pausa(ctx);
            }
            if der {
                self.fallos_seguidos = 0;
                self.saltar_siguiente = false;
                self.siguiente(ctx);
            }
            if izq {
                self.fallos_seguidos = 0;
                self.saltar_siguiente = false;
                self.anterior(ctx);
            }
        }

        let en_pausa = self.player.as_ref().is_some_and(|p| p.is_paused());
        let sonando = self.player.is_some() && !en_pausa;

        // — barra superior —
        egui::TopBottomPanel::top("arriba").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.buscando_carpeta, egui::Button::new("Carpeta…"))
                    .clicked()
                {
                    self.elegir_carpeta(ctx);
                }
                let ruta = self
                    .carpeta
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.add(egui::Label::new(RichText::new(ruta).weak()).truncate());
            });
            ui.add_space(2.0);
        });

        // — controles —
        egui::TopBottomPanel::bottom("abajo").show(ctx, |ui| {
            ui.add_space(6.0);
            // progreso
            let dur = self.meta.as_ref().and_then(|m| m.duracion);
            let pos_real = self
                .player
                .as_ref()
                .map(|p| p.get_pos())
                .unwrap_or_default();
            let pos_seg = self.arrastre.unwrap_or(pos_real.as_secs_f32());
            let pos_mostrada = dur.map(|d| pos_seg.min(d.as_secs_f32())).unwrap_or(pos_seg);
            let total = dur.map(|d| d.as_secs_f32()).unwrap_or(0.0).max(0.1);
            ui.horizontal(|ui| {
                ui.label(formatear(Duration::from_secs_f32(pos_mostrada)));
                let mut valor = pos_seg.min(total);
                ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(80.0);
                let r = ui.add_enabled(
                    dur.is_some() && self.player.is_some(),
                    egui::Slider::new(&mut valor, 0.0..=total).show_value(false),
                );
                if r.changed() {
                    self.arrastre = Some(valor);
                }
                if (r.drag_stopped() || (r.changed() && !r.dragged()))
                    && let (Some(p), Some(v)) = (&self.player, self.arrastre.take())
                {
                    let _ = p.try_seek(Duration::from_secs_f32(v));
                }
                ui.label(dur.map(formatear).unwrap_or_else(|| "--:--".into()));
            });
            // botones
            ui.horizontal(|ui| {
                if ui.button("Anterior").clicked() {
                    self.fallos_seguidos = 0;
                    self.saltar_siguiente = false;
                    self.anterior(ctx);
                }
                let txt = if sonando { "Pausa" } else { "Reproducir" };
                if ui
                    .add(egui::Button::new(txt).min_size(egui::vec2(96.0, 0.0)))
                    .clicked()
                {
                    self.alternar_pausa(ctx);
                }
                if ui.button("Siguiente").clicked() {
                    self.fallos_seguidos = 0;
                    self.saltar_siguiente = false;
                    self.siguiente(ctx);
                }
                ui.separator();
                let mut alea = self.config.aleatorio;
                if ui.checkbox(&mut alea, "Aleatorio").changed() {
                    self.config.aleatorio = alea;
                    self.fallos_seguidos = 0;
                    self.saltar_siguiente = false;
                    self.reconstruir_orden(self.actual);
                    self.config.guardar();
                }
                ui.separator();
                ui.label("Vol");
                let mut vol = self.config.volumen;
                ui.spacing_mut().slider_width = 110.0;
                let r = ui.add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false));
                if r.changed() {
                    self.config.volumen = vol;
                    if let Some(p) = &self.player {
                        p.set_volume(vol);
                    }
                }
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.config.guardar();
                }
            });
            ui.add_space(6.0);
        });

        // — centro: portada + info + lista —
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(LADO_PORTADA, LADO_PORTADA),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(rect, 2.0, Color32::from_rgb(0x0d, 0x0d, 0x0e));
                if let Some(t) = &self.portada {
                    let s = t.size_vec2();
                    let escala = (LADO_PORTADA / s.x).min(LADO_PORTADA / s.y);
                    let r = egui::Rect::from_center_size(rect.center(), s * escala);
                    ui.painter().image(
                        t.id(),
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    if let Some(i) = self.actual {
                        let ruta = &self.pistas[i];
                        let titulo = self
                            .meta
                            .as_ref()
                            .and_then(|m| m.titulo.clone())
                            .unwrap_or_else(|| nombre_pista(ruta));
                        ui.add(
                            egui::Label::new(RichText::new(titulo).size(20.0).strong()).truncate(),
                        );
                        if let Some(m) = &self.meta {
                            if let Some(a) = &m.artista {
                                ui.add(egui::Label::new(a.as_str()).truncate());
                            }
                            if let Some(a) = &m.album {
                                ui.add(
                                    egui::Label::new(RichText::new(a.as_str()).weak()).truncate(),
                                );
                            }
                        }
                    } else {
                        ui.label(RichText::new("Nada en reproducción").weak());
                    }
                    ui.add_space(8.0);
                    if self.cargando {
                        ui.label(RichText::new("Cargando…").weak());
                    } else if !self.estado.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&self.estado)
                                    .color(Color32::from_rgb(0xd0, 0x8a, 0x6a)),
                            )
                            .wrap(),
                        );
                    } else {
                        let texto = if self.pistas.len() == 1 {
                            "1 canción".to_string()
                        } else {
                            format!("{} canciones", self.pistas.len())
                        };
                        ui.label(RichText::new(texto).weak());
                    }
                });
            });
            ui.add_space(6.0);
            ui.separator();

            let total = self.pistas.len();
            let mut sc = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if self.ir_a_actual {
                if let Some(i) = self.actual {
                    sc = sc.vertical_scroll_offset(
                        (i as f32 * (FILA_ALTO + ui.spacing().item_spacing.y) - 60.0).max(0.0),
                    );
                }
                self.ir_a_actual = false;
            }
            let mut elegido: Option<usize> = None;
            sc.show_rows(ui, FILA_ALTO, total, |ui, rango| {
                for i in rango {
                    let sel = self.actual == Some(i);
                    let nombre = nombre_pista(&self.pistas[i]);
                    let r = ui.add_sized(
                        [ui.available_width(), FILA_ALTO],
                        egui::Button::selectable(sel, nombre)
                            .truncate()
                            .frame_when_inactive(false),
                    );
                    if r.clicked() {
                        elegido = Some(i);
                    }
                }
            });
            if let Some(i) = elegido {
                self.fallos_seguidos = 0;
                self.saltar_siguiente = false;
                self.reconstruir_orden(Some(i));
                self.reproducir_indice(i, ctx);
            }
        });

        // Solo despierta periódicamente mientras suena (progreso + siguiente pista).
        if sonando {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.guardar();
    }
}

fn main() -> eframe::Result {
    let opciones = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 520.0])
            .with_min_inner_size([520.0, 360.0])
            .with_title("dimtune")
            .with_app_id("dimtune"),
        ..Default::default()
    };
    eframe::run_native(
        "dimtune",
        opciones,
        Box::new(|cc| Ok(Box::new(App::nueva(cc)))),
    )
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use std::fs;

    fn dir_temporal() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("dimtune-test-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn formato_de_duraciones() {
        assert_eq!(formatear(Duration::from_secs(0)), "0:00");
        assert_eq!(formatear(Duration::from_secs(59)), "0:59");
        assert_eq!(formatear(Duration::from_secs(60)), "1:00");
        assert_eq!(formatear(Duration::from_secs(3599)), "59:59");
        assert_eq!(formatear(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(formatear(Duration::from_secs(3661)), "1:01:01");
    }

    #[test]
    fn nombre_sin_extension() {
        assert_eq!(nombre_pista(Path::new("/musica/cancion.mp3")), "cancion");
    }

    #[test]
    fn mezcla_es_una_permutacion() {
        let mut v: Vec<usize> = (0..64).collect();
        let original = v.clone();
        Azar(1).mezclar(&mut v);
        let mut ordenado = v.clone();
        ordenado.sort_unstable();
        assert_eq!(ordenado, original);
        assert_ne!(v, original);
    }

    #[test]
    fn config_desde_texto() {
        let c = Config::desde_texto(
            "carpeta=/tmp/musica\naleatorio=1\nvolumen=1.7\ndesconocido=valor\n",
        );
        assert_eq!(c.carpeta, Some(PathBuf::from("/tmp/musica")));
        assert!(c.aleatorio);
        assert_eq!(c.volumen, 1.0);

        let d = Config::desde_texto("volumen=-0.5\n");
        assert_eq!(d.volumen, 0.0);
        assert_eq!(d.carpeta, None);
        assert!(!d.aleatorio);

        assert_eq!(Config::desde_texto("volumen=NaN\n").volumen, 0.8);
        assert_eq!(Config::desde_texto("carpeta=\n").carpeta, None);
    }

    #[test]
    fn escaneo_recursivo_y_extensiones() {
        let base = dir_temporal();
        let sub = base.join("sub");
        let profundo = sub.join("mas");
        fs::create_dir_all(&profundo).unwrap();
        for nombre in ["a.mp3", "B.FLAC", "nota.txt"] {
            fs::write(base.join(nombre), b"").unwrap();
        }
        fs::write(sub.join("c.ogg"), b"").unwrap();
        fs::write(profundo.join("d.wav"), b"").unwrap();
        fs::write(profundo.join("e.opus"), b"").unwrap();

        let mut pistas = Vec::new();
        escanear(&base, &mut pistas, 0);
        let nombres: Vec<String> = pistas
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(pistas.len(), 4);
        for nombre in ["a.mp3", "B.FLAC", "c.ogg", "d.wav"] {
            assert!(nombres.iter().any(|n| n == nombre), "falta {nombre}");
        }

        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn portada_de_carpeta_sin_distinguir_mayusculas() {
        let base = dir_temporal();
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        fs::write(base.join("COVER.PNG"), bytes).unwrap();

        let portada = portada_de_carpeta(&base.join("cancion.mp3")).unwrap();
        assert!(portada.size[0] > 0 && portada.size[1] > 0);

        fs::remove_dir_all(&base).unwrap();
    }
}
