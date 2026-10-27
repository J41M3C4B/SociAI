//! AI settings kept in `app_settings`: provider, model per tier, price table, rate limits
//! and monthly cap. Model names and prices are configuration, never hard-coded in logic (ADR-005).

use super::{ModelTier, Usage};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const KEY: &str = "ai.settings";

/// Which AI service answers. One at a time; each has its own key and its own models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Scenario B of ADR-007: the main provider for now.
    #[default]
    Gemini,
    Anthropic,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 2] = [ProviderKind::Gemini, ProviderKind::Anthropic];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Gemini => "gemini",
            ProviderKind::Anthropic => "anthropic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// A price that starts on a date (providers announce increases in advance).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceChange {
    /// First day (UTC, `YYYY-MM-DD`) the new price applies.
    pub from: String,
    pub input: f64,
    pub output: f64,
}

/// USD per million tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub then: Option<PriceChange>,
}

impl ModelPrice {
    fn flat(input: f64, output: f64) -> Self {
        ModelPrice { input, output, then: None }
    }

    fn rising(input: f64, output: f64, from: &str, new_input: f64, new_output: f64) -> Self {
        ModelPrice { input, output, then: Some(PriceChange { from: from.into(), input: new_input, output: new_output }) }
    }

    /// (input, output) in force on `today`.
    fn on(&self, today: &str) -> (f64, f64) {
        match &self.then {
            Some(c) if c.from.as_str() <= today => (c.input, c.output),
            _ => (self.input, self.output),
        }
    }
}

/// What the provider allows per model. Free plans are tight, so the pipeline paces itself
/// instead of finding out with an error. Zero means "no limit known".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RateLimit {
    pub per_minute: u32,
    pub tokens_per_minute: u64,
    pub per_day: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: ProviderKind,
    pub model_light: String,
    pub model_strong: String,
    pub gemini_model_light: String,
    pub gemini_model_strong: String,
    /// Models tried, in order, when the main one of a tier is overloaded, unavailable or out of
    /// daily allowance (ADR-008). Each has its own allowance. Empty = no backup.
    pub gemini_light_fallbacks: Vec<String>,
    pub gemini_strong_fallbacks: Vec<String>,
    /// Models where the fixed temperature of a classifying task (`AiTask::sampling`) was measured
    /// and does no harm. Anywhere else the provider's defaults are used: `gemini-3-flash-preview`
    /// with temperature 0 reasoned until it filled its whole output (13,000 tokens, 45 s) on three
    /// runs out of three, the repetition Google warns about for the Gemini 3 series, while
    /// `gemini-3.5-flash-lite` went from 96.8 % to 100 % agreement with itself. A model is added
    /// here only after measuring it (see ADR-015).
    pub sampling_models: Vec<String>,
    /// Models kept in the chain's configuration but skipped while they fail or are not wanted
    /// (e.g. a main model that keeps answering 503). Turning one back on is removing it here.
    pub disabled_models: Vec<String>,
    /// Thinking depth for the strong tier (`low`/`medium`/`high`); empty = provider default.
    pub effort_strong: String,
    /// Extra output tokens allowed for the model's own thinking on the strong tier.
    pub thinking_headroom: u32,
    /// Extra output tokens for the light tier on providers whose models think a little anyway,
    /// so a short answer is never cut off by its own thinking.
    pub light_headroom: u32,
    /// Ask the API to retry on a safe model if a request is declined (Anthropic only).
    pub use_fallbacks: bool,
    pub monthly_cap_mxn: f64,
    pub usd_to_mxn: f64,
    pub prices: BTreeMap<String, ModelPrice>,
    pub rate_limits: BTreeMap<String, RateLimit>,
}

