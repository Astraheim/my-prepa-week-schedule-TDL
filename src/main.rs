// PrepaWeek - v0.7
//
// Ajouts par rapport à la v0.6 :
// - history.json : chaque semaine est archivée automatiquement (au
//   changement de semaine détecté, ou en cliquant "Nouvelle semaine").
// - Écran "📊 Historique" : liste des dernières semaines avec leur %.
// - Série en cours : "🔥 N semaines consécutives à +80 %".
// - Comparaison avec la semaine précédente, affichée sous la progression
//   actuelle et dans l'écran d'historique.
// - Éditeur de tâches intégré ("✏" dans la barre du haut) : ajouter,
//   modifier, supprimer catégories et tâches directement depuis l'appli,
//   sans toucher à config.toml à la main (le fichier est réécrit par
//   l'appli, en conservant un en-tête explicatif).
//
// Tout le reste (icônes filtrées, cases multiples, state.json, détection
// de nouvelle semaine, questionnaire hebdomadaire, paliers, heures,
// animations/confettis, thème persistant) est inchangé par rapport à la
// v0.6.

#![windows_subsystem = "windows"]

use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, Timelike};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

// ============================================================
// Config (config.toml) — lu ET écrit (éditeur intégré)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct ConfigFile {
    categories: Vec<CategoryConfig>,
}

#[derive(Serialize, Deserialize, Clone)]
struct CategoryConfig {
    name: String,
    icon: String,
    tasks: Vec<TaskConfig>,
}

#[derive(Serialize, Deserialize, Clone)]
struct TaskConfig {
    name: String,
    #[serde(default = "default_occurrences")]
    occurrences: u32,
    #[serde(default = "default_true")]
    count_in_progress: bool,
    /// Si true, l'inclusion de cette tâche est redemandée à chaque nouvelle
    /// semaine (ex. "vocabulaire anglais, 1 semaine sur 2").
    #[serde(default)]
    ask_include: bool,
    /// Si true, le nombre de cases de cette tâche est redemandé à chaque
    /// nouvelle semaine (ex. "combien de séances de sport ?").
    #[serde(default)]
    ask_occurrences: bool,
    /// Si true, cette tâche est comptée à part dans une barre de
    /// progression "avant vendredi soir" (lundi → vendredi), en plus du
    /// calcul principal sur les 7 jours (qui reste celui utilisé pour
    /// l'historique et la détection de semaine complète).
    #[serde(default)]
    weekday_only: bool,
}

fn default_occurrences() -> u32 {
    1
}
fn default_true() -> bool {
    true
}

fn fallback_raw_categories() -> Vec<CategoryConfig> {
    vec![CategoryConfig {
        name: "Exemple".to_string(),
        icon: "⚠".to_string(),
        tasks: vec![TaskConfig {
            name: "Crée/complète ton fichier config.toml (ou utilise le bouton ✏) puis relance l'appli".to_string(),
            occurrences: 1,
            count_in_progress: true,
            ask_include: false,
            ask_occurrences: false,
            weekday_only: false,
        }],
    }]
}

const CONFIG_HEADER: &str = "\
# Configuration de PrepaWeek
#
# Modifiable ici à la main, OU depuis l'appli via le bouton \"✏\" (cet
# en-tête est réécrit automatiquement à chaque sauvegarde depuis l'appli,
# mais la structure ci-dessous reste la même).
#
# Structure :
#   [[categories]]
#   name = \"Nom de la catégorie\"
#   icon = \"🎯\"          <- utilise uniquement des icônes confirmées visibles
#
#   [[categories.tasks]]
#   name = \"Nom de la tâche\"
#   occurrences = 1          <- nombre de cases à cocher sur la semaine
#   count_in_progress = true <- false = tâche \"bonus\", hors avancement
#   ask_include = false      <- true = redemandé chaque semaine (inclure ?)
#   ask_occurrences = false  <- true = redemandé chaque semaine (combien ?)
#   weekday_only = false     <- true = suivie aussi dans la barre \"avant
#                              vendredi soir\" séparée (lundi → vendredi)
";

fn save_config(raw: &[CategoryConfig]) -> std::io::Result<()> {
    let cfg = ConfigFile { categories: raw.to_vec() };
    let body = toml::to_string_pretty(&cfg).unwrap_or_default();
    std::fs::write("config.toml", format!("{CONFIG_HEADER}\n{body}"))
}

// ============================================================
// Modèle runtime
// ============================================================

struct Task {
    name: String,
    occurrences: Vec<bool>,
    count_in_progress: bool,
    weekday_only: bool,
}

impl Task {
    fn total(&self) -> usize {
        self.occurrences.len()
    }
    fn done(&self) -> usize {
        self.occurrences.iter().filter(|d| **d).count()
    }
}

struct Category {
    name: String,
    icon: String,
    tasks: Vec<Task>,
}

fn state_key(cat: &str, task: &str) -> String {
    format!("{cat}::{task}")
}

fn materialize_categories(raw: &[CategoryConfig], choices: Option<&WeekChoices>) -> Vec<Category> {
    raw.iter()
        .map(|c| {
            let tasks = c
                .tasks
                .iter()
                .filter_map(|t| {
                    let key = state_key(&c.name, &t.name);
                    if t.ask_include {
                        let included = choices.and_then(|ch| ch.include.get(&key)).copied().unwrap_or(true);
                        if !included {
                            return None;
                        }
                        Some(Task { name: t.name.clone(), occurrences: vec![false; 1], count_in_progress: t.count_in_progress, weekday_only: t.weekday_only })
                    } else if t.ask_occurrences {
                        let n = choices.and_then(|ch| ch.occurrences.get(&key)).copied().unwrap_or(t.occurrences.max(1));
                        if n == 0 {
                            return None;
                        }
                        Some(Task { name: t.name.clone(), occurrences: vec![false; n as usize], count_in_progress: t.count_in_progress, weekday_only: t.weekday_only })
                    } else {
                        Some(Task { name: t.name.clone(), occurrences: vec![false; t.occurrences.max(1) as usize], count_in_progress: t.count_in_progress, weekday_only: t.weekday_only })
                    }
                })
                .collect();
            Category { name: c.name.clone(), icon: c.icon.clone(), tasks }
        })
        .collect()
}

fn has_dynamic_tasks(raw: &[CategoryConfig]) -> bool {
    raw.iter().any(|c| c.tasks.iter().any(|t| t.ask_include || t.ask_occurrences))
}

fn count_in_progress_map(raw: &[CategoryConfig]) -> HashMap<String, bool> {
    let mut m = HashMap::new();
    for c in raw {
        for t in &c.tasks {
            m.insert(state_key(&c.name, &t.name), t.count_in_progress);
        }
    }
    m
}

// ============================================================
// state.json — persistance de l'état + des choix de la semaine
// ============================================================

#[derive(Serialize, Deserialize, Default, Clone)]
struct WeekChoices {
    include: HashMap<String, bool>,
    occurrences: HashMap<String, u32>,
}

#[derive(Serialize, Deserialize, Default)]
struct StateFile {
    week_start: Option<String>,
    tasks: HashMap<String, Vec<bool>>,
    #[serde(default)]
    choices: Option<WeekChoices>,
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    let wd = date.weekday().num_days_from_monday();
    date - ChronoDuration::days(wd as i64)
}

