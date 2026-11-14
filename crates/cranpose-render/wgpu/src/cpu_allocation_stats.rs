#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebugCpuAllocationStats {
    pub scene_graph_node_count: usize,
    pub scene_graph_heap_bytes: usize,
    pub scene_hits: cranpose_render_common::graph_scene::HitStorage,
    pub text_renderer_pool_len: usize,
    pub text_renderer_pool_cap: usize,
    pub image_texture_cache_len: usize,
    pub image_texture_cache_cap: usize,
    pub run_arena_staging_bytes: usize,
    pub run_store_bytes: usize,
    pub run_store_runs: usize,
    pub scratch_image_vertices_cap: usize,
    pub scratch_image_indices_cap: usize,
    pub scratch_image_cmds_cap: usize,
    pub scratch_glyph_instances_cap: usize,
    pub layer_cache_len: usize,
    pub layer_cache_bytes: u64,
}
