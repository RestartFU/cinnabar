//! Presentation adapter for read-only loaded-block highlights.
use crate::{
    app::ClientFrameSet, camera::FlyCamera, menu::MenuRuntime, runtime::world::ClientWorld,
};
use bevy::prelude::*;
use client_ui::ui_runtime::UiRuntime;
use render::ModRenderScene;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct BlockHighlights;

pub(super) fn configure(app: &mut App) {
    if std::env::var(super::BLOCK_HIGHLIGHTS_ENV).is_ok_and(|value| value == "1") {
        let _ = registry();
    }
    app.add_systems(
        Update,
        publish
            .in_set(BlockHighlights)
            .after(ClientFrameSet::Camera)
            .before(ClientFrameSet::UiPreparation),
    );
}

#[derive(Default)]
struct Discovery {
    session: u64,
    assets: usize,
    mode: Option<assets::NetworkIdMode>,
    identifiers: Vec<String>,
    ids: Vec<u32>,
    scan: world::BlockHighlightScan,
}

fn registry() -> Option<&'static [assets::RegistryRecord]> {
    static RECORDS: OnceLock<Box<[assets::RegistryRecord]>> = OnceLock::new();
    static STARTED: AtomicBool = AtomicBool::new(false);
    if !STARTED.swap(true, Ordering::AcqRel)
        && std::thread::Builder::new()
            .name("block-highlight-identities".into())
            .spawn(|| {
                let records = assets::read_registry_for_protocol(
                    assets::pinned_block_registry_bytes(),
                    assets::active_content_registry_protocol(),
                )
                .unwrap_or_default();
                let _ = RECORDS.set(records);
            })
            .is_err()
    {
        STARTED.store(false, Ordering::Release);
    }
    RECORDS.get().map(|records| records.as_ref())
}

#[allow(clippy::too_many_arguments)]
fn publish(
    extension: Option<Res<super::ModRuntime>>,
    world: Option<Res<ClientWorld>>,
    menu: Option<Res<MenuRuntime>>,
    ui: Option<Res<UiRuntime>>,
    player: Option<Res<crate::player_runtime::PlayerRuntime>>,
    cameras: Query<&Transform, With<FlyCamera>>,
    mut discovery: Local<Discovery>,
    mut scene: ResMut<ModRenderScene>,
) {
    let visible = !menu.as_deref().is_some_and(MenuRuntime::is_visible)
        && ui
            .as_deref()
            .zip(player.as_deref())
            .is_some_and(|(ui, player)| !ui.ui_focused(player));
    let stream = world
        .as_deref()
        .filter(|world| world.fatal_error.is_none())
        .and_then(|world| world.stream.as_ref());
    let spec = extension
        .as_deref()
        .filter(|runtime| !runtime.suspended)
        .and_then(|runtime| {
            (0..runtime.host_count()).find_map(|index| runtime.host(index).block_highlights())
        });
    let camera = cameras.single().ok();
    let (Some(stream), Some(spec), Some(camera)) = (stream, spec, camera) else {
        discovery.scan.clear();
        discovery.session = 0;
        scene.set_block_highlights(&[], [0.0; 4]);
        return;
    };
    let session = stream.authority().actor_session_id();
    if discovery.session != session {
        discovery.scan.clear();
        discovery.session = session;
    }
    let asset_identity = std::sync::Arc::as_ptr(stream.runtime_assets()) as usize;
    let mode = stream.network_id_mode();
    if discovery.assets != asset_identity
        || discovery.mode != Some(mode)
        || discovery.identifiers != spec.identifiers
    {
        let Some(records) = registry() else {
            scene.set_block_highlights(&[], spec.color);
            return;
        };
        discovery.assets = asset_identity;
        discovery.mode = Some(mode);
        discovery.identifiers.clone_from(&spec.identifiers);
        discovery.ids.clear();
        for record in records.iter().filter(|record| {
            spec.identifiers
                .iter()
                .any(|name| name.as_str() == record.name.as_ref())
        }) {
            let id = match mode {
                assets::NetworkIdMode::Sequential => record.sequential_id,
                assets::NetworkIdMode::Hashed => record.network_hash,
            };
            if stream.runtime_assets().is_known(mode, id) {
                discovery.ids.push(id);
            }
        }
        discovery.scan.clear();
    }
    let Discovery { scan, ids, .. } = &mut *discovery;
    let positions = scan.update(
        stream.collision_store(),
        stream.current_dimension(),
        camera.translation.to_array(),
        spec.range,
        ids,
        render::MAX_BLOCK_HIGHLIGHTS,
    );
    scene.set_block_highlights(if visible { positions } else { &[] }, spec.color);
}
