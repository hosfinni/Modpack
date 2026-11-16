//! hz_vfx: small visual effects. The rules are `hz-vfx-core`; this crate hooks them
//! into Horizon.
//!
//! What everyone sees is sent from the server as particles (vanilla's own packets,
//! so a player without the mod sees them too): dust where a block is placed, debris
//! where one breaks, sparks where a blow lands, smoke after an explosion. What only
//! the window's own player feels is drawn by the client: dust where they land or
//! sprint, a splash, their breath, fireflies, embers and cave dust around them, their
//! swing's trail, and the camera's shake.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Instant;

use horizon_client::mod_api::{
    BlockPlaceEvent, CameraShakeEvent, ClientAttackEvent, ClientHurtEvent, ClientLevelLoadEvent,
    ClientTickPostEvent, ConfigLoadingEvent, ConfigReloadingEvent, DestroyBlockEvent,
    ExplosionDetonateEvent, Flow, Hooks, Host, HurtEnemyEvent, NativeConfig, NativeConfigKind,
    NativeScreenKind, NetFromServerEvent, ParticleOptions, ParticleType, RegisterConfigsEvent,
};
use hz_vfx_core::{
    Blow, Config, DEFAULT_CONFIG, Ember, LANDING_MIN, Motion, Rng, SHAKE_CHANNEL, Shake, arc, blow,
    decode_shake, dust_color, ember, encode_shake, explosion_shake, facing, firefly_ground,
    has_fireflies, is_cold, is_night, is_soft, landing_shake,
};

const MOD_ID: &str = "hz_vfx";
const CONFIG_FILE: &str = "hz_vfx.toml";
/// How high a player's eyes are over their feet, standing.
const EYE: f64 = 1.62;

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Vfx {
        config: RwLock::new(Arc::new(Config::shipped())),
        client: Mutex::new(Client::default()),
        seed: AtomicU64::new(0x5EED),
    })
}

/// What the window's player has been doing, tick to tick.
#[derive(Default)]
struct Client {
    motion: Motion,
    shake: Shake,
    ticks: u64,
    /// The block at the player's feet last tick was water.
    in_water: bool,
    rng: Option<Rng>,
}

