// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeName {
    CatppuccinMocha,
}

impl Default for ThemeName {
    fn default() -> Self {
        Self::CatppuccinMocha
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ThemeEffects {
    pub glow_enabled: bool,
    pub flicker_hz: f32,
    pub flicker_intensity: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeSemantic {
    pub text: Color,
    pub subtext0: Color,
    pub subtext1: Color,
    pub overlay0: Color,
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,
    pub border: Color,
    pub white: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeHeatmap {
    pub low: Color,
    pub medium: Color,
    pub high: Color,
    pub empty: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeStream {
    pub inflow: Color,
    pub outflow: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeDust {
    pub foreground: Color,
    pub midground: Color,
    pub background: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeCategorical {
    pub rosewater: Color,
    pub flamingo: Color,
    pub pink: Color,
    pub mauve: Color,
    pub red: Color,
    pub maroon: Color,
    pub peach: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub sky: Color,
    pub sapphire: Color,
    pub blue: Color,
    pub lavender: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemeScale {
    pub speed: [Color; 8],
    pub ip_hash: [Color; 14],
    pub heatmap: ThemeHeatmap,
    pub stream: ThemeStream,
    pub dust: ThemeDust,
    pub categorical: ThemeCategorical,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: ThemeName,
    pub effects: ThemeEffects,
    pub semantic: ThemeSemantic,
    pub scale: ThemeScale,
}

impl Theme {
    pub fn builtin(name: ThemeName) -> Self {
        match name {
            ThemeName::CatppuccinMocha => Self::catppuccin_mocha(),
        }
    }

    pub fn catppuccin_mocha() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(245, 224, 220),
            flamingo: Color::Rgb(242, 205, 205),
            pink: Color::Rgb(245, 194, 231),
            mauve: Color::Rgb(203, 166, 247),
            red: Color::Rgb(243, 139, 168),
            maroon: Color::Rgb(235, 160, 172),
            peach: Color::Rgb(250, 179, 135),
            yellow: Color::Rgb(249, 226, 175),
            green: Color::Rgb(166, 227, 161),
            teal: Color::Rgb(148, 226, 213),
            sky: Color::Rgb(137, 220, 235),
            sapphire: Color::Rgb(116, 199, 236),
            blue: Color::Rgb(137, 180, 250),
            lavender: Color::Rgb(180, 190, 254),
        };

        Self {
            name: ThemeName::CatppuccinMocha,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(205, 214, 244),
                subtext1: Color::Rgb(186, 194, 222),
                subtext0: Color::Rgb(166, 173, 200),
                overlay0: Color::Rgb(108, 112, 134),
                surface2: Color::Rgb(88, 91, 112),
                surface1: Color::Rgb(69, 71, 90),
                surface0: Color::Rgb(49, 50, 68),
                border: Color::Rgb(88, 91, 112),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.maroon,
                    categorical.red,
                    categorical.flamingo,
                    categorical.pink,
                ],
                ip_hash: [
                    categorical.rosewater,
                    categorical.flamingo,
                    categorical.pink,
                    categorical.mauve,
                    categorical.red,
                    categorical.maroon,
                    categorical.peach,
                    categorical.yellow,
                    categorical.green,
                    categorical.teal,
                    categorical.sky,
                    categorical.sapphire,
                    categorical.blue,
                    categorical.lavender,
                ],
                heatmap: ThemeHeatmap {
                    low: categorical.mauve,
                    medium: categorical.mauve,
                    high: categorical.mauve,
                    empty: Color::Rgb(69, 71, 90),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.green,
                    midground: categorical.blue,
                    background: Color::Rgb(88, 91, 112),
                },
                categorical,
            },
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::catppuccin_mocha()
    }
}
