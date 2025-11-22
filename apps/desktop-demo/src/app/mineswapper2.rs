use compose_core::useState;
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier,
    Row, RowSpec, Size, Spacer, Text, VerticalAlignment,
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MineswapperTool {
    Reveal,
    Flag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPreset {
    pub name: &'static str,
    pub width: usize,
    pub height: usize,
    pub mines: usize,
}

const GRID_PRESETS: [GridPreset; 3] = [
    GridPreset {
        name: "Compact (8x8)",
        width: 8,
        height: 8,
        mines: 10,
    },
    GridPreset {
        name: "Roomy (12x12)",
        width: 12,
        height: 12,
        mines: 22,
    },
    GridPreset {
        name: "Spacious (16x16)",
        width: 16,
        height: 16,
        mines: 40,
    },
];

#[derive(Copy, Clone, Debug)]
struct MineswapperCell {
    is_mine: bool,
    is_revealed: bool,
    is_flagged: bool,
    adjacent: u8,
}

#[derive(Clone, Debug)]
struct MineswapperGame {
    width: usize,
    height: usize,
    mines: usize,
    cells: Vec<MineswapperCell>,
    is_lost: bool,
    is_won: bool,
    revealed_count: usize,
    seed: u64,
}

impl MineswapperGame {
    fn new_from_preset(preset: GridPreset, seed: u64) -> Self {
        let mut game = Self {
            width: preset.width,
            height: preset.height,
            mines: preset.mines,
            cells: vec![
                MineswapperCell {
                    is_mine: false,
                    is_revealed: false,
                    is_flagged: false,
                    adjacent: 0,
                };
                preset.width * preset.height
            ],
            is_lost: false,
            is_won: false,
            revealed_count: 0,
            seed,
        };
        game.reset(seed);
        game
    }

    fn reset(&mut self, seed: u64) {
        self.seed = seed;
        self.is_lost = false;
        self.is_won = false;
        self.revealed_count = 0;
        for cell in &mut self.cells {
            *cell = MineswapperCell {
                is_mine: false,
                is_revealed: false,
                is_flagged: false,
                adjacent: 0,
            };
        }

        let mut rng = seed | 1;
        let mut placed = 0;
        while placed < self.mines {
            rng ^= rng << 7;
            rng ^= rng >> 9;
            rng ^= rng << 8;
            let idx = (rng as usize) % self.cells.len();
            if !self.cells[idx].is_mine {
                self.cells[idx].is_mine = true;
                placed += 1;
            }
        }

        for y in 0..self.height {
            for x in 0..self.width {
                let adjacent = self
                    .neighbors(x, y)
                    .filter(|&(nx, ny)| self.cell_at(nx, ny).is_mine)
                    .count() as u8;
                self.cell_at_mut(x, y).adjacent = adjacent;
            }
        }
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    fn cell_at(&self, x: usize, y: usize) -> &MineswapperCell {
        &self.cells[self.idx(x, y)]
    }

    fn cell_at_mut(&mut self, x: usize, y: usize) -> &mut MineswapperCell {
        let idx = self.idx(x, y);
        &mut self.cells[idx]
    }

    fn neighbors(&self, x: usize, y: usize) -> impl Iterator<Item = (usize, usize)> {
        let width = self.width as isize;
        let height = self.height as isize;
        let x = x as isize;
        let y = y as isize;

        (-1..=1).flat_map(move |dy| {
            (-1..=1).filter_map(move |dx| {
                if dx == 0 && dy == 0 {
                    return None;
                }
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && nx < width && ny >= 0 && ny < height {
                    Some((nx as usize, ny as usize))
                } else {
                    None
                }
            })
        })
    }

    fn reveal(&mut self, x: usize, y: usize) {
        if self.is_lost || self.is_won {
            return;
        }
        if x >= self.width || y >= self.height {
            return;
        }
        if self.cell_at(x, y).is_flagged || self.cell_at(x, y).is_revealed {
            return;
        }

        if self.cell_at(x, y).is_mine {
            self.is_lost = true;
            for cell in &mut self.cells {
                if cell.is_mine {
                    cell.is_revealed = true;
                }
            }
            return;
        }

        let mut stack = vec![(x, y)];
        while let Some((cx, cy)) = stack.pop() {
            let idx = self.idx(cx, cy);
            if self.cells[idx].is_revealed || self.cells[idx].is_flagged {
                continue;
            }

            let adjacent = self.cells[idx].adjacent;
            self.cells[idx].is_revealed = true;
            self.revealed_count += 1;

            if adjacent == 0 {
                for (nx, ny) in self.neighbors(cx, cy) {
                    let neighbor = self.cell_at(nx, ny);
                    if !neighbor.is_revealed && !neighbor.is_flagged && !neighbor.is_mine {
                        stack.push((nx, ny));
                    }
                }
            }
        }

        if self.revealed_count + self.mines == self.width * self.height {
            self.is_won = true;
            for cell in &mut self.cells {
                if cell.is_mine {
                    cell.is_flagged = true;
                }
            }
        }
    }

    fn toggle_flag(&mut self, x: usize, y: usize) {
        if self.is_lost || self.is_won {
            return;
        }
        if x >= self.width || y >= self.height {
            return;
        }
        let cell = self.cell_at_mut(x, y);
        if cell.is_revealed {
            return;
        }
        cell.is_flagged = !cell.is_flagged;
    }

    fn flags_placed(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_flagged).count()
    }
}

fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[composable]
pub fn mineswapper2_tab() {
    let preset_state = useState(|| GRID_PRESETS[0]);
    let game_state = useState(|| MineswapperGame::new_from_preset(GRID_PRESETS[0], random_seed()));
    let flag_mode = useState(|| MineswapperTool::Reveal);

    Column(
        Modifier::empty()
            .padding(24.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        {
            let header_game_state = game_state;
            let header_flag_mode = flag_mode;
            let header_preset_state = preset_state;
            let content_game_state = game_state;
            let content_flag_mode = flag_mode;
            move || {
                Row(
                    Modifier::empty().fill_max_width().padding(8.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let game_state = header_game_state;
                        let flag_mode = header_flag_mode;
                        let preset_state = header_preset_state;
                        move || {
                            Text(
                                "Mineswapper 2",
                                Modifier::empty()
                                    .padding(10.0)
                                    .background(Color(1.0, 1.0, 1.0, 0.08))
                                    .rounded_corners(14.0),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 0.0,
                            });

                            Row(
                                Modifier::empty(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                {
                                    move || {
                                        for preset in GRID_PRESETS {
                                            let is_active = preset_state.get() == preset;
                                            Button(
                                                Modifier::empty()
                                                    .rounded_corners(12.0)
                                                    .draw_behind(move |scope| {
                                                        scope.draw_round_rect(
                                                            Brush::solid(if is_active {
                                                                Color(0.28, 0.48, 0.88, 1.0)
                                                            } else {
                                                                Color(0.20, 0.24, 0.34, 0.8)
                                                            }),
                                                            CornerRadii::uniform(12.0),
                                                        );
                                                    })
                                                    .padding(8.0),
                                                move || {
                                                    preset_state.set(preset);
                                                    game_state.set(
                                                        MineswapperGame::new_from_preset(
                                                            preset,
                                                            random_seed(),
                                                        ),
                                                    );
                                                },
                                                {
                                                    let label = preset.name;
                                                    move || {
                                                        Text(label, Modifier::empty().padding(4.0));
                                                    }
                                                },
                                            );
                                        }
                                    }
                                },
                            );

                            Button(
                                Modifier::empty()
                                    .rounded_corners(14.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.2, 0.45, 0.9, 1.0)),
                                            CornerRadii::uniform(14.0),
                                        );
                                    })
                                    .padding(10.0),
                                {
                                    move || {
                                        let preset = preset_state.get();
                                        game_state.set(MineswapperGame::new_from_preset(
                                            preset,
                                            random_seed(),
                                        ))
                                    }
                                },
                                || {
                                    Text("New Game", Modifier::empty().padding(4.0));
                                },
                            );

                            let mode_toggle = flag_mode;
                            Button(
                                Modifier::empty()
                                    .rounded_corners(14.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.45, 0.25, 0.45, 1.0)),
                                            CornerRadii::uniform(14.0),
                                        );
                                    })
                                    .padding(10.0),
                                move || {
                                    let next_mode = match mode_toggle.get() {
                                        MineswapperTool::Reveal => MineswapperTool::Flag,
                                        MineswapperTool::Flag => MineswapperTool::Reveal,
                                    };
                                    mode_toggle.set(next_mode);
                                },
                                {
                                    let mode_label = flag_mode;
                                    move || {
                                        let label = match mode_label.get() {
                                            MineswapperTool::Flag => "Flag mode",
                                            MineswapperTool::Reveal => "Reveal mode",
                                        };
                                        Text(label, Modifier::empty().padding(4.0));
                                    }
                                },
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                let game = content_game_state.get();
                let flag_mode_value = content_flag_mode.get();
                let status_text = if game.is_lost {
                    "You hit a mine!"
                } else if game.is_won {
                    "You cleared the field!"
                } else if flag_mode_value == MineswapperTool::Flag {
                    "Flag cells you suspect contain mines"
                } else {
                    "Reveal safe cells to clear the board"
                };

                Text(
                    format!(
                        "{} — Mines: {} | Flags: {} | Seed: {}",
                        preset_state.get().name,
                        game.mines,
                        game.flags_placed(),
                        game.seed % 100000
                    ),
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.15, 0.22, 0.34, 0.7))
                        .rounded_corners(12.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 8.0,
                });

                Text(
                    status_text,
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.12, 0.16, 0.28, 0.6))
                        .rounded_corners(12.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                let grid_width = game.width;
                let grid_height = game.height;
                let grid_state = content_game_state;
                let grid_flag_mode = content_flag_mode;

                Column(
                    Modifier::empty()
                        .background(Color(0.06, 0.08, 0.16, 0.9))
                        .rounded_corners(18.0)
                        .padding(12.0),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                    move || {
                        for y in 0..grid_height {
                            let game_state = grid_state;
                            let flag_mode = grid_flag_mode;
                            Row(
                                Modifier::empty().fill_max_width().padding(2.0),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(6.0))
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                move || {
                                    for x in 0..grid_width {
                                        let game_snapshot = game_state.get();
                                        let cell = *game_snapshot.cell_at(x, y);
                                        let display = if cell.is_revealed {
                                            if cell.is_mine {
                                                "💣".to_string()
                                            } else if cell.adjacent == 0 {
                                                "".to_string()
                                            } else {
                                                cell.adjacent.to_string()
                                            }
                                        } else if cell.is_flagged {
                                            "🚩".to_string()
                                        } else {
                                            "".to_string()
                                        };

                                        let background = if cell.is_revealed {
                                            if cell.is_mine {
                                                Color(0.6, 0.18, 0.2, 0.9)
                                            } else {
                                                Color(0.18, 0.26, 0.34, 0.9)
                                            }
                                        } else if cell.is_flagged {
                                            Color(0.26, 0.20, 0.32, 0.9)
                                        } else {
                                            Color(0.12, 0.14, 0.22, 0.9)
                                        };

                                        let game_action = game_state;
                                        let mode_state = flag_mode;
                                        Button(
                                            Modifier::empty()
                                                .size_points(36.0, 36.0)
                                                .rounded_corners(8.0)
                                                .draw_behind(move |scope| {
                                                    scope.draw_round_rect(
                                                        Brush::solid(background),
                                                        CornerRadii::uniform(8.0),
                                                    );
                                                }),
                                            move || match mode_state.get() {
                                                MineswapperTool::Flag => {
                                                    game_action
                                                        .update(|game| game.toggle_flag(x, y));
                                                }
                                                MineswapperTool::Reveal => {
                                                    game_action.update(|game| game.reveal(x, y));
                                                }
                                            },
                                            {
                                                let display_text = display;
                                                move || {
                                                    Text(
                                                        display_text.clone(),
                                                        Modifier::empty().padding(4.0),
                                                    );
                                                }
                                            },
                                        );
                                    }
                                },
                            );
                        }
                    },
                );
            }
        },
    );
}