fn default_prices() -> BTreeMap<String, ModelPrice> {
    let mut prices = BTreeMap::new();
    for (m, i, o) in [
        ("claude-haiku-4-5", 1.0, 5.0),
        ("claude-sonnet-5-5", 2.0, 10.0),
        ("claude-sonnet-5", 2.0, 10.0),
        ("claude-opus-5-5", 4.0, 20.0),
        ("gemini-3.5-flash-lite", 0.30, 2.50),
        // an older Flash-Lite, offered to pick by hand when 3.5 Flash-Lite is overloaded; not a
        // backup of the chain and not in `sampling_models` until it is measured
        ("gemini-3.1-flash-lite", 0.25, 1.50),
        ("gemini-3.5-flash", 1.50, 9.00),
        // preview model: Google may change its price when it leaves preview
        ("gemini-3-flash-preview", 0.50, 3.00),
        // vectors of the documents of the institution (IA5): only what goes in is charged
        ("gemini-embedding-2", 0.20, 0.0),
    ] {
        prices.insert(m.to_string(), ModelPrice::flat(i, o));
    }
    // Introductory price until 2026-12-31; it doubles on 2027-01-01.
    for m in ["gemini-3.6-flash", "gemini-3.7-flash", "gemini-3.8-flash"] {
        prices.insert(m.to_string(), ModelPrice::rising(0.75, 3.75, "2027-01-01", 1.50, 7.50));
    }
    prices
}

/// Limits read from the Google AI Studio table (columns: per minute, tokens per minute, per day).
fn default_rate_limits() -> BTreeMap<String, RateLimit> {
    let mut limits = BTreeMap::new();
    // 3.1 Flash-Lite is given the limits of 3.5 Flash-Lite until they are read from the table
    for m in ["gemini-3.5-flash-lite", "gemini-3.1-flash-lite"] {
        limits.insert(m.to_string(), RateLimit { per_minute: 15, tokens_per_minute: 250_000, per_day: 500 });
    }
    for m in ["gemini-3-flash-preview", "gemini-3.5-flash", "gemini-3.6-flash", "gemini-3.7-flash", "gemini-3.8-flash"] {
        limits.insert(m.to_string(), RateLimit { per_minute: 5, tokens_per_minute: 250_000, per_day: 20 });
    }
    limits
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            provider: ProviderKind::default(),
            model_light: "claude-haiku-4-5".into(),
            model_strong: "claude-sonnet-5-5".into(),
            gemini_model_light: "gemini-3.5-flash-lite".into(),
            // 3.5 Flash stays the main model of the strong tier (ADR-009) but is switched off in
            // `disabled_models` while it answers 503 after up to 111 s; switched on again, it goes
            // first. Meanwhile 3 Flash (preview) answers the full request in 8 s (ADR-012).
            gemini_model_strong: "gemini-3.5-flash".into(),
            // Light has no backup: the Gemini 2.x family stopped working (it is out) and the next
            // Flash costs five times more.
            gemini_light_fallbacks: Vec::new(),
            // Each backup has its own daily allowance and is cheaper than the main model.
            // Only the models that answer today (3.6, 3.7 and 3.8 kept returning 503, and Gemini 2.x
            // stopped working): the preview model, with 3.5 Flash in front of it once it is switched on.
            gemini_strong_fallbacks: vec!["gemini-3-flash-preview".into()],
            disabled_models: vec!["gemini-3.5-flash".into()],
            sampling_models: vec!["gemini-3.5-flash-lite".into()],
            effort_strong: "medium".into(),
            thinking_headroom: 4000,
            light_headroom: 200,
            use_fallbacks: true,
            monthly_cap_mxn: 200.0,
            usd_to_mxn: 18.5,
            prices: default_prices(),
            rate_limits: default_rate_limits(),
        }
    }
}

impl AiSettings {
    /// The model a given provider uses for a tier.
    pub fn model_of(&self, provider: ProviderKind, tier: ModelTier) -> &str {
        match (provider, tier) {
            (ProviderKind::Anthropic, ModelTier::Light) => &self.model_light,
            (ProviderKind::Anthropic, ModelTier::Strong) => &self.model_strong,
            (ProviderKind::Gemini, ModelTier::Light) => &self.gemini_model_light,
            (ProviderKind::Gemini, ModelTier::Strong) => &self.gemini_model_strong,
        }
    }

    /// Models of the price table that belong to a provider: what the model pickers offer.
    pub fn known_models(&self, provider: ProviderKind) -> Vec<String> {
        let prefix = match provider {
            ProviderKind::Gemini => "gemini",
            ProviderKind::Anthropic => "claude",
        };
        self.prices.keys().filter(|m| m.starts_with(prefix)).cloned().collect()
    }

