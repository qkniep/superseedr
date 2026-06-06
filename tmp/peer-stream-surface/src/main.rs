use std::f64::consts::{PI, TAU};
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::Line as TextLine;
use ratatui::widgets::canvas::{Canvas, Context, Line, Points};
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};
use ratatui::{DefaultTerminal, Frame};

const CAMERA_DISTANCE: f64 = 4.0;
const MIN_DISPLAY_VOL: f64 = 5.0;
const MAX_DISPLAY_VOL: f64 = 80.0;

fn main() -> io::Result<()> {
    ratatui::run(|terminal| App::new().run(terminal))
}

struct App {
    paused: bool,
    fps_counter: FpsCounter,
    vol_engine: VolatilityEngine,
    surface_3d: Surface3D,
}

impl App {
    fn new() -> Self {
        Self {
            paused: false,
            fps_counter: FpsCounter::new(),
            vol_engine: VolatilityEngine::new(),
            surface_3d: Surface3D::new(),
        }
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        let tick_rate = Duration::from_secs_f64(1.0 / 50.0);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|frame| self.render(frame))?;
            self.fps_counter.update();

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let event::Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press && self.handle_key(key.code, key.modifiers) {
                        break;
                    }
                }
            }

            if last_tick.elapsed() >= tick_rate {
                if !self.paused {
                    self.vol_engine.update();
                }
                last_tick = Instant::now();
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.vol_engine.reset();
            }
            KeyCode::Up | KeyCode::Char('k') => self.surface_3d.rotate_x(0.1),
            KeyCode::Down | KeyCode::Char('j') => self.surface_3d.rotate_x(-0.1),
            KeyCode::Left | KeyCode::Char('h') => self.surface_3d.rotate_z(0.1),
            KeyCode::Right | KeyCode::Char('l') => self.surface_3d.rotate_z(-0.1),
            KeyCode::Char('z') => self.surface_3d.zoom(1.1),
            KeyCode::Char('x') => self.surface_3d.zoom(0.9),
            KeyCode::Char('p') => self.surface_3d.cycle_palette(),
            _ => {}
        }
        false
    }

    fn render(&mut self, frame: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

        self.render_header(frame, chunks[0]);
        self.render_surface(frame, chunks[1]);
        Self::render_footer(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let status = if self.paused { "Paused" } else { "Live" };
        let title = format!(
            "volatility-surface - Status: {status}, Palette: {}, FPS: {}",
            self.surface_3d.palette_name(),
            self.fps_counter.fps()
        );

        frame.render_widget(
            Paragraph::new(title)
                .centered()
                .style(Style::default().fg(Color::Gray)),
            area,
        );
    }

    fn render_surface(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_stateful_widget(
            VolatilitySurface::new(self.vol_engine.get_surface()),
            area,
            &mut self.surface_3d,
        );
    }

    fn render_footer(frame: &mut Frame, area: Rect) {
        let controls = TextLine::from(vec![
            "↑↓←→/hjkl".cyan(),
            " Rotate | ".into(),
            "zx".cyan(),
            " Zoom | ".into(),
            "p".cyan(),
            " Palette | ".into(),
            "space".cyan(),
            " Pause | ".into(),
            "ctrl-r".cyan(),
            " Reset | ".into(),
            "q".cyan(),
            " Quit".into(),
        ]);

        frame.render_widget(
            Paragraph::new(controls)
                .centered()
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

struct FpsCounter {
    frame_count: usize,
    last_instant: Instant,
    fps: f64,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            frame_count: 0,
            last_instant: Instant::now(),
            fps: 0.0,
        }
    }

    fn update(&mut self) {
        self.frame_count += 1;
        let elapsed = self.last_instant.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.fps = self.frame_count as f64 / elapsed.as_secs_f64();
            self.frame_count = 0;
            self.last_instant = Instant::now();
        }
    }

    const fn fps(&self) -> usize {
        self.fps as usize
    }
}

#[derive(Clone, Copy)]
enum Palette {
    Plasma,
    Viridis,
    Fire,
    Ocean,
}