fn load_state() -> StateFile {
    match std::fs::read_to_string("state.json") {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => StateFile::default(),
    }
}

fn save_state(categories: &[Category], choices: &WeekChoices) {
    let mut tasks = HashMap::new();
    for c in categories {
        for t in &c.tasks {
            tasks.insert(state_key(&c.name, &t.name), t.occurrences.clone());
        }
    }
    let today = Local::now().date_naive();
    let state = StateFile {
        week_start: Some(monday_of(today).format("%Y-%m-%d").to_string()),
        tasks,
        choices: Some(choices.clone()),
    };
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::write("state.json", json);
    }
}

// ============================================================
// history.json — archive des semaines passées
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct CategorySummary {
    name: String,
    done: usize,
    total: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct WeekRecord {
    week_start: String,
    total: usize,
    done: usize,
    percent: f32,
    categories: Vec<CategorySummary>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct HistoryFile {
    weeks: Vec<WeekRecord>,
}

fn load_history() -> HistoryFile {
    match std::fs::read_to_string("history.json") {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HistoryFile::default(),
    }
}

fn save_history(h: &HistoryFile) {
    if let Ok(json) = serde_json::to_string_pretty(h) {
        let _ = std::fs::write("history.json", json);
    }
}

/// Ajoute (ou remplace si déjà présente) une semaine dans l'historique, et
/// sauvegarde le fichier. Ignore les semaines vides (aucune tâche comptée).
fn archive_week(history: &mut HistoryFile, record: WeekRecord) {
    if record.total == 0 {
        return;
    }
    if let Some(existing) = history.weeks.iter_mut().find(|w| w.week_start == record.week_start) {
        *existing = record;
    } else {
        history.weeks.push(record);
    }
    history.weeks.sort_by(|a, b| a.week_start.cmp(&b.week_start));
    if history.weeks.len() > 104 {
        let excess = history.weeks.len() - 104;
        history.weeks.drain(0..excess);
    }
    save_history(history);
}

fn summarize_categories(categories: &[Category], week_start: &str) -> WeekRecord {
    let mut total = 0usize;
    let mut done = 0usize;
    let mut categories_out = Vec::new();
    for c in categories {
        let t: usize = c.tasks.iter().filter(|t| t.count_in_progress).map(|t| t.total()).sum();
        let d: usize = c.tasks.iter().filter(|t| t.count_in_progress).map(|t| t.done()).sum();
        total += t;
        done += d;
        categories_out.push(CategorySummary { name: c.name.clone(), done: d, total: t });
    }
    let percent = if total > 0 { done as f32 / total as f32 * 100.0 } else { 0.0 };
    WeekRecord { week_start: week_start.to_string(), total, done, percent, categories: categories_out }
}

fn summarize_state(tasks: &HashMap<String, Vec<bool>>, raw: &[CategoryConfig], week_start: &str) -> WeekRecord {
    let cip = count_in_progress_map(raw);
    let mut cat_totals: HashMap<String, (usize, usize)> = HashMap::new();
    let mut total = 0usize;
    let mut done = 0usize;
    for (key, occs) in tasks {
        let counts = cip.get(key).copied().unwrap_or(true);
        if !counts {
            continue;
        }
        let t = occs.len();
        let d = occs.iter().filter(|x| **x).count();
        total += t;
        done += d;
        if let Some((cat, _task)) = key.split_once("::") {
            let e = cat_totals.entry(cat.to_string()).or_insert((0, 0));
            e.0 += d;
            e.1 += t;
        }
    }
    let categories_out = cat_totals.into_iter().map(|(name, (d, t))| CategorySummary { name, done: d, total: t }).collect();
    let percent = if total > 0 { done as f32 / total as f32 * 100.0 } else { 0.0 };
    WeekRecord { week_start: week_start.to_string(), total, done, percent, categories: categories_out }
}

// ============================================================
// prefs.json — préférence d'affichage (clair/sombre), persistante
// ============================================================

#[derive(Serialize, Deserialize)]
struct Prefs {
    dark_mode: bool,
}
impl Default for Prefs {
    fn default() -> Self {
        Prefs { dark_mode: true }
    }
}
fn load_prefs() -> Prefs {
    match std::fs::read_to_string("prefs.json") {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Prefs::default(),
    }
}
fn save_prefs(dark_mode: bool) {
    if let Ok(json) = serde_json::to_string_pretty(&Prefs { dark_mode }) {
        let _ = std::fs::write("prefs.json", json);
    }
}

// ============================================================
// Questionnaire de nouvelle semaine (tâches ask_include / ask_occurrences)
// ============================================================

enum DynamicKind {
    Include { default: bool },
    Occurrences { default: u32 },
}

struct DynamicTaskDef {
    category: String,
    task_name: String,
    kind: DynamicKind,
}

fn build_setup_defs(raw: &[CategoryConfig], prior: Option<&WeekChoices>) -> Vec<DynamicTaskDef> {
    let mut defs = Vec::new();
    for c in raw {
        for t in &c.tasks {
            let key = state_key(&c.name, &t.name);
            if t.ask_include {
                let default = match prior.and_then(|p| p.include.get(&key)).copied() {
                    Some(prev) => !prev,
                    None => true,
                };
                defs.push(DynamicTaskDef { category: c.name.clone(), task_name: t.name.clone(), kind: DynamicKind::Include { default } });
            } else if t.ask_occurrences {
                let default = prior.and_then(|p| p.occurrences.get(&key)).copied().unwrap_or(t.occurrences.max(1));
                defs.push(DynamicTaskDef { category: c.name.clone(), task_name: t.name.clone(), kind: DynamicKind::Occurrences { default } });
            }
        }
    }
    defs
}

struct WeekSetupState {
    defs: Vec<DynamicTaskDef>,
    include_answers: HashMap<String, bool>,
    occurrence_answers: HashMap<String, u32>,
}

fn new_week_setup_state(raw: &[CategoryConfig], prior: Option<&WeekChoices>) -> WeekSetupState {
    let defs = build_setup_defs(raw, prior);
    let mut include_answers = HashMap::new();
    let mut occurrence_answers = HashMap::new();
    for d in &defs {
        let key = state_key(&d.category, &d.task_name);
        match d.kind {
            DynamicKind::Include { default } => {
                include_answers.insert(key, default);
            }
            DynamicKind::Occurrences { default } => {
                occurrence_answers.insert(key, default);
            }
        }
    }
    WeekSetupState { defs, include_answers, occurrence_answers }
}

// ============================================================
// Éditeur de tâches intégré
// ============================================================

struct EditorState {
    categories: Vec<CategoryConfig>,
}

// ============================================================
// Chargement complet au démarrage
// ============================================================

struct LoadResult {
    raw_categories: Vec<CategoryConfig>,
    categories: Vec<Category>,
    current_choices: WeekChoices,
    warning: Option<String>,
    new_week_detected: bool,
    week_setup: Option<WeekSetupState>,
    history: HistoryFile,
    had_prior_state: bool,
}

fn load_all() -> LoadResult {
    let (raw_categories, warning) = match std::fs::read_to_string("config.toml") {
        Ok(content) => match toml::from_str::<ConfigFile>(&content) {
            Ok(cfg) => (cfg.categories, None),
            Err(e) => (fallback_raw_categories(), Some(format!("⚠ config.toml invalide : {e}"))),
        },
        Err(_) => (fallback_raw_categories(), Some("⚠ config.toml introuvable à côté de l'exécutable.".to_string())),
    };

    let today = Local::now().date_naive();
    let current_monday_str = monday_of(today).format("%Y-%m-%d").to_string();
    let state = load_state();

    let is_new_week = match &state.week_start {
        Some(saved) => saved != &current_monday_str,
        None => false,
    };

    let mut history = load_history();
    if is_new_week {
        if let Some(prev_week_start) = &state.week_start {
            let record = summarize_state(&state.tasks, &raw_categories, prev_week_start);
            archive_week(&mut history, record);
        }
    }

    let needs_setup = has_dynamic_tasks(&raw_categories) && (is_new_week || state.choices.is_none());

    let effective_choices = if needs_setup { None } else { state.choices.clone() };
    let mut categories = materialize_categories(&raw_categories, effective_choices.as_ref());

    if !is_new_week && !needs_setup {
        for c in &mut categories {
            for t in &mut c.tasks {
                if let Some(saved) = state.tasks.get(&state_key(&c.name, &t.name)) {
                    for (i, slot) in t.occurrences.iter_mut().enumerate() {
                        *slot = saved.get(i).copied().unwrap_or(false);
                    }
                }
            }
        }
    }

    let current_choices = effective_choices.unwrap_or_default();

    if !needs_setup {
        save_state(&categories, &current_choices);
    }

    let week_setup = if needs_setup { Some(new_week_setup_state(&raw_categories, state.choices.as_ref())) } else { None };
    let had_prior_state = state.week_start.is_some();

    LoadResult { raw_categories, categories, current_choices, warning, new_week_detected: is_new_week, week_setup, history, had_prior_state }
}

// ============================================================
// Paliers d'avance / retard (progressif, pas binaire)
// ============================================================

struct Tier {
    label: &'static str,
    icon: &'static str,
    color: egui::Color32,
}

fn progress_tier(delta: f32) -> Tier {
    if delta >= 20.0 {
        Tier { label: "TRÈS EN AVANCE", icon: "🚀", color: egui::Color32::from_rgb(0, 150, 60) }
    } else if delta >= 5.0 {
        Tier { label: "EN AVANCE", icon: "🚀", color: egui::Color32::from_rgb(70, 170, 100) }
    } else if delta > -5.0 {
        Tier { label: "DANS LE RYTHME", icon: "➡", color: egui::Color32::from_rgb(90, 140, 200) }
    } else if delta > -20.0 {
        Tier { label: "EN RETARD", icon: "⚠", color: egui::Color32::from_rgb(220, 150, 40) }
    } else {
        Tier { label: "TRÈS EN RETARD", icon: "⚠", color: egui::Color32::from_rgb(210, 60, 60) }
    }
}

// ============================================================
// Petit générateur pseudo-aléatoire (évite une dépendance externe)
// ============================================================

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 16) as u32
    }
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() % 1_000_000) as f32 / 1_000_000.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }
}