    /// Swaps the main model of each tier for a provider and the thinking depth of the strong tier.
    /// A model that is picked is switched on again if it was in `disabled_models`: choosing it by
    /// hand is the way to try it. Blank names are ignored (the tier keeps its model); an effort
    /// outside `low`/`medium`/`high` means "provider default".
    pub fn set_models(&mut self, provider: ProviderKind, light: &str, strong: &str, effort: &str) {
        let (light, strong) = (light.trim(), strong.trim());
        let (light_slot, strong_slot) = match provider {
            ProviderKind::Gemini => (&mut self.gemini_model_light, &mut self.gemini_model_strong),
            ProviderKind::Anthropic => (&mut self.model_light, &mut self.model_strong),
        };
        if !light.is_empty() {
            *light_slot = light.to_string();
        }
        if !strong.is_empty() {
            *strong_slot = strong.to_string();
        }
        let picked = [light, strong];
        self.disabled_models.retain(|d| !picked.contains(&d.as_str()));
        self.effort_strong = match effort.trim() {
            e @ ("low" | "medium" | "high") => e.to_string(),
            _ => String::new(),
        };
    }

    /// The model of the active provider that a call of this tier is sent to first: the first one
    /// of the chain that is not switched off.
    pub fn model_for(&self, tier: ModelTier) -> String {
        self.model_in_use(self.provider, tier)
    }

    pub fn model_in_use(&self, provider: ProviderKind, tier: ModelTier) -> String {
        self.chain_of(provider, tier).into_iter().next().unwrap_or_else(|| self.model_of(provider, tier).to_string())
    }

    /// The main model of a tier followed by its backups, without repeats, blanks or switched-off models.
    /// Only Gemini has backups: Anthropic does its own fallback on the server (ADR-006).
    pub fn chain_of(&self, provider: ProviderKind, tier: ModelTier) -> Vec<String> {
        let backups: &[String] = match (provider, tier) {
            (ProviderKind::Gemini, ModelTier::Light) => &self.gemini_light_fallbacks,
            (ProviderKind::Gemini, ModelTier::Strong) => &self.gemini_strong_fallbacks,
            (ProviderKind::Anthropic, _) => &[],
        };
        let mut chain: Vec<String> = Vec::new();
        for m in std::iter::once(self.model_of(provider, tier)).chain(backups.iter().map(String::as_str)) {
            if !m.trim().is_empty() && !chain.iter().any(|c| c == m) && !self.disabled_models.iter().any(|d| d == m) {
                chain.push(m.to_string());
            }
        }
        chain
    }

    /// Price of a model: exact name first, then the longest known name it starts with
    /// (so `gemini-3.8-flash-001` is still priced as `gemini-3.8-flash`). A model the table
    /// does not know is charged at the most expensive known price, so the counter never under-reports.
    fn price_of(&self, model: &str) -> ModelPrice {
        if let Some(p) = self.prices.get(model) {
            return p.clone();
        }
        let by_prefix = self
            .prices
            .iter()
            .filter(|(name, _)| model.starts_with(name.as_str()))
            .max_by_key(|(name, _)| name.len());
        if let Some((_, p)) = by_prefix {
            return p.clone();
        }
        self.prices.values().fold(ModelPrice::flat(0.0, 0.0), |a, b| {
            let (ai, ao) = (a.input.max(b.input).max(b.then.as_ref().map_or(0.0, |c| c.input)), a.output.max(b.output).max(b.then.as_ref().map_or(0.0, |c| c.output)));
            ModelPrice::flat(ai, ao)
        })
    }

    /// Estimated cost in pesos today.
    pub fn cost_mxn(&self, model: &str, u: &Usage) -> f64 {
        self.cost_mxn_on(model, u, &today_utc())
    }

    /// Estimated cost in pesos with the prices in force on `today` (`YYYY-MM-DD`).
    pub fn cost_mxn_on(&self, model: &str, u: &Usage, today: &str) -> f64 {
        let (input, output) = self.price_of(model).on(today);
        let usd = (u.input_tokens as f64 * input
            + u.output_tokens as f64 * output
            + u.cache_read_tokens as f64 * input * 0.1
            + u.cache_write_tokens as f64 * input * 1.25)
            / 1_000_000.0;
        usd * self.usd_to_mxn
    }
}