impl Palette {
    const fn name(self) -> &'static str {
        match self {
            Self::Plasma => "plasma",
            Self::Viridis => "viridis",
            Self::Fire => "fire",
            Self::Ocean => "ocean",
        }
    }

    const fn next(self) -> Self {
        match self {
            Self::Plasma => Self::Viridis,
            Self::Viridis => Self::Fire,
            Self::Fire => Self::Ocean,
            Self::Ocean => Self::Plasma,
        }
    }

    fn get_color(self, value: f64) -> Color {
        let value = value.clamp(0.0, 1.0);
        match self {
            Self::Plasma => match value {
                v if v < 0.22 => Color::Rgb(70, 22, 113),
                v if v < 0.42 => Color::Rgb(143, 27, 130),
                v if v < 0.62 => Color::Rgb(210, 72, 89),
                v if v < 0.82 => Color::Rgb(245, 136, 48),
                _ => Color::Rgb(252, 215, 72),
            },
            Self::Viridis => match value {
                v if v < 0.22 => Color::Rgb(68, 1, 84),
                v if v < 0.42 => Color::Rgb(59, 82, 139),
                v if v < 0.62 => Color::Rgb(33, 145, 140),
                v if v < 0.82 => Color::Rgb(94, 201, 98),
                _ => Color::Rgb(253, 231, 37),
            },
            Self::Fire => match value {
                v if v < 0.22 => Color::Rgb(70, 0, 0),
                v if v < 0.42 => Color::Rgb(150, 22, 11),
                v if v < 0.62 => Color::Rgb(220, 70, 18),
                v if v < 0.82 => Color::Rgb(250, 150, 40),
                _ => Color::Rgb(255, 235, 130),
            },
            Self::Ocean => match value {
                v if v < 0.22 => Color::Rgb(8, 42, 82),
                v if v < 0.42 => Color::Rgb(12, 93, 125),
                v if v < 0.62 => Color::Rgb(22, 145, 151),
                v if v < 0.82 => Color::Rgb(85, 190, 162),
                _ => Color::Rgb(190, 235, 185),
            },
        }
    }
}

struct Surface3D {
    rotation_x: f64,
    rotation_z: f64,
    zoom: f64,
    palette: Palette,
}

impl Surface3D {
    const fn new() -> Self {
        Self {
            rotation_x: 0.6,
            rotation_z: 0.3,
            zoom: 1.0,
            palette: Palette::Plasma,
        }
    }

    fn rotate_x(&mut self, delta: f64) {
        self.rotation_x = (self.rotation_x + delta).clamp(-PI / 2.0, PI / 2.0);
    }

    fn rotate_z(&mut self, delta: f64) {
        self.rotation_z = (self.rotation_z + delta).rem_euclid(TAU);
    }

    fn zoom(&mut self, factor: f64) {
        self.zoom = (self.zoom * factor).clamp(0.3, 3.0);
    }

