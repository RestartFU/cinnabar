//! Player capes: the cape geometry drawn with the player's own pose and a cape raster carried
//! in the skin layer payload.
use std::sync::{Arc, Mutex};

use assets::{CAPE_GEOMETRY_IDENTIFIER, RuntimeEntityAssets};
use client_world::{ActorRigSnapshot, PlayerProfile};
use protocol::{PlayerSkin, SkinRgba8};
use render::{ACTOR_LAYER_BODY, ActorRigRoute, ActorRigSubmission};
use render_model::{
    ActorRigGeometry, EntityRigId, MAX_RENDERED_PLAYERS, RenderBoneTransform, STANDARD_SKIN_BYTES,
    STANDARD_SKIN_SIDE, entity_geometry, equipment_rig_id, find_geometry_index,
    geometry_bone_names,
};

use super::actors::ActorPresentationBatch;

/// Render layer of a player's cape, below the extra texture layers.
pub const ACTOR_LAYER_CAPE: u8 = 24;

/// The cape geometry and its bone order, resolved once from the entity catalog.
#[derive(Clone)]
pub struct CapeRig {
    pub id: EntityRigId,
    pub geometry: ActorRigGeometry,
    bone_names: Vec<Box<str>>,
}

impl CapeRig {
    pub fn resolve(assets: &RuntimeEntityAssets) -> Option<Self> {
        let index = find_geometry_index(assets, CAPE_GEOMETRY_IDENTIFIER)?;
        let id = equipment_rig_id(index);
        Some(Self {
            id,
            geometry: entity_geometry(assets, index as usize, id).ok()?,
            bone_names: geometry_bone_names(assets, index as usize)?,
        })
    }
}

/// The cape rig, resolved on first use; an absent geometry leaves capes undrawn.
#[derive(Default)]
pub struct CapeState {
    resolved: bool,
    rig: Option<CapeRig>,
}

impl CapeState {
    pub fn rig(&mut self, assets: Option<&RuntimeEntityAssets>) -> Option<&CapeRig> {
        if !self.resolved
            && let Some(assets) = assets
        {
            self.resolved = true;
            self.rig = CapeRig::resolve(assets);
        }
        self.rig.as_ref()
    }
}

/// Resamples a cape raster into one standard skin layer; the cape geometry's texture
/// coordinates are normalised, so any cape size maps onto the layer exactly.
pub fn cape_layer(width: u32, height: u32, rgba8: &[u8]) -> Option<Arc<[u8]>> {
    let (width, height) = (width as usize, height as usize);
    let expected = width.checked_mul(height)?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba8.len() != expected {
        return None;
    }
    let side = STANDARD_SKIN_SIDE;
    let mut layer = Vec::with_capacity(STANDARD_SKIN_BYTES);
    for y in 0..side {
        let source_y = y * height / side;
        for x in 0..side {
            let source_x = x * width / side;
            let offset = (source_y * width + source_x) * 4;
            layer.extend_from_slice(&rgba8[offset..offset + 4]);
        }
    }
    Some(layer.into())
}

/// Reuses completed body poses; the cape mesh already carries its face-orienting bind rotation.
fn cape_pose(
    cape: &CapeRig,
    body_names: &[Box<str>],
    body: &[RenderBoneTransform],
) -> Arc<[RenderBoneTransform]> {
    cape.bone_names
        .iter()
        .map(|name| {
            let pose = body_names
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(name))
                .and_then(|index| body.get(index).copied());
            match pose {
                Some(pose) => pose,
                None => RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0; 4],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                },
            }
        })
        .collect()
}

/// The cape layer, resampled and hashed once per source raster; entries hold their source, so a
/// matched pointer is never a reused allocation.
fn cape_of(profile: &PlayerProfile) -> Option<SkinRgba8> {
    type Entry = (Arc<[u8]>, u32, u32, Option<SkinRgba8>);
    static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    let PlayerSkin::Standard(skin) = &profile.skin else {
        return None;
    };
    let cape = skin.cape.as_ref()?;
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((.., layer)) = cache.iter().find(|(source, width, height, _)| {
        Arc::ptr_eq(source, &cape.rgba8) && *width == cape.width && *height == cape.height
    }) {
        return layer.clone();
    }
    let layer = cape_layer(cape.width, cape.height, &cape.rgba8).map(SkinRgba8::new);
    if cache.len() == MAX_RENDERED_PLAYERS {
        cache.remove(0);
    }
    cache.push((
        Arc::clone(&cape.rgba8),
        cape.width,
        cape.height,
        layer.clone(),
    ));
    layer
}

