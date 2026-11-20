//! The calls of Meta's spatial entities that the binding wraps in no safe function.
#![expect(
    unsafe_code,
    reason = "XR_FB_spatial_entity, its query and its storage, whose structs hold pointers"
)]

use std::{ffi::c_char, ptr};

use openxr as xr;
use xr::sys::Handle;

use super::uuid_wire::wire_of;
use crate::Uuid;

// Which entities a query asks for.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Filter {
    // The ones in the app's local storage
    Stored,
    // The one with this identifier
    Uuid(Uuid),
    // Those that have this component enabled
    Component(xr::sys::SpaceComponentTypeFB),
}

// Takes the ownership of a space that the runtime made for the session: it ends with the value.
pub(crate) fn adopt(
    session: &xr::Session<super::super::graphics::Api>,
    space: xr::sys::Space,
) -> xr::Space {
    // SAFETY: the runtime made the space for this session, and nothing else owns it.
    unsafe { xr::Space::reference_from_raw(session.clone(), space) }
}

// Runs a request of the storage extension, whose function table the instance holds.
fn in_storage(
    instance: &xr::Instance,
    call: impl FnOnce(
        &xr::raw::SpatialEntityStorageFB,
        &mut xr::sys::AsyncRequestIdFB,
    ) -> xr::sys::Result,
) -> Result<u64, xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_spatial_entity_storage
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    requested(|request| call(fp, request))
}

fn failed(result: xr::sys::Result) -> bool {
    result.into_raw() < 0
}

// Runs a call that starts an asynchronous request, with a request id for the runtime to fill, and
// returns the id or the result that the runtime refused the request with.
fn requested(
    call: impl FnOnce(&mut xr::sys::AsyncRequestIdFB) -> xr::sys::Result,
) -> Result<u64, xr::sys::Result> {
    let mut request = xr::sys::AsyncRequestIdFB::from_raw(0);
    let result = call(&mut request);
    if failed(result) {
        Err(result)
    } else {
        Ok(request.into_raw())
    }
}

// Starts a query of the session's spatial entities. The runtime answers with events: the results
// are available, then the query is complete. Returns the id of the request. `exclude` leaves out the
// entities that have the component enabled.
pub(crate) fn query(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    filter: Filter,
    exclude: Option<xr::sys::SpaceComponentTypeFB>,
    limit: u32,
) -> Result<u64, xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_spatial_entity_query
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let mut wire = wire_of(match filter {
        Filter::Uuid(uuid) => uuid,
        _ => Uuid::from_bytes([0; 16]),
    });
    let location = xr::sys::SpaceStorageLocationFilterInfoFB {
        ty: xr::sys::SpaceStorageLocationFilterInfoFB::TYPE,
        next: ptr::null(),
        location: xr::sys::SpaceStorageLocationFB::LOCAL,
    };
    let by_uuid = xr::sys::SpaceUuidFilterInfoFB {
        ty: xr::sys::SpaceUuidFilterInfoFB::TYPE,
        next: ptr::null(),
        uuid_count: 1,
        uuids: &mut wire,
    };
    let by_component = |component| xr::sys::SpaceComponentFilterInfoFB {
        ty: xr::sys::SpaceComponentFilterInfoFB::TYPE,
        next: ptr::null(),
        component_type: component,
    };
    let included = match filter {
        Filter::Component(component) => by_component(component),
        _ => by_component(xr::sys::SpaceComponentTypeFB::LOCATABLE),
    };
    let left_out = exclude.map(by_component);
    let filter_header: *const xr::sys::SpaceFilterInfoBaseHeaderFB = match filter {
        Filter::Stored => ptr::from_ref(&location).cast(),
        Filter::Uuid(_) => ptr::from_ref(&by_uuid).cast(),
        Filter::Component(_) => ptr::from_ref(&included).cast(),
    };
    let info = xr::sys::SpaceQueryInfoFB {
        ty: xr::sys::SpaceQueryInfoFB::TYPE,
        next: ptr::null(),
        query_action: xr::sys::SpaceQueryActionFB::LOAD,
        max_result_count: limit,
        timeout: xr::Duration::from_nanos(0),
        filter: filter_header,
        exclude_filter: left_out
            .as_ref()
            .map_or(ptr::null(), |left_out| ptr::from_ref(left_out).cast()),
    };
    // SAFETY: the session is live. `info` and the filters it names are initialized with their types
    // and outlive the call.
    requested(|request| unsafe {
        (fp.query_spaces)(session.as_raw(), ptr::from_ref(&info).cast(), request)
    })
}