    const fn palette_name(&self) -> &'static str {
        self.palette.name()
    }

    const fn cycle_palette(&mut self) {
        self.palette = self.palette.next();
    }

    fn render(&self, area: Rect, buf: &mut Buffer, surface_data: &[Vec<f64>]) {
        let n_exp = surface_data.len();
        let n_strike = surface_data.first().map_or(0, Vec::len);
        if n_exp == 0 || n_strike == 0 {
            return;
        }

        let (min_vol, max_vol) = Self::find_volatility_range(surface_data);
        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([-2.0, 2.0])
            .y_bounds([-1.5, 1.5])
            .paint(|ctx| {
                self.draw_strike_grid_lines(ctx, surface_data, n_exp, n_strike, min_vol, max_vol);
                self.draw_expiry_grid_lines(ctx, surface_data, n_exp, n_strike, min_vol, max_vol);
                self.draw_peak_highlights(ctx, surface_data, n_exp, n_strike, min_vol, max_vol);
            });

        canvas.render(area, buf);
    }

    fn find_volatility_range(surface_data: &[Vec<f64>]) -> (f64, f64) {
        let _ = surface_data;
        (MIN_DISPLAY_VOL, MAX_DISPLAY_VOL)
    }

    fn draw_strike_grid_lines(
        &self,
        ctx: &mut Context,
        surface_data: &[Vec<f64>],
        n_exp: usize,
        n_strike: usize,
        min_vol: f64,
        max_vol: f64,
    ) {
        for (i, row) in surface_data.iter().enumerate() {
            let points = self.project_row_to_points(row, i, n_exp, n_strike, min_vol, max_vol);
            let color = self
                .palette
                .get_color((i as f64 / n_exp as f64 * 0.7 + 0.3).min(1.0));
            Self::draw_line_strip(ctx, &points, color);
        }
    }

    fn draw_expiry_grid_lines(
        &self,
        ctx: &mut Context,
        surface_data: &[Vec<f64>],
        n_exp: usize,
        n_strike: usize,
        min_vol: f64,
        max_vol: f64,
    ) {
        for j in (0..n_strike).step_by(2) {
            let points =
                self.project_column_to_points(surface_data, j, n_exp, n_strike, min_vol, max_vol);
            let strike_norm = j as f64 / (n_strike - 1) as f64;
            let color = self.palette.get_color((strike_norm * 0.7 + 0.3).min(1.0));
            Self::draw_line_strip(ctx, &points, color);
        }
    }

    fn draw_peak_highlights(
        &self,
        ctx: &mut Context,
        surface_data: &[Vec<f64>],
        n_exp: usize,
        n_strike: usize,
        min_vol: f64,
        max_vol: f64,
    ) {
        let peak_points: Vec<(f64, f64)> = (0..n_exp)
            .step_by(2)
            .flat_map(|i| {
                (0..n_strike).step_by(2).filter_map(move |j| {
                    let vol_norm = (surface_data[i][j] - min_vol) / (max_vol - min_vol);
                    (vol_norm > 0.7).then(|| {
                        self.project_normalized_point(
                            j as f64 / (n_strike - 1) as f64,
                            i as f64 / (n_exp - 1) as f64,
                            vol_norm,
                        )
                    })
                })
            })
            .collect();

        if !peak_points.is_empty() {
            ctx.draw(&Points {
                coords: &peak_points,
                color: self.palette.get_color(0.9),
            });
        }
    }

    fn project_row_to_points(
        &self,
        row: &[f64],
        row_idx: usize,
        n_exp: usize,
        n_strike: usize,
        min_vol: f64,
        max_vol: f64,
    ) -> Vec<(f64, f64)> {
        let exp_norm = row_idx as f64 / (n_exp - 1) as f64;
        row.iter()
            .enumerate()
            .take(n_strike)
            .map(|(j, &vol)| {
                let strike_norm = j as f64 / (n_strike - 1) as f64;
                let vol_norm = (vol - min_vol) / (max_vol - min_vol);
                self.project_normalized_point(strike_norm, exp_norm, vol_norm)
            })
            .collect()
    }

    fn project_column_to_points(
        &self,
        surface_data: &[Vec<f64>],
        col_idx: usize,
        n_exp: usize,
        n_strike: usize,
        min_vol: f64,
        max_vol: f64,
    ) -> Vec<(f64, f64)> {
        let strike_norm = col_idx as f64 / (n_strike - 1) as f64;
        surface_data
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let exp_norm = i as f64 / (n_exp - 1) as f64;
                let vol_norm = (row[col_idx] - min_vol) / (max_vol - min_vol);
                self.project_normalized_point(strike_norm, exp_norm, vol_norm)
            })
            .collect()
    }

    fn project_normalized_point(
        &self,
        strike_norm: f64,
        exp_norm: f64,
        vol_norm: f64,
    ) -> (f64, f64) {
        let x = (strike_norm - 0.5) * 3.0;
        let y = (exp_norm - 0.5) * 3.0;
        let z = (vol_norm - 0.5) * 2.0;
        self.project(x, y, z)
    }

    fn project(&self, x: f64, y: f64, z: f64) -> (f64, f64) {
        let (sin_x, cos_x) = self.rotation_x.sin_cos();
        let (sin_z, cos_z) = self.rotation_z.sin_cos();

        let x1 = x * cos_z - y * sin_z;
        let y1 = x * sin_z + y * cos_z;
        let y2 = y1 * cos_x - z * sin_x;
        let z2 = y1 * sin_x + z * cos_x;
        let perspective = CAMERA_DISTANCE / (CAMERA_DISTANCE + z2);

        (x1 * perspective * self.zoom, y2 * perspective * self.zoom)
    }

    fn draw_line_strip(ctx: &mut Context, points: &[(f64, f64)], color: Color) {
        for window in points.windows(2) {
            ctx.draw(&Line {
                x1: window[0].0,
                y1: window[0].1,
                x2: window[1].0,
                y2: window[1].1,
                color,
            });
        }
    }
}