/// Appends a cape instance for every drawn player body whose skin carries one; capes past the
/// skin layer budget are dropped rather than invalidating the frame.
pub fn apply_capes<'a>(
    batch: &mut ActorPresentationBatch,
    cape: &CapeRig,
    rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
    profile_of: impl Fn(u64) -> Option<&'a PlayerProfile>,
) {
    let mut capes: Vec<(SkinRgba8, usize)> = Vec::new();
    let mut extras = Vec::new();
    for body in &batch.submissions {
        let identity = body.input.identity;
        if identity.layer != ACTOR_LAYER_BODY
            || body.route == ActorRigRoute::NoDraw
            || batch.artwork.contains_key(&identity)
        {
            continue;
        }
        let Some(cape_pixels) = profile_of(identity.runtime_id).and_then(cape_of) else {
            continue;
        };
        let Some(rig) = rig_of(identity.runtime_id) else {
            continue;
        };
        let layer = match capes.iter().find(|(known, _)| *known == cape_pixels) {
            Some((_, layer)) => *layer,
            None if batch.skin_layers.len() < MAX_RENDERED_PLAYERS => {
                batch.skin_layers.push(cape_pixels.clone());
                capes.push((cape_pixels, batch.skin_layers.len() - 1));
                batch.skin_layers.len() - 1
            }
            None => continue,
        };
        let mut submission: ActorRigSubmission = body.clone();
        submission.input.identity.layer = ACTOR_LAYER_CAPE;
        submission.input.rig = cape.id;
        submission.input.previous_bones =
            cape_pose(cape, rig.bone_names, &body.input.previous_bones);
        submission.input.current_bones = cape_pose(cape, rig.bone_names, &body.input.current_bones);
        submission.texture_layer = layer as u32;
        submission.tint = 0;
        submission.overlay_rgba8 = 0;
        extras.push(submission);
    }
    batch.submissions.extend(extras);
}

#[cfg(test)]
mod tests {
    use super::cape_layer;

    #[test]
    fn cape_rasters_resample_onto_one_skin_layer() {
        let mut cape = vec![0u8; 64 * 32 * 4];
        cape[..4].copy_from_slice(&[9, 8, 7, 255]);
        let layer = cape_layer(64, 32, &cape).unwrap();
        assert_eq!(layer.len(), render_model::STANDARD_SKIN_BYTES);
        assert_eq!(&layer[..4], &[9, 8, 7, 255]);
        let row = render_model::STANDARD_SKIN_SIDE * 4;
        let rows_per_source = render_model::STANDARD_SKIN_SIDE / 32;
        assert_eq!(&layer[row..row + 4], &[9, 8, 7, 255]);
        let next = rows_per_source * row;
        assert_eq!(&layer[next..next + 4], &[0, 0, 0, 0]);
        assert!(cape_layer(64, 32, &cape[1..]).is_none());
    }
    #[test]
    fn review_render_cape_rejects_overflowing_dimensions() {
        assert!(cape_layer(1 << 31, 1 << 31, &[]).is_none());
        assert!(cape_layer(u32::MAX, u32::MAX, &[]).is_none());
    }

    #[test]
    fn cape_outer_face_keeps_the_front_raster_under_parent_tilt() {
        use super::{CapeRig, cape_pose};
        use bevy::math::{Quat, Vec3};
        use render_model::{EntityRigId, RenderBoneTransform};
        let source = serde_json::json!({
            "format_version":"1.12.0",
            "minecraft:geometry":[{
                "description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":32},
                "bones":[{"name":"cape","pivot":[0,24,3],"bind_pose_rotation":[0,180,0],
                    "cubes":[{"origin":[-5,8,3],"size":[10,16,1],"uv":[0,0]}]}]
            }]
        });
        let model = assets::parse_skin_geometry(
            r#"{"geometry":{"default":"geometry.fixture"}}"#,
            &source.to_string(),
        )
        .unwrap()
        .unwrap();
        let geometry = render_model::skin_geometry(&model, EntityRigId(1)).unwrap();
        let cape = CapeRig {
            id: geometry.id,
            geometry,
            bone_names: vec!["cape".into()],
        };
        for tilt in [0.0, 0.45] {
            let parent = Quat::from_rotation_x(tilt);
            let body = [RenderBoneTransform {
                rotation: parent.to_array(),
                translation_scale: [0.0, 0.0, 0.0, 1.0],
                axis_scale: render_model::UNIT_AXIS_SCALE,
            }];
            let poses = cape_pose(&cape, &["cape".into()], &body);
            let rotation = Quat::from_array(poses[0].rotation);
            let outer = parent * Vec3::Z;
            let outward = cape
                .geometry
                .vertices
                .iter()
                .filter(|vertex| (rotation * Vec3::from_array(vertex.normal)).dot(outer) > 0.99)
                .collect::<Vec<_>>();
            assert_eq!(outward.len(), 6);
            assert!(
                outward.iter().all(|vertex| vertex.uv[0] <= 11.0 / 64.0),
                "outward face must sample the front strip, not the inside strip"
            );
        }
    }
}
