use crate::{
    instant_triangles::InstantStandIn,
    render::{ImageSlot, TurnedGlyphs},
};

/// The quads a pass drew from its scratch, uploaded for its draws: fallback
/// triangles, image vertices and indices, and glyph instances.
#[derive(Default)]
pub(super) struct PassBuffers {
    pub(super) meshes: Option<crate::mesh_fallback::MeshBuffers>,
    pub(super) images: Option<ImageSlot>,
    pub(super) shadows: Option<crate::frame_graph::BufferUpload>,
    pub(super) glyphs: Option<crate::frame_graph::BufferUpload>,
    pub(super) turned_glyphs: Option<crate::frame_graph::BufferUpload>,
    pub(super) instant: Option<crate::instant_triangles::InstantBuffers>,
}

impl PassBuffers {
    // Where the pass draws its turned glyphs from: see `TurnedGlyphs`.
    pub(super) fn turned_glyphs<'a>(
        &'a self,
        stand_in: Option<InstantStandIn<'a>>,
    ) -> TurnedGlyphs<'a> {
        let laid = self
            .instant
            .as_ref()
            .and_then(|laid| laid.glyphs(stand_in?));
        laid.map_or(
            TurnedGlyphs::Own(self.turned_glyphs.as_ref()),
            TurnedGlyphs::Instant,
        )
    }

    /// The pass's fallback triangles, with the index range of each run draw
    /// of `meshes` that takes some.
    pub(super) fn run_meshes<'a>(
        &'a self,
        meshes: &'a crate::mesh_fallback::PassMeshes,
    ) -> Option<crate::mesh_fallback::RunMeshes<'a>> {
        self.meshes
            .as_ref()
            .map(|buffers| crate::mesh_fallback::RunMeshes {
                ranges: &meshes.ranges,
                chunks: meshes.builder.chunks(),
                buffers,
            })
    }
}
