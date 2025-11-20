use compose_core::{self, MutableState};
use compose_foundation::PointerEventKind;
use compose_ui::{
    composable, BoxSpec, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement,
    Modifier, Point, PointerInputScope, Row, RowSpec, Size, Spacer, Text, VerticalAlignment,
};

// Game difficulty levels
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}

impl Difficulty {
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy (8x8, 10 mines)",
            Difficulty::Medium => "Medium (10x10, 15 mines)",
            Difficulty::Hard => "Hard (12x12, 25 mines)",
        }
    }

    pub fn grid_size(self) -> (usize, usize) {
        match self {
            Difficulty::Easy => (8, 8),
            Difficulty::Medium => (10, 10),
            Difficulty::Hard => (12, 12),
        }
    }

    pub fn mine_count(self) -> usize {
        match self {
            Difficulty::Easy => 10,
            Difficulty::Medium => 15,
            Difficulty::Hard => 25,
        }
    }
}

// Cell states
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CellState {
    Hidden,
    Revealed,
    Flagged,
}

// Game status
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GameStatus {
    Playing,
    Won,
    Lost,
}

// Minesweeper grid state
#[derive(Clone, Debug)]
struct MinesweeperGrid {
    width: usize,
    height: usize,
    mines: Vec<Vec<bool>>,
    states: Vec<Vec<CellState>>,
    adjacent_counts: Vec<Vec<u8>>,
    status: GameStatus,
    total_mines: usize,
    start_time: Option<std::time::Instant>,
}

impl MinesweeperGrid {
    fn new(width: usize, height: usize, num_mines: usize) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut mines = vec![vec![false; width]; height];
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut rng = nanos;

        // Place mines randomly
        let mut placed = 0;
        while placed < num_mines {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let row = ((rng / 65536) % (height as u128)) as usize;
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let col = ((rng / 65536) % (width as u128)) as usize;

            if !mines[row][col] {
                mines[row][col] = true;
                placed += 1;
            }
        }

        // Calculate adjacent mine counts
        let mut adjacent_counts = vec![vec![0u8; width]; height];
        for row in 0..height {
            for col in 0..width {
                if !mines[row][col] {
                    let mut count = 0u8;
                    for dr in -1i32..=1 {
                        for dc in -1i32..=1 {
                            if dr == 0 && dc == 0 {
                                continue;
                            }
                            let nr = row as i32 + dr;
                            let nc = col as i32 + dc;
                            if nr >= 0 && nr < height as i32 && nc >= 0 && nc < width as i32 {
                                if mines[nr as usize][nc as usize] {
                                    count += 1;
                                }
                            }
                        }
                    }
                    adjacent_counts[row][col] = count;
                }
            }
        }

        Self {
            width,
            height,
            mines,
            states: vec![vec![CellState::Hidden; width]; height],
            adjacent_counts,
            status: GameStatus::Playing,
            total_mines: num_mines,
            start_time: None,
        }
    }

    fn reveal(&mut self, row: usize, col: usize) {
        if self.status != GameStatus::Playing {
            return;
        }

        // Start timer on first move
        if self.start_time.is_none() {
            self.start_time = Some(std::time::Instant::now());
        }

        if self.states[row][col] != CellState::Hidden {
            return;
        }

        if self.mines[row][col] {
            // Hit a mine - game over
            self.states[row][col] = CellState::Revealed;
            self.status = GameStatus::Lost;
            // Reveal all mines
            for r in 0..self.height {
                for c in 0..self.width {
                    if self.mines[r][c] {
                        self.states[r][c] = CellState::Revealed;
                    }
                }
            }
            return;
        }

        // Reveal cell
        self.states[row][col] = CellState::Revealed;

        // If no adjacent mines, reveal adjacent cells recursively
        if self.adjacent_counts[row][col] == 0 {
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = row as i32 + dr;
                    let nc = col as i32 + dc;
                    if nr >= 0 && nr < self.height as i32 && nc >= 0 && nc < self.width as i32 {
                        self.reveal(nr as usize, nc as usize);
                    }
                }
            }
        }

        // Check if won
        self.check_win();
    }

    fn toggle_flag(&mut self, row: usize, col: usize) {
        if self.status != GameStatus::Playing {
            return;
        }

        // Start timer on first move
        if self.start_time.is_none() {
            self.start_time = Some(std::time::Instant::now());
        }

        match self.states[row][col] {
            CellState::Hidden => self.states[row][col] = CellState::Flagged,
            CellState::Flagged => self.states[row][col] = CellState::Hidden,
            CellState::Revealed => {}
        }

        self.check_win();
    }

    fn check_win(&mut self) {
        // Check if all non-mine cells are revealed
        let mut all_revealed = true;
        for row in 0..self.height {
            for col in 0..self.width {
                if !self.mines[row][col] && self.states[row][col] != CellState::Revealed {
                    all_revealed = false;
                    break;
                }
            }
            if !all_revealed {
                break;
            }
        }

        if all_revealed {
            self.status = GameStatus::Won;
        }
    }

    fn flag_count(&self) -> usize {
        let mut count = 0;
        for row in 0..self.height {
            for col in 0..self.width {
                if self.states[row][col] == CellState::Flagged {
                    count += 1;
                }
            }
        }
        count
    }

    fn elapsed_seconds(&self) -> u64 {
        self.start_time
            .map(|start| start.elapsed().as_secs())
            .unwrap_or(0)
    }
}