/// One instance serves the window's game and the server thread's, so what it keeps
/// is behind a lock or an atomic. No lock is held across a call into the game.
struct Vfx {
    config: RwLock<Arc<Config>>,
    client: Mutex<Client>,
    /// For the server's random numbers: each effect takes a fresh seed.
    seed: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Seconds since the mod started, for the shake.
fn now() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

fn floor(p: [f64; 3]) -> [i32; 3] {
    [
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    ]
}

fn dust(color: [f32; 3], scale: f32) -> ParticleOptions {
    let mut options = ParticleOptions::simple(ParticleType::Dust);
    options.color = [color[0], color[1], color[2], 1.0];
    options.scale = scale;
    options
}

impl Vfx {
    fn config(&self) -> Arc<Config> {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn rng(&self) -> Rng {
        Rng::new(self.seed.fetch_add(0x9E37_79B9, Ordering::Relaxed))
    }

    fn load(&self, host: &dyn Host, mod_id: &str, file: &str, text: &str) {
        if mod_id != MOD_ID || file != CONFIG_FILE {
            return;
        }
        match Config::parse(text) {
            Ok(config) => {
                *self.config.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(config);
            }
            Err(error) => host.log("ERROR", &format!("[hz_vfx] config/{CONFIG_FILE}: {error}")),
        }
    }

    // The server's side: particles everyone near sees.

    /// A burst of `options` at `at`, scaled by the intensity.
    #[allow(clippy::too_many_arguments)]
    fn burst(
        host: &mut dyn Host,
        config: &Config,
        rng: &mut Rng,
        options: &ParticleOptions,
        state: Option<horizon_client::mod_api::BlockStateId>,
        at: [f64; 3],
        spread: [f64; 3],
        speed: f64,
        count: u32,
    ) {
        let count = config.count(count, rng.unit());
        if count > 0 {
            host.send_particles(options, state, at, spread, speed, count as i32);
        }
    }

    // The window's side.

    /// One tick of the window's player: landing, footsteps, splash, breath, and the
    /// ambient effects around them.
    fn client_tick(&self, host: &mut dyn Host) {
        let config = self.config();
        if config.intensity <= 0.0 {
            return;
        }
        let Some(state) = host.client_state() else {
            return;
        };
        // Paused, in a menu, dead: nothing moves.
        if !matches!(
            state.screen,
            NativeScreenKind::Playing | NativeScreenKind::Chat
        ) {
            return;
        }
        let Some(player) = host.player() else {
            return;
        };
        let feet = [player.x, player.y, player.z];
        let mut client = lock(&self.client);
        let step = client.motion.step(feet);
        client.ticks += 1;
        let ticks = client.ticks;
        let mut rng = client
            .rng
            .take()
            .unwrap_or_else(|| Rng::new(ticks ^ feet[0].to_bits()));
        let was_in_water = client.in_water;
        let at_feet = host.block(floor(feet));
        let in_water = at_feet == "minecraft:water";
        client.in_water = in_water;
        let below = host.block(floor([feet[0], feet[1] - 0.2, feet[2]]));
        let ground = if is_soft(&below) { None } else { Some(below) };

        // Landing: dust in a ring from the ground, and a jolt for a hard one.
        if let (Some(height), Some(ground)) = (step.landed, ground.as_deref())
            && config.movement.landing
            && !in_water
        {
            let color = dust_color(ground);
            let n = config.count((6.0 + height.min(20.0) * 1.5) as u32, rng.unit());
            for i in 0..n {
                let angle = i as f64 / n.max(1) as f64 * std::f64::consts::TAU + rng.signed() * 0.3;
                let r = 0.35 + rng.unit() * 0.25;
                let (cx, cz) = (angle.cos(), angle.sin());
                host.add_particle(
                    &dust(color, 1.2 + rng.unit() as f32 * 0.8),
                    [feet[0] + cx * r, feet[1] + 0.05, feet[2] + cz * r],
                    [cx * 0.08, 0.02, cz * 0.08],
                );
            }
            if height >= LANDING_MIN {
                client
                    .shake
                    .kick(now(), config.shake(landing_shake(height)));
            }
        }

        // Sprinting: a puff behind the feet every few ticks.
        if config.movement.footsteps
            && step.speed > 0.2
            && step.rise.abs() < 0.01
            && ticks.is_multiple_of(3)
            && let Some(ground) = ground.as_deref()
            && !in_water
        {
            let color = dust_color(ground);
            let back = facing(player.yaw, 0.0);
            for _ in 0..config.count(2, rng.unit()) {
                host.add_particle(
                    &dust(color, 0.9 + rng.unit() as f32 * 0.5),
                    [
                        feet[0] - back[0] * 0.3 + rng.signed() * 0.2,
                        feet[1] + 0.05,
                        feet[2] - back[2] * 0.3 + rng.signed() * 0.2,
                    ],
                    [-back[0] * 0.03, 0.01, -back[2] * 0.03],
                );
            }
        }

        // Into the water from above: a splash ring and bubbles.
        if config.movement.splash && in_water && !was_in_water && step.rise < -0.15 {
            let surface = feet[1].floor() + 1.0;
            for i in 0..config.count(16, rng.unit()) {
                let angle = i as f64 / 16.0 * std::f64::consts::TAU;
                host.add_particle(
                    &ParticleOptions::simple(ParticleType::Splash),
                    [
                        feet[0] + angle.cos() * 0.6,
                        surface,
                        feet[2] + angle.sin() * 0.6,
                    ],
                    [angle.cos() * 0.1, 0.25, angle.sin() * 0.1],
                );
            }
            for _ in 0..config.count(8, rng.unit()) {
                host.add_particle(
                    &ParticleOptions::simple(ParticleType::Bubble),
                    [
                        feet[0] + rng.signed() * 0.4,
                        feet[1] + 0.3,
                        feet[2] + rng.signed() * 0.4,
                    ],
                    [0.0, 0.08, 0.0],
                );
            }
        }

        // Breath, now and then, in the cold.
        if config.movement.breath
            && ticks % 50 == 7
            && !in_water
            && host.biome(floor(feet)).is_some_and(|biome| is_cold(&biome))
        {
            let look = facing(player.yaw, player.pitch);
            let mouth = [
                feet[0] + look[0] * 0.35,
                feet[1] + EYE - 0.15 + look[1] * 0.35,
                feet[2] + look[2] * 0.35,
            ];
            for _ in 0..config.count(3, rng.unit()) {
                host.add_particle(
                    &ParticleOptions::simple(ParticleType::WhiteSmoke),
                    mouth,
                    [look[0] * 0.03, 0.01, look[2] * 0.03],
                );
            }
        }

        // Around the player.
        let radius = config.ambient.radius;
        let night = host.clock().is_some_and(|clock| is_night(clock.day_time));
        let base = floor(feet);
        // A few blocks looked at a tick: cheap, and over a second the area is covered
        // well enough for effects that come and go.
        let looks = config.count(6, rng.unit());
        for _ in 0..looks {
            let pos = [
                base[0] + rng.range(-radius, radius),
                base[1] + rng.range(-4, 6),
                base[2] + rng.range(-radius, radius),
            ];
            let block = host.block(pos);
            let top = [
                f64::from(pos[0]) + 0.5,
                f64::from(pos[1]) + 1.0,
                f64::from(pos[2]) + 0.5,
            ];
            if config.ambient.embers
                && let Some(kind) = ember(&block)
            {
                let above = host.block([pos[0], pos[1] + 1, pos[2]]);
                if !is_soft(&above) || !rng.chance(0.5) {
                    continue;
                }
                let (options, velocity) = match kind {
                    Ember::Sparks => (ParticleOptions::simple(ParticleType::Lava), [0.0, 0.0, 0.0]),
                    Ember::Lava => (ParticleOptions::simple(ParticleType::Lava), [0.0, 0.0, 0.0]),
                    Ember::Soul => (
                        ParticleOptions::simple(ParticleType::Soul),
                        [0.0, 0.03, 0.0],
                    ),
                };
                host.add_particle(
                    &options,
                    [
                        top[0] + rng.signed() * 0.3,
                        top[1] - 0.2,
                        top[2] + rng.signed() * 0.3,
                    ],
                    velocity,
                );
                if kind == Ember::Sparks && rng.chance(0.4) {
                    host.add_particle(
                        &ParticleOptions::simple(ParticleType::WhiteAsh),
                        [
                            top[0] + rng.signed() * 0.5,
                            top[1] + 0.3,
                            top[2] + rng.signed() * 0.5,
                        ],
                        [0.0, 0.02, 0.0],
                    );
                }
                continue;
            }
            if config.ambient.fireflies
                && night
                && firefly_ground(&block)
                && rng.chance(0.12)
                && is_soft(&host.block([pos[0], pos[1] + 1, pos[2]]))
                && host.biome(pos).is_some_and(|biome| has_fireflies(&biome))
            {
                host.add_particle(
                    &ParticleOptions::simple(ParticleType::Firefly),
                    [
                        top[0] + rng.signed() * 0.5,
                        top[1] + 0.5 + rng.unit(),
                        top[2] + rng.signed() * 0.5,
                    ],
                    [0.0, 0.0, 0.0],
                );
            }
        }

        // Deep under the ground, dust hangs in the air.
        if config.ambient.cave_dust
            && player.y < 40.0
            && ticks.is_multiple_of(4)
            && (2..10)
                .step_by(2)
                .any(|up| !is_soft(&host.block([base[0], base[1] + up, base[2]])))
        {
            for _ in 0..config.count(2, rng.unit()) {
                let at = [
                    feet[0] + rng.signed() * 6.0,
                    feet[1] + 0.5 + rng.unit() * 3.0,
                    feet[2] + rng.signed() * 6.0,
                ];
                if is_soft(&host.block(floor(at))) {
                    host.add_particle(
                        &ParticleOptions::simple(ParticleType::WhiteAsh),
                        at,
                        [0.0, -0.005, 0.0],
                    );
                }
            }
        }
        client.rng = Some(rng);
    }
}

impl Hooks for Vfx {
    fn register_configs(&self, _host: &mut dyn Host, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(
            NativeConfig::new(MOD_ID, NativeConfigKind::Common, DEFAULT_CONFIG).file(CONFIG_FILE),
        );
    }

