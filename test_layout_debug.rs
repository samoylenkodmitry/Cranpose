// Simple test to debug layout constraint propagation

use compose_ui::{
    composable, Column, ColumnSpec, Row, RowSpec, Modifier, Text, LinearArrangement,
};

#[composable]
fn test_layout() {
    // Simulating the counter_app structure
    Column(Modifier::empty().padding(20.0), ColumnSpec::default(), move || {
        // This Row should get max_width = 800 - 40 = 760
        Row(
            Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
            RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
            move || {
                Text("Button 1", Modifier::empty().padding(10.0));
                Text("Button 2", Modifier::empty().padding(10.0));
            }
        );
    });
}

fn main() {
    println!("Test layout constraints");
    // This would need the full compose runtime to actually run
}