#[composable]
pub fn minesweeper_game() {
    let difficulty = compose_core::useState(|| Difficulty::Medium);
    let grid = compose_core::useState(|| {
        let diff = Difficulty::Medium;
        let (width, height) = diff.grid_size();
        MinesweeperGrid::new(width, height, diff.mine_count())
    });
    let flag_mode = compose_core::useState(|| false);

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            let grid_state = grid.clone();
            let flag_mode_state = flag_mode.clone();
            let difficulty_state = difficulty.clone();

            Text(
                "Minesweeper",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(16.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Difficulty selection
            let difficulty_for_selection = difficulty_state.clone();
            let grid_for_difficulty = grid_state.clone();
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding(8.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    let diff_state = difficulty_for_selection.clone();
                    let grid_for_diff = grid_for_difficulty.clone();
                    let current_diff = diff_state.get();

                    Text(
                        "Difficulty:",
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(0.15, 0.18, 0.25, 0.8))
                            .rounded_corners(10.0),
                    );

                    for diff in [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard] {
                        let is_selected = current_diff == diff;
                        Button(
                            Modifier::empty()
                                .rounded_corners(10.0)
                                .draw_behind(move |scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(if is_selected {
                                            Color(0.25, 0.45, 0.85, 1.0)
                                        } else {
                                            Color(0.2, 0.25, 0.35, 0.8)
                                        }),
                                        CornerRadii::uniform(10.0),
                                    );
                                })
                                .padding(8.0),
                            {
                                let diff_state = diff_state.clone();
                                let grid = grid_for_diff.clone();
                                move || {
                                    diff_state.set(diff);
                                    let (width, height) = diff.grid_size();
                                    grid.set(MinesweeperGrid::new(width, height, diff.mine_count()));
                                }
                            },
                            {
                                let label = diff.label();
                                move || {
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

            // Game stats (timer and mine counter)
            let grid_for_stats = grid_state.clone();
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding(8.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    let current_grid = grid_for_stats.get();
                    let elapsed = current_grid.elapsed_seconds();
                    let flags_used = current_grid.flag_count();
                    let total_mines = current_grid.total_mines;
                    let mines_remaining = total_mines.saturating_sub(flags_used);

                    // Timer
                    Text(
                        format!("Time: {}s", elapsed),
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.15, 0.25, 0.35, 0.8))
                            .rounded_corners(12.0),
                    );

                    // Mine counter
                    Text(
                        format!("Mines: {} / {}", mines_remaining, total_mines),
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.35, 0.25, 0.15, 0.8))
                            .rounded_corners(12.0),
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            // Status and controls
            let grid_for_controls = grid_state.clone();
            let flag_mode_for_controls = flag_mode_state.clone();
            let difficulty_for_new_game = difficulty_state.clone();
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding(8.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    let grid_inner = grid_for_controls.clone();
                    let flag_mode_inner = flag_mode_for_controls.clone();
                    let current_grid = grid_inner.get();

                    // Status message
                    let status_text = match current_grid.status {
                        GameStatus::Playing => {
                            "Playing - Click to reveal, toggle flag mode to mark mines"
                        }
                        GameStatus::Won => "You Won! Start a new game.",
                        GameStatus::Lost => "Game Over! You hit a mine.",
                    };

                    let status_color = match current_grid.status {
                        GameStatus::Playing => Color(0.2, 0.4, 0.6, 0.8),
                        GameStatus::Won => Color(0.2, 0.7, 0.3, 0.8),
                        GameStatus::Lost => Color(0.7, 0.2, 0.2, 0.8),
                    };

                    Text(
                        status_text,
                        Modifier::empty()
                            .padding(10.0)
                            .background(status_color)
                            .rounded_corners(12.0),
                    );

                    Spacer(Size {
                        width: 12.0,
                        height: 0.0,
                    });

                    // Flag mode toggle button
                    let is_flag_mode = flag_mode_inner.get();
                    Button(
                        Modifier::empty()
                            .rounded_corners(12.0)
                            .draw_behind(move |scope| {
                                scope.draw_round_rect(
                                    Brush::solid(if is_flag_mode {
                                        Color(0.9, 0.6, 0.2, 1.0)
                                    } else {
                                        Color(0.3, 0.4, 0.5, 1.0)
                                    }),
                                    CornerRadii::uniform(12.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let flag_mode = flag_mode_inner.clone();
                            move || {
                                flag_mode.set(!flag_mode.get());
                            }
                        },
                        {
                            let mode_text = if is_flag_mode {
                                "🚩 Flag Mode"
                            } else {
                                "🔍 Reveal Mode"
                            };
                            move || {
                                Text(mode_text, Modifier::empty().padding(4.0));
                            }
                        },
                    );

                    Spacer(Size {
                        width: 12.0,
                        height: 0.0,
                    });

                    // New game button
                    Button(
                        Modifier::empty()
                            .rounded_corners(12.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.2, 0.6, 0.4, 1.0)),
                                    CornerRadii::uniform(12.0),
                                );
                            })
                            .padding(10.0),
                        {
                            let grid = grid_inner.clone();
                            let diff_state = difficulty_for_new_game.clone();
                            move || {
                                let diff = diff_state.get();
                                let (width, height) = diff.grid_size();
                                grid.set(MinesweeperGrid::new(width, height, diff.mine_count()));
                            }
                        },
                        || {
                            Text("🔄 New Game", Modifier::empty().padding(4.0));
                        },
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Game grid - use with_key to force recreation when dimensions change
            let grid_for_render = grid.clone();
            let flag_mode_for_render = flag_mode.clone();
            let grid_key = grid_for_render.get();
            let grid_size_key = (grid_key.width, grid_key.height);

            // Mouse pointer follower
            let pointer_position = compose_core::useState(|| Point { x: 0.0, y: 0.0 });
            let pointer_inside = compose_core::useState(|| false);

            compose_core::with_key(&grid_size_key, || {
                let pointer_pos = pointer_position.clone();
                let pointer_in = pointer_inside.clone();
                let flag_mode_for_pointer = flag_mode_for_render.clone();

                compose_ui::Box(
                    Modifier::empty()
                        .pointer_input((), {
                            let pointer_position = pointer_pos.clone();
                            let pointer_inside = pointer_in.clone();
                            move |scope: PointerInputScope| {
                                let pointer_position = pointer_position.clone();
                                let pointer_inside = pointer_inside.clone();
                                async move {
                                    scope
                                        .await_pointer_event_scope(|await_scope| async move {
                                            loop {
                                                let event = await_scope.await_pointer_event().await;
                                                match event.kind {
                                                    PointerEventKind::Move => {
                                                        pointer_position.set(Point {
                                                            x: event.position.x,
                                                            y: event.position.y,
                                                        });
                                                        pointer_inside.set(true);
                                                    }
                                                    PointerEventKind::Cancel => {
                                                        pointer_inside.set(false);
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        })
                                        .await;
                                }
                            }
                        }),
                    BoxSpec::default(),
                    move || {
                        let grid_for_column = grid_for_render.clone();
                        let flag_mode_for_column = flag_mode_for_render.clone();

                        Column(
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.06, 0.08, 0.16, 0.9))
                                .rounded_corners(20.0),
                            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                            move || {
                                let current_grid = grid_for_column.get();
                                for row in 0..current_grid.height {
                                    let grid_for_row = grid_for_column.clone();
                                    let flag_mode_for_row = flag_mode_for_column.clone();
                                    Row(
                                        Modifier::empty(),
                                        RowSpec::new()
                                            .horizontal_arrangement(LinearArrangement::SpacedBy(4.0)),
                                        move || {
                                            let grid_row = grid_for_row.clone();
                                            let flag_mode_row = flag_mode_for_row.clone();
                                            let width = grid_row.get().width;
                                            for col in 0..width {
                                                let grid_cell = grid_row.clone();
                                                let flag_mode_cell = flag_mode_row.clone();
                                                render_cell(grid_cell, flag_mode_cell, row, col);
                                            }
                                        },
                                    );
                                }
                            },
                        );

                        // Render pointer follower if mouse is inside
                        let is_inside = pointer_in.get();
                        if is_inside {
                            let pos = pointer_pos.get();
                            let is_flag_mode = flag_mode_for_pointer.get();
                            let icon = if is_flag_mode { "🚩" } else { "🔍" };
                            let icon_color = if is_flag_mode {
                                Color(0.9, 0.6, 0.2, 0.9)
                            } else {
                                Color(0.4, 0.6, 0.9, 0.9)
                            };

                            compose_ui::Box(
                                Modifier::empty()
                                    .size_points(30.0, 30.0)
                                    .offset(pos.x - 15.0, pos.y - 15.0)
                                    .rounded_corners(15.0)
                                    .background(icon_color)
                                    .padding(5.0),
                                BoxSpec::default(),
                                move || {
                                    Text(icon, Modifier::empty());
                                },
                            );
                        }
                    },
                );
            });
        },
    );
}

#[composable]
fn render_cell(
    grid_state: MutableState<MinesweeperGrid>,
    flag_mode: MutableState<bool>,
    row: usize,
    col: usize,
) {
    let grid = grid_state.get();

    // Safety check: ensure we're within grid bounds
    if row >= grid.height || col >= grid.width {
        return;
    }

    let cell_state = grid.states[row][col];
    let is_mine = grid.mines[row][col];
    let adjacent_count = grid.adjacent_counts[row][col];

    let (bg_color, text_content) = match cell_state {
        CellState::Hidden => (Color(0.3, 0.35, 0.45, 1.0), String::new()),
        CellState::Flagged => (Color(0.9, 0.6, 0.2, 1.0), "🚩".to_string()),
        CellState::Revealed => {
            if is_mine {
                (Color(0.8, 0.2, 0.2, 1.0), "💣".to_string())
            } else if adjacent_count > 0 {
                (
                    Color(0.15, 0.18, 0.25, 1.0),
                    adjacent_count.to_string(),
                )
            } else {
                (Color(0.15, 0.18, 0.25, 1.0), String::new())
            }
        }
    };

    Button(
        Modifier::empty()
            .size_points(35.0, 35.0)
            .rounded_corners(6.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(bg_color), CornerRadii::uniform(6.0));
            })
            .padding(2.0),
        {
            let grid = grid_state.clone();
            let flag_mode = flag_mode.clone();
            move || {
                let mut current_grid = grid.get();
                let is_flag_mode = flag_mode.get();

                if is_flag_mode {
                    current_grid.toggle_flag(row, col);
                } else {
                    current_grid.reveal(row, col);
                }

                grid.set(current_grid);
            }
        },
        {
            move || {
                if !text_content.is_empty() {
                    Text(text_content.clone(), Modifier::empty());
                }
            }
        },
    );
}
