// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeName {
    CatppuccinMocha,
    Neon,
    #[serde(alias = "candly_land_pink")]
    CandyLandPink,
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
            ThemeName::Neon => Self::neon(),
            ThemeName::CandyLandPink => Self::candy_land_pink(),
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

    pub fn neon() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 220, 245),
            flamingo: Color::Rgb(255, 150, 230),
            pink: Color::Rgb(255, 70, 230),
            mauve: Color::Rgb(210, 90, 255),
            red: Color::Rgb(255, 60, 120),
            maroon: Color::Rgb(255, 90, 160),
            peach: Color::Rgb(255, 170, 80),
            yellow: Color::Rgb(255, 240, 90),
            green: Color::Rgb(100, 255, 190),
            teal: Color::Rgb(0, 255, 255),
            sky: Color::Rgb(80, 220, 255),
            sapphire: Color::Rgb(40, 190, 255),
            blue: Color::Rgb(40, 110, 255),
            lavender: Color::Rgb(190, 170, 255),
        };

        Self {
            name: ThemeName::Neon,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 18.0,
                flicker_intensity: 0.35,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(230, 255, 255),
                subtext1: Color::Rgb(140, 230, 245),
                subtext0: Color::Rgb(90, 200, 220),
                overlay0: Color::Rgb(30, 70, 95),
                surface2: Color::Rgb(18, 40, 64),
                surface1: Color::Rgb(12, 30, 52),
                surface0: Color::Rgb(8, 22, 42),
                border: Color::Rgb(18, 40, 64),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(200, 255, 255),
                    Color::Rgb(120, 255, 240),
                    Color::Rgb(60, 245, 255),
                    Color::Rgb(80, 190, 255),
                    Color::Rgb(170, 120, 255),
                    Color::Rgb(255, 90, 230),
                    Color::Rgb(255, 60, 190),
                    Color::Rgb(255, 40, 150),
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
                    medium: categorical.pink,
                    high: categorical.teal,
                    empty: Color::Rgb(30, 45, 65),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.green,
                    midground: categorical.blue,
                    background: Color::Rgb(40, 60, 80),
                },
                categorical,
            },
        }
    }

    pub fn candy_land_pink() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 228, 241),
            flamingo: Color::Rgb(255, 204, 231),
            pink: Color::Rgb(255, 176, 224),
            mauve: Color::Rgb(236, 178, 255),
            red: Color::Rgb(255, 154, 198),
            maroon: Color::Rgb(255, 170, 210),
            peach: Color::Rgb(255, 196, 168),
            yellow: Color::Rgb(255, 236, 170),
            green: Color::Rgb(198, 235, 200),
            teal: Color::Rgb(176, 228, 220),
            sky: Color::Rgb(176, 214, 255),
            sapphire: Color::Rgb(156, 198, 255),
            blue: Color::Rgb(136, 180, 255),
            lavender: Color::Rgb(214, 186, 255),
        };

        Self {
            name: ThemeName::CandyLandPink,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(255, 245, 252),
                subtext1: Color::Rgb(246, 218, 236),
                subtext0: Color::Rgb(232, 190, 218),
                overlay0: Color::Rgb(208, 160, 190),
                surface2: Color::Rgb(190, 128, 170),
                surface1: Color::Rgb(170, 110, 150),
                surface0: Color::Rgb(152, 94, 134),
                border: Color::Rgb(190, 128, 170),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(255, 248, 252),
                    Color::Rgb(255, 224, 241),
                    Color::Rgb(255, 204, 234),
                    Color::Rgb(255, 184, 226),
                    Color::Rgb(255, 164, 218),
                    Color::Rgb(255, 144, 210),
                    Color::Rgb(255, 124, 200),
                    Color::Rgb(255, 104, 192),
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
                    categorical.lavender,
                    categorical.sky,
                    categorical.sapphire,
                    categorical.blue,
                    categorical.teal,
                    categorical.green,
                ],
                heatmap: ThemeHeatmap {
                    low: categorical.rosewater,
                    medium: categorical.pink,
                    high: categorical.mauve,
                    empty: Color::Rgb(170, 110, 150),
                },
                stream: ThemeStream {
                    inflow: categorical.sky,
                    outflow: categorical.pink,
                },
                dust: ThemeDust {
                    foreground: categorical.pink,
                    midground: categorical.lavender,
                    background: Color::Rgb(190, 128, 170),
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