struct VolatilitySurface<'a> {
    surface_data: &'a [Vec<f64>],
}

impl<'a> VolatilitySurface<'a> {
    const fn new(surface_data: &'a [Vec<f64>]) -> Self {
        Self { surface_data }
    }
}

impl StatefulWidget for VolatilitySurface<'_> {
    type State = Surface3D;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.render(area, buf, self.surface_data);
    }
}

struct VolatilityEngine {
    strikes: Vec<f64>,
    expirations: Vec<f64>,
    surface: Vec<Vec<f64>>,
    base_vol: f64,
    skew: f64,
    term_structure: Vec<f64>,
    time: f64,
}

impl VolatilityEngine {
    fn new() -> Self {
        let strikes = (0..25).map(|i| 0.7 + f64::from(i) * 0.025).collect();
        let expirations = (0..20).map(|i| 0.02 + f64::from(i) * 0.1).collect();

        let mut engine = Self {
            strikes,
            expirations,
            surface: Vec::new(),
            base_vol: 20.0,
            skew: 0.3,
            term_structure: Vec::new(),
            time: 0.0,
        };
        engine.initialize();
        engine
    }

    fn initialize(&mut self) {
        self.term_structure = self
            .expirations
            .iter()
            .map(|&expiry| self.base_vol + 5.0 * (1.0 - (-expiry * 2.0).exp()))
            .collect();
        self.regenerate_surface(0.0);
    }

    fn update(&mut self) {
        self.time += 0.05;
        self.regenerate_surface(self.time);
    }

    fn reset(&mut self) {
        self.time = 0.0;
        self.regenerate_surface(self.time);
    }

    fn get_surface(&self) -> &[Vec<f64>] {
        &self.surface
    }

    fn regenerate_surface(&mut self, time: f64) {
        self.surface.clear();

        for (exp_idx, &expiry) in self.expirations.iter().enumerate() {
            let term_vol = self.term_structure[exp_idx];
            let time_wave = (time * 0.5 + exp_idx as f64 * 0.1).sin() * 20.0;
            let vol_shock = (time * 0.3).sin() * 1.5;
            let mut row = Vec::with_capacity(self.strikes.len());

            for (strike_idx, &strike) in self.strikes.iter().enumerate() {
                let log_moneyness = strike.ln();
                let short_end_damping = (expiry + 0.18).sqrt();
                let skew_component = -self.skew * log_moneyness * 100.0 / short_end_damping;
                let smile_component = 5.0 * log_moneyness.powi(2) / short_end_damping;
                let wing_component = if (0.95..=1.05).contains(&strike) {
                    0.0
                } else {
                    ((strike - 1.0).abs() - 0.05) * 20.0
                };
                let cluster = (time * 2.0 + strike * 10.0).sin().abs() * 1.5;
                let noise = deterministic_noise(exp_idx, strike_idx, time) * 0.5;
                let iv = term_vol
                    + skew_component
                    + smile_component
                    + wing_component
                    + time_wave
                    + vol_shock
                    + cluster
                    + noise;

                row.push(iv.clamp(5.0, 80.0));
            }
            self.surface.push(row);
        }
    }
}

fn deterministic_noise(exp_idx: usize, strike_idx: usize, time: f64) -> f64 {
    let seed = exp_idx as f64 * 12.989_8 + strike_idx as f64 * 78.233 + time * 0.37;
    (seed.sin() * 43_758.545_3).fract() - 0.5
}