// Reads the results of the query `request` into `out`, which keeps its storage: the two-call
// idiom. Each result holds a space that the caller owns.
pub(crate) fn results(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    request: u64,
    out: &mut Vec<xr::sys::SpaceQueryResultFB>,
) -> Result<(), xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_spatial_entity_query
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let request = xr::sys::AsyncRequestIdFB::from_raw(request);
    let mut results = xr::sys::SpaceQueryResultsFB {
        ty: xr::sys::SpaceQueryResultsFB::TYPE,
        next: ptr::null_mut(),
        result_capacity_input: 0,
        result_count_output: 0,
        results: ptr::null_mut(),
    };
    // SAFETY: the session is live, and `results` is initialized with its type. A capacity of zero
    // asks for the count and reads no array.
    let asked =
        unsafe { (fp.retrieve_space_query_results)(session.as_raw(), request, &mut results) };
    if failed(asked) {
        return Err(asked);
    }
    out.clear();
    let blank = xr::sys::SpaceQueryResultFB {
        space: xr::sys::Space::NULL,
        uuid: xr::sys::Uuid::default(),
    };
    out.resize(results.result_count_output as usize, blank);
    results.result_capacity_input = results.result_count_output;
    results.results = out.as_mut_ptr();
    // SAFETY: `out` holds `result_capacity_input` results, which the runtime fills.
    let filled =
        unsafe { (fp.retrieve_space_query_results)(session.as_raw(), request, &mut results) };
    if failed(filled) {
        out.clear();
        return Err(filled);
    }
    out.truncate(results.result_count_output as usize);
    Ok(())
}

// Whether the entity has the component enabled, or `None` when the runtime cannot tell.
pub(crate) fn component_enabled(
    instance: &xr::Instance,
    space: xr::sys::Space,
    component: xr::sys::SpaceComponentTypeFB,
) -> Option<bool> {
    let fp = instance.exts().fb_spatial_entity.as_ref()?;
    let mut status = xr::sys::SpaceComponentStatusFB {
        ty: xr::sys::SpaceComponentStatusFB::TYPE,
        next: ptr::null_mut(),
        enabled: false.into(),
        change_pending: false.into(),
    };
    // SAFETY: the space is live, and `status` is initialized with its type.
    let result = unsafe { (fp.get_space_component_status)(space, component, &mut status) };
    (!failed(result)).then(|| status.enabled.into())
}

// Asks the runtime to enable the component of the entity. It answers with an event. Returns the id
// of the request.
pub(crate) fn enable_component(
    instance: &xr::Instance,
    space: xr::sys::Space,
    component: xr::sys::SpaceComponentTypeFB,
) -> Result<u64, xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_spatial_entity
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let info = xr::sys::SpaceComponentStatusSetInfoFB {
        ty: xr::sys::SpaceComponentStatusSetInfoFB::TYPE,
        next: ptr::null(),
        component_type: component,
        enabled: true.into(),
        timeout: xr::Duration::from_nanos(0),
    };
    // SAFETY: the space is live, and `info` is initialized with its type.
    requested(|request| unsafe { (fp.set_space_component_status)(space, &info, request) })
}

// Asks the runtime to create an anchor at `pose` in `base` at `time`. It answers with an event that
// carries the anchor's space. Returns the id of the request.
pub(crate) fn create_anchor(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    base: &xr::Space,
    time: xr::Time,
    pose: xr::Posef,
) -> Result<u64, xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_spatial_entity
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let info = xr::sys::SpatialAnchorCreateInfoFB {
        ty: xr::sys::SpatialAnchorCreateInfoFB::TYPE,
        next: ptr::null(),
        space: base.as_raw(),
        pose_in_space: pose,
        time,
    };
    // SAFETY: the session and the base space are live, and `info` is initialized with its type.
    requested(|request| unsafe { (fp.create_spatial_anchor)(session.as_raw(), &info, request) })
}

// Asks the runtime to keep the entity in the app's local storage for good. It answers with an
// event that carries the entity's identifier. Returns the id of the request.
pub(crate) fn save(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    space: xr::sys::Space,
) -> Result<u64, xr::sys::Result> {
    let info = xr::sys::SpaceSaveInfoFB {
        ty: xr::sys::SpaceSaveInfoFB::TYPE,
        next: ptr::null(),
        space,
        location: xr::sys::SpaceStorageLocationFB::LOCAL,
        persistence_mode: xr::sys::SpacePersistenceModeFB::INDEFINITE,
    };
    // SAFETY: the session and the space are live, and `info` is initialized with its type.
    in_storage(instance, |fp, request| unsafe {
        (fp.save_space)(session.as_raw(), &info, request)
    })
}