fn palette_color(i: u32) -> egui::Color32 {
    match i % 6 {
        0 => egui::Color32::from_rgb(255, 99, 132),
        1 => egui::Color32::from_rgb(54, 162, 235),
        2 => egui::Color32::from_rgb(255, 206, 86),
        3 => egui::Color32::from_rgb(75, 192, 192),
        4 => egui::Color32::from_rgb(153, 102, 255),
        _ => egui::Color32::from_rgb(255, 159, 64),
    }
}

/// Couleur d'accent attribuée à chaque catégorie (par position dans la
/// liste), utilisée pour la barre latérale de la carte, le badge d'icône
/// et le remplissage des cases à cocher de cette catégorie.
fn category_color(index: usize) -> egui::Color32 {
    const PALETTE: [egui::Color32; 8] = [
        egui::Color32::from_rgb(255, 107, 129), // rose corail
        egui::Color32::from_rgb(78, 205, 196),  // turquoise
        egui::Color32::from_rgb(255, 190, 92),  // ambre
        egui::Color32::from_rgb(108, 140, 255), // indigo
        egui::Color32::from_rgb(167, 139, 250), // violet
        egui::Color32::from_rgb(74, 222, 128),  // vert
        egui::Color32::from_rgb(255, 140, 97),  // corail
        egui::Color32::from_rgb(94, 234, 212),  // menthe
    ];
    PALETTE[index % PALETTE.len()]
}

// ============================================================
// Particules (pop de case cochée + confettis de fin de semaine)
// ============================================================

#[derive(Clone, Copy, PartialEq)]
enum ParticleShape {
    Circle,
    Square,
}

struct Particle {
    pos: egui::Pos2,
    vel: egui::Vec2,
    color: egui::Color32,
    radius: f32,
    born: Instant,
    lifetime: f32,
    gravity: f32,
    shape: ParticleShape,
    rotation: f32,
    rot_speed: f32,
}

// ============================================================
// App
// ============================================================

struct PrepaWeekApp {
    raw_categories: Vec<CategoryConfig>,
    categories: Vec<Category>,
    current_choices: WeekChoices,
    history: HistoryFile,

    show_reset_confirm: bool,
    week_setup: Option<WeekSetupState>,
    show_history: bool,
    editor: Option<EditorState>,
    config_warning: Option<String>,
    new_week_detected: bool,
    dark_mode: bool,
    theme_applied: bool,
    has_real_week_data: bool,

    particles: Vec<Particle>,
    rng: Rng,
    category_complete_since: HashMap<String, Instant>,
    prev_category_complete: HashMap<String, bool>,
    prev_week_complete: bool,
}

impl Default for PrepaWeekApp {
    fn default() -> Self {
        let loaded = load_all();
        let prefs = load_prefs();
        let seed = Instant::now().elapsed().as_nanos() as u64 ^ 0x9E37_79B9_7F4A_7C15;
        Self {
            raw_categories: loaded.raw_categories,
            categories: loaded.categories,
            current_choices: loaded.current_choices,
            history: loaded.history,
            show_reset_confirm: false,
            week_setup: loaded.week_setup,
            show_history: false,
            editor: None,
            config_warning: loaded.warning,
            new_week_detected: loaded.new_week_detected,
            dark_mode: prefs.dark_mode,
            theme_applied: false,
            has_real_week_data: loaded.had_prior_state,
            particles: Vec::new(),
            rng: Rng::new(seed.max(1)),
            category_complete_since: HashMap::new(),
            prev_category_complete: HashMap::new(),
            prev_week_complete: false,
        }
    }
}

impl PrepaWeekApp {
    fn current_week_start_str(&self) -> String {
        monday_of(Local::now().date_naive()).format("%Y-%m-%d").to_string()
    }

    fn reset_week(&mut self) {
        if self.has_real_week_data {
            let week_start_str = self.current_week_start_str();
            let record = summarize_categories(&self.categories, &week_start_str);
            archive_week(&mut self.history, record);
        }

        for cat in &mut self.categories {
            for task in &mut cat.tasks {
                for occ in task.occurrences.iter_mut() {
                    *occ = false;
                }
            }
        }
        save_state(&self.categories, &self.current_choices);
        self.particles.clear();
        self.category_complete_since.clear();
        self.prev_category_complete.clear();
        self.prev_week_complete = false;
        self.has_real_week_data = true;
    }

