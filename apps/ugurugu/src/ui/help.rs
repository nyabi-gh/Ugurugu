// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! The help window (F1), as 2.2.13's `ui/HelpDialog.cpp`: it stays open
//! beside the canvas. 3.0 adds installing, updates and reporting a problem.

use crate::i18n::tr;
use crate::theme;

/// Each section's heading and paragraphs.
const SECTIONS: [(&str, &[&str]); 4] = [
    ("help-start", &["help-start-draw", "help-start-export"]),
    (
        "help-files",
        &["help-files-format", "help-files-import", "help-files-older"],
    ),
    (
        "help-install",
        &[
            "help-install-beside",
            "help-install-smartscreen",
            "help-install-updates",
        ],
    ),
    ("help-problems", &["help-problems-logs"]),
];

pub fn show(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(tr("help-title"))
        .open(open)
        .collapsible(false)
        .default_size([520.0, 440.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (heading, paragraphs) in SECTIONS {
                    ui.label(
                        egui::RichText::new(tr(heading))
                            .size(theme::BODY + 2.0)
                            .strong(),
                    );
                    for paragraph in paragraphs {
                        ui.label(tr(paragraph));
                        ui.add_space(4.0);
                    }
                    ui.add_space(8.0);
                }
                ui.label(egui::RichText::new(tr("help-again")).color(theme::MUTED));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_heading_and_paragraph_has_a_text() {
        let keys = SECTIONS
            .iter()
            .flat_map(|(heading, paragraphs)| std::iter::once(heading).chain(paragraphs.iter()))
            .chain([&"help-title", &"help-again"]);
        for key in keys {
            assert_ne!(tr(key), *key);
        }
    }
}