// Asks the runtime to delete the entity from the app's local storage. It answers with an event.
// Returns the id of the request.
pub(crate) fn erase(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    space: xr::sys::Space,
) -> Result<u64, xr::sys::Result> {
    let info = xr::sys::SpaceEraseInfoFB {
        ty: xr::sys::SpaceEraseInfoFB::TYPE,
        next: ptr::null(),
        space,
        location: xr::sys::SpaceStorageLocationFB::LOCAL,
    };
    // SAFETY: the session and the space are live, and `info` is initialized with its type.
    in_storage(instance, |fp, request| unsafe {
        (fp.erase_space)(session.as_raw(), &info, request)
    })
}

// The rectangle of a scene entity in the X-Y plane of its space: its minimum corner and its size.
pub(crate) fn bounding_box(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    space: xr::sys::Space,
) -> Result<([f32; 2], [f32; 2]), xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_scene
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let mut rect = xr::sys::Rect2Df::default();
    // SAFETY: the session and the space are live, and `rect` is a value that the runtime fills.
    let result = unsafe { (fp.get_space_bounding_box2_d)(session.as_raw(), space, &mut rect) };
    if failed(result) {
        return Err(result);
    }
    Ok((
        [rect.offset.x, rect.offset.y],
        [rect.extent.width, rect.extent.height],
    ))
}

// The semantic labels of a scene entity as the runtime's string, into `out`, which keeps its
// storage: the two-call idiom.
pub(crate) fn labels(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    space: xr::sys::Space,
    out: &mut Vec<u8>,
) -> Result<(), xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_scene
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let mut labels = xr::sys::SemanticLabelsFB {
        ty: xr::sys::SemanticLabelsFB::TYPE,
        next: ptr::null(),
        buffer_capacity_input: 0,
        buffer_count_output: 0,
        buffer: ptr::null_mut(),
    };
    // SAFETY: the session and the space are live, and `labels` is initialized with its type. A
    // capacity of zero asks for the size and reads no buffer.
    let asked = unsafe { (fp.get_space_semantic_labels)(session.as_raw(), space, &mut labels) };
    if failed(asked) {
        return Err(asked);
    }
    out.clear();
    out.resize(labels.buffer_count_output as usize, 0);
    labels.buffer_capacity_input = labels.buffer_count_output;
    labels.buffer = out.as_mut_ptr().cast::<c_char>();
    // SAFETY: `out` holds `buffer_capacity_input` bytes, which the runtime fills.
    let filled = unsafe { (fp.get_space_semantic_labels)(session.as_raw(), space, &mut labels) };
    if failed(filled) {
        out.clear();
        return Err(filled);
    }
    out.truncate(labels.buffer_count_output as usize);
    Ok(())
}

// The polygon of a scene entity in the X-Y plane of its space, into `out`, which keeps its
// storage: the two-call idiom.
pub(crate) fn boundary(
    instance: &xr::Instance,
    session: &xr::Session<super::super::graphics::Api>,
    space: xr::sys::Space,
    out: &mut Vec<xr::sys::Vector2f>,
) -> Result<(), xr::sys::Result> {
    let fp = instance
        .exts()
        .fb_scene
        .as_ref()
        .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
    let mut boundary = xr::sys::Boundary2DFB {
        ty: xr::sys::Boundary2DFB::TYPE,
        next: ptr::null(),
        vertex_capacity_input: 0,
        vertex_count_output: 0,
        vertices: ptr::null_mut(),
    };
    // SAFETY: the session and the space are live, and `boundary` is initialized with its type. A
    // capacity of zero asks for the count and reads no array.
    let asked = unsafe { (fp.get_space_boundary2_d)(session.as_raw(), space, &mut boundary) };
    if failed(asked) {
        return Err(asked);
    }
    out.clear();
    out.resize(
        boundary.vertex_count_output as usize,
        xr::sys::Vector2f { x: 0.0, y: 0.0 },
    );
    boundary.vertex_capacity_input = boundary.vertex_count_output;
    boundary.vertices = out.as_mut_ptr();
    // SAFETY: `out` holds `vertex_capacity_input` points, which the runtime fills.
    let filled = unsafe { (fp.get_space_boundary2_d)(session.as_raw(), space, &mut boundary) };
    if failed(filled) {
        out.clear();
        return Err(filled);
    }
    out.truncate(boundary.vertex_count_output as usize);
    Ok(())
}
