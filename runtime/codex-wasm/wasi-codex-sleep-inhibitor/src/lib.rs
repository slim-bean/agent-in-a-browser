#![allow(dead_code, unused_variables)]
//! Stub for codex-utils-sleep-inhibitor in wasip2.

#[derive(Clone, Debug, Default)]
pub struct SleepInhibitor;

impl SleepInhibitor {
    pub fn new(_prevent_idle_sleep: bool) -> Self {
        Self
    }

    pub fn set_inhibit(&self, _inhibit: bool) {}

    pub fn set_turn_running(&self, _running: bool) {}
}