    fn open_new_week_flow(&mut self) {
        if has_dynamic_tasks(&self.raw_categories) {
            self.week_setup = Some(new_week_setup_state(&self.raw_categories, Some(&self.current_choices)));
        } else {
            self.show_reset_confirm = true;
        }
    }

    fn finalize_week_setup(&mut self) {
        if let Some(setup) = self.week_setup.take() {
            if self.has_real_week_data {
                let week_start_str = self.current_week_start_str();
                let record = summarize_categories(&self.categories, &week_start_str);
                archive_week(&mut self.history, record);
            }

            let choices = WeekChoices { include: setup.include_answers, occurrences: setup.occurrence_answers };
            self.categories = materialize_categories(&self.raw_categories, Some(&choices));
            self.current_choices = choices;
            save_state(&self.categories, &self.current_choices);
            self.particles.clear();
            self.category_complete_since.clear();
            self.prev_category_complete.clear();
            self.prev_week_complete = false;
            self.new_week_detected = false;
            self.has_real_week_data = true;
        }
    }

    fn spawn_check_burst(&mut self, center: egui::Pos2) {
        for _ in 0..12 {
            let angle = self.rng.range(0.0, std::f32::consts::TAU);
            let speed = self.rng.range(60.0, 160.0);
            let vel = egui::vec2(angle.cos(), angle.sin()) * speed;
            let color_idx = self.rng.next_u32();
            let shape = if self.rng.next_f32() > 0.5 { ParticleShape::Circle } else { ParticleShape::Square };
            self.particles.push(Particle {
                pos: center,
                vel,
                color: palette_color(color_idx),
                radius: self.rng.range(1.5, 3.2),
                born: Instant::now(),
                lifetime: self.rng.range(0.35, 0.6),
                gravity: 220.0,
                shape,
                rotation: self.rng.range(0.0, std::f32::consts::TAU),
                rot_speed: self.rng.range(-6.0, 6.0),
            });
        }
    }

    fn spawn_confetti(&mut self, width: f32) {
        let width = if width > 10.0 { width } else { 528.0 };
        for _ in 0..110 {
            let x = self.rng.range(0.0, width);
            let y = self.rng.range(-40.0, -5.0);
            let vx = self.rng.range(-40.0, 40.0);
            let vy = self.rng.range(40.0, 120.0);
            let color_idx = self.rng.next_u32();
            let shape = if self.rng.next_f32() > 0.45 { ParticleShape::Square } else { ParticleShape::Circle };
            self.particles.push(Particle {
                pos: egui::pos2(x, y),
                vel: egui::vec2(vx, vy),
                color: palette_color(color_idx),
                radius: self.rng.range(2.5, 4.8),
                born: Instant::now(),
                lifetime: self.rng.range(2.5, 4.0),
                gravity: 90.0,
                shape,
                rotation: self.rng.range(0.0, std::f32::consts::TAU),
                rot_speed: self.rng.range(-4.0, 4.0),
            });
        }
    }

    /// % de la semaine précédente (immédiatement avant la semaine en
    /// cours), si elle est archivée dans l'historique.
    fn previous_week_percent(&self, current_monday: NaiveDate) -> Option<f32> {
        let prev = (current_monday - ChronoDuration::days(7)).format("%Y-%m-%d").to_string();
        self.history.weeks.iter().find(|w| w.week_start == prev).map(|w| w.percent)
    }

    /// Nombre de semaines consécutives (immédiatement avant la semaine en
    /// cours, sans trou) à au moins `threshold` %.
    fn current_streak(&self, current_monday: NaiveDate, threshold: f32) -> u32 {
        let mut sorted: Vec<&WeekRecord> = self.history.weeks.iter().collect();
        sorted.sort_by(|a, b| b.week_start.cmp(&a.week_start));
        let mut streak = 0u32;
        let mut expected = current_monday - ChronoDuration::days(7);
        for w in sorted {
            let expected_str = expected.format("%Y-%m-%d").to_string();
            if w.week_start == expected_str && w.percent >= threshold {
                streak += 1;
                expected -= ChronoDuration::days(7);
            } else {
                break;
            }
        }
        streak
    }

    fn save_editor(&mut self) {
        if let Some(editor) = self.editor.take() {
            let raw = editor.categories;
            let _ = save_config(&raw);

            // Si le nombre de cases par défaut d'une tâche "nombre
            // variable" (ask_occurrences) a vraiment changé ici, on
            // applique ce nouveau nombre tout de suite à la semaine en
            // cours — sinon l'ancien choix mémorisé la semaine dernière
            // primerait silencieusement et la modification semblerait ne
            // rien faire. On ne touche au choix en cours que si la valeur
            // a réellement changé, pour ne pas écraser par erreur un choix
            // hebdomadaire différent en enregistrant l'éditeur sans avoir
            // modifié cette tâche précise.
            let old_occurrences: HashMap<String, u32> = self
                .raw_categories
                .iter()
                .flat_map(|c| c.tasks.iter().map(move |t| (state_key(&c.name, &t.name), t.occurrences)))
                .collect();
            for c in &raw {
                for t in &c.tasks {
                    if t.ask_occurrences {
                        let key = state_key(&c.name, &t.name);
                        if old_occurrences.get(&key).copied() != Some(t.occurrences) {
                            self.current_choices.occurrences.insert(key, t.occurrences.max(1));
                        }
                    }
                }
            }

            self.raw_categories = raw;

            let mut new_categories = materialize_categories(&self.raw_categories, Some(&self.current_choices));
            let old_state: HashMap<String, Vec<bool>> = self
                .categories
                .iter()
                .flat_map(|c| c.tasks.iter().map(move |t| (state_key(&c.name, &t.name), t.occurrences.clone())))
                .collect();
            for c in &mut new_categories {
                for t in &mut c.tasks {
                    if let Some(saved) = old_state.get(&state_key(&c.name, &t.name)) {
                        for (i, slot) in t.occurrences.iter_mut().enumerate() {
                            *slot = saved.get(i).copied().unwrap_or(false);
                        }
                    }
                }
            }
            self.categories = new_categories;
            save_state(&self.categories, &self.current_choices);
        }
    }
}

const DAY_NAMES: [&str; 7] = [
    "LUNDI", "MARDI", "MERCREDI", "JEUDI", "VENDREDI", "SAMEDI", "DIMANCHE",
];
const MONTH_NAMES: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août",
    "septembre", "octobre", "novembre", "décembre",
];

const CATEGORY_PULSE_DURATION: f32 = 1.2;
const STREAK_THRESHOLD: f32 = 80.0;

/// Petit contrôle ➖ [valeur] ➕ pour ajuster un nombre de cases sans avoir
/// à taper ou glisser — plus rapide et plus adapté au tactile qu'un
/// DragValue. Retourne true si la valeur a changé.
fn stepper(ui: &mut egui::Ui, value: &mut u32, min: u32, max: u32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let minus = egui::Button::new(egui::RichText::new("➖").size(15.0)).min_size(egui::vec2(28.0, 24.0));
        if ui.add_enabled(*value > min, minus).clicked() {
            *value -= 1;
            changed = true;
        }
        ui.label(egui::RichText::new(format!("{value}")).strong().size(16.0));
        let plus = egui::Button::new(egui::RichText::new("➕").size(15.0)).min_size(egui::vec2(28.0, 24.0));
        if ui.add_enabled(*value < max, plus).clicked() {
            *value += 1;
            changed = true;
        }
    });
    changed
}