    fn config_loading(&self, host: &mut dyn Host, event: &mut ConfigLoadingEvent<'_>) {
        let config = event.config;
        self.load(host, &config.mod_id, &config.file, &config.text);
    }

    fn config_reloading(&self, host: &mut dyn Host, event: &mut ConfigReloadingEvent<'_>) {
        let config = event.config;
        self.load(host, &config.mod_id, &config.file, &config.text);
    }

    // The server's side.

    /// A block placed: a puff of its own particles round its foot.
    fn block_place(&self, host: &mut dyn Host, event: &mut BlockPlaceEvent<'_>) -> Flow {
        let config = self.config();
        if config.blocks.place {
            let [x, y, z] = event.pos.map(f64::from);
            Self::burst(
                host,
                &config,
                &mut self.rng(),
                &ParticleOptions::simple(ParticleType::Block),
                Some(event.state),
                [x + 0.5, y + 0.1, z + 0.5],
                [0.35, 0.05, 0.35],
                0.05,
                8,
            );
        }
        Flow::Continue
    }

    /// A block broken: a little more debris than vanilla's, thrown wider.
    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        let config = self.config();
        if config.blocks.broken {
            let [x, y, z] = event.pos.map(f64::from);
            Self::burst(
                host,
                &config,
                &mut self.rng(),
                &ParticleOptions::simple(ParticleType::Block),
                Some(event.state),
                [x + 0.5, y + 0.5, z + 0.5],
                [0.45, 0.45, 0.45],
                0.12,
                10,
            );
        }
        Flow::Continue
    }