/// Today's date in UTC as `YYYY-MM-DD` (civil-from-days, no date library needed).
pub fn today_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn load(conn: &Connection) -> rusqlite::Result<AiSettings> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM app_settings WHERE key=?1", [KEY], |r| r.get(0))
        .optional()?;
    // Settings saved before `disabled_models` existed carry the chain of an older version (with
    // 3.8, 3.7 and 3.6 as backups, which kept answering 503, and without 3-flash-preview). The app
    // has no screen to edit the chain, so nobody chose it: it is replaced by the current one. What
    // the person did choose (provider, monthly cap, prices) is kept. Once the settings are saved
    // again they carry the field, and the chain they hold is respected.
    let value: Option<serde_json::Value> = raw.as_deref().and_then(|r| serde_json::from_str(r).ok());
    let from_an_older_chain = value.as_ref().is_some_and(|v| v.is_object() && v.get("disabled_models").is_none());
    let value_lacks_sampling_models = value.as_ref().is_some_and(|v| v.is_object() && v.get("sampling_models").is_none());
    let mut s: AiSettings = value.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    if from_an_older_chain {
        let d = AiSettings::default();
        s.gemini_model_strong = d.gemini_model_strong;
        s.gemini_strong_fallbacks = d.gemini_strong_fallbacks;
        s.gemini_light_fallbacks = d.gemini_light_fallbacks;
        s.disabled_models = d.disabled_models;
    }
    // the list of models measured safe for a fixed temperature is the code's, not the person's
    if value_lacks_sampling_models {
        s.sampling_models = AiSettings::default().sampling_models;
    }
    // Settings saved by an older version lack the newer models: add them without touching
    // anything the person changed.
    for (model, price) in default_prices() {
        s.prices.entry(model).or_insert(price);
    }
    for (model, limit) in default_rate_limits() {
        s.rate_limits.entry(model).or_insert(limit);
    }
    Ok(s)
}

pub fn save(conn: &Connection, s: &AiSettings) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![KEY, serde_json::to_string(s).unwrap()],
    )?;
    Ok(())
}