/// Palette et style personnalisés (couleurs plus riches, coins arrondis,
/// espacement plus aéré) appliqués par-dessus les thèmes clair/sombre de
/// base d'egui.
fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };

    if dark {
        visuals.window_fill = egui::Color32::from_rgb(19, 21, 29);
        visuals.panel_fill = egui::Color32::from_rgb(19, 21, 29);
        visuals.extreme_bg_color = egui::Color32::from_rgb(13, 14, 20);
        visuals.faint_bg_color = egui::Color32::from_rgb(27, 30, 41);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(27, 30, 41);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(33, 37, 50);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(45, 50, 67);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(53, 58, 78);
        visuals.selection.bg_fill = egui::Color32::from_rgb(108, 140, 255);
        visuals.hyperlink_color = egui::Color32::from_rgb(120, 150, 255);
        visuals.window_shadow.color = egui::Color32::from_black_alpha(90);
    } else {
        visuals.window_fill = egui::Color32::from_rgb(250, 250, 253);
        visuals.panel_fill = egui::Color32::from_rgb(250, 250, 253);
        visuals.extreme_bg_color = egui::Color32::from_rgb(235, 236, 242);
        visuals.faint_bg_color = egui::Color32::from_rgb(240, 241, 247);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(240, 241, 247);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(228, 230, 239);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(216, 219, 233);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(203, 207, 226);
        visuals.selection.bg_fill = egui::Color32::from_rgb(108, 140, 255);
        visuals.hyperlink_color = egui::Color32::from_rgb(80, 100, 215);
        visuals.window_shadow.color = egui::Color32::from_black_alpha(35);
    }

    let rounding = egui::Rounding::same(10.0);
    visuals.window_rounding = egui::Rounding::same(14.0);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.active.rounding = rounding;
    visuals.widgets.open.rounding = rounding;
    visuals.menu_rounding = rounding;

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
    )
}

/// Case à cocher personnalisée : carré arrondi qui se remplit d'une
/// couleur d'accent avec une coche qui se dessine en fondu, plus
/// satisfaisante qu'une case standard. `id` doit être unique par case
/// (catégorie + tâche + numéro d'occurrence).
fn animated_checkbox(ui: &mut egui::Ui, id: egui::Id, checked: &mut bool, accent: egui::Color32) -> egui::Response {
    let size = egui::vec2(24.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let just_clicked = response.clicked();
    if just_clicked {
        *checked = !*checked;
    }
    if response.hovered() {
        ui.ctx().output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }

    let anim = ui.ctx().animate_bool_with_time(id, *checked, 0.22);
    let rounding = egui::Rounding::same(7.0);
    let empty_bg = ui.visuals().widgets.inactive.bg_fill;
    let border = lerp_color(accent.gamma_multiply(0.55), accent, if response.hovered() { 1.0 } else { anim.max(0.35) });

    let painter = ui.painter();
    painter.rect_filled(rect, rounding, lerp_color(empty_bg, accent, anim));
    painter.rect_stroke(rect, rounding, egui::Stroke::new(1.6, border));

    if anim > 0.02 {
        let c = rect.center();
        let s = rect.width() * 0.28;
        let p1 = c + egui::vec2(-s, -0.05 * s);
        let p2 = c + egui::vec2(-0.25 * s, 0.62 * s);
        let p3 = c + egui::vec2(1.05 * s, -0.75 * s);
        let alpha = (anim * 255.0) as u8;
        let stroke = egui::Stroke::new(2.6, egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha));
        painter.line_segment([p1, p2], stroke);
        painter.line_segment([p2, p3], stroke);
    }

    response
}

/// Barre de progression personnalisée : remplissage plein arrondi dans la
/// couleur d'accent donnée, reflet brillant, et un chatoiement animé qui
/// balaie doucement la barre en continu.
fn fancy_progress_bar(ui: &mut egui::Ui, progress: f32, color: egui::Color32, height: f32) {
    let desired_width = ui.available_width();
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(desired_width, height), egui::Sense::hover());
    let rounding = egui::Rounding::same(height / 2.0);
    let painter = ui.painter();

    painter.rect_filled(rect, rounding, ui.visuals().extreme_bg_color);

    let progress = progress.clamp(0.0, 1.0);
    if progress > 0.004 {
        let fill_w = (rect.width() * progress).max(height).min(rect.width());
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, rounding, color);

        let shine_h = (rect.height() * 0.4).max(2.0);
        if fill_rect.width() > 4.0 {
            let shine_rect = egui::Rect::from_min_size(
                fill_rect.min + egui::vec2(2.0, 1.5),
                egui::vec2(fill_rect.width() - 4.0, shine_h),
            );
            painter.rect_filled(shine_rect, egui::Rounding::same(shine_h / 2.0), egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40));
        }

        let t = ui.input(|i| i.time) as f32;
        let phase = (t % 2.6) / 2.6;
        let sweep_x = rect.left() + phase * (rect.width() + 70.0) - 45.0;
        let sweep_rect = egui::Rect::from_min_max(egui::pos2(sweep_x, rect.top()), egui::pos2(sweep_x + 36.0, rect.bottom()));
        let clipped = painter.with_clip_rect(fill_rect);
        clipped.rect_filled(sweep_rect, rounding, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 32));
    }
}