    /// A blow landed on a mob: sparks of the weapon's kind.
    fn hurt_enemy(&self, host: &mut dyn Host, event: &mut HurtEnemyEvent<'_>) {
        let config = self.config();
        if !config.combat.hits {
            return;
        }
        let Some(at) = host
            .entity(event.entity)
            .and_then(|entity| host.entity_position(entity))
        else {
            return;
        };
        let chest = [at[0], at[1] + 0.9, at[2]];
        let mut rng = self.rng();
        let simple = ParticleOptions::simple;
        match blow(&event.stack.item) {
            Blow::Slash => {
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::SweepAttack),
                    None,
                    chest,
                    [0.1, 0.1, 0.1],
                    0.0,
                    1,
                );
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::EnchantedHit),
                    None,
                    chest,
                    [0.25, 0.3, 0.25],
                    0.25,
                    6,
                );
            }
            Blow::Sweep => {
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::SweepAttack),
                    None,
                    chest,
                    [0.4, 0.1, 0.4],
                    0.0,
                    2,
                );
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::Crit),
                    None,
                    chest,
                    [0.35, 0.3, 0.35],
                    0.2,
                    6,
                );
            }
            Blow::Crush => {
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::Crit),
                    None,
                    chest,
                    [0.3, 0.3, 0.3],
                    0.3,
                    8,
                );
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &dust([0.48, 0.40, 0.32], 1.6),
                    None,
                    [at[0], at[1] + 0.1, at[2]],
                    [0.6, 0.05, 0.6],
                    0.0,
                    12,
                );
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::Poof),
                    None,
                    [at[0], at[1] + 0.1, at[2]],
                    [0.4, 0.0, 0.4],
                    0.05,
                    3,
                );
            }
            Blow::Lash => {
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::ElectricSpark),
                    None,
                    chest,
                    [0.3, 0.4, 0.3],
                    0.15,
                    6,
                );
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::Crit),
                    None,
                    chest,
                    [0.2, 0.2, 0.2],
                    0.2,
                    3,
                );
            }
            Blow::Plain => {
                Self::burst(
                    host,
                    &config,
                    &mut rng,
                    &simple(ParticleType::Crit),
                    None,
                    chest,
                    [0.2, 0.25, 0.2],
                    0.15,
                    3,
                );
            }
        }
    }

    /// An explosion: smoke that hangs on, ash, and a shake for whoever is near.
    fn explosion_detonate(&self, host: &mut dyn Host, event: &mut ExplosionDetonateEvent<'_>) {
        let config = self.config();
        let mut rng = self.rng();
        let center = event.center;
        Self::burst(
            host,
            &config,
            &mut rng,
            &ParticleOptions::simple(ParticleType::LargeSmoke),
            None,
            center,
            [1.2, 0.8, 1.2],
            0.02,
            14,
        );
        Self::burst(
            host,
            &config,
            &mut rng,
            &ParticleOptions::simple(ParticleType::WhiteAsh),
            None,
            [center[0], center[1] + 1.0, center[2]],
            [2.5, 1.5, 2.5],
            0.02,
            24,
        );
        if !config.shake.enabled {
            return;
        }
        let dimension = event.dimension.to_owned();
        for player in host.server_players() {
            if player.dimension != dimension {
                continue;
            }
            let d = [
                player.position[0] - center[0],
                player.position[1] - center[1],
                player.position[2] - center[2],
            ];
            let amount = explosion_shake((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt());
            if amount > 0.0 {
                let _ = host.net_send_to_player(player.uuid, SHAKE_CHANNEL, &encode_shake(amount));
            }
        }
    }

    // The window's side.

    fn client_level_load(&self, _host: &mut dyn Host, _event: &mut ClientLevelLoadEvent<'_>) {
        let mut client = lock(&self.client);
        client.motion.reset();
        client.in_water = false;
    }

    fn client_tick_post(&self, host: &mut dyn Host, _event: &mut ClientTickPostEvent) {
        self.client_tick(host);
    }

    /// The window's player swung at something: a short trail from the swing to it.
    fn client_attack(&self, host: &mut dyn Host, event: &mut ClientAttackEvent) {
        let config = self.config();
        if !config.combat.swing_trails || config.intensity <= 0.0 {
            return;
        }
        let Some(player) = host.player() else {
            return;
        };
        let Some(target) = host
            .client_entities()
            .into_iter()
            .find(|entity| entity.id == event.target)
        else {
            return;
        };
        let look = facing(player.yaw, player.pitch);
        let eye = [player.x, player.y + EYE, player.z];
        let from = [
            eye[0] + look[0] * 0.6,
            eye[1] - 0.3 + look[1] * 0.6,
            eye[2] + look[2] * 0.6,
        ];
        let to = [
            target.position[0],
            target.position[1] + f64::from(target.height) * 0.6,
            target.position[2],
        ];
        let mut rng = Rng::new(u64::from(event.target) ^ player.x.to_bits());
        let points = config.count(7, rng.unit()) as usize;
        let bow = 0.35 * if rng.chance(0.5) { 1.0 } else { -1.0 };
        for point in arc(from, to, points, bow) {
            host.add_particle(
                &ParticleOptions::simple(ParticleType::Crit),
                point,
                [0.0, 0.0, 0.0],
            );
        }
    }

    /// The window's player was hurt: a small jolt.
    fn client_hurt(&self, _host: &mut dyn Host, event: &mut ClientHurtEvent) {
        let config = self.config();
        if event.attacker.is_some() {
            lock(&self.client).shake.kick(now(), config.shake(0.35));
        }
    }

    fn net_from_server(&self, _host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        if event.channel != SHAKE_CHANNEL {
            return;
        }
        if let Some(amount) = decode_shake(event.data) {
            let config = self.config();
            lock(&self.client).shake.kick(now(), config.shake(amount));
        }
    }

    fn camera_shake(&self, _host: &dyn Host, event: &mut CameraShakeEvent<'_>) {
        let sway = lock(&self.client).shake.sway(now());
        event.shake.pitch += sway.pitch;
        event.shake.yaw += sway.yaw;
        event.shake.roll += sway.roll;
    }
}
