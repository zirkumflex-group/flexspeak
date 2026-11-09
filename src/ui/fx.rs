//! The window's compositor: Rune's own frame, with received video drawn in the place of each
//! video element.

use rune::compose::{Compositor, Frame};
use rune::render::EffectPhase;

use crate::App;

#[derive(Default)]
pub struct Effects {
    videos: super::video::Videos,
}

impl Compositor<App> for Effects {
    fn render(&mut self, f: &mut Frame<'_, App>) {
        // The first frame built the frosted backdrop's pipelines; the next can drop the speck.
        f.rt.state.ui.warm_blur = false;
        if !self.videos.prepare(f) {
            f.draw_ui();
            return;
        }
        let (device, queue, size) = (f.device, f.queue, f.size);
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.draw_with_backdrop(f.encoder, f.target, clear, &mut |phase, site| {
            if let EffectPhase::Draw(pass) = phase {
                self.videos.draw(device, pass, site);
            }
        });
    }
}
