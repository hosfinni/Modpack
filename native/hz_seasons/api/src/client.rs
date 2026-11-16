//! Biome Shift in the window: the black screen and its banner, the season's tint on
//! grass and leaves, and the season handed to the shader pack.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use horizon_client::mod_api::{BiomeColorsEvent, GuiRenderPostEvent, Host, ShaderParamsEvent};
use hz_seasons_core::{
    Config, Message, Season, banner, evergreen, season, tinted, warmth, year_fraction,
};

use crate::server::subtitle;

/// The banner's gold.
const GOLD: (f32, f32, f32) = (232.0, 195.0, 106.0);
/// How many times the game's font the words are drawn at.
const WORDS_SCALE: f32 = 3.0;
const WORDS: &str = "BIOME SHIFT";

pub struct Client {
    /// What the server last said: the epoch, and how many shifts make a year.
    epoch: AtomicI64,
    shifts_per_year: AtomicU32,
    /// When the banner started, in milliseconds on `clock()`; 0 for none.
    banner_at: AtomicU64,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            epoch: AtomicI64::new(0),
            shifts_per_year: AtomicU32::new(8),
            banner_at: AtomicU64::new(0),
        }
    }
}

fn clock() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    // Never 0, which means "no banner".
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

impl Client {
    fn season(&self) -> Season {
        season(
            self.epoch.load(Ordering::Relaxed),
            self.shifts_per_year.load(Ordering::Relaxed),
        )
    }

    /// The server said the epoch: the climate just turned, or this client is
    /// catching up after joining.
    pub fn message(&self, host: &mut dyn Host, config: &Config, message: Message) {
        let before = self.season();
        self.epoch.store(message.epoch, Ordering::Relaxed);
        self.shifts_per_year
            .store(message.shifts_per_year, Ordering::Relaxed);
        if message.shift && config.client.banner {
            self.banner_at.store(clock(), Ordering::Relaxed);
        }
        // The terrain is rebuilt with the new season's colours, behind the banner.
        if config.client.tints && (self.season() != before || !message.shift) {
            host.refresh_biome_colors();
        }
    }

    /// The window left its server: the next world starts in its own season.
    pub fn left(&self) {
        self.epoch.store(0, Ordering::Relaxed);
        self.banner_at.store(0, Ordering::Relaxed);
    }

    /// The game asks for a biome's colours: grass and leaves take the season's tint.
    pub fn biome_colors(&self, config: &Config, event: &mut BiomeColorsEvent<'_>) {
        if !config.client.tints {
            return;
        }
        let season = self.season();
        let evergreen = evergreen(event.biome);
        let colors = &mut *event.colors;
        colors.grass = tinted(season, colors.grass, false, evergreen);
        colors.foliage = tinted(season, colors.foliage, true, evergreen);
        colors.dry_foliage = tinted(season, colors.dry_foliage, true, evergreen);
    }

    /// Hands the season to the pack's shaders as `frame.mods[0]`: how far into the
    /// year it is (0 to 1), how warm the season is (-1 to 1), and how dark the banner
    /// has the screen (0 to 1).
    pub fn shader_params(&self, event: &mut ShaderParamsEvent<'_>) {
        let epoch = self.epoch.load(Ordering::Relaxed);
        let shifts_per_year = self.shifts_per_year.load(Ordering::Relaxed);
        let dark = self.banner_now().map_or(0.0, |banner| banner.black);
        event.params[0] = [
            year_fraction(epoch, shifts_per_year) as f32,
            warmth(epoch, shifts_per_year) as f32,
            dark,
            0.0,
        ];
    }

    fn banner_now(&self) -> Option<hz_seasons_core::Banner> {
        let started = self.banner_at.load(Ordering::Relaxed);
        if started == 0 {
            return None;
        }
        banner(clock().saturating_sub(started))
    }

    /// Draws the banner over everything: the screen goes black, the words come in
    /// gold between two thin rules, and the world returns changed.
    pub fn draw(&self, host: &dyn Host, event: &mut GuiRenderPostEvent<'_>) {
        let Some(banner) = self.banner_now() else {
            return;
        };
        let batch = &mut *event.batch;
        let (width, height) = (batch.width, batch.height);
        let alpha = (banner.black * 255.0).round() as u32;
        host.fill(batch, [0.0, 0.0, width, height], alpha << 24);
        if banner.words <= 0.0 {
            return;
        }
        // The words fade against black, so fading their colour fades them.
        let gold = |brightness: f32| -> u32 {
            let channel = |value: f32| ((value * brightness).round() as u32).min(255);
            channel(GOLD.0) << 16 | channel(GOLD.1) << 8 | channel(GOLD.2)
        };
        let (centre_x, centre_y) = (width / 2.0, height / 2.0);

        let words_width = host.text_width(WORDS);
        let start = batch.vertices.len();
        host.text(
            batch,
            WORDS,
            [centre_x - words_width / 2.0, centre_y - 8.0],
            gold(banner.words),
            false,
        );
        batch.scale_since(start, centre_x, centre_y - 4.0, WORDS_SCALE, WORDS_SCALE);

        // A rule above and below, drawing out from the middle.
        let half = (words_width * WORDS_SCALE * 0.62) * banner.rule;
        let rule = 0xFF00_0000 | gold(banner.words * 0.7);
        for y in [centre_y - 24.0, centre_y + 14.0] {
            host.fill(batch, [centre_x - half, y, half * 2.0, 1.0], rule);
        }

        let epoch = self.epoch.load(Ordering::Relaxed);
        let line = subtitle(epoch, self.shifts_per_year.load(Ordering::Relaxed));
        let line_width = host.text_width(&line);
        host.text(
            batch,
            &line,
            [centre_x - line_width / 2.0, centre_y + 22.0],
            gold(banner.words * 0.8),
            false,
        );
    }
}