impl eframe::App for PrepaWeekApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            apply_theme(ctx, self.dark_mode);
            self.theme_applied = true;
        }

        let now = Local::now();
        let today = now.date_naive();
        let weekday_index = today.weekday().num_days_from_monday();
        let day_name = DAY_NAMES[weekday_index as usize];
        let month_name = MONTH_NAMES[today.month0() as usize];

        let monday = monday_of(today);
        let sunday = monday + ChronoDuration::days(6);
        let day_number = weekday_index + 1;
        let days_remaining = 7 - day_number;

        let monday_midnight = monday.and_hms_opt(0, 0, 0).unwrap();
        let elapsed = now.naive_local() - monday_midnight;
        let elapsed_hours = (elapsed.num_minutes() as f32 / 60.0).clamp(0.0, 168.0);
        let time_elapsed_pct = elapsed_hours / 168.0 * 100.0;
        let hour_of_day = now.hour();
        let minute_of_day = now.minute();

        let now_instant = Instant::now();
        let mut total = 0usize;
        let mut done = 0usize;
        for c in &self.categories {
            let mut cat_total = 0usize;
            let mut cat_done = 0usize;
            for t in &c.tasks {
                if t.count_in_progress {
                    total += t.total();
                    done += t.done();
                    cat_total += t.total();
                    cat_done += t.done();
                }
            }
            let is_complete = cat_total > 0 && cat_done == cat_total;
            let was_complete = self.prev_category_complete.get(&c.name).copied().unwrap_or(false);
            if is_complete && !was_complete {
                self.category_complete_since.insert(c.name.clone(), now_instant);
            }
            if !is_complete {
                self.category_complete_since.remove(&c.name);
            }
            self.prev_category_complete.insert(c.name.clone(), is_complete);
        }

        let progress = if total > 0 { done as f32 / total as f32 } else { 0.0 };
        let progress_pct = progress * 100.0;
        let delta = progress_pct - time_elapsed_pct;

        // --- Calcul secondaire : tâches "avant vendredi soir" (lundi →
        //     vendredi, hors week-end). Purement indicatif : le calcul
        //     principal ci-dessus (7 jours, toutes les tâches) reste seul
        //     utilisé pour l'historique et la détection de semaine complète.
        let weekday_elapsed_hours = (elapsed.num_minutes() as f32 / 60.0).clamp(0.0, 120.0);
        let weekday_time_elapsed_pct = weekday_elapsed_hours / 120.0 * 100.0;
        let mut weekday_total = 0usize;
        let mut weekday_done = 0usize;
        for c in &self.categories {
            for t in &c.tasks {
                if t.count_in_progress && t.weekday_only {
                    weekday_total += t.total();
                    weekday_done += t.done();
                }
            }
        }
        let weekday_progress = if weekday_total > 0 { weekday_done as f32 / weekday_total as f32 } else { 0.0 };
        let weekday_progress_pct = weekday_progress * 100.0;
        let weekday_delta = weekday_progress_pct - weekday_time_elapsed_pct;
        let weekday_displayed_progress = ctx.animate_value_with_time(egui::Id::new("weekday_progress_bar"), weekday_progress, 0.4);

        let week_complete = total > 0 && done == total;
        if week_complete && !self.prev_week_complete {
            self.spawn_confetti(ctx.screen_rect().width());
        }
        self.prev_week_complete = week_complete;

        let category_complete_since = self.category_complete_since.clone();

        let displayed_progress = ctx.animate_value_with_time(egui::Id::new("progress_bar"), progress, 0.4);
        let complete_anim = ctx.animate_bool_with_time(egui::Id::new("week_complete_text"), week_complete, 0.4);

        let mut changed = false;
        let mut burst_positions: Vec<egui::Pos2> = Vec::new();

        let prev_week_pct = self.previous_week_percent(monday);
        let streak = self.current_streak(monday, STREAK_THRESHOLD);

        // --- Barre du haut : titre + historique + éditeur + clair/sombre ---
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("PrepaWeek").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let icon = if self.dark_mode { "☀" } else { "🌙" };
                    let hover = if self.dark_mode { "Passer en mode clair" } else { "Passer en mode sombre" };
                    if ui.button(icon).on_hover_text(hover).clicked() {
                        self.dark_mode = !self.dark_mode;
                        apply_theme(ctx, self.dark_mode);
                        save_prefs(self.dark_mode);
                    }
                    if ui.button("📊").on_hover_text("Historique").clicked() {
                        self.show_history = !self.show_history;
                    }
                    if ui.button("✏").on_hover_text("Modifier mes tâches").clicked() {
                        self.editor = Some(EditorState { categories: self.raw_categories.clone() });
                        self.show_history = false;
                    }
                });
            });
            ui.add_space(2.0);
        });

        // --- Barre du bas : message de fin de semaine + reset ---
        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.add_space(6.0);
            if complete_anim > 0.01 {
                ui.vertical_centered(|ui| {
                    let alpha = (complete_anim * 255.0) as u8;
                    let size = 13.0 + complete_anim * 5.0;
                    ui.label(
                        egui::RichText::new("🎉 SEMAINE TERMINÉE ! 100 %")
                            .size(size)
                            .strong()
                            .color(egui::Color32::from_rgba_unmultiplied(230, 170, 0, alpha)),
                    );
                    ui.add_space(4.0);
                });
            }
            ui.vertical_centered(|ui| {
                if ui.button("🔄 Nouvelle semaine").clicked() {
                    self.open_new_week_flow();
                }
            });
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(warning) = &self.config_warning {
                ui.colored_label(egui::Color32::from_rgb(200, 60, 60), warning);
                ui.add_space(4.0);
            }
            if self.new_week_detected && self.week_setup.is_none() {
                ui.colored_label(
                    egui::Color32::from_rgb(90, 140, 200),
                    "🔄 Nouvelle semaine détectée : les cases ont été remises à zéro.",
                );
                ui.add_space(4.0);
            }

            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                ui.heading("📚 MA SEMAINE DE PRÉPA");
                ui.add_space(4.0);
                ui.label(format!("{} {} {} {}", day_name, today.day(), month_name, today.year()));
                ui.label(format!(
                    "Semaine du {} au {} {}",
                    monday.day(), sunday.day(), MONTH_NAMES[sunday.month0() as usize]
                ));
                ui.add_space(2.0);
                ui.label(format!(
                    "Jour {} / 7   •   ⏳ {} jour{} restant{}",
                    day_number, days_remaining,
                    if days_remaining > 1 { "s" } else { "" },
                    if days_remaining > 1 { "s" } else { "" },
                ));
                ui.label(
                    egui::RichText::new(format!(
                        "🕐 {:02}h{:02} / 24h   •   {:.0}h / 168h cette semaine",
                        hour_of_day, minute_of_day, elapsed_hours
                    ))
                    .small()
                    .weak(),
                );
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);

            ui.label(egui::RichText::new(format!("Progression — {:.0} %", displayed_progress * 100.0)).strong());
            fancy_progress_bar(ui, displayed_progress, egui::Color32::from_rgb(108, 140, 255), 18.0);
            ui.label(format!("{} / {} cases cochées", done, total));

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label(format!("Temps écoulé : {:.0} %", time_elapsed_pct));
                ui.label(format!("   Tâches réalisées : {:.0} %", progress_pct));
            });
            if total > 0 {
                let tier = progress_tier(delta);
                ui.colored_label(tier.color, format!("{} {} ({:+.0} %)", tier.icon, tier.label, delta));
            }

            // --- Comparaison avec la semaine précédente + série ---
            if let Some(prev) = prev_week_pct {
                let diff = progress_pct - prev;
                let color = if diff > 0.5 {
                    egui::Color32::from_rgb(70, 170, 100)
                } else if diff < -0.5 {
                    egui::Color32::from_rgb(210, 60, 60)
                } else {
                    egui::Color32::GRAY
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(format!("Semaine précédente : {:.0} %", prev));
                    ui.colored_label(color, format!("({:+.0} % vs cette semaine)", diff));
                });
            }
            if streak >= 2 {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 130, 30),
                    format!("🔥 {streak} semaines consécutives à +{:.0} %", STREAK_THRESHOLD),
                );
            }

            // --- Barre secondaire : tâches "avant vendredi soir" ---
            if weekday_total > 0 {
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(egui::RichText::new(format!("📅 Avant vendredi soir (lundi ➡ vendredi) — {:.0} %", weekday_displayed_progress * 100.0)).strong());
                fancy_progress_bar(ui, weekday_displayed_progress, egui::Color32::from_rgb(255, 190, 92), 14.0);
                ui.label(format!("{} / {} cases (hors week-end)", weekday_done, weekday_total));
                let wtier = progress_tier(weekday_delta);
                ui.colored_label(wtier.color, format!("{} {} ({:+.0} %)", wtier.icon, wtier.label, weekday_delta));
            }

            ui.add_space(10.0);
            ui.separator();

            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (idx, cat) in self.categories.iter_mut().enumerate() {
                    ui.add_space(10.0);

                    let cat_done: usize = cat.tasks.iter().filter(|t| t.count_in_progress).map(|t| t.done()).sum();
                    let cat_total: usize = cat.tasks.iter().filter(|t| t.count_in_progress).map(|t| t.total()).sum();
                    let accent = category_color(idx);
                    let cat_name = cat.name.clone();

                    let pulse_alpha = category_complete_since
                        .get(&cat.name)
                        .map(|inst| {
                            let age = now_instant.duration_since(*inst).as_secs_f32();
                            (1.0 - age / CATEGORY_PULSE_DURATION).clamp(0.0, 1.0)
                        })
                        .unwrap_or(0.0);
                    let base_card_bg = ui.visuals().faint_bg_color;
                    let card_bg = if pulse_alpha > 0.0 {
                        lerp_color(base_card_bg, egui::Color32::from_rgb(80, 200, 120), pulse_alpha * 0.6)
                    } else {
                        base_card_bg
                    };

                    let frame = egui::Frame::none()
                        .fill(card_bg)
                        .rounding(egui::Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(12.0, 10.0));

                    let card_resp = frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let badge_size = 28.0;
                            let (badge_rect, _) = ui.allocate_exact_size(egui::vec2(badge_size, badge_size), egui::Sense::hover());
                            ui.painter().circle_filled(badge_rect.center(), badge_size / 2.0, accent);
                            ui.painter().text(
                                badge_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &cat.icon,
                                egui::FontId::proportional(15.0),
                                egui::Color32::WHITE,
                            );
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(cat.name.to_uppercase()).strong().size(16.0));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if cat_total > 0 && cat_done == cat_total {
                                    ui.label(egui::RichText::new("✅").size(15.0));
                                } else if cat_total > 0 {
                                    ui.label(egui::RichText::new(format!("{cat_done}/{cat_total}")).small().weak());
                                }
                            });
                        });

                        if cat.tasks.is_empty() {
                            ui.label(egui::RichText::new("(rien cette semaine)").small().weak());
                        }

                        ui.add_space(4.0);

                        for task in &mut cat.tasks {
                            ui.horizontal_wrapped(|ui| {
                                if task.occurrences.len() == 1 {
                                    let id = egui::Id::new(format!("chk::{cat_name}::{}::0", task.name));
                                    let resp = animated_checkbox(ui, id, &mut task.occurrences[0], accent);
                                    if resp.clicked() {
                                        changed = true;
                                        if task.occurrences[0] {
                                            burst_positions.push(resp.rect.center());
                                        }
                                    }
                                    ui.add_space(6.0);
                                    ui.label(&task.name);
                                } else {
                                    ui.label(&task.name);
                                    ui.add_space(6.0);
                                    for (i, occ) in task.occurrences.iter_mut().enumerate() {
                                        let id = egui::Id::new(format!("chk::{cat_name}::{}::{i}", task.name));
                                        let resp = animated_checkbox(ui, id, occ, accent);
                                        if resp.clicked() {
                                            changed = true;
                                            if *occ {
                                                burst_positions.push(resp.rect.center());
                                            }
                                        }
                                    }
                                }
                                if !task.count_in_progress {
                                    ui.label(egui::RichText::new("(bonus)").small().weak());
                                }
                            });
                        }
                    });

                    let bar_rect = egui::Rect::from_min_size(
                        card_resp.response.rect.min,
                        egui::vec2(4.0, card_resp.response.rect.height()),
                    );
                    ui.painter().rect_filled(
                        bar_rect,
                        egui::Rounding { nw: 12.0, ne: 0.0, sw: 12.0, se: 0.0 },
                        accent,
                    );
                }
                ui.add_space(10.0);
            });
        });

        for pos in burst_positions {
            self.spawn_check_burst(pos);
        }

        if changed {
            save_state(&self.categories, &self.current_choices);
        }

        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        self.particles.retain_mut(|p| {
            p.vel.y += p.gravity * dt;
            p.pos += p.vel * dt;
            p.rotation += p.rot_speed * dt;
            let age = now_instant.duration_since(p.born).as_secs_f32();
            age < p.lifetime
        });
        if !self.particles.is_empty() {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("particles_layer")));
            for p in &self.particles {
                let age = now_instant.duration_since(p.born).as_secs_f32();
                let life_frac = (age / p.lifetime).clamp(0.0, 1.0);
                let alpha = ((1.0 - life_frac) * 255.0) as u8;
                let c = p.color;
                let faded = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha);
                match p.shape {
                    ParticleShape::Circle => {
                        painter.circle_filled(p.pos, p.radius, faded);
                    }
                    ParticleShape::Square => {
                        let r = p.radius;
                        let corners = [
                            egui::vec2(-r, -r),
                            egui::vec2(r, -r),
                            egui::vec2(r, r),
                            egui::vec2(-r, r),
                        ];
                        let (sin, cos) = p.rotation.sin_cos();
                        let points: Vec<egui::Pos2> = corners
                            .iter()
                            .map(|c| p.pos + egui::vec2(c.x * cos - c.y * sin, c.x * sin + c.y * cos))
                            .collect();
                        painter.add(egui::Shape::convex_polygon(points, faded, egui::Stroke::NONE));
                    }
                }
            }
        }

        // Rafraîchissement continu : le chatoiement des barres de
        // progression anime en permanence, donc on redessine à chaque
        // frame plutôt que seulement quand une particule/pulse est active.
        ctx.request_repaint();

        // --- Fenêtre : confirmation simple (pas de tâches dynamiques) ---
        if self.show_reset_confirm {
            egui::Window::new("Commencer une nouvelle semaine ?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Les cases actuelles seront remises à zéro (la semaine est archivée dans l'historique).");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Annuler").clicked() {
                            self.show_reset_confirm = false;
                        }
                        if ui.button("Nouvelle semaine").clicked() {
                            self.reset_week();
                            self.show_reset_confirm = false;
                        }
                    });
                });
        }

        // --- Fenêtre : questionnaire de nouvelle semaine ---
        let mut setup_action: Option<bool> = None;
        if let Some(setup) = &mut self.week_setup {
            egui::Window::new("📅 Nouvelle semaine")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_width(320.0);
                    ui.label("Les cases de la semaine vont être remises à zéro (la semaine actuelle est archivée dans l'historique).");
                    ui.add_space(8.0);
                    if !setup.defs.is_empty() {
                        ui.label(egui::RichText::new("Pour cette semaine :").strong());
                        ui.add_space(4.0);
                        for def in &setup.defs {
                            let key = state_key(&def.category, &def.task_name);
                            match def.kind {
                                DynamicKind::Include { .. } => {
                                    if let Some(val) = setup.include_answers.get_mut(&key) {
                                        ui.checkbox(val, format!("{} — {}", def.category, def.task_name));
                                    }
                                }
                                DynamicKind::Occurrences { .. } => {
                                    ui.label(format!("{} — {} :", def.category, def.task_name));
                                    if let Some(val) = setup.occurrence_answers.get_mut(&key) {
                                        stepper(ui, val, 0, 14);
                                    }
                                    ui.add_space(4.0);
                                }
                            }
                        }
                        ui.add_space(8.0);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Annuler").clicked() {
                            setup_action = Some(false);
                        }
                        if ui.button("Commencer la semaine").clicked() {
                            setup_action = Some(true);
                        }
                    });
                });
        }
        match setup_action {
            Some(true) => self.finalize_week_setup(),
            Some(false) => self.week_setup = None,
            None => {}
        }

        // --- Fenêtre : historique ---
        if self.show_history {
            let mut close = false;
            egui::Window::new("📊 Historique")
                .collapsible(false)
                .resizable(true)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_width(340.0);
                    if self.history.weeks.is_empty() {
                        ui.label("Pas encore d'historique — termine ta première semaine !");
                    } else {
                        if streak >= 2 {
                            ui.colored_label(
                                egui::Color32::from_rgb(230, 130, 30),
                                format!("🔥 {streak} semaines consécutives à +{:.0} %", STREAK_THRESHOLD),
                            );
                            ui.add_space(4.0);
                        }
                        if let Some(prev) = prev_week_pct {
                            ui.label(format!(
                                "Cette semaine : {:.0} %   •   Semaine précédente : {:.0} %",
                                progress_pct, prev
                            ));
                            ui.add_space(4.0);
                        }
                        ui.separator();
                        ui.add_space(4.0);
                        let mut week_to_delete: Option<String> = None;
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for w in self.history.weeks.iter().rev() {
                                if let Ok(d) = NaiveDate::parse_from_str(&w.week_start, "%Y-%m-%d") {
                                    let iso_week = d.iso_week().week();
                                    ui.horizontal(|ui| {
                                        ui.label(format!("Semaine {iso_week}"));
                                        ui.add(
                                            egui::ProgressBar::new((w.percent / 100.0).clamp(0.0, 1.0))
                                                .text(format!("{:.0} %", w.percent))
                                                .desired_width(160.0),
                                        );
                                        if ui
                                            .small_button("🗑")
                                            .on_hover_text("Supprimer cette semaine de l'historique (ex. erreur ou test)")
                                            .clicked()
                                        {
                                            week_to_delete = Some(w.week_start.clone());
                                        }
                                    });
                                }
                            }
                        });
                        if let Some(ws) = week_to_delete {
                            self.history.weeks.retain(|w| w.week_start != ws);
                            save_history(&self.history);
                        }
                    }
                    ui.add_space(8.0);
                    if ui.button("Fermer").clicked() {
                        close = true;
                    }
                });
            if close {
                self.show_history = false;
            }
        }

        // --- Fenêtre : éditeur de tâches ---
        let mut editor_action: Option<bool> = None;
        if let Some(editor) = &mut self.editor {
            egui::Window::new("✏ Modifier mes tâches")
                .collapsible(false)
                .resizable(true)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_max_width(380.0);
                    ui.label(
                        egui::RichText::new("Icônes qui s'affichent bien : 📚 📐 ⚛ 💻 📖 📝 🏠 🏃 🔤 💡 🎓 🎯 ⭐ 🔥 …")
                            .small()
                            .weak(),
                    );
                    ui.add_space(6.0);

                    let mut remove_cat: Option<usize> = None;
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for (ci, cat) in editor.categories.iter_mut().enumerate() {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut cat.icon).desired_width(28.0));
                                    ui.add(egui::TextEdit::singleline(&mut cat.name).desired_width(160.0));
                                    if ui.button("🗑").on_hover_text("Supprimer la catégorie").clicked() {
                                        remove_cat = Some(ci);
                                    }
                                });

                                let mut remove_task: Option<usize> = None;
                                for (ti, task) in cat.tasks.iter_mut().enumerate() {
                                    ui.separator();
                                    ui.add(egui::TextEdit::singleline(&mut task.name).desired_width(300.0));
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("cases :").on_hover_text(
                                            "Nombre de cases par défaut. Reste modifiable même si \"nombre \
                                             variable\" est coché ci-dessous : la modification s'applique \
                                             immédiatement à la semaine en cours, sans attendre la prochaine \
                                             question hebdomadaire.",
                                        );
                                        stepper(ui, &mut task.occurrences, 1, 14);

                                        let mut is_bonus = !task.count_in_progress;
                                        if ui
                                            .checkbox(&mut is_bonus, "bonus")
                                            .on_hover_text(
                                                "Coché : cette tâche reste visible et cochable, mais ne compte ni \
                                                 dans la barre de progression ni dans le comparatif avec le temps \
                                                 écoulé.\n\
                                                 Décoché : la tâche compte normalement dans l'avancement.",
                                            )
                                            .changed()
                                        {
                                            task.count_in_progress = !is_bonus;
                                        }

                                        ui.checkbox(&mut task.ask_include, "demander chaque semaine")
                                            .on_hover_text(
                                                "Coché : à chaque nouvelle semaine, l'appli demande si cette tâche \
                                                 est incluse ou non cette semaine-là (ex. vocabulaire anglais, une \
                                                 semaine sur deux).\n\
                                                 Décoché : la tâche est toujours incluse.",
                                            );

                                        ui.checkbox(&mut task.ask_occurrences, "nombre variable")
                                            .on_hover_text(
                                                "Coché : à chaque nouvelle semaine, l'appli demande combien de cases \
                                                 prévoir pour cette tâche cette semaine-là (ex. nombre de séances de \
                                                 sport visées), au lieu du nombre fixe défini par \"cases\".\n\
                                                 Décoché : le nombre de cases reste toujours celui défini ci-dessus.",
                                            );

                                        ui.checkbox(&mut task.weekday_only, "avant vendredi")
                                            .on_hover_text(
                                                "Coché : cette tâche est aussi suivie dans une barre de progression \
                                                 séparée \"avant vendredi soir\" (lundi ➡ vendredi, hors week-end), \
                                                 en plus de la progression générale sur les 7 jours.\n\
                                                 Décoché : cette tâche ne compte que dans la progression générale.",
                                            );

                                        if ui.button("🗑").on_hover_text("Supprimer la tâche").clicked() {
                                            remove_task = Some(ti);
                                        }
                                    });
                                }
                                if let Some(i) = remove_task {
                                    cat.tasks.remove(i);
                                }
                                ui.add_space(4.0);
                                if ui.button("➕ Ajouter une tâche").clicked() {
                                    cat.tasks.push(TaskConfig {
                                        name: "Nouvelle tâche".to_string(),
                                        occurrences: 1,
                                        count_in_progress: true,
                                        ask_include: false,
                                        ask_occurrences: false,
                                        weekday_only: false,
                                    });
                                }
                            });
                            ui.add_space(6.0);
                        }
                    });
                    if let Some(i) = remove_cat {
                        editor.categories.remove(i);
                    }
                    if ui.button("➕ Ajouter une catégorie").clicked() {
                        editor.categories.push(CategoryConfig { name: "Nouvelle catégorie".to_string(), icon: "⭐".to_string(), tasks: vec![] });
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Annuler").clicked() {
                            editor_action = Some(false);
                        }
                        if ui.button("Enregistrer").clicked() {
                            editor_action = Some(true);
                        }
                    });
                });
        }
        match editor_action {
            Some(true) => self.save_editor(),
            Some(false) => self.editor = None,
            None => {}
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([528.0, 816.0])
            .with_min_inner_size([432.0, 576.0]),
        ..Default::default()
    };

    eframe::run_native(
        "PrepaWeek",
        options,
        Box::new(|_cc| Box::new(PrepaWeekApp::default())),
    )
}
