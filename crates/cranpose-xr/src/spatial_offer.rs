//! The anchor and plane extensions that a runtime lists and the app enables.

use crate::spatial_request::SpatialRequest;

// Which family of extensions holds the anchors and planes of a runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpatialProvider {
    // Meta's spatial entities: `XR_FB_spatial_entity` and its siblings
    Meta,
    // Android XR's trackables: `XR_ANDROID_trackables` and its siblings
    Android,
}

// The anchor and plane extensions of a set, as plain flags. The runtime's list of extensions fills
// one, and the extensions to enable make another.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpatialOffer {
    pub(crate) fb_spatial_entity: bool,
    pub(crate) fb_spatial_entity_query: bool,
    pub(crate) fb_spatial_entity_storage: bool,
    pub(crate) fb_scene: bool,
    pub(crate) android_trackables: bool,
    pub(crate) android_device_anchor_persistence: bool,
}

impl SpatialOffer {
    // The family that holds this runtime's anchors, or `None` where the list names none. A
    // runtime that lists both families uses Android XR's, which no Meta runtime lists.
    pub(crate) const fn provider(self) -> Option<SpatialProvider> {
        if self.android_trackables {
            Some(SpatialProvider::Android)
        } else if self.fb_spatial_entity {
            Some(SpatialProvider::Meta)
        } else {
            None
        }
    }

    // Whether the list holds an extension that creates anchors.
    pub(crate) const fn offers_anchors(self) -> bool {
        self.provider().is_some()
    }

    // Whether the list holds the extensions that keep anchors and load them back. Meta's storage
    // saves and erases, and its query lists what the storage holds, so persistence needs both.
    pub(crate) const fn offers_persistence(self) -> bool {
        match self.provider() {
            Some(SpatialProvider::Android) => self.android_device_anchor_persistence,
            Some(SpatialProvider::Meta) => {
                self.fb_spatial_entity_storage && self.fb_spatial_entity_query
            }
            None => false,
        }
    }

    // Whether the list holds the extensions that give the room's planes. Meta's scene lists the
    // planes through a query of spatial entities.
    pub(crate) const fn offers_planes(self) -> bool {
        match self.provider() {
            Some(SpatialProvider::Android) => true,
            Some(SpatialProvider::Meta) => self.fb_scene && self.fb_spatial_entity_query,
            None => false,
        }
    }

    // The part of this list that the request needs, which the instance enables and nothing else.
    // Meta's scene and storage need spatial entities and their query. Android XR's persistence
    // needs trackables. An app that asks for nothing gets nothing.
    pub(crate) fn enabled_for(self, request: SpatialRequest) -> Self {
        let Some(provider) = self.provider().filter(|_| request.asks_any()) else {
            return Self::default();
        };
        let anchors = request.anchors;
        let persistence = anchors && self.offers_persistence();
        let planes = request.planes && self.offers_planes();
        match provider {
            SpatialProvider::Android => Self {
                android_trackables: anchors || planes,
                android_device_anchor_persistence: persistence,
                ..Self::default()
            },
            SpatialProvider::Meta => Self {
                fb_spatial_entity: anchors || planes,
                fb_spatial_entity_query: persistence || planes,
                fb_spatial_entity_storage: persistence,
                fb_scene: planes,
                ..Self::default()
            },
        }
    }
}
