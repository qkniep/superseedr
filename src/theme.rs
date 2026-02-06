// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use ratatui::style::{Color, Style, Modifier};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Deserializer, Serialize};
use strum_macros::{Display, EnumIter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, Display)]
pub enum ThemeName {
    #[strum(serialize = "Andromeda")]
    Andromeda,
    #[strum(serialize = "Aurora")]
    Aurora,
    #[strum(serialize = "Ayu Dark")]
    AyuDark,
    #[strum(serialize = "Bubblegum")]
    Bubblegum,
    #[strum(serialize = "Catppuccin Latte")]
    CatppuccinLatte,
    #[strum(serialize = "Catppuccin Mocha")]
    CatppuccinMocha,
    #[strum(serialize = "Cyberpunk")]
    Cyberpunk,
    #[strum(serialize = "Dracula")]
    Dracula,
    #[strum(serialize = "Everforest Dark")]
    EverforestDark,
    #[strum(serialize = "GitHub Dark")]
    GitHubDark,
    #[strum(serialize = "GitHub Light")]
    GitHubLight,
    #[strum(serialize = "Gruvbox Dark")]
    GruvboxDark,
    #[strum(serialize = "Gruvbox Light")]
    GruvboxLight,
    #[strum(serialize = "Inferno")]
    Inferno,
    #[strum(serialize = "Kanagawa")]
    Kanagawa,
    #[strum(serialize = "Material Ocean")]
    MaterialOcean,
    #[strum(serialize = "Matrix")]
    Matrix,
    #[strum(serialize = "Monokai")]
    Monokai,
    #[strum(serialize = "Neon")]
    Neon,
    #[strum(serialize = "Nord")]
    Nord,
    #[strum(serialize = "One Dark")]
    OneDark,
    #[strum(serialize = "Oxocarbon")]
    Oxocarbon,
    #[strum(serialize = "Rainbow")]
    Rainbow,
    #[strum(serialize = "Rose Pine")]
    RosePine,
    #[strum(serialize = "Solarized Dark")]
    SolarizedDark,
    #[strum(serialize = "Solarized Light")]
    SolarizedLight,
    #[strum(serialize = "Synthwave '84")]
    Synthwave84,
    #[strum(serialize = "Tokyo Night")]
    TokyoNight,
    #[strum(serialize = "Vesper")]
    Vesper,
    #[strum(serialize = "Zenburn")]
    Zenburn,
}

impl Default for ThemeName {
    fn default() -> Self {
        Self::CatppuccinMocha
    }
}

impl Serialize for ThemeName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            ThemeName::Andromeda => "andromeda",
            ThemeName::Aurora => "aurora",
            ThemeName::AyuDark => "ayu_dark",
            ThemeName::Bubblegum => "bubblegum",
            ThemeName::CatppuccinLatte => "catppuccin_latte",
            ThemeName::CatppuccinMocha => "catppuccin_mocha",
            ThemeName::Cyberpunk => "cyberpunk",
            ThemeName::Dracula => "dracula",
            ThemeName::EverforestDark => "everforest_dark",
            ThemeName::GitHubDark => "github_dark",
            ThemeName::GitHubLight => "github_light",
            ThemeName::GruvboxDark => "gruvbox_dark",
            ThemeName::GruvboxLight => "gruvbox_light",
            ThemeName::Inferno => "inferno",
            ThemeName::Kanagawa => "kanagawa",
            ThemeName::MaterialOcean => "material_ocean",
            ThemeName::Matrix => "matrix",
            ThemeName::Monokai => "monokai",
            ThemeName::Neon => "neon",
            ThemeName::Nord => "nord",
            ThemeName::OneDark => "one_dark",
            ThemeName::Oxocarbon => "oxocarbon",
            ThemeName::Rainbow => "rainbow",
            ThemeName::RosePine => "rose_pine",
            ThemeName::SolarizedDark => "solarized_dark",
            ThemeName::SolarizedLight => "solarized_light",
            ThemeName::Synthwave84 => "synthwave_84",
            ThemeName::TokyoNight => "tokyo_night",
            ThemeName::Vesper => "vesper",
            ThemeName::Zenburn => "zenburn",
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for ThemeName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "andromeda" | "Andromeda" => Ok(ThemeName::Andromeda),
            "aurora" | "Aurora" => Ok(ThemeName::Aurora),
            "ayu_dark" | "Ayu Dark" => Ok(ThemeName::AyuDark),
            "bubblegum" | "Bubblegum" => Ok(ThemeName::Bubblegum),
            "catppuccin_latte" | "Catppuccin Latte" => Ok(ThemeName::CatppuccinLatte),
            "catppuccin_mocha" | "Catppuccin Mocha" => Ok(ThemeName::CatppuccinMocha),
            "cyberpunk" | "Cyberpunk" => Ok(ThemeName::Cyberpunk),
            "dracula" | "Dracula" => Ok(ThemeName::Dracula),
            "everforest_dark" | "Everforest Dark" => Ok(ThemeName::EverforestDark),
            "github_dark" | "GitHub Dark" => Ok(ThemeName::GitHubDark),
            "github_light" | "GitHub Light" => Ok(ThemeName::GitHubLight),
            "gruvbox_dark" | "Gruvbox Dark" => Ok(ThemeName::GruvboxDark),
            "gruvbox_light" | "Gruvbox Light" => Ok(ThemeName::GruvboxLight),
            "inferno" | "Inferno" => Ok(ThemeName::Inferno),
            "kanagawa" | "Kanagawa" => Ok(ThemeName::Kanagawa),
            "material_ocean" | "Material Ocean" => Ok(ThemeName::MaterialOcean),
            "matrix" | "Matrix" => Ok(ThemeName::Matrix),
            "monokai" | "Monokai" => Ok(ThemeName::Monokai),
            "neon" | "Neon" => Ok(ThemeName::Neon),
            "nord" | "Nord" => Ok(ThemeName::Nord),
            "one_dark" | "One Dark" => Ok(ThemeName::OneDark),
            "oxocarbon" | "Oxocarbon" => Ok(ThemeName::Oxocarbon),
            "rainbow" | "Rainbow" => Ok(ThemeName::Rainbow),
            "rose_pine" | "Rose Pine" => Ok(ThemeName::RosePine),
            "solarized_dark" | "Solarized Dark" => Ok(ThemeName::SolarizedDark),
            "solarized_light" | "Solarized Light" => Ok(ThemeName::SolarizedLight),
            "synthwave_84" | "Synthwave '84" => Ok(ThemeName::Synthwave84),
            "tokyo_night" | "Tokyo Night" => Ok(ThemeName::TokyoNight),
            "vesper" | "Vesper" => Ok(ThemeName::Vesper),
            "zenburn" | "Zenburn" => Ok(ThemeName::Zenburn),
            _ => {
                // Unknown theme - default to CatppuccinMocha
                Ok(ThemeName::CatppuccinMocha)
            }
        }
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

pub fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::White => (255, 255, 255),
        Color::Black => (0, 0, 0),
        _ => (255, 255, 255),
    }
}

pub fn blend_colors(c1: (u8, u8, u8), c2: (u8, u8, u8), ratio: f64) -> Color {
    let r = (c1.0 as f64 * (1.0 - ratio) + c2.0 as f64 * ratio) as u8;
    let g = (c1.1 as f64 * (1.0 - ratio) + c2.1 as f64 * ratio) as u8;
    let b = (c1.2 as f64 * (1.0 - ratio) + c2.2 as f64 * ratio) as u8;
    Color::Rgb(r, g, b)
}

