//! Web-Mercator math and coordinate parsing.

/// Fractional Web-Mercator tile coordinates of a lat/lon at zoom z.
pub fn lonlat_to_tile(lat: f64, lon: f64, z: u32) -> (f64, f64) {
    let n = 2f64.powi(z as i32);
    let x = (lon + 180.0) / 360.0 * n;
    let lat_r = lat.to_radians();
    let y = (1.0 - (lat_r.tan() + 1.0 / lat_r.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// Ground resolution of a 256px Web-Mercator tile pixel at a latitude.
/// (2 * pi * 6378137 / 256 = 156543.03392804097)
pub fn meters_per_pixel(lat: f64, z: u32) -> f64 {
    156_543.033_928_040_97 * lat.to_radians().cos() / 2f64.powi(z as i32)
}

/// Parses "lat, lon" (comma and/or whitespace separated), e.g. pasted from Google Maps.
pub fn parse_latlon(s: &str) -> Option<(f64, f64)> {
    let v: Vec<f64> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if v.len() == 2 {
        Some((v[0], v[1]))
    } else {
        None
    }
}
