//! Optional local typography for the explicitly granted personal controls panel.

use std::{fs::File, io::Read, path::Path, sync::Arc};

use assets::{CellGlyph, FontTexturePage, RuntimeFontCatalog, encode_font_catalog, pack_cells};
use sha2::{Digest, Sha256};

const FONT_ENV: &str = "CINNABAR_MOD_FONT";
const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const ATLAS_SIDE: u32 = 256;
const FONT_EM: f32 = 18.0;

/// Reads and rasterizes once before UI texture ownership is initialized.
pub(crate) fn with_optional_font(base: Arc<RuntimeFontCatalog>) -> Arc<RuntimeFontCatalog> {
    if std::env::var_os(super::COMPONENT_ENV).is_none()
        || !std::env::var(super::CONTROLS_ENV).is_ok_and(|value| value == "1")
    {
        return base;
    }
    let Some(path) = std::env::var_os(FONT_ENV) else {
        return base;
    };
    match load(Path::new(&path)).and_then(|font| {
        base.with_named_font(ui::mod_panel::FONT_NAME, &font)
            .map_err(|error| error.to_string())
    }) {
        Ok(font) => Arc::new(font),
        Err(error) => {
            eprintln!("Optional personal-panel font unavailable: {error}");
            base
        }
    }
}

fn load(path: &Path) -> Result<RuntimeFontCatalog, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err("font source exceeds local byte limit".into());
    }
    rasterize(&bytes)
}

fn rasterize(bytes: &[u8]) -> Result<RuntimeFontCatalog, String> {
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(str::to_owned)?;
    let mut cells = Vec::new();
    let characters = (0x20..=0x7e).filter_map(char::from_u32).chain([
        '\u{b7}', '\u{d7}', '\u{2014}', '\u{2022}', '\u{2039}', '\u{203a}', '\u{2212}', '\u{fffd}',
    ]);
    for codepoint in characters {
        let source = if font.lookup_glyph_index(codepoint) == 0 {
            '?'
        } else {
            codepoint
        };
        let metrics = font.metrics(source, FONT_EM);
        if metrics.width > 64 || metrics.height > 64 || !metrics.advance_width.is_finite() {
            return Err("font glyph dimensions exceed local bounds".into());
        }
        let (metrics, alpha) = font.rasterize(source, FONT_EM);
        // Empty glyphs keep their advance and get a transparent texel with valid carrier UVs.
        let empty = metrics.width == 0 || metrics.height == 0;
        let size = if empty {
            [1, 1]
        } else {
            [metrics.width as u32, metrics.height as u32]
        };
        cells.push(CellGlyph {
            codepoint,
            size,
            rgba8: if empty {
                vec![255, 255, 255, 0].into()
            } else {
                alpha
                    .into_iter()
                    .flat_map(|alpha| [255, 255, 255, alpha])
                    .collect()
            },
            bearing: [
                metrics.xmin as i16,
                -(metrics.ymin + metrics.height as i32) as i16,
            ],
            advance_64: (metrics.advance_width * 64.0)
                .round()
                .clamp(0.0, i16::MAX as f32) as i16,
            draw_size_64: size.map(|value| value * 64),
        });
    }
    let atlas = pack_cells(&cells, 0, ATLAS_SIDE, 1);
    if atlas.glyphs.len() != cells.len() || atlas.pages.len() != 1 {
        return Err("font exceeds local atlas bounds".into());
    }
    let source_hash: [u8; 32] = Sha256::digest(bytes).into();
    let pages = atlas
        .pages
        .into_iter()
        .map(|pixels| FontTexturePage {
            source_path: "font/personal-panel.png".into(),
            source_bytes: bytes.len() as u32,
            source_sha256: source_hash,
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: ATLAS_SIDE,
            height: ATLAS_SIDE,
            rgba8: pixels,
        })
        .collect::<Vec<_>>();
    let glyphs = atlas
        .glyphs
        .into_iter()
        .map(|glyph| glyph.metrics)
        .collect::<Vec<_>>();
    let encoded =
        encode_font_catalog(source_hash, &glyphs, &pages).map_err(|error| error.to_string())?;
    RuntimeFontCatalog::decode(&encoded, source_hash).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_oversized_optional_fonts_fail_before_attachment() {
        assert!(rasterize(b"not a font").is_err());
        let path =
            std::env::temp_dir().join(format!("cinnabar-optional-font-{}.ttf", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len((MAX_SOURCE_BYTES + 1) as u64).unwrap();
        assert!(load(&path).unwrap_err().contains("byte limit"));
        drop(file);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn supplied_font_fixture_is_antialiased_and_fits_one_private_page() {
        let Some(path) = std::env::var_os(FONT_ENV) else {
            eprintln!(
                "skipping supplied_font_fixture_is_antialiased_and_fits_one_private_page: fixture unavailable; set CINNABAR_MOD_FONT to a local outline font"
            );
            return;
        };
        let font = load(Path::new(&path)).unwrap();
        assert_eq!(font.pages().len(), 1);
        assert_eq!(
            [font.pages()[0].width, font.pages()[0].height],
            [ATLAS_SIDE; 2]
        );
        assert!(font.glyph('A').is_some_and(|glyph| glyph.advance_64 > 0));
        let space = font.glyph(' ').unwrap();
        assert!(space.advance_64 > 0);
        assert_eq!(
            [space.uv[2] - space.uv[0], space.uv[3] - space.uv[1]],
            [1; 2]
        );
        let texel = (usize::from(space.uv[1]) * ATLAS_SIDE as usize + usize::from(space.uv[0])) * 4;
        assert_eq!(font.pages()[0].rgba8[texel + 3], 0);
        let attached = font
            .with_named_font(ui::mod_panel::FONT_NAME, &font)
            .unwrap();
        assert_eq!(attached.glyph('A').unwrap().page, 0);
        assert_eq!(
            attached
                .font_named(ui::mod_panel::FONT_NAME)
                .glyph('A')
                .unwrap()
                .page,
            1
        );
        assert!(
            font.pages()[0]
                .rgba8
                .chunks_exact(4)
                .any(|pixel| (1..255).contains(&pixel[3]))
        );
    }
}
