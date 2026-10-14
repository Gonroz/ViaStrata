//! Satellite tile sources: download, stitch, and crop around a coordinate.

use std::io::Read;
use std::time::Duration;

use crate::geo::{lonlat_to_tile, meters_per_pixel};

const TILE: u32 = 256;

#[derive(PartialEq, Clone, Copy)]
pub enum Source {
    Usgs,
    Esri,
    /// Esri's "Clarity" imagery layer. Same tiling scheme as `Esri` (so the
    /// rest of this module needs no special-casing) but in many urban areas
    /// it serves a different, often sharper vintage of the same base imagery.
    /// It has the same z19 ceiling as the standard layer, but at z19 the
    /// imagery itself is often crisper.
    EsriClarity,
}

impl Source {
    /// Highest zoom level the source is willing to serve.
    ///
    /// The Esri endpoints nominally top out at z19. We allow z20 anyway:
    /// the server typically *accepts* z20 and returns upscaled z19 tiles.
    /// That is not more detail, but it is smoother than the app's own GPU
    /// magnification with no mipmaps, and it is useful when you want a
    /// higher-resolution crop without changing anything else. If a request
    /// at z20 fails, back off to z19.
    ///
    /// USGS tops out at z16 — trying to go past it just returns the same
    /// tiles upsampled, so we clamp there.
    pub fn max_zoom(self) -> u32 {
        match self {
            Source::Usgs => 16,
            Source::Esri => 20,
            Source::EsriClarity => 19,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Usgs => "USGS (US only)",
            Source::Esri => "Esri",
            Source::EsriClarity => "Esri Clarity",
        }
    }

    pub fn attribution(self) -> &'static str {
        match self {
            Source::Usgs => "USDA, USGS The National Map: Orthoimagery",
            Source::Esri | Source::EsriClarity => {
                "Source: Esri, Maxar, Earthstar Geographics, and the GIS User Community"
            }
        }
    }

    fn tile_url(self, z: u32, x: i64, y: i64, token: &str) -> String {
        // The three Esri-style endpoints differ only in host + path; the
        // tile URL shape (`/tile/{z}/{y}/{x}?token=...`) is identical.
        let base = match self {
            Source::Usgs => {
                return format!(
                    "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}"
                );
            }
            Source::Esri => {
                "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer"
            }
            Source::EsriClarity => {
                "https://clarity.maptiles.arcgis.com/arcgis/rest/services/World_Imagery/MapServer"
            }
        };

        let mut u = format!("{base}/tile/{z}/{y}/{x}");
        if !token.is_empty() {
            u.push_str(&format!("?token={token}"));
        }
        u
    }
}

pub struct Fetched {
    pub img: image::RgbaImage,
    pub px_per_m: f32,
    pub m_per_px: f64,
    pub z: u32,
}

type Row = Result<Vec<(u32, u32, image::RgbaImage)>, String>;

/// Downloads a (2r+1)x(2r+1) block of tiles around the coordinate, stitches
/// them, and crops a square centered exactly on the coordinate.
///
/// Note the size relationship: the mosaic is `(2r+1)` tiles across but the
/// crop is only `2r` tiles across. The extra tile of margin on each side
/// exists so the crop can be positioned on a sub-tile boundary without
/// running off the mosaic. So radius 2 -> a 1024x1024 image, radius 3 ->
/// 1536x1536.
pub fn fetch_imagery(
    src: Source,
    token: &str,
    lat: f64,
    lon: f64,
    z: u32,
    r: u32,
) -> Result<Fetched, String> {
    let n: i64 = 1i64 << z;
    let (fx, fy) = lonlat_to_tile(lat, lon, z);
    let (cx, cy) = (fx.floor() as i64, fy.floor() as i64);
    let r = r as i64;
    let side = (2 * r + 1) as u32;

    let agent = ureq::AgentBuilder::new()
        .user_agent("via_strata/0.1 (personal desktop tool)")
        .timeout(Duration::from_secs(20))
        .build();

    // One worker thread per tile row.
    let rows: Vec<Row> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..side)
            .map(|row| {
                let agent = agent.clone();
                s.spawn(move || -> Row {
                    let ty = cy - r + row as i64;
                    if ty < 0 || ty >= n {
                        return Err("Location is too close to the poles".into());
                    }
                    let mut out = Vec::new();
                    for col in 0..side {
                        let tx = (cx - r + col as i64).rem_euclid(n);
                        let url = src.tile_url(z, tx, ty, token);
                        let resp = agent
                            .get(&url)
                            .call()
                            .map_err(|e| format!("tile {z}/{ty}/{tx}: {e}"))?;
                        let mut buf = Vec::new();
                        resp.into_reader()
                            .take(10_000_000)
                            .read_to_end(&mut buf)
                            .map_err(|e| format!("tile {z}/{ty}/{tx}: {e}"))?;
                        let img = image::load_from_memory(&buf)
                            .map_err(|e| format!("bad tile {z}/{ty}/{tx}: {e}"))?
                            .to_rgba8();
                        if img.width() != TILE || img.height() != TILE {
                            return Err(format!(
                                "unexpected tile size {}x{}",
                                img.width(),
                                img.height()
                            ));
                        }
                        out.push((col, row, img));
                    }
                    Ok(out)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err("worker thread panicked".to_string()))
            })
            .collect()
    });

    let mut big = image::RgbaImage::new(side * TILE, side * TILE);
    for row in rows {
        for (col, row, tile) in row? {
            image::imageops::replace(&mut big, &tile, (col * TILE) as i64, (row * TILE) as i64);
        }
    }

    // Pixel position of the coordinate inside the stitched mosaic.
    let ox = (cx - r) as f64 * TILE as f64;
    let oy = (cy - r) as f64 * TILE as f64;
    let px = fx * TILE as f64 - ox;
    let py = fy * TILE as f64 - oy;
    let half = (r as u32 * TILE) as f64;
    let w = 2 * r as u32 * TILE;
    let x0 = ((px - half).round().max(0.0) as u32).min(big.width() - w);
    let y0 = ((py - half).round().max(0.0) as u32).min(big.height() - w);
    let cropped = image::imageops::crop_imm(&big, x0, y0, w, w).to_image();

    let mpp = meters_per_pixel(lat, z);
    Ok(Fetched {
        img: cropped,
        px_per_m: (1.0 / mpp) as f32,
        m_per_px: mpp,
        z,
    })
}