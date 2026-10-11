
#[cfg(test)]
mod review_audit {
    use super::*;

    #[test]
    fn default_layout_at_common_logical_window_sizes() {
        for [width, height] in [[1600., 900.], [1280., 720.], [960., 540.], [800., 600.]] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            ctx.enable_accesskit();
            let mut canvas = Canvas::new(ugu_core::document::Document::new([64, 64]), |_| {});
            let mut files = Files::new(0, |_| {});
            let mut clipboard = Clipboard::new(|_| {});
            let mut settings = Store::open(None);
            let mut panels = Panels::default();
            for pass in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height))),
                    focused: true,
                    ..Default::default()
                };
                let mut area = [0; 4];
                let output = ctx.run_ui(input, |ui| {
                    area = window(ui, Parts {
                        canvas: &mut canvas, files: &mut files, clipboard: &mut clipboard,
                        settings: &mut settings, panels: &mut panels, typed: &[], warning: None,
                        diagnostics: None, probe: None,
                    }).canvas_area;
                });
                if pass == 2 {
                    println!("UI {}x{} canvas={area:?} canvas_width={} canvas_height={}",
                        width, height, area[2]-area[0], area[3]-area[1]);
                    if let Some(tree) = output.platform_output.accesskit_update {
                        for (_, node) in tree.nodes {
                            if let Some(bounds) = node.bounds() {
                                if bounds.y0 >= f64::from(height) || bounds.x0 >= f64::from(width) {
                                    println!("UI outside role={:?} label={:?} value={:?} bounds={bounds:?}",
                                        node.role(), node.label(), node.value());
                                }
                            }
                        }
                    }
                }
            }
            files.end();
        }
    }
}
