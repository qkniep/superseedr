// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppState;
use crate::config::Settings;
use crate::theme::ThemeContext;

pub struct AppViewModel<'a> {
    pub state: &'a AppState,
}

impl<'a> AppViewModel<'a> {
    pub fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

pub struct ScreenContext<'a> {
    pub ui: &'a AppState,
    pub app: AppViewModel<'a>,
    pub settings: &'a Settings,
    pub theme: &'a ThemeContext,
}

impl<'a> ScreenContext<'a> {
    pub fn new(ui: &'a AppState, settings: &'a Settings, theme: &'a ThemeContext) -> Self {
        Self {
            ui,
            app: AppViewModel::new(ui),
            settings,
            theme,
        }
    }
}
