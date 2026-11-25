use perf_data::{Launch, Scenario};

use super::{Box2, Gauntlet, Painter, Patchwork};

pub(crate) enum Workload {
    Gauntlet(Gauntlet),
    Patchwork(Patchwork),
}
impl Workload {
    pub fn new(load: Launch) -> Option<Self> {
        match load.scenario {
            Scenario::Gauntlet => Some(Self::Gauntlet(Gauntlet::new())),
            Scenario::Patchwork => Some(Self::Patchwork(Patchwork::new(load.tier))),
            Scenario::Wall => None,
        }
    }
    pub fn draw(&mut self, p: &mut impl Painter, area: Box2, k: u32, advance: bool) {
        match self {
            Self::Gauntlet(screen) => screen.draw(p, area, k),
            Self::Patchwork(screen) => {
                if advance {
                    screen.advance();
                }
                screen.draw(p, area, k);
            }
        }
    }
}