pub fn apply_theme_effects(style: Style, theme: &Theme) -> Style {
    if !theme.effects.glow_enabled {
        return style;
    }

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    // frequency and intensity from theme
    let freq = theme.effects.flicker_hz as f64;
    let intensity = theme.effects.flicker_intensity as f64;

    if intensity <= 0.001 {
        return style;
    }

    if let Some(fg) = style.fg {
        let (r, g, b) = color_to_rgb(fg);

        // Interference Pattern Algorithm:
        // 1. Global Rhythm: A steady pulse shared by everyone (sync anchor)
        // 2. Local Drift: A faster, phase-shifted pulse unique to the color (chaos)
        
        // Color-based phase shift
        let phase_offset = (r as f64 * 3.0 + g as f64 * 5.0 + b as f64 * 7.0) * 0.01;

        // Base wave (Global sync)
        let base_wave = (time * freq).sin();
        
        // Offset wave (Local drift) - 1.4x speed creates a polyrhythm
        let drift_wave = ((time * freq * 1.4) + phase_offset).sin();

        // Combined wave [-1.0, 1.0]
        let wave = (base_wave + drift_wave) / 2.0;

        let final_color = if wave > 0.0 {
            // Positive cycle: Glow (mix with White)
            let factor = wave * intensity;
            blend_colors((r, g, b), (255, 255, 255), factor)
        } else {
            // Negative cycle: Dim (mix with Black)
            // We use a slightly reduced intensity for dimming to preserve legibility
            let factor = wave.abs() * (intensity * 0.8); 
            blend_colors((r, g, b), (0, 0, 0), factor)
        };

        style.fg(final_color)
    } else {
        style
    }
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
            ThemeName::Andromeda => Self::andromeda(),
            ThemeName::Aurora => Self::aurora(),
            ThemeName::AyuDark => Self::ayu_dark(),
            ThemeName::Bubblegum => Self::bubblegum(),
            ThemeName::CatppuccinLatte => Self::catppuccin_latte(),
            ThemeName::CatppuccinMocha => Self::catppuccin_mocha(),
            ThemeName::Cyberpunk => Self::cyberpunk(),
            ThemeName::Dracula => Self::dracula(),
            ThemeName::EverforestDark => Self::everforest_dark(),
            ThemeName::GitHubDark => Self::github_dark(),
            ThemeName::GitHubLight => Self::github_light(),
            ThemeName::GruvboxDark => Self::gruvbox_dark(),
            ThemeName::GruvboxLight => Self::gruvbox_light(),
            ThemeName::Inferno => Self::inferno(),
            ThemeName::Kanagawa => Self::kanagawa(),
            ThemeName::MaterialOcean => Self::material_ocean(),
            ThemeName::Matrix => Self::matrix(),
            ThemeName::Monokai => Self::monokai(),
            ThemeName::Neon => Self::neon(),
            ThemeName::Nord => Self::nord(),
            ThemeName::OneDark => Self::one_dark(),
            ThemeName::Oxocarbon => Self::oxocarbon(),
            ThemeName::Rainbow => Self::rainbow(),
            ThemeName::RosePine => Self::rose_pine(),
            ThemeName::SolarizedDark => Self::solarized_dark(),
            ThemeName::SolarizedLight => Self::solarized_light(),
            ThemeName::Synthwave84 => Self::synthwave_84(),
            ThemeName::TokyoNight => Self::tokyo_night(),
            ThemeName::Vesper => Self::vesper(),
            ThemeName::Zenburn => Self::zenburn(),
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
                ip_hash: categorical_ip_hash(categorical),
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
                flicker_hz: 3.0,
                flicker_intensity: 0.6,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(230, 255, 255),
                subtext1: Color::Rgb(140, 230, 245),
                subtext0: Color::Rgb(90, 200, 220),
                overlay0: Color::Rgb(30, 70, 95),
                surface2: Color::Rgb(18, 40, 64),
                surface1: Color::Rgb(35, 70, 100),
                surface0: Color::Rgb(8, 22, 42),
                border: Color::Rgb(60, 100, 160),
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
                ip_hash: categorical_ip_hash(categorical),
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

    pub fn bubblegum() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 200, 230),
            flamingo: Color::Rgb(255, 150, 220),
            pink: Color::Rgb(255, 100, 200),
            mauve: Color::Rgb(255, 120, 220),
            red: Color::Rgb(255, 80, 180),
            maroon: Color::Rgb(255, 90, 190),
            peach: Color::Rgb(255, 160, 210),
            yellow: Color::Rgb(255, 200, 230),
            green: Color::Rgb(255, 180, 220),
            teal: Color::Rgb(255, 170, 215),
            sky: Color::Rgb(255, 190, 225),
            sapphire: Color::Rgb(255, 185, 223),
            blue: Color::Rgb(255, 175, 218),
            lavender: Color::Rgb(255, 195, 228),
        };

        Self {
            name: ThemeName::Bubblegum,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(255, 240, 250),
                subtext1: Color::Rgb(255, 220, 240),
                subtext0: Color::Rgb(255, 200, 230),
                overlay0: Color::Rgb(255, 180, 220),
                surface2: Color::Rgb(255, 150, 210),
                surface1: Color::Rgb(255, 120, 200),
                surface0: Color::Rgb(255, 100, 190),
                border: Color::Rgb(255, 160, 215),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(255, 240, 250),
                    Color::Rgb(255, 220, 240),
                    Color::Rgb(255, 200, 230),
                    Color::Rgb(255, 180, 220),
                    Color::Rgb(255, 160, 210),
                    Color::Rgb(255, 140, 205),
                    Color::Rgb(255, 120, 200),
                    Color::Rgb(255, 100, 195),
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
                    empty: Color::Rgb(255, 130, 205),
                },
                stream: ThemeStream {
                    inflow: categorical.sky,
                    outflow: categorical.pink,
                },
                dust: ThemeDust {
                    foreground: categorical.pink,
                    midground: categorical.lavender,
                    background: Color::Rgb(255, 140, 208),
                },
                categorical,
            },
        }
    }

    pub fn dracula() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(248, 248, 242),
            flamingo: Color::Rgb(255, 184, 108),
            pink: Color::Rgb(255, 121, 198),
            mauve: Color::Rgb(189, 147, 249),
            red: Color::Rgb(255, 85, 85),
            maroon: Color::Rgb(255, 110, 139),
            peach: Color::Rgb(255, 184, 108),
            yellow: Color::Rgb(241, 250, 140),
            green: Color::Rgb(80, 250, 123),
            teal: Color::Rgb(139, 233, 253),
            sky: Color::Rgb(139, 233, 253),
            sapphire: Color::Rgb(98, 114, 164),
            blue: Color::Rgb(139, 233, 253),
            lavender: Color::Rgb(189, 147, 249),
        };

        Self {
            name: ThemeName::Dracula,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(248, 248, 242),
                subtext1: Color::Rgb(189, 147, 249),
                subtext0: Color::Rgb(98, 114, 164),
                overlay0: Color::Rgb(68, 71, 90),
                surface2: Color::Rgb(68, 71, 90),
                surface1: Color::Rgb(56, 59, 77),
                surface0: Color::Rgb(40, 42, 54),
                border: Color::Rgb(68, 71, 90),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.mauve,
                    medium: categorical.pink,
                    high: categorical.green,
                    empty: Color::Rgb(68, 71, 90),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.pink,
                    midground: categorical.mauve,
                    background: Color::Rgb(68, 71, 90),
                },
                categorical,
            },
        }
    }

    pub fn nord() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(236, 239, 244),
            flamingo: Color::Rgb(216, 222, 233),
            pink: Color::Rgb(191, 97, 106),
            mauve: Color::Rgb(180, 142, 173),
            red: Color::Rgb(191, 97, 106),
            maroon: Color::Rgb(208, 135, 112),
            peach: Color::Rgb(208, 135, 112),
            yellow: Color::Rgb(235, 203, 139),
            green: Color::Rgb(163, 190, 140),
            teal: Color::Rgb(143, 188, 187),
            sky: Color::Rgb(136, 192, 208),
            sapphire: Color::Rgb(129, 161, 193),
            blue: Color::Rgb(94, 129, 172),
            lavender: Color::Rgb(180, 142, 173),
        };

        Self {
            name: ThemeName::Nord,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(236, 239, 244),
                subtext1: Color::Rgb(216, 222, 233),
                subtext0: Color::Rgb(143, 188, 187),
                overlay0: Color::Rgb(76, 86, 106),
                surface2: Color::Rgb(59, 66, 82),
                surface1: Color::Rgb(46, 52, 64),
                surface0: Color::Rgb(43, 48, 59),
                border: Color::Rgb(76, 86, 106),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.mauve,
                    categorical.blue,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.sky,
                    high: categorical.green,
                    empty: Color::Rgb(46, 52, 64),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.sky,
                    midground: categorical.blue,
                    background: Color::Rgb(59, 66, 82),
                },
                categorical,
            },
        }
    }

    pub fn gruvbox_dark() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(235, 219, 178),
            flamingo: Color::Rgb(214, 93, 14),
            pink: Color::Rgb(211, 134, 155),
            mauve: Color::Rgb(211, 134, 155),
            red: Color::Rgb(251, 73, 52),
            maroon: Color::Rgb(204, 36, 29),
            peach: Color::Rgb(254, 128, 25),
            yellow: Color::Rgb(250, 189, 47),
            green: Color::Rgb(184, 187, 38),
            teal: Color::Rgb(142, 192, 124),
            sky: Color::Rgb(131, 165, 152),
            sapphire: Color::Rgb(69, 133, 136),
            blue: Color::Rgb(131, 165, 152),
            lavender: Color::Rgb(214, 93, 14),
        };

        Self {
            name: ThemeName::GruvboxDark,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(235, 219, 178),
                subtext1: Color::Rgb(213, 196, 161),
                subtext0: Color::Rgb(168, 153, 132),
                overlay0: Color::Rgb(124, 111, 100),
                surface2: Color::Rgb(60, 56, 54),
                surface1: Color::Rgb(50, 48, 47),
                surface0: Color::Rgb(40, 40, 40),
                border: Color::Rgb(60, 56, 54),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.blue,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.maroon,
                    categorical.mauve,
                    categorical.pink,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(60, 56, 54),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(60, 56, 54),
                },
                categorical,
            },
        }
    }

    pub fn tokyo_night() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(192, 202, 245),
            flamingo: Color::Rgb(255, 158, 100),
            pink: Color::Rgb(247, 118, 142),
            mauve: Color::Rgb(187, 154, 247),
            red: Color::Rgb(247, 118, 142),
            maroon: Color::Rgb(255, 158, 100),
            peach: Color::Rgb(255, 158, 100),
            yellow: Color::Rgb(224, 175, 104),
            green: Color::Rgb(158, 206, 106),
            teal: Color::Rgb(125, 207, 255),
            sky: Color::Rgb(125, 207, 255),
            sapphire: Color::Rgb(122, 162, 247),
            blue: Color::Rgb(122, 162, 247),
            lavender: Color::Rgb(187, 154, 247),
        };

        Self {
            name: ThemeName::TokyoNight,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(192, 202, 245),
                subtext1: Color::Rgb(169, 177, 214),
                subtext0: Color::Rgb(86, 95, 137),
                overlay0: Color::Rgb(65, 72, 104),
                surface2: Color::Rgb(41, 46, 66),
                surface1: Color::Rgb(60, 65, 90),
                surface0: Color::Rgb(26, 27, 38),
                border: Color::Rgb(65, 72, 104),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.mauve,
                    categorical.blue,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.sky,
                    high: categorical.red,
                    empty: Color::Rgb(36, 40, 59),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.sky,
                    midground: categorical.blue,
                    background: Color::Rgb(41, 46, 66),
                },
                categorical,
            },
        }
    }

    pub fn one_dark() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(171, 178, 191),
            flamingo: Color::Rgb(209, 154, 102),
            pink: Color::Rgb(198, 120, 221),
            mauve: Color::Rgb(198, 120, 221),
            red: Color::Rgb(224, 108, 117),
            maroon: Color::Rgb(190, 80, 70),
            peach: Color::Rgb(209, 154, 102),
            yellow: Color::Rgb(229, 192, 123),
            green: Color::Rgb(152, 195, 121),
            teal: Color::Rgb(86, 182, 194),
            sky: Color::Rgb(97, 175, 239),
            sapphire: Color::Rgb(97, 175, 239),
            blue: Color::Rgb(97, 175, 239),
            lavender: Color::Rgb(198, 120, 221),
        };

        Self {
            name: ThemeName::OneDark,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(171, 178, 191),
                subtext1: Color::Rgb(146, 150, 165),
                subtext0: Color::Rgb(92, 99, 112),
                overlay0: Color::Rgb(73, 78, 90),
                surface2: Color::Rgb(40, 44, 52),
                surface1: Color::Rgb(65, 72, 80),
                surface0: Color::Rgb(30, 33, 39),
                border: Color::Rgb(73, 78, 90),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(40, 44, 52),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.sky,
                    midground: categorical.blue,
                    background: Color::Rgb(40, 44, 52),
                },
                categorical,
            },
        }
    }

    pub fn solarized_dark() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(131, 148, 150),
            flamingo: Color::Rgb(203, 75, 22),
            pink: Color::Rgb(211, 54, 130),
            mauve: Color::Rgb(108, 113, 196),
            red: Color::Rgb(220, 50, 47),
            maroon: Color::Rgb(203, 75, 22),
            peach: Color::Rgb(203, 75, 22),
            yellow: Color::Rgb(181, 137, 0),
            green: Color::Rgb(133, 153, 0),
            teal: Color::Rgb(42, 161, 152),
            sky: Color::Rgb(38, 139, 210),
            sapphire: Color::Rgb(38, 139, 210),
            blue: Color::Rgb(38, 139, 210),
            lavender: Color::Rgb(108, 113, 196),
        };

        Self {
            name: ThemeName::SolarizedDark,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(131, 148, 150),
                subtext1: Color::Rgb(147, 161, 161),
                subtext0: Color::Rgb(101, 123, 131),
                overlay0: Color::Rgb(88, 110, 117),
                surface2: Color::Rgb(7, 54, 66),
                surface1: Color::Rgb(0, 90, 110),
                surface0: Color::Rgb(0, 33, 44),
                border: Color::Rgb(0, 130, 160),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(7, 54, 66),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(7, 54, 66),
                },
                categorical,
            },
        }
    }

    pub fn monokai() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(248, 248, 242),
            flamingo: Color::Rgb(253, 151, 31),
            pink: Color::Rgb(249, 38, 114),
            mauve: Color::Rgb(174, 129, 255),
            red: Color::Rgb(249, 38, 114),
            maroon: Color::Rgb(204, 102, 119),
            peach: Color::Rgb(253, 151, 31),
            yellow: Color::Rgb(230, 219, 116),
            green: Color::Rgb(166, 226, 46),
            teal: Color::Rgb(102, 217, 239),
            sky: Color::Rgb(102, 217, 239),
            sapphire: Color::Rgb(117, 113, 94),
            blue: Color::Rgb(102, 217, 239),
            lavender: Color::Rgb(174, 129, 255),
        };

        Self {
            name: ThemeName::Monokai,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(248, 248, 242),
                subtext1: Color::Rgb(174, 129, 255),
                subtext0: Color::Rgb(117, 113, 94),
                overlay0: Color::Rgb(73, 72, 62),
                surface2: Color::Rgb(39, 40, 34),
                surface1: Color::Rgb(70, 72, 65),
                surface0: Color::Rgb(27, 28, 24),
                border: Color::Rgb(73, 72, 62),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(39, 40, 34),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(39, 40, 34),
                },
                categorical,
            },
        }
    }

    pub fn everforest_dark() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(211, 198, 170),
            flamingo: Color::Rgb(230, 126, 128),
            pink: Color::Rgb(231, 138, 131),
            mauve: Color::Rgb(215, 153, 33),
            red: Color::Rgb(230, 126, 128),
            maroon: Color::Rgb(229, 152, 117),
            peach: Color::Rgb(229, 152, 117),
            yellow: Color::Rgb(219, 188, 127),
            green: Color::Rgb(167, 192, 128),
            teal: Color::Rgb(131, 192, 146),
            sky: Color::Rgb(127, 187, 179),
            sapphire: Color::Rgb(115, 163, 145),
            blue: Color::Rgb(127, 187, 179),
            lavender: Color::Rgb(214, 153, 182),
        };

        Self {
            name: ThemeName::EverforestDark,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(211, 198, 170),
                subtext1: Color::Rgb(167, 192, 128),
                subtext0: Color::Rgb(133, 147, 138),
                overlay0: Color::Rgb(94, 100, 104),
                surface2: Color::Rgb(59, 69, 71),
                surface1: Color::Rgb(47, 56, 58),
                surface0: Color::Rgb(43, 51, 57),
                border: Color::Rgb(59, 69, 71),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(59, 69, 71),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(59, 69, 71),
                },
                categorical,
            },
        }
    }

    pub fn kanagawa() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(220, 215, 186), // oldPaper
            flamingo: Color::Rgb(210, 126, 154),  // sakuraPink
            pink: Color::Rgb(210, 126, 154),      // sakuraPink
            mauve: Color::Rgb(149, 127, 184),     // oniViolet
            red: Color::Rgb(195, 64, 67),         // autumnRed
            maroon: Color::Rgb(195, 64, 67),      // autumnRed
            peach: Color::Rgb(255, 160, 102),     // surimiOrange
            yellow: Color::Rgb(192, 163, 110),    // boatmansYellow
            green: Color::Rgb(118, 148, 106),     // autumnGreen
            teal: Color::Rgb(106, 149, 137),      // waveAqua
            sky: Color::Rgb(126, 156, 216),       // crystalBlue
            sapphire: Color::Rgb(101, 133, 153),  // dragonBlue
            blue: Color::Rgb(126, 156, 216),      // crystalBlue
            lavender: Color::Rgb(149, 127, 184),  // oniViolet
        };

        Self {
            name: ThemeName::Kanagawa,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(220, 215, 186),     // oldPaper
                subtext1: Color::Rgb(166, 173, 200), // subtext1
                subtext0: Color::Rgb(114, 113, 133), // fujiGray
                overlay0: Color::Rgb(84, 84, 111),   // sumiInk4
                surface2: Color::Rgb(54, 54, 75),    // sumiInk3
                surface1: Color::Rgb(42, 42, 62),    // sumiInk2
                surface0: Color::Rgb(31, 31, 40),    // sumiInk1
                border: Color::Rgb(84, 84, 111),     // sumiInk4
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(54, 54, 75),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(54, 54, 75),
                },
                categorical,
            },
        }
    }

    pub fn github_dark() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(201, 209, 217),
            flamingo: Color::Rgb(255, 122, 127),
            pink: Color::Rgb(210, 153, 255),
            mauve: Color::Rgb(188, 140, 255),
            red: Color::Rgb(248, 81, 73),
            maroon: Color::Rgb(255, 123, 114),
            peach: Color::Rgb(255, 166, 87),
            yellow: Color::Rgb(210, 153, 34),
            green: Color::Rgb(63, 185, 80),
            teal: Color::Rgb(57, 197, 207),
            sky: Color::Rgb(103, 193, 255),
            sapphire: Color::Rgb(88, 166, 255),
            blue: Color::Rgb(88, 166, 255),
            lavender: Color::Rgb(188, 140, 255),
        };

        Self {
            name: ThemeName::GitHubDark,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(201, 209, 217),
                subtext1: Color::Rgb(139, 148, 158),
                subtext0: Color::Rgb(110, 118, 129),
                overlay0: Color::Rgb(48, 54, 61),
                surface2: Color::Rgb(33, 38, 45),
                surface1: Color::Rgb(50, 60, 70),
                surface0: Color::Rgb(13, 17, 23),
                border: Color::Rgb(48, 54, 61),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(33, 38, 45),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(33, 38, 45),
                },
                categorical,
            },
        }
    }

    pub fn solarized_light() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(101, 123, 131), // base00
            flamingo: Color::Rgb(203, 75, 22),    // orange
            pink: Color::Rgb(211, 54, 130),       // magenta
            mauve: Color::Rgb(108, 113, 196),     // violet
            red: Color::Rgb(220, 50, 47),         // red
            maroon: Color::Rgb(203, 75, 22),      // orange
            peach: Color::Rgb(203, 75, 22),       // orange
            yellow: Color::Rgb(181, 137, 0),      // yellow
            green: Color::Rgb(133, 153, 0),       // green
            teal: Color::Rgb(42, 161, 152),       // cyan
            sky: Color::Rgb(38, 139, 210),        // blue
            sapphire: Color::Rgb(38, 139, 210),   // blue
            blue: Color::Rgb(38, 139, 210),       // blue
            lavender: Color::Rgb(108, 113, 196),  // violet
        };

        Self {
            name: ThemeName::SolarizedLight,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(101, 123, 131),     // base00
                subtext1: Color::Rgb(88, 110, 117),  // base01
                subtext0: Color::Rgb(131, 148, 150), // base0
                overlay0: Color::Rgb(147, 161, 161), // base1
                surface2: Color::Rgb(238, 232, 213), // base2
                surface1: Color::Rgb(253, 246, 227), // base3
                surface0: Color::Rgb(255, 255, 240), // extra light
                border: Color::Rgb(222, 214, 193),   // custom border
                white: Color::Black,                 // Inverted for light theme
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(238, 232, 213),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(238, 232, 213),
                },
                categorical,
            },
        }
    }

    pub fn matrix() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(0, 255, 65),
            flamingo: Color::Rgb(0, 143, 17),
            pink: Color::Rgb(0, 255, 65),
            mauve: Color::Rgb(0, 59, 0),
            red: Color::Rgb(0, 255, 65),
            maroon: Color::Rgb(0, 143, 17),
            peach: Color::Rgb(0, 255, 65),
            yellow: Color::Rgb(0, 255, 65),
            green: Color::Rgb(0, 255, 65),
            teal: Color::Rgb(0, 255, 65),
            sky: Color::Rgb(0, 255, 65),
            sapphire: Color::Rgb(0, 255, 65),
            blue: Color::Rgb(0, 255, 65),
            lavender: Color::Rgb(0, 255, 65),
        };

        Self {
            name: ThemeName::Matrix,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 5.0,
                flicker_intensity: 0.2,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(0, 255, 65),
                subtext1: Color::Rgb(0, 204, 52),
                subtext0: Color::Rgb(0, 143, 17),
                overlay0: Color::Rgb(0, 89, 11),
                surface2: Color::Rgb(0, 59, 0),
                surface1: Color::Rgb(0, 180, 0),
                surface0: Color::Rgb(0, 0, 0),
                border: Color::Rgb(0, 143, 17),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(0, 59, 0),
                    Color::Rgb(0, 89, 11),
                    Color::Rgb(0, 143, 17),
                    Color::Rgb(0, 204, 52),
                    Color::Rgb(0, 255, 65),
                    Color::Rgb(102, 255, 102),
                    Color::Rgb(153, 255, 153),
                    Color::Rgb(204, 255, 204),
                ],
                ip_hash: [
                    Color::Rgb(0, 255, 65),
                    Color::Rgb(0, 204, 52),
                    Color::Rgb(0, 143, 17),
                    Color::Rgb(0, 89, 11),
                    Color::Rgb(0, 59, 0),
                    Color::Rgb(0, 255, 65),
                    Color::Rgb(0, 204, 52),
                    Color::Rgb(0, 143, 17),
                    Color::Rgb(0, 89, 11),
                    Color::Rgb(0, 59, 0),
                    Color::Rgb(0, 255, 65),
                    Color::Rgb(0, 204, 52),
                    Color::Rgb(0, 143, 17),
                    Color::Rgb(0, 89, 11),
                ],
                heatmap: ThemeHeatmap {
                    low: Color::Rgb(0, 59, 0),
                    medium: Color::Rgb(0, 143, 17),
                    high: Color::Rgb(0, 255, 65),
                    empty: Color::Rgb(0, 20, 0),
                },
                stream: ThemeStream {
                    inflow: Color::Rgb(0, 255, 65),
                    outflow: Color::Rgb(0, 143, 17),
                },
                dust: ThemeDust {
                    foreground: Color::Rgb(0, 255, 65),
                    midground: Color::Rgb(0, 143, 17),
                    background: Color::Rgb(0, 59, 0),
                },
                categorical,
            },
        }
    }

    pub fn catppuccin_latte() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(220, 138, 120),
            flamingo: Color::Rgb(221, 120, 120),
            pink: Color::Rgb(234, 118, 203),
            mauve: Color::Rgb(136, 57, 239),
            red: Color::Rgb(210, 15, 57),
            maroon: Color::Rgb(230, 69, 83),
            peach: Color::Rgb(254, 100, 11),
            yellow: Color::Rgb(223, 142, 29),
            green: Color::Rgb(64, 160, 43),
            teal: Color::Rgb(23, 146, 153),
            sky: Color::Rgb(4, 165, 229),
            sapphire: Color::Rgb(32, 159, 181),
            blue: Color::Rgb(30, 102, 245),
            lavender: Color::Rgb(114, 135, 253),
        };

        Self {
            name: ThemeName::CatppuccinLatte,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(76, 79, 105),
                subtext1: Color::Rgb(92, 95, 119),
                subtext0: Color::Rgb(108, 111, 133),
                overlay0: Color::Rgb(156, 160, 176),
                surface2: Color::Rgb(172, 176, 190),
                surface1: Color::Rgb(188, 192, 204),
                surface0: Color::Rgb(204, 208, 218),
                border: Color::Rgb(172, 176, 190),
                white: Color::Black,
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
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.mauve,
                    medium: categorical.mauve,
                    high: categorical.mauve,
                    empty: Color::Rgb(188, 192, 204),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.green,
                    midground: categorical.blue,
                    background: Color::Rgb(172, 176, 190),
                },
                categorical,
            },
        }
    }

    pub fn cyberpunk() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 255, 255),
            flamingo: Color::Rgb(255, 0, 255),
            pink: Color::Rgb(255, 0, 255),
            mauve: Color::Rgb(150, 0, 255),
            red: Color::Rgb(255, 0, 60),
            maroon: Color::Rgb(255, 0, 100),
            peach: Color::Rgb(255, 100, 0),
            yellow: Color::Rgb(253, 245, 0),
            green: Color::Rgb(0, 255, 159),
            teal: Color::Rgb(0, 255, 255),
            sky: Color::Rgb(0, 184, 255),
            sapphire: Color::Rgb(0, 114, 255),
            blue: Color::Rgb(5, 217, 255),
            lavender: Color::Rgb(150, 0, 255),
        };

        Self {
            name: ThemeName::Cyberpunk,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 12.0,
                flicker_intensity: 0.4,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(253, 245, 0),
                subtext1: Color::Rgb(0, 255, 255),
                subtext0: Color::Rgb(255, 0, 255),
                overlay0: Color::Rgb(50, 0, 100),
                surface2: Color::Rgb(30, 0, 60),
                surface1: Color::Rgb(120, 50, 160),
                surface0: Color::Rgb(0, 0, 0),
                border: Color::Rgb(255, 0, 255),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.blue,
                    categorical.teal,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.pink,
                    high: categorical.yellow,
                    empty: Color::Rgb(30, 0, 60),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.pink,
                    background: Color::Rgb(30, 0, 60),
                },
                categorical,
            },
        }
    }

    pub fn ayu_dark() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(230, 225, 207),
            flamingo: Color::Rgb(255, 180, 84),
            pink: Color::Rgb(240, 117, 181),
            mauve: Color::Rgb(223, 177, 242),
            red: Color::Rgb(255, 51, 51),
            maroon: Color::Rgb(242, 121, 131),
            peach: Color::Rgb(255, 180, 84),
            yellow: Color::Rgb(242, 151, 24),
            green: Color::Rgb(184, 204, 82),
            teal: Color::Rgb(149, 230, 203),
            sky: Color::Rgb(54, 163, 217),
            sapphire: Color::Rgb(54, 163, 217),
            blue: Color::Rgb(54, 163, 217),
            lavender: Color::Rgb(223, 177, 242),
        };

        Self {
            name: ThemeName::AyuDark,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(230, 225, 207),
                subtext1: Color::Rgb(171, 176, 191),
                subtext0: Color::Rgb(92, 103, 115),
                overlay0: Color::Rgb(62, 71, 82),
                surface2: Color::Rgb(25, 30, 36),
                surface1: Color::Rgb(55, 65, 75),
                surface0: Color::Rgb(15, 20, 25),
                border: Color::Rgb(62, 71, 82),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(25, 30, 36),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(25, 30, 36),
                },
                categorical,
            },
        }
    }

    pub fn zenburn() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(220, 220, 204),
            flamingo: Color::Rgb(223, 175, 143),
            pink: Color::Rgb(220, 140, 195),
            mauve: Color::Rgb(156, 144, 186),
            red: Color::Rgb(204, 147, 147),
            maroon: Color::Rgb(188, 131, 121),
            peach: Color::Rgb(223, 175, 143),
            yellow: Color::Rgb(240, 223, 175),
            green: Color::Rgb(127, 159, 127),
            teal: Color::Rgb(147, 177, 187),
            sky: Color::Rgb(140, 208, 211),
            sapphire: Color::Rgb(115, 139, 140),
            blue: Color::Rgb(140, 208, 211),
            lavender: Color::Rgb(156, 144, 186),
        };

        Self {
            name: ThemeName::Zenburn,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(220, 220, 204),
                subtext1: Color::Rgb(159, 159, 159),
                subtext0: Color::Rgb(127, 159, 127),
                overlay0: Color::Rgb(83, 83, 83),
                surface2: Color::Rgb(71, 71, 71),
                surface1: Color::Rgb(63, 63, 63),
                surface0: Color::Rgb(50, 50, 50),
                border: Color::Rgb(83, 83, 83),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(71, 71, 71),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(71, 71, 71),
                },
                categorical,
            },
        }
    }

    pub fn synthwave_84() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 255, 255),
            flamingo: Color::Rgb(255, 126, 219),
            pink: Color::Rgb(255, 126, 219),
            mauve: Color::Rgb(114, 241, 184),
            red: Color::Rgb(249, 126, 114),
            maroon: Color::Rgb(249, 126, 114),
            peach: Color::Rgb(254, 238, 0),
            yellow: Color::Rgb(254, 238, 0),
            green: Color::Rgb(114, 241, 184),
            teal: Color::Rgb(54, 249, 246),
            sky: Color::Rgb(54, 249, 246),
            sapphire: Color::Rgb(54, 249, 246),
            blue: Color::Rgb(54, 249, 246),
            lavender: Color::Rgb(114, 241, 184),
        };

        Self {
            name: ThemeName::Synthwave84,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 8.0,
                flicker_intensity: 0.3,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(249, 126, 114),
                subtext1: Color::Rgb(255, 126, 219),
                subtext0: Color::Rgb(54, 249, 246),
                overlay0: Color::Rgb(103, 78, 131),
                surface2: Color::Rgb(52, 43, 73),
                surface1: Color::Rgb(70, 50, 90),
                surface0: Color::Rgb(36, 27, 47),
                border: Color::Rgb(255, 126, 219),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.teal,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.rosewater,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.pink,
                    high: categorical.yellow,
                    empty: Color::Rgb(52, 43, 73),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.pink,
                    background: Color::Rgb(52, 43, 73),
                },
                categorical,
            },
        }
    }

    pub fn github_light() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(36, 41, 47),
            flamingo: Color::Rgb(207, 34, 46),
            pink: Color::Rgb(130, 80, 223),
            mauve: Color::Rgb(130, 80, 223),
            red: Color::Rgb(207, 34, 46),
            maroon: Color::Rgb(207, 34, 46),
            peach: Color::Rgb(154, 103, 0),
            yellow: Color::Rgb(154, 103, 0),
            green: Color::Rgb(26, 127, 55),
            teal: Color::Rgb(5, 153, 112),
            sky: Color::Rgb(5, 153, 112),
            sapphire: Color::Rgb(9, 105, 218),
            blue: Color::Rgb(9, 105, 218),
            lavender: Color::Rgb(130, 80, 223),
        };

        Self {
            name: ThemeName::GitHubLight,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(36, 41, 47),
                subtext1: Color::Rgb(87, 96, 106),
                subtext0: Color::Rgb(101, 109, 118),
                overlay0: Color::Rgb(208, 215, 222),
                surface2: Color::Rgb(234, 238, 242),
                surface1: Color::Rgb(246, 248, 250),
                surface0: Color::Rgb(255, 255, 255),
                border: Color::Rgb(208, 215, 222),
                white: Color::Black,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sapphire,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(234, 238, 242),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(234, 238, 242),
                },
                categorical,
            },
        }
    }

    pub fn vesper() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 255, 255),
            flamingo: Color::Rgb(255, 128, 0),
            pink: Color::Rgb(255, 175, 0),
            mauve: Color::Rgb(160, 160, 160),
            red: Color::Rgb(255, 128, 0),
            maroon: Color::Rgb(255, 128, 0),
            peach: Color::Rgb(255, 175, 0),
            yellow: Color::Rgb(255, 175, 0),
            green: Color::Rgb(160, 160, 160),
            teal: Color::Rgb(160, 160, 160),
            sky: Color::Rgb(160, 160, 160),
            sapphire: Color::Rgb(160, 160, 160),
            blue: Color::Rgb(160, 160, 160),
            lavender: Color::Rgb(160, 160, 160),
        };

        Self {
            name: ThemeName::Vesper,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(255, 255, 255),
                subtext1: Color::Rgb(160, 160, 160),
                subtext0: Color::Rgb(110, 110, 110),
                overlay0: Color::Rgb(70, 70, 70),
                surface2: Color::Rgb(40, 40, 40),
                surface1: Color::Rgb(60, 60, 60),
                surface0: Color::Rgb(16, 16, 16),
                border: Color::Rgb(120, 120, 120),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(80, 80, 80),
                    Color::Rgb(110, 110, 110),
                    Color::Rgb(140, 140, 140),
                    Color::Rgb(160, 160, 160),
                    Color::Rgb(180, 180, 180),
                    Color::Rgb(255, 175, 0),
                    Color::Rgb(255, 128, 0),
                    Color::Rgb(255, 255, 255),
                ],
                ip_hash: [
                    Color::Rgb(255, 255, 255),
                    Color::Rgb(255, 128, 0),
                    Color::Rgb(255, 175, 0),
                    Color::Rgb(160, 160, 160),
                    Color::Rgb(110, 110, 110),
                    Color::Rgb(255, 255, 255),
                    Color::Rgb(255, 128, 0),
                    Color::Rgb(255, 175, 0),
                    Color::Rgb(160, 160, 160),
                    Color::Rgb(110, 110, 110),
                    Color::Rgb(255, 255, 255),
                    Color::Rgb(255, 128, 0),
                    Color::Rgb(255, 175, 0),
                    Color::Rgb(160, 160, 160),
                ],
                heatmap: ThemeHeatmap {
                    low: Color::Rgb(110, 110, 110),
                    medium: Color::Rgb(255, 175, 0),
                    high: Color::Rgb(255, 128, 0),
                    empty: Color::Rgb(30, 30, 30),
                },
                stream: ThemeStream {
                    inflow: Color::Rgb(255, 175, 0),
                    outflow: Color::Rgb(255, 128, 0),
                },
                dust: ThemeDust {
                    foreground: Color::Rgb(255, 128, 0),
                    midground: Color::Rgb(255, 175, 0),
                    background: Color::Rgb(40, 40, 40),
                },
                categorical,
            },
        }
    }

    pub fn material_ocean() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(143, 147, 162),
            flamingo: Color::Rgb(255, 83, 112),
            pink: Color::Rgb(240, 113, 120),
            mauve: Color::Rgb(199, 146, 234),
            red: Color::Rgb(240, 113, 120),
            maroon: Color::Rgb(240, 113, 120),
            peach: Color::Rgb(247, 140, 108),
            yellow: Color::Rgb(255, 203, 107),
            green: Color::Rgb(195, 232, 141),
            teal: Color::Rgb(137, 221, 255),
            sky: Color::Rgb(137, 221, 255),
            sapphire: Color::Rgb(130, 170, 255),
            blue: Color::Rgb(130, 170, 255),
            lavender: Color::Rgb(199, 146, 234),
        };

        Self {
            name: ThemeName::MaterialOcean,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(143, 147, 162),
                subtext1: Color::Rgb(113, 123, 145),
                subtext0: Color::Rgb(84, 91, 107),
                overlay0: Color::Rgb(53, 57, 74),
                surface2: Color::Rgb(25, 27, 41),
                surface1: Color::Rgb(45, 50, 75),
                surface0: Color::Rgb(15, 17, 26),
                border: Color::Rgb(100, 110, 140),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(25, 27, 41),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(25, 27, 41),
                },
                categorical,
            },
        }
    }

    pub fn gruvbox_light() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(60, 56, 54),
            flamingo: Color::Rgb(175, 58, 3),
            pink: Color::Rgb(143, 63, 113),
            mauve: Color::Rgb(143, 63, 113),
            red: Color::Rgb(157, 0, 6),
            maroon: Color::Rgb(157, 0, 6),
            peach: Color::Rgb(175, 58, 3),
            yellow: Color::Rgb(181, 118, 20),
            green: Color::Rgb(121, 116, 14),
            teal: Color::Rgb(66, 123, 88),
            sky: Color::Rgb(7, 102, 120),
            sapphire: Color::Rgb(7, 102, 120),
            blue: Color::Rgb(7, 102, 120),
            lavender: Color::Rgb(143, 63, 113),
        };

        Self {
            name: ThemeName::GruvboxLight,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(60, 56, 54),
                subtext1: Color::Rgb(80, 73, 69),
                subtext0: Color::Rgb(102, 92, 84),
                overlay0: Color::Rgb(146, 131, 116),
                surface2: Color::Rgb(213, 196, 161),
                surface1: Color::Rgb(200, 185, 155),
                surface0: Color::Rgb(251, 241, 199),
                border: Color::Rgb(213, 196, 161),
                white: Color::Black,
            },
            scale: ThemeScale {
                speed: [
                    categorical.blue,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.maroon,
                    categorical.mauve,
                    categorical.pink,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(213, 196, 161),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(213, 196, 161),
                },
                categorical,
            },
        }
    }

    pub fn oxocarbon() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 255, 255),
            flamingo: Color::Rgb(255, 126, 182),
            pink: Color::Rgb(255, 126, 182),
            mauve: Color::Rgb(190, 149, 255),
            red: Color::Rgb(238, 83, 103),
            maroon: Color::Rgb(238, 83, 103),
            peach: Color::Rgb(255, 169, 123),
            yellow: Color::Rgb(255, 233, 123),
            green: Color::Rgb(66, 190, 101),
            teal: Color::Rgb(51, 177, 255),
            sky: Color::Rgb(130, 207, 255),
            sapphire: Color::Rgb(130, 207, 255),
            blue: Color::Rgb(130, 207, 255),
            lavender: Color::Rgb(190, 149, 255),
        };

        Self {
            name: ThemeName::Oxocarbon,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(255, 255, 255),
                subtext1: Color::Rgb(221, 221, 221),
                subtext0: Color::Rgb(171, 171, 171),
                overlay0: Color::Rgb(82, 82, 82),
                surface2: Color::Rgb(57, 57, 57),
                surface1: Color::Rgb(50, 55, 65),
                surface0: Color::Rgb(22, 22, 22),
                border: Color::Rgb(110, 110, 110),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(38, 38, 38),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(38, 38, 38),
                },
                categorical,
            },
        }
    }

    pub fn rainbow() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 0, 0),
            flamingo: Color::Rgb(255, 127, 0),
            pink: Color::Rgb(255, 0, 255),
            mauve: Color::Rgb(127, 0, 255),
            red: Color::Rgb(255, 0, 0),
            maroon: Color::Rgb(127, 0, 0),
            peach: Color::Rgb(255, 127, 0),
            yellow: Color::Rgb(255, 255, 0),
            green: Color::Rgb(0, 255, 0),
            teal: Color::Rgb(0, 255, 255),
            sky: Color::Rgb(0, 127, 255),
            sapphire: Color::Rgb(0, 0, 255),
            blue: Color::Rgb(0, 0, 255),
            lavender: Color::Rgb(127, 0, 255),
        };

        Self {
            name: ThemeName::Rainbow,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 2.0,
                flicker_intensity: 0.1,
            },
            semantic: ThemeSemantic {
                text: Color::White,
                subtext1: Color::Rgb(255, 255, 255),
                subtext0: Color::Rgb(255, 255, 255),
                overlay0: Color::Rgb(255, 255, 255),
                surface2: Color::Rgb(255, 255, 255),
                surface1: Color::Rgb(255, 255, 0),
                surface0: Color::Rgb(0, 255, 255),
                border: Color::Rgb(255, 255, 255),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(255, 0, 0),
                    Color::Rgb(255, 127, 0),
                    Color::Rgb(255, 255, 0),
                    Color::Rgb(0, 255, 0),
                    Color::Rgb(0, 255, 255),
                    Color::Rgb(0, 0, 255),
                    Color::Rgb(127, 0, 255),
                    Color::Rgb(255, 0, 255),
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: Color::Rgb(0, 0, 255),
                    medium: Color::Rgb(0, 255, 0),
                    high: Color::Rgb(255, 0, 0),
                    empty: Color::Rgb(50, 50, 50),
                },
                stream: ThemeStream {
                    inflow: Color::Rgb(0, 255, 255),
                    outflow: Color::Rgb(255, 0, 255),
                },
                dust: ThemeDust {
                    foreground: Color::Rgb(255, 255, 0),
                    midground: Color::Rgb(0, 255, 0),
                    background: Color::Rgb(50, 50, 50),
                },
                categorical,
            },
        }
    }

    pub fn inferno() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 255, 255),
            flamingo: Color::Rgb(255, 100, 0),
            pink: Color::Rgb(255, 50, 0),
            mauve: Color::Rgb(150, 0, 0),
            red: Color::Rgb(255, 0, 0),
            maroon: Color::Rgb(150, 0, 0),
            peach: Color::Rgb(255, 150, 0),
            yellow: Color::Rgb(255, 255, 0),
            green: Color::Rgb(255, 200, 0),
            teal: Color::Rgb(255, 220, 100),
            sky: Color::Rgb(255, 240, 150),
            sapphire: Color::Rgb(255, 255, 200),
            blue: Color::Rgb(255, 255, 200),
            lavender: Color::Rgb(150, 0, 0),
        };

        Self {
            name: ThemeName::Inferno,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 15.0,
                flicker_intensity: 0.5,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(255, 200, 0),
                subtext1: Color::Rgb(255, 150, 0),
                subtext0: Color::Rgb(255, 100, 0),
                overlay0: Color::Rgb(150, 50, 0),
                surface2: Color::Rgb(80, 20, 0),
                surface1: Color::Rgb(100, 40, 20),
                surface0: Color::Rgb(20, 0, 0),
                border: Color::Rgb(255, 50, 0),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(80, 0, 0),
                    Color::Rgb(150, 0, 0),
                    Color::Rgb(200, 50, 0),
                    Color::Rgb(255, 80, 0),
                    Color::Rgb(255, 120, 0),
                    Color::Rgb(255, 180, 0),
                    Color::Rgb(255, 220, 0),
                    Color::Rgb(255, 255, 100),
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: Color::Rgb(150, 0, 0),
                    medium: Color::Rgb(255, 80, 0),
                    high: Color::Rgb(255, 255, 0),
                    empty: Color::Rgb(40, 10, 0),
                },
                stream: ThemeStream {
                    inflow: Color::Rgb(255, 255, 0),
                    outflow: Color::Rgb(255, 50, 0),
                },
                dust: ThemeDust {
                    foreground: Color::Rgb(255, 255, 0),
                    midground: Color::Rgb(255, 100, 0),
                    background: Color::Rgb(60, 10, 0),
                },
                categorical,
            },
        }
    }

    pub fn aurora() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(200, 255, 200),
            flamingo: Color::Rgb(150, 255, 150),
            pink: Color::Rgb(255, 150, 255),
            mauve: Color::Rgb(200, 150, 255),
            red: Color::Rgb(100, 255, 100),
            maroon: Color::Rgb(50, 200, 150),
            peach: Color::Rgb(100, 200, 255),
            yellow: Color::Rgb(150, 255, 255),
            green: Color::Rgb(0, 255, 128),
            teal: Color::Rgb(0, 255, 255),
            sky: Color::Rgb(128, 255, 255),
            sapphire: Color::Rgb(128, 128, 255),
            blue: Color::Rgb(150, 150, 255),
            lavender: Color::Rgb(200, 150, 255),
        };

        Self {
            name: ThemeName::Aurora,
            effects: ThemeEffects {
                glow_enabled: true,
                flicker_hz: 2.0,
                flicker_intensity: 0.15,
            },
            semantic: ThemeSemantic {
                text: Color::Rgb(150, 255, 200),
                subtext1: Color::Rgb(100, 200, 255),
                subtext0: Color::Rgb(150, 150, 255),
                overlay0: Color::Rgb(40, 60, 100),
                surface2: Color::Rgb(20, 30, 60),
                surface1: Color::Rgb(30, 45, 90),
                surface0: Color::Rgb(5, 5, 25),
                border: Color::Rgb(0, 255, 128),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    Color::Rgb(5, 5, 40),
                    Color::Rgb(20, 40, 100),
                    Color::Rgb(40, 80, 150),
                    Color::Rgb(0, 150, 150),
                    Color::Rgb(0, 200, 100),
                    Color::Rgb(0, 255, 128),
                    Color::Rgb(100, 255, 200),
                    Color::Rgb(200, 255, 255),
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: Color::Rgb(50, 50, 150),
                    medium: Color::Rgb(0, 150, 150),
                    high: Color::Rgb(0, 255, 128),
                    empty: Color::Rgb(20, 30, 60),
                },
                stream: ThemeStream {
                    inflow: Color::Rgb(0, 255, 255),
                    outflow: Color::Rgb(200, 150, 255),
                },
                dust: ThemeDust {
                    foreground: Color::Rgb(0, 255, 128),
                    midground: Color::Rgb(100, 200, 255),
                    background: Color::Rgb(20, 30, 60),
                },
                categorical,
            },
        }
    }

    pub fn andromeda() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(255, 255, 255),
            flamingo: Color::Rgb(255, 74, 133),
            pink: Color::Rgb(255, 74, 133),
            mauve: Color::Rgb(173, 112, 255),
            red: Color::Rgb(255, 76, 110),
            maroon: Color::Rgb(255, 76, 110),
            peach: Color::Rgb(255, 202, 125),
            yellow: Color::Rgb(255, 230, 109),
            green: Color::Rgb(0, 230, 152),
            teal: Color::Rgb(0, 230, 230),
            sky: Color::Rgb(0, 150, 255),
            sapphire: Color::Rgb(0, 150, 255),
            blue: Color::Rgb(0, 150, 255),
            lavender: Color::Rgb(173, 112, 255),
        };

        Self {
            name: ThemeName::Andromeda,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(213, 218, 227),
                subtext1: Color::Rgb(153, 158, 167),
                subtext0: Color::Rgb(116, 123, 136),
                overlay0: Color::Rgb(59, 64, 72),
                surface2: Color::Rgb(43, 48, 59),
                surface1: Color::Rgb(65, 70, 80),
                surface0: Color::Rgb(29, 32, 38),
                border: Color::Rgb(59, 64, 72),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.green,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(43, 48, 59),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(43, 48, 59),
                },
                categorical,
            },
        }
    }

    pub fn rose_pine() -> Self {
        let categorical = ThemeCategorical {
            rosewater: Color::Rgb(224, 222, 244),
            flamingo: Color::Rgb(246, 193, 119),
            pink: Color::Rgb(235, 111, 146),
            mauve: Color::Rgb(196, 167, 231),
            red: Color::Rgb(235, 111, 146),
            maroon: Color::Rgb(235, 188, 186),
            peach: Color::Rgb(246, 193, 119),
            yellow: Color::Rgb(246, 193, 119),
            green: Color::Rgb(49, 116, 143),
            teal: Color::Rgb(156, 207, 216),
            sky: Color::Rgb(156, 207, 216),
            sapphire: Color::Rgb(144, 140, 170),
            blue: Color::Rgb(156, 207, 216),
            lavender: Color::Rgb(196, 167, 231),
        };

        Self {
            name: ThemeName::RosePine,
            effects: ThemeEffects::default(),
            semantic: ThemeSemantic {
                text: Color::Rgb(224, 222, 244),
                subtext1: Color::Rgb(144, 140, 170),
                subtext0: Color::Rgb(110, 106, 134),
                overlay0: Color::Rgb(64, 61, 82),
                surface2: Color::Rgb(38, 35, 58),
                surface1: Color::Rgb(65, 60, 90),
                surface0: Color::Rgb(25, 23, 36),
                border: Color::Rgb(64, 61, 82),
                white: Color::White,
            },
            scale: ThemeScale {
                speed: [
                    categorical.sky,
                    categorical.teal,
                    categorical.yellow,
                    categorical.peach,
                    categorical.red,
                    categorical.pink,
                    categorical.mauve,
                    categorical.lavender,
                ],
                ip_hash: categorical_ip_hash(categorical),
                heatmap: ThemeHeatmap {
                    low: categorical.blue,
                    medium: categorical.yellow,
                    high: categorical.red,
                    empty: Color::Rgb(38, 35, 58),
                },
                stream: ThemeStream {
                    inflow: categorical.blue,
                    outflow: categorical.green,
                },
                dust: ThemeDust {
                    foreground: categorical.yellow,
                    midground: categorical.blue,
                    background: Color::Rgb(38, 35, 58),
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

fn categorical_ip_hash(categorical: ThemeCategorical) -> [Color; 14] {
    [
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_themes_snake_case() {
        // Test that all known themes deserialize correctly in snake_case
        let themes = vec![
            ("andromeda", ThemeName::Andromeda),
            ("aurora", ThemeName::Aurora),
            ("ayu_dark", ThemeName::AyuDark),
            ("bubblegum", ThemeName::Bubblegum),
            ("catppuccin_latte", ThemeName::CatppuccinLatte),
            ("catppuccin_mocha", ThemeName::CatppuccinMocha),
            ("cyberpunk", ThemeName::Cyberpunk),
            ("dracula", ThemeName::Dracula),
            ("everforest_dark", ThemeName::EverforestDark),
            ("github_dark", ThemeName::GitHubDark),
            ("github_light", ThemeName::GitHubLight),
            ("gruvbox_dark", ThemeName::GruvboxDark),
            ("gruvbox_light", ThemeName::GruvboxLight),
            ("inferno", ThemeName::Inferno),
            ("kanagawa", ThemeName::Kanagawa),
            ("material_ocean", ThemeName::MaterialOcean),
            ("matrix", ThemeName::Matrix),
            ("monokai", ThemeName::Monokai),
            ("neon", ThemeName::Neon),
            ("nord", ThemeName::Nord),
            ("one_dark", ThemeName::OneDark),
            ("oxocarbon", ThemeName::Oxocarbon),
            ("rainbow", ThemeName::Rainbow),
            ("rose_pine", ThemeName::RosePine),
            ("solarized_dark", ThemeName::SolarizedDark),
            ("solarized_light", ThemeName::SolarizedLight),
            ("synthwave_84", ThemeName::Synthwave84),
            ("tokyo_night", ThemeName::TokyoNight),
            ("vesper", ThemeName::Vesper),
            ("zenburn", ThemeName::Zenburn),
        ];

        for (input, expected) in themes {
            let deserialized: ThemeName = serde_json::from_str(&format!("\"{}\"", input)).unwrap();
            assert_eq!(deserialized, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_known_themes_display_format() {
        // Test that all known themes deserialize correctly in display format
        let themes = vec![
            ("Andromeda", ThemeName::Andromeda),
            ("Aurora", ThemeName::Aurora),
            ("Ayu Dark", ThemeName::AyuDark),
            ("Bubblegum", ThemeName::Bubblegum),
            ("Catppuccin Latte", ThemeName::CatppuccinLatte),
            ("Catppuccin Mocha", ThemeName::CatppuccinMocha),
            ("Cyberpunk", ThemeName::Cyberpunk),
            ("Dracula", ThemeName::Dracula),
            ("Everforest Dark", ThemeName::EverforestDark),
            ("GitHub Dark", ThemeName::GitHubDark),
            ("GitHub Light", ThemeName::GitHubLight),
            ("Gruvbox Dark", ThemeName::GruvboxDark),
            ("Gruvbox Light", ThemeName::GruvboxLight),
            ("Inferno", ThemeName::Inferno),
            ("Kanagawa", ThemeName::Kanagawa),
            ("Material Ocean", ThemeName::MaterialOcean),
            ("Matrix", ThemeName::Matrix),
            ("Monokai", ThemeName::Monokai),
            ("Neon", ThemeName::Neon),
            ("Nord", ThemeName::Nord),
            ("One Dark", ThemeName::OneDark),
            ("Oxocarbon", ThemeName::Oxocarbon),
            ("Rainbow", ThemeName::Rainbow),
            ("Rose Pine", ThemeName::RosePine),
            ("Solarized Dark", ThemeName::SolarizedDark),
            ("Solarized Light", ThemeName::SolarizedLight),
            ("Synthwave '84", ThemeName::Synthwave84),
            ("Tokyo Night", ThemeName::TokyoNight),
            ("Vesper", ThemeName::Vesper),
            ("Zenburn", ThemeName::Zenburn),
        ];

        for (input, expected) in themes {
            let deserialized: ThemeName = serde_json::from_str(&format!("\"{}\"", input)).unwrap();
            assert_eq!(deserialized, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_unknown_themes_default_to_catppuccin_mocha() {
        // Test that unknown themes default to CatppuccinMocha
        let unknown_themes = vec![
            "cuppochinmocha",
            "invalid_theme",
            "unknown",
            "",
            "   ",
            "CatpuccinMocha",
            "mocha",
            "dark_theme",
        ];

        for input in unknown_themes {
            let deserialized: ThemeName = serde_json::from_str(&format!("\"{}\"", input)).unwrap();
            assert_eq!(
                deserialized,
                ThemeName::CatppuccinMocha,
                "Unknown theme '{}' should default to CatppuccinMocha",
                input
            );
        }
    }

    #[test]
    fn test_theme_default_is_catppuccin_mocha() {
        assert_eq!(ThemeName::default(), ThemeName::CatppuccinMocha);
    }

    #[test]
    fn test_theme_name_roundtrip() {
        // Test that all themes can be serialized and deserialized
        let all_themes = vec![
            ThemeName::Andromeda,
            ThemeName::Aurora,
            ThemeName::AyuDark,
            ThemeName::Bubblegum,
            ThemeName::CatppuccinLatte,
            ThemeName::CatppuccinMocha,
            ThemeName::Cyberpunk,
            ThemeName::Dracula,
            ThemeName::EverforestDark,
            ThemeName::GitHubDark,
            ThemeName::GitHubLight,
            ThemeName::GruvboxDark,
            ThemeName::GruvboxLight,
            ThemeName::Inferno,
            ThemeName::Kanagawa,
            ThemeName::MaterialOcean,
            ThemeName::Matrix,
            ThemeName::Monokai,
            ThemeName::Neon,
            ThemeName::Nord,
            ThemeName::OneDark,
            ThemeName::Oxocarbon,
            ThemeName::Rainbow,
            ThemeName::RosePine,
            ThemeName::SolarizedDark,
            ThemeName::SolarizedLight,
            ThemeName::Synthwave84,
            ThemeName::TokyoNight,
            ThemeName::Vesper,
            ThemeName::Zenburn,
        ];

        for theme in all_themes {
            let serialized = serde_json::to_string(&theme).unwrap();
            let deserialized: ThemeName = serde_json::from_str(&serialized).unwrap();
            assert_eq!(theme, deserialized);
        }
    }
}