/// Pesos spent on AI so far this calendar month.
pub fn month_spend_mxn(conn: &Connection) -> rusqlite::Result<f64> {
    conn.query_row(
        "SELECT COALESCE(SUM(estimated_cost_mxn),0) FROM ai_usage WHERE at >= strftime('%Y-%m-01T00:00:00Z','now')",
        [],
        |r| r.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_uses_the_price_table_and_cache_discounts() {
        let s = AiSettings { usd_to_mxn: 20.0, ..Default::default() };
        // haiku: 1M input = $1, 1M output = $5 -> $6 -> 120 pesos
        let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, ..Default::default() };
        assert!((s.cost_mxn("claude-haiku-4-5", &u) - 120.0).abs() < 1e-9);
        // cache read costs a tenth of input, cache write 1.25x
        let u = Usage { cache_read_tokens: 1_000_000, cache_write_tokens: 1_000_000, ..Default::default() };
        assert!((s.cost_mxn("claude-haiku-4-5", &u) - (0.1 + 1.25) * 20.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_is_charged_at_the_highest_known_price() {
        let s = AiSettings { usd_to_mxn: 1.0, ..Default::default() };
        let u = Usage { input_tokens: 1_000_000, ..Default::default() };
        assert!((s.cost_mxn("modelo-nuevo", &u) - 4.0).abs() < 1e-9); // opus-5-5 input price
    }

    #[test]
    fn a_versioned_model_name_is_priced_like_its_family() {
        let s = AiSettings { usd_to_mxn: 1.0, ..Default::default() };
        let u = Usage { input_tokens: 1_000_000, ..Default::default() };
        let exact = s.cost_mxn_on("gemini-3.5-flash", &u, "2026-10-01");
        let lite = s.cost_mxn_on("gemini-3.5-flash-lite-preview", &u, "2026-10-01");
        assert!((exact - 1.50).abs() < 1e-9);
        // the longest known prefix wins: "-lite" is not priced as the bigger "gemini-3.5-flash"
        assert!((lite - 0.30).abs() < 1e-9, "{lite}");
    }

    #[test]
    fn gemini_price_doubles_on_new_year_2027() {
        let s = AiSettings { usd_to_mxn: 1.0, ..Default::default() };
        let u = Usage { input_tokens: 1_000_000, output_tokens: 1_000_000, ..Default::default() };
        // the preview model has its own price, well below the bigger "gemini-3.5-flash"
        assert!(s.prices.contains_key("gemini-3-flash-preview"));
        assert!(s.cost_mxn_on("gemini-3-flash-preview", &u, "2026-10-01") < s.cost_mxn_on("gemini-3.5-flash", &u, "2026-10-01") / 2.0);
        assert!((s.cost_mxn_on("gemini-3.8-flash", &u, "2026-12-31") - 4.50).abs() < 1e-9);
        assert!((s.cost_mxn_on("gemini-3.8-flash", &u, "2027-01-01") - 9.00).abs() < 1e-9);
        // cached input follows the same tenth of the input price
        let c = Usage { cache_read_tokens: 1_000_000, ..Default::default() };
        assert!((s.cost_mxn_on("gemini-3.8-flash", &c, "2026-10-01") - 0.075).abs() < 1e-9);
        assert!((s.cost_mxn_on("gemini-3.8-flash", &c, "2027-02-01") - 0.15).abs() < 1e-9);
    }

    #[test]
    fn today_is_a_plausible_calendar_date() {
        let t = today_utc();
        assert_eq!(t.len(), 10);
        assert!(t.as_str() >= "2026-10-01", "{t}");
    }

    #[test]
    fn each_provider_has_its_own_models_and_gemini_is_the_default() {
        let s = AiSettings::default();
        assert_eq!(s.provider, ProviderKind::Gemini);
        assert_eq!(s.model_for(ModelTier::Light), "gemini-3.5-flash-lite");
        assert_eq!(s.model_for(ModelTier::Strong), "gemini-3-flash-preview");
        assert_eq!(s.model_of(ProviderKind::Anthropic, ModelTier::Light), "claude-haiku-4-5");
        assert_eq!(s.model_of(ProviderKind::Anthropic, ModelTier::Strong), "claude-sonnet-5-5");
    }

    #[test]
    fn the_chain_is_the_main_model_then_its_backups_without_repeats() {
        let mut s = AiSettings::default();
        assert_eq!(
            s.chain_of(ProviderKind::Gemini, ModelTier::Strong),
            vec!["gemini-3-flash-preview"],
            "3.5 Flash is the main model but switched off"
        );
        let mut on = s.clone();
        on.disabled_models.clear();
        assert_eq!(on.chain_of(ProviderKind::Gemini, ModelTier::Strong)[0], "gemini-3.5-flash");
        assert_eq!(on.model_for(ModelTier::Strong), "gemini-3.5-flash");
        assert_eq!(s.chain_of(ProviderKind::Gemini, ModelTier::Light), vec!["gemini-3.5-flash-lite"]);
        assert_eq!(s.chain_of(ProviderKind::Anthropic, ModelTier::Strong), vec!["claude-sonnet-5-5"]);
        s.gemini_strong_fallbacks = vec!["gemini-3.5-flash".into(), " ".into(), "gemini-3.7-flash".into(), "gemini-3.7-flash".into()];
        assert_eq!(s.chain_of(ProviderKind::Gemini, ModelTier::Strong), vec!["gemini-3.7-flash"]);
        // every model of the default chain is priced and has a known allowance
        let d = AiSettings::default();
        for m in d.chain_of(ProviderKind::Gemini, ModelTier::Strong) {
            assert!(d.prices.contains_key(&m) && d.rate_limits.contains_key(&m), "{m}");
        }
    }

    #[test]
    fn picking_models_by_hand_swaps_the_tier_and_switches_a_disabled_model_back_on() {
        let mut s = AiSettings::default();
        assert_eq!(s.model_for(ModelTier::Strong), "gemini-3-flash-preview");
        s.set_models(ProviderKind::Gemini, "", "gemini-3.5-flash", "high");
        assert_eq!(s.model_for(ModelTier::Strong), "gemini-3.5-flash", "it was switched off, picking it turns it on");
        assert_eq!(s.model_for(ModelTier::Light), "gemini-3.5-flash-lite", "a blank name leaves the tier as it was");
        assert_eq!(s.effort_strong, "high");
        // the other provider's models are untouched, and a made-up effort means "default"
        s.set_models(ProviderKind::Anthropic, "claude-haiku-4-5", "claude-opus-5-5", "bogus");
        assert_eq!(s.model_of(ProviderKind::Anthropic, ModelTier::Strong), "claude-opus-5-5");
        assert_eq!(s.model_of(ProviderKind::Gemini, ModelTier::Strong), "gemini-3.5-flash");
        assert_eq!(s.effort_strong, "");
    }

    #[test]
    fn known_models_are_split_by_provider() {
        let s = AiSettings::default();
        let g = s.known_models(ProviderKind::Gemini);
        let a = s.known_models(ProviderKind::Anthropic);
        assert!(g.iter().all(|m| m.starts_with("gemini")) && g.contains(&"gemini-3.5-flash".to_string()));
        assert!(a.iter().all(|m| m.starts_with("claude")) && a.contains(&"claude-opus-5-5".to_string()));
    }

    #[test]
    fn settings_round_trip_and_default_when_missing() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../../migrations/0001_initial.sql")).unwrap();
        assert_eq!(load(&c).unwrap(), AiSettings::default());
        let mut s = AiSettings::default();
        s.monthly_cap_mxn = 50.0;
        s.model_light = "otro-modelo".into();
        save(&c, &s).unwrap();
        assert_eq!(load(&c).unwrap(), s);
        save(&c, &s).unwrap(); // upsert
    }

    #[test]
    fn settings_saved_by_phase_2_gain_the_gemini_prices_and_limits() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../../migrations/0001_initial.sql")).unwrap();
        // what Phase 2 stored: no provider, no gemini models, only the Claude prices
        let old = r#"{"model_light":"claude-haiku-4-5","model_strong":"claude-sonnet-5-5","monthly_cap_mxn":80.0,
                      "prices":{"claude-haiku-4-5":{"input":1.0,"output":5.0}}}"#;
        c.execute("INSERT INTO app_settings (key,value) VALUES ('ai.settings', ?1)", [old]).unwrap();
        let s = load(&c).unwrap();
        assert_eq!(s.monthly_cap_mxn, 80.0, "the person's change is kept");
        assert!(s.prices.contains_key("gemini-3.8-flash") && s.prices.contains_key("gemini-3.5-flash-lite"));
        assert!(s.known_models(ProviderKind::Gemini).contains(&"gemini-3.1-flash-lite".to_string()), "it can be picked by hand");
        assert_eq!(s.rate_limits["gemini-3.8-flash"].per_day, 20);
        assert_eq!(s.rate_limits["gemini-3.1-flash-lite"].per_minute, 15);
        assert_eq!(s.provider, ProviderKind::Gemini);
    }

    #[test]
    fn settings_saved_with_the_old_chain_get_the_current_one_and_keep_what_the_person_chose() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../../migrations/0001_initial.sql")).unwrap();
        // what the app stored before the chain changed: 3.5 as main, 3.8 / 3.7 / 3.6 behind it, no
        // `disabled_models`, and a monthly cap the person had set
        let old = r#"{"provider":"gemini","gemini_model_strong":"gemini-3.5-flash",
                      "gemini_strong_fallbacks":["gemini-3.8-flash","gemini-3.7-flash","gemini-3.6-flash"],
                      "gemini_light_fallbacks":[],"monthly_cap_mxn":120.0}"#;
        c.execute("INSERT INTO app_settings (key,value) VALUES ('ai.settings', ?1)", [old]).unwrap();
        let s = load(&c).unwrap();
        assert_eq!(s.monthly_cap_mxn, 120.0, "the person's cap is kept");
        assert_eq!(s.chain_of(ProviderKind::Gemini, ModelTier::Strong), vec!["gemini-3-flash-preview"], "preview is reached again and 3.8 is not");
        // once saved again, the chain the settings hold is respected: it carries `disabled_models`
        let mut mine = s.clone();
        mine.gemini_strong_fallbacks = vec!["gemini-3.7-flash".into()];
        save(&c, &mine).unwrap();
        let again = load(&c).unwrap();
        assert_eq!(again.gemini_strong_fallbacks, vec!["gemini-3.7-flash"]);
    }

    #[test]
    fn month_spend_adds_only_this_month() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../../migrations/0001_initial.sql")).unwrap();
        c.execute("INSERT INTO ai_usage (id,at,task,provider,model,input_tokens,output_tokens,estimated_cost_mxn,success) VALUES ('a','2020-01-05T00:00:00Z','t','p','m',1,1,99.0,1)", []).unwrap();
        c.execute("INSERT INTO ai_usage (id,at,task,provider,model,input_tokens,output_tokens,estimated_cost_mxn,success) VALUES ('b',strftime('%Y-%m-%dT%H:%M:%SZ','now'),'t','p','m',1,1,1.5,1)", []).unwrap();
        assert!((month_spend_mxn(&c).unwrap() - 1.5).abs() < 1e-9);
    }
}
