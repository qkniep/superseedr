// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::prelude::{Color, Frame};
use std::f64::consts::TAU;

use crate::theme::{
    blend_colors, color_to_rgb, ParticleProfile, ThemeContext, ThemeParticleEffect,
};

// Terminal cells are typically taller than wide. Scale Y up in radial math so circles render visually circular.
const BLACK_HOLE_Y_ASPECT: f64 = 2.0;

pub(crate) fn apply_theme_particles_background_to_frame(f: &mut Frame, ctx: &ThemeContext) {
    let particle = ctx.theme.effects.particle;
    if !particle.enabled || !particle.layer_mode.has_background() {
        return;
    }
    render_particles(f, ctx, particle, false);
}

pub(crate) fn apply_theme_particles_foreground_to_frame(f: &mut Frame, ctx: &ThemeContext) {
    let particle = ctx.theme.effects.particle;
    if !particle.enabled || !particle.layer_mode.has_foreground() {
        return;
    }
    render_particles(f, ctx, particle, true);
}

fn render_particles(
    f: &mut Frame,
    ctx: &ThemeContext,
    particle: ThemeParticleEffect,
    is_foreground: bool,
) {
    if matches!(particle.profile, ParticleProfile::None) {
        return;
    }

    let area = f.area();
    let width = area.width as f64;
    let height = area.height as f64;
    if width <= 0.0 || height <= 0.0 {
        return;
    }

    let area_scale = ((width * height) / 12_000.0).sqrt().max(1.0);
    let base_density = particle.density.max(0.001) as f64;
    let density = (base_density / area_scale).clamp(0.001, 0.20);
    let phase = ctx.frame_time * particle.speed.max(0.1) as f64;
    let glow = (particle.intensity as f64).clamp(0.1, 1.0);

    if matches!(particle.profile, ParticleProfile::BlackHole) {
        render_black_hole_particles(f, phase, density, glow, is_foreground);
        return;
    }

    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let local_x = x - area.left();
            let local_y = y - area.top();
            if let Some(cell) = buf.cell_mut((x, y)) {
                let underlying_fg = cell.fg;
                if let Some((glyph, color)) = sample_particle(
                    ctx,
                    particle.profile,
                    local_x as f64,
                    local_y as f64,
                    width,
                    height,
                    phase,
                    density,
                    glow,
                    underlying_fg,
                    is_foreground,
                ) {
                    cell.set_symbol(glyph);
                    cell.fg = color;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct BlackHoleBurst {
    active: bool,
    cx: f64,
    cy: f64,
    inner_radius: f64,
    outer_radius: f64,
    ring_radius: f64,
    spin_speed: f64,
    arm_count: f64,
    color_seed: f64,
}

fn render_black_hole_particles(
    f: &mut Frame,
    phase: f64,
    density: f64,
    glow: f64,
    is_foreground: bool,
) {
    if !is_foreground {
        return;
    }
    let area = f.area();
    let width = area.width as f64;
    let height = area.height as f64;
    if width <= 2.0 || height <= 2.0 {
        return;
    }

    let burst = black_hole_burst_state(width, height, phase);
    if !burst.active {
        return;
    }

    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let local_x = x as f64 - area.left() as f64;
            let local_y = y as f64 - area.top() as f64;
            let dx = local_x - burst.cx;
            let dy = local_y - burst.cy;
            let dy_scaled = dy * BLACK_HOLE_Y_ASPECT;
            let radius = (dx * dx + dy_scaled * dy_scaled).sqrt();

            if let Some(cell) = buf.cell_mut((x, y)) {
                if radius <= burst.inner_radius {
                    cell.set_symbol(" ");
                    cell.bg = Color::Rgb(0, 0, 0);
                    cell.fg = Color::Rgb(0, 0, 0);
                    continue;
                }

                if radius <= burst.ring_radius {
                    let ring_mix = (1.0
                        - ((radius - burst.inner_radius)
                            / (burst.ring_radius - burst.inner_radius + 0.0001)))
                        .clamp(0.0, 1.0);
                    let hue = (burst.color_seed + phase * 0.06 + ring_mix * 0.18).fract();
                    let ring_color = glow_color(
                        color_from_hsv(hue, 0.85, 0.92),
                        Color::White,
                        (0.24 * glow * ring_mix).clamp(0.0, 0.40),
                    );
                    cell.set_symbol("◌");
                    cell.fg = ring_color;
                    continue;
                }

                if radius > burst.outer_radius {
                    continue;
                }

                let theta = dy_scaled.atan2(dx);
                let normalized_r = (radius / burst.outer_radius).clamp(0.0, 1.0);
                let inward = 1.0 - normalized_r;
                let spiral = ((theta * burst.arm_count) + (radius * 0.34)
                    - (phase * burst.spin_speed))
                    .sin()
                    .abs();
                let trail = (spiral * 0.75) + (inward * 0.25);
                let jitter = hash01(local_x, local_y, phase * 0.8, 911.0);
                let threshold = (0.82 - density * 4.8 - inward * 0.25).clamp(0.25, 0.93);
                if trail + jitter * 0.2 < threshold {
                    continue;
                }

                let glyph = if inward > 0.8 && jitter > 0.7 {
                    "✦"
                } else if inward > 0.55 {
                    "•"
                } else if jitter > 0.66 {
                    "·"
                } else {
                    "."
                };

                let hue = (burst.color_seed
                    + hash01(local_x, local_y, phase, 919.0) * 0.35
                    + phase * 0.035
                    + (1.0 - normalized_r) * 0.18)
                    .fract();
                let sat = (0.72 + inward * 0.25).clamp(0.0, 1.0);
                let val = (0.66 + inward * 0.30).clamp(0.0, 1.0);
                let base = color_from_hsv(hue, sat, val);
                let color = glow_color(base, Color::White, (0.10 + inward * 0.22) * glow);
                cell.set_symbol(glyph);
                cell.fg = color;
            }
        }
    }
}

fn black_hole_burst_state(width: f64, height: f64, phase: f64) -> BlackHoleBurst {
    const WINDOW_SECS: f64 = 14.0;
    let slot = (phase / WINDOW_SECS).floor();
    let t = phase - slot * WINDOW_SECS;
    let active_len = 4.0 + hash01(slot, 0.0, 0.0, 801.0) * 5.0;
    let latest_start = (WINDOW_SECS - active_len).max(0.4);
    let start = hash01(slot, 0.0, 0.0, 809.0) * latest_start;
    let active = t >= start && t <= start + active_len;

    let min_dim = width.min(height).max(8.0);
    let outer_radius =
        (min_dim * (0.14 + hash01(slot, 0.0, 0.0, 817.0) * 0.20)).clamp(3.0, min_dim * 0.42);
    let inner_radius = (outer_radius * (0.30 + hash01(slot, 0.0, 0.0, 821.0) * 0.22))
        .clamp(1.8, outer_radius - 0.8);
    let ring_radius = inner_radius + (0.9 + hash01(slot, 0.0, 0.0, 823.0) * 1.8);

    let margin_x = outer_radius + 2.0;
    let margin_y = outer_radius + 1.5;
    let usable_w = (width - margin_x * 2.0).max(1.0);
    let usable_h = (height - margin_y * 2.0).max(1.0);
    let cx = margin_x + hash01(slot, 0.0, 0.0, 827.0) * usable_w;
    let cy = margin_y + hash01(slot, 0.0, 0.0, 829.0) * usable_h;

    BlackHoleBurst {
        active,
        cx,
        cy,
        inner_radius,
        outer_radius,
        ring_radius,
        spin_speed: 2.2 + hash01(slot, 0.0, 0.0, 839.0) * 2.4,
        arm_count: 2.0 + (hash01(slot, 0.0, 0.0, 853.0) * 3.0).floor(),
        color_seed: hash01(slot, 0.0, 0.0, 857.0),
    }
}

fn sample_particle(
    ctx: &ThemeContext,
    profile: ParticleProfile,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    phase: f64,
    density: f64,
    glow: f64,
    underlying_fg: Color,
    reactive_tint: bool,
) -> Option<(&'static str, Color)> {
    match profile {
        ParticleProfile::Sakura => sample_sakura(
            ctx,
            x,
            y,
            width,
            height,
            phase,
            density,
            glow,
            underlying_fg,
            reactive_tint,
        ),
        ParticleProfile::Matrix => sample_matrix(
            ctx,
            x,
            y,
            width,
            height,
            phase,
            density,
            glow,
            underlying_fg,
        ),
        ParticleProfile::Diamond => sample_diamond(ctx, x, y, width, height, phase, density, glow),
        ParticleProfile::BioluminescentReef => sample_bioluminescent_reef(
            ctx,
            x,
            y,
            width,
            height,
            phase,
            density,
            glow,
            underlying_fg,
        ),
        ParticleProfile::BlackHole => None,
        ParticleProfile::None => None,
    }
}

fn sample_diamond(
    ctx: &ThemeContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    phase: f64,
    density: f64,
    glow: f64,
) -> Option<(&'static str, Color)> {
    let w = width.max(2.0);
    let h = height.max(2.0);
    let nx = x / (w - 1.0);
    let ny = y / (h - 1.0);
    let drift_x = (phase * 1.18) + ((ny * 6.0) + phase * 0.33).sin() * 0.35;
    let drift_y = (phase * 0.52) + ((nx * 4.4) - phase * 0.27).cos() * 0.22;
    let field_x = x - drift_x;
    let field_y = y + drift_y;
    let density_bias = (1.0 - density).clamp(0.0, 1.0);

    // Rare large 3x3 facets for noticeable size variation.
    let huge_x = (field_x / 3.0).floor();
    let huge_y = (field_y / 3.0).floor();
    let huge_seed = hash01(huge_x, huge_y, 0.0, 739.0);
    let huge_cluster = (((huge_x * 0.26) + (huge_y * 0.17) + phase * 0.04).sin() * 0.5) + 0.5;
    if huge_seed > 0.92 + density_bias * 0.06 && huge_cluster > 0.61 {
        let huge_twinkle = ((phase * 0.56) + (huge_seed * TAU)).sin() * 0.5 + 0.5;
        let huge_depth = (((nx * 1.7) + (ny * 1.5) - phase * 0.02).cos() * 0.5) + 0.5;
        let huge_base = if huge_twinkle > 0.64 {
            ctx.theme.semantic.white
        } else if huge_depth > 0.52 {
            ctx.theme.scale.categorical.sky
        } else {
            ctx.theme.scale.categorical.sapphire
        };
        let huge_shine = (0.10 + huge_twinkle * 0.26 + huge_cluster * 0.10).clamp(0.0, 0.46);
        return Some((
            "=",
            glow_color(
                huge_base,
                ctx.theme.semantic.white,
                (glow * huge_shine).clamp(0.0, 0.46),
            ),
        ));
    }

    // Medium 2x2 facets.
    let big_x = (field_x / 2.0).floor();
    let big_y = (field_y / 2.0).floor();
    let big_seed = hash01(big_x, big_y, 0.0, 743.0);
    let big_cluster = (((big_x * 0.33) + (big_y * 0.19) + phase * 0.06).sin() * 0.5) + 0.5;
    if big_seed > 0.82 + density_bias * 0.12 && big_cluster > 0.58 {
        let big_twinkle = ((phase * 0.62) + (big_seed * TAU)).sin() * 0.5 + 0.5;
        let big_depth = (((nx * 2.1) + (ny * 1.9) - phase * 0.03).cos() * 0.5) + 0.5;
        let big_base = if big_twinkle > 0.66 {
            ctx.theme.semantic.white
        } else if big_depth > 0.54 {
            ctx.theme.scale.categorical.sky
        } else {
            ctx.theme.scale.categorical.sapphire
        };
        let big_shine = (0.08 + big_twinkle * 0.24 + big_cluster * 0.08).clamp(0.0, 0.42);
        return Some((
            "=",
            glow_color(
                big_base,
                ctx.theme.semantic.white,
                (glow * big_shine).clamp(0.0, 0.42),
            ),
        ));
    }

    // Coarse lattice with per-cell jitter keeps placement visibly uneven.
    let grid_band = hash01(0.0, (field_y / 6.0).floor(), 0.0, 757.0);
    let grid_w = if grid_band > 0.55 { 7.0 } else { 10.0 };
    let grid_h = if grid_band > 0.55 { 3.0 } else { 5.0 };
    let gx = (field_x / grid_w).floor();
    let gy = (field_y / grid_h).floor();
    let cell_seed = hash01(gx, gy, 0.0, 701.0);
    if cell_seed < 0.52 + density_bias * 0.30 {
        return None;
    }

    let twinkle_phase = (phase * (0.70 + cell_seed * 0.45)) + (cell_seed * TAU);
    let center_x = ((gx + 0.5) * grid_w)
        + (hash01(gx, gy, 0.0, 709.0) - 0.5) * 1.8
        + twinkle_phase.sin() * 0.28;
    let center_y = ((gy + 0.5) * grid_h)
        + (hash01(gx, gy, 0.0, 719.0) - 0.5) * 1.3
        + (twinkle_phase * 0.9).cos() * 0.20;
    let dx = x - center_x;
    let dy = y - center_y;
    let dist = (dx * dx + dy * dy).sqrt();

    let size_seed = hash01(gx, gy, 0.0, 727.0);
    let radius = 0.58 + size_seed * 1.05;
    if dist > radius {
        return None;
    }

    // Slower twinkle than drift, with lane motifs that form --==-- and --==.
    let twinkle = ((phase * 1.10) + (cell_seed * TAU)).sin() * 0.5 + 0.5;
    let edge_falloff = (1.0 - (dist / radius)).clamp(0.0, 1.0);
    let lane = (field_y / 2.0).floor();
    let motif_seed = hash01(gx, gy, 0.0, 761.0);
    let motif_len = if motif_seed > 0.54 { 6 } else { 4 };
    let motif_idx = ((field_x + lane * 0.7 + phase * 1.6).floor() as i32).rem_euclid(motif_len);
    let motif_core = match motif_len {
        6 => matches!(motif_idx, 2 | 3), // --==--
        _ => motif_idx >= 2,             // --==
    };
    let glyph = if (edge_falloff > 0.82 && twinkle > 0.76) || (motif_core && twinkle > 0.68) {
        "="
    } else {
        "-"
    };

    let depth_band = (((nx * 3.4) + (ny * 2.8) - phase * 0.04).sin() * 0.5) + 0.5;
    let base = if twinkle > 0.68 {
        ctx.theme.semantic.white
    } else if depth_band > 0.52 {
        ctx.theme.scale.categorical.sky
    } else {
        ctx.theme.scale.categorical.sapphire
    };
    let highlight = glow_color(
        ctx.theme.semantic.white,
        ctx.theme.scale.categorical.sky,
        0.14,
    );
    let shine = (0.04 + edge_falloff * 0.11 + twinkle * 0.20).clamp(0.0, 0.34);

    Some((
        glyph,
        glow_color(base, highlight, (glow * shine).clamp(0.0, 0.34)),
    ))
}

fn sample_sakura(
    ctx: &ThemeContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    phase: f64,
    density: f64,
    glow: f64,
    underlying_fg: Color,
    reactive_tint: bool,
) -> Option<(&'static str, Color)> {
    let _ = (width, height);
    // Sakura intentionally reuses the original flowers-style motion profile.
    let drift = ((x * 0.12) - (phase * 1.9)).sin() + ((y * 0.07) - (phase * 1.2)).cos();
    let grain = ((x * 0.31) + (y * 0.15) - phase).sin();
    let score = (drift * 0.7) + (grain * 0.3);
    let threshold = 1.75 - density * 9.0;
    if score < threshold {
        return None;
    }

    let pick = hash01(x, y, phase, 24.0);
    let glyph = if pick > 0.80 {
        "o"
    } else if pick > 0.55 {
        "*"
    } else if pick > 0.30 {
        "+"
    } else {
        "."
    };
    let palette = [
        ctx.theme.scale.categorical.pink,
        ctx.theme.scale.categorical.pink,
        ctx.theme.scale.categorical.pink,
        ctx.theme.scale.categorical.flamingo,
        ctx.theme.scale.categorical.rosewater,
        ctx.theme.scale.categorical.flamingo,
    ];
    let mut base = palette[(pick * palette.len() as f64) as usize % palette.len()];
    if reactive_tint && !matches!(underlying_fg, Color::Reset) {
        base = glow_color(base, underlying_fg, 0.08);
    }
    Some((
        glyph,
        glow_color(base, ctx.theme.semantic.white, glow * 0.10),
    ))
}

fn sample_matrix(
    ctx: &ThemeContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    phase: f64,
    density: f64,
    glow: f64,
    underlying_fg: Color,
) -> Option<(&'static str, Color)> {
    let _ = (width, underlying_fg);
    let h = height.max(2.0);
    let col = x.floor();

    // Columns randomly phase in/out over time windows.
    let window_t = (phase * 1.35).floor();
    let col_seed = hash01(col, window_t, 0.0, 101.0);
    let active = col_seed > (0.32 + (1.0 - density).clamp(0.0, 1.0) * 0.28);
    if !active {
        return None;
    }

    // Per-column falling head and string length.
    let speed = 3.0 + hash01(col, 0.0, phase, 17.0) * 3.2;
    let head = (phase * speed + hash01(col, 0.0, 0.0, 23.0) * h).rem_euclid(h);
    let len = (4.0 + hash01(col, 0.0, phase, 29.0) * (h * 0.28)).clamp(4.0, h * 0.45);
    let dy = (head - y).rem_euclid(h);
    if dy > len {
        return None;
    }

    // Random dropout inside active strings creates hacking in/out behavior.
    let dropout = hash01(col, y.floor(), (phase * 8.0).floor(), 59.0);
    if dropout < 0.18 {
        return None;
    }

    let pick = hash01(col, (y * 0.61).floor(), (phase * 12.0).floor(), 53.0);
    let glyph = if pick > 0.88 {
        "1"
    } else if pick > 0.76 {
        "0"
    } else if pick > 0.64 {
        "7"
    } else if pick > 0.52 {
        "3"
    } else if pick > 0.40 {
        "9"
    } else if pick > 0.30 {
        "|"
    } else {
        ":"
    };

    let tail_ratio = if len <= 0.001 {
        0.0
    } else {
        (dy / len).clamp(0.0, 1.0)
    };
    let base = if tail_ratio < 0.08 {
        ctx.theme.semantic.white
    } else if tail_ratio < 0.24 {
        ctx.theme.scale.categorical.sky
    } else if tail_ratio < 0.55 {
        ctx.theme.scale.categorical.teal
    } else {
        ctx.theme.scale.categorical.green
    };

    Some((
        glyph,
        glow_color(
            base,
            ctx.theme.semantic.white,
            (glow * 0.18).clamp(0.0, 0.24),
        ),
    ))
}

fn sample_bioluminescent_reef(
    ctx: &ThemeContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    phase: f64,
    density: f64,
    glow: f64,
    underlying_fg: Color,
) -> Option<(&'static str, Color)> {
    let _ = underlying_fg;
    let w = width.max(2.0);
    let h = height.max(2.0);
    let nx = x / (w - 1.0);
    let ny = y / (h - 1.0);
    let area_scale = ((w * h) / 10_000.0).sqrt().clamp(0.85, 1.6);

    // Use a low-frequency cluster mask so particles form patches instead of uniform noise.
    let current_x = x - (phase * 0.82)
        + ((ny * 8.0) + phase * 0.21).sin() * 1.2
        + ((ny * 2.9) - phase * 0.09).cos() * 0.5;
    let current_y = y
        + (phase * 0.46)
        + ((nx * 7.0) - phase * 0.19).cos() * 0.85
        + ((nx * 3.3) + phase * 0.12).sin() * 0.45;
    let field_a = ((current_x * 0.16) + (current_y * 0.10) + phase * 0.04)
        .sin()
        .abs();
    let field_b = ((current_x * 0.06) - (current_y * 0.14) + phase * 0.30)
        .cos()
        .abs();
    let eddy = (((nx * 10.0) - (ny * 7.0) + phase * 0.35).sin() * 0.5) + 0.5;
    let field = (field_a * 0.45) + (field_b * 0.30) + (eddy * 0.25);
    let cell_w = (11.0 * area_scale).max(7.0);
    let cell_h = (6.5 * area_scale).max(5.0);
    let cx = (x / cell_w).floor();
    let cy = (y / cell_h).floor();
    let cell_seed = hash01(cx, cy, 0.0, 311.0);
    let jitter_x = (hash01(cx, cy, 0.0, 313.0) - 0.5) * cell_w * 0.55;
    let jitter_y = (hash01(cx, cy, 0.0, 317.0) - 0.5) * cell_h * 0.50;
    let center_x = ((cx + 0.5) * cell_w) + jitter_x + phase * (0.06 + cell_seed * 0.03);
    let center_y = ((cy + 0.5) * cell_h) + jitter_y + phase * (0.03 + cell_seed * 0.02);
    let dx = x - center_x;
    let dy = y - center_y;
    let dist = (dx * dx + dy * dy).sqrt();
    let radius = (1.0 + (cell_seed * 2.8)) * area_scale;
    let blob = (1.0 - (dist / radius)).clamp(0.0, 1.0);
    let cluster_a = (((nx * 4.0) - (ny * 2.7) + phase * 0.10).sin() * 0.5) + 0.5;
    let cluster_b = (((nx * 2.0) + (ny * 2.2) - phase * 0.07).cos() * 0.5) + 0.5;
    let cluster_mask = (blob * 0.62) + (cluster_a * 0.23) + (cluster_b * 0.15);

    let sparkle_seed = hash01(current_x * 0.77, current_y * 0.91, 0.0, 149.0);
    let pulse = ((phase * 0.24) + (sparkle_seed * TAU)).sin() * 0.5 + 0.5;
    let threshold = 0.88 + ((1.0 - density).clamp(0.0, 1.0) * 0.06) - (pulse * 0.03);
    if field < threshold || cluster_mask < 0.50 || sparkle_seed < 0.54 {
        return None;
    }

    let pick = hash01(x, y, 0.0, 163.0);
    let glyph = if blob > 0.78 && pick > 0.74 {
        "•"
    } else if blob > 0.56 && pick > 0.62 {
        "·"
    } else if pick > 0.88 {
        "·"
    } else if pick > 0.56 {
        "."
    } else if pick > 0.28 {
        "•"
    } else {
        "∙"
    };

    let depth = hash01(x * 0.41, y * 0.73, 0.0, 173.0);
    let base = if depth > 0.85 {
        ctx.theme.scale.categorical.sky
    } else if depth > 0.56 {
        ctx.theme.scale.categorical.teal
    } else {
        ctx.theme.scale.categorical.green
    };
    let shimmer = (0.04 + depth * 0.10 + pulse * 0.07 + eddy * 0.05 + blob * 0.06).clamp(0.0, 0.22);

    Some((
        glyph,
        glow_color(
            base,
            ctx.theme.semantic.white,
            (glow * shimmer).clamp(0.0, 0.22),
        ),
    ))
}

fn hash01(x: f64, y: f64, phase: f64, salt: f64) -> f64 {
    let n = ((x * 12.9898) + (y * 78.233) + (phase * 37.719) + salt).sin() * 43758.5453;
    n.fract().abs()
}

fn glow_color(base: Color, highlight: Color, amount: f64) -> Color {
    blend_colors(
        color_to_rgb(base),
        color_to_rgb(highlight),
        amount.clamp(0.0, 0.65),
    )
}

fn color_from_hsv(h: f64, s: f64, v: f64) -> Color {
    let hue = (h.fract() * 6.0).clamp(0.0, 5.999_999);
    let i = hue.floor() as i32;
    let f = hue - i as f64;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
