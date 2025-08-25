/*
 * This file is part of Hyperfan.
 *
 * Copyright (C) 2025 Hyperfan contributors
 *
 * Hyperfan is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Hyperfan is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Hyperfan. If not, see <https://www.gnu.org/licenses/>.
 */

//! Hyperfan - Fan control TUI for Linux using hwmon
//! 
//! This library provides the core functionality for reading hardware monitoring
//! sensors, controlling PWM fans, and managing fan curves.

pub mod hwmon;
pub mod app;
pub mod config;
pub mod system;
pub mod handlers;
pub mod ui;
pub mod service;
pub mod ec;
pub mod curves;
pub mod logger;

#[cfg(test)]
pub mod test_utils;
