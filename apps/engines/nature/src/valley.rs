use crate::{
    mesh::MeshData,
    noise::{Rng, fbm},
    plants,
    terrain::{RIVER_HALF_WIDTH, Terrain},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
}

#[derive(Clone, Debug)]
pub struct PlantGroup {
    pub mesh: MeshData,
    pub double_sided: bool,
    pub placements: Vec<Placement>,
}

#[derive(Clone, Debug)]
pub struct Valley {
    pub terrain: Terrain,
    pub groups: Vec<PlantGroup>,
}

struct Rule {
    spacing: f32,
    accept: fn(&Site) -> f32,
    scale: (f32, f32),
    sink: f32,
    variants: std::ops::Range<usize>,
    clustered: bool,
}

struct Site {
    height: f32,
    slope: f32,
    river: f32,
    forest: f32,
    meadow: f32,
}

fn group(mesh: MeshData, double_sided: bool) -> PlantGroup {
    PlantGroup {
        mesh,
        double_sided,
        placements: Vec::new(),
    }
}

impl Valley {
    pub fn new(seed: u32) -> Self {
        let terrain = Terrain::new(seed);
        let mut groups = shapes(u64::from(seed));
        let mut rng = Rng::new(u64::from(seed) ^ 0xA5A5);
        for rule in &RULES {
            scatter(&terrain, seed, rule, &mut groups, &mut rng);
        }
        Self { terrain, groups }
    }

    pub fn instance_count(&self) -> usize {
        self.groups.iter().map(|group| group.placements.len()).sum()
    }
}

const PETALS: [[f32; 3]; 5] = [
    [0.92, 0.2, 0.22],
    [0.98, 0.84, 0.2],
    [0.6, 0.35, 0.9],
    [0.97, 0.97, 0.95],
    [0.98, 0.52, 0.75],
];

fn shapes(seed: u64) -> Vec<PlantGroup> {
    let grass = |offset, base, tip| plants::blades(seed + offset, 7, (0.25, 0.55), base, tip);
    let mut groups = vec![
        group(plants::pine(seed + 1), false),
        group(plants::pine(seed + 2), false),
        group(plants::broadleaf(seed + 3), false),
        group(plants::broadleaf(seed + 4), false),
        group(plants::bush(seed + 5), false),
        group(plants::bush(seed + 6), false),
        group(plants::rock(seed + 7), false),
        group(plants::rock(seed + 8), false),
        group(grass(9, [0.18, 0.36, 0.1], [0.55, 0.72, 0.28]), true),
        group(grass(10, [0.2, 0.4, 0.12], [0.62, 0.7, 0.3]), true),
        group(
            plants::blades(
                seed + 11,
                9,
                (0.9, 1.6),
                [0.2, 0.32, 0.12],
                [0.55, 0.62, 0.3],
            ),
            true,
        ),
    ];
    groups.extend(
        PETALS
            .iter()
            .enumerate()
            .map(|(index, petal)| group(plants::flowers(seed + 20 + index as u64, *petal), true)),
    );
    groups
}

const RULES: [Rule; 6] = [
    Rule {
        spacing: 5.0,
        accept: trees,
        scale: (0.75, 1.35),
        sink: 0.1,
        variants: 0..4,
        clustered: false,
    },
    Rule {
        spacing: 4.0,
        accept: bushes,
        scale: (0.7, 1.3),
        sink: 0.1,
        variants: 4..6,
        clustered: false,
    },
    Rule {
        spacing: 8.0,
        accept: rocks,
        scale: (0.4, 1.8),
        sink: 0.2,
        variants: 6..8,
        clustered: false,
    },
    Rule {
        spacing: 1.7,
        accept: grass,
        scale: (0.8, 1.3),
        sink: 0.02,
        variants: 8..10,
        clustered: false,
    },
    Rule {
        spacing: 1.5,
        accept: reeds,
        scale: (0.8, 1.2),
        sink: 0.05,
        variants: 10..11,
        clustered: false,
    },
    Rule {
        spacing: 2.4,
        accept: flowers,
        scale: (0.9, 1.25),
        sink: 0.0,
        variants: 11..16,
        clustered: true,
    },
];

fn trees(site: &Site) -> f32 {
    let ground = site.height > 0.9 && site.height < 30.0 && site.slope < 0.4;
    if ground && site.river > RIVER_HALF_WIDTH * 1.7 {
        ((site.forest + 0.05) * 2.4).clamp(0.0, 0.9)
    } else {
        0.0
    }
}

fn bushes(site: &Site) -> f32 {
    if site.height > 0.8 && site.slope < 0.4 && site.river > RIVER_HALF_WIDTH * 1.3 {
        ((site.forest + 0.3) * 0.6).clamp(0.0, 0.35)
    } else {
        0.0
    }
}

fn rocks(site: &Site) -> f32 {
    let riverside = if site.river < RIVER_HALF_WIDTH * 2.0 {
        0.4
    } else {
        0.0
    };
    (0.12 + site.slope + riverside).min(0.8)
}

fn grass(site: &Site) -> f32 {
    if site.height > 0.6 && site.height < 22.0 && site.slope < 0.45 {
        (0.45 + site.meadow * 0.5).clamp(0.0, 0.8)
    } else {
        0.0
    }
}

fn reeds(site: &Site) -> f32 {
    let edge = site.river > RIVER_HALF_WIDTH * 0.75 && site.river < RIVER_HALF_WIDTH * 1.5;
    if edge && site.height > -0.7 && site.height < 0.9 {
        0.7
    } else {
        0.0
    }
}

fn flowers(site: &Site) -> f32 {
    let ground = site.height > 0.7 && site.height < 14.0 && site.slope < 0.3;
    if ground && site.river > RIVER_HALF_WIDTH * 1.2 {
        ((site.meadow + 0.05) * 1.6).clamp(0.0, 0.85)
    } else {
        0.0
    }
}

fn scatter(terrain: &Terrain, seed: u32, rule: &Rule, groups: &mut [PlantGroup], rng: &mut Rng) {
    let edge = terrain.half_size - 3.0;
    let cells = (edge * 2.0 / rule.spacing) as i32;
    for row in 0..cells {
        for column in 0..cells {
            let x = -edge + (column as f32 + rng.next()) * rule.spacing;
            let z = -edge + (row as f32 + rng.next()) * rule.spacing;
            let site = site(terrain, seed, x, z);
            if rng.next() >= (rule.accept)(&site) {
                continue;
            }
            let pick = if rule.clustered {
                (fbm(x * 0.04, z * 0.04, 2, seed.wrapping_add(44)) * 1.8 + 0.5).clamp(0.0, 1.0)
            } else {
                rng.next()
            };
            let count = rule.variants.len();
            let variant = rule.variants.start + ((pick * count as f32) as usize).min(count - 1);
            groups[variant].placements.push(Placement {
                position: [x, site.height - rule.sink, z],
                yaw: rng.range(0.0, std::f32::consts::TAU),
                scale: rng.range(rule.scale.0, rule.scale.1),
            });
        }
    }
}

fn site(terrain: &Terrain, seed: u32, x: f32, z: f32) -> Site {
    Site {
        height: terrain.height(x, z),
        slope: terrain.slope(x, z),
        river: terrain.river_distance(x, z),
        forest: fbm(x * 0.018, z * 0.018, 3, seed.wrapping_add(21)),
        meadow: fbm(x * 0.025, z * 0.025, 3, seed.wrapping_add(33)),
    }
}
