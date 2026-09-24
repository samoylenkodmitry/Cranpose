//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod scene_probe;

mod app_supplied_font_raster;
mod draw_scope_text_raster;
mod font_tracking;
mod gpos_kerning_measure;
mod layer_and_raster_rules;
mod lazy_trim_scene;
mod scaling_list_scene;
mod wear_faded_row_composite;
mod weight_synthesis_fakery;
