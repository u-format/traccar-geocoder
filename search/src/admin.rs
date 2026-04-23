use crate::binary::{BinaryIndex, AdminPolygon, NodeCoord, INTERIOR_FLAG, ID_MASK, NO_DATA};
use s2::cellid::CellID;
use s2::latlng::LatLng;
use std::collections::HashMap;

const ADMIN_CELL_LEVEL: u64 = 10;
const ADMIN_CACHE_LEVEL: u64 = 14;  
const PLACE_CELL_LEVEL: u64 = 12;

// max distance to snap a place node as suburb context for a street
const SUBURB_SNAP_DIST_SQ: f64 = (500.0 / 111_320.0) * (500.0 / 111_320.0);

#[derive(Clone, Default)]
pub struct AdminContext {
    pub suburb:  Option<String>,
    pub city:    Option<String>,
    pub county:  Option<String>,
    pub state:   Option<String>,
    pub country: Option<String>,
}

pub struct AdminCache {
    cache: HashMap<u64, AdminContext>,
}

impl AdminContext {
    pub fn context_tokens(&self) -> String {
        [&self.suburb, &self.city, &self.county, &self.state, &self.country]
            .iter()
            .filter_map(|o| o.as_deref())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn display_suffix(&self) -> String {
        [&self.suburb, &self.city, &self.county, &self.state, &self.country]
            .iter()
            .filter_map(|o| o.as_deref())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl AdminCache {
    pub fn new() -> Self {
        AdminCache { cache: HashMap::new() }
    }

    pub fn get(&mut self, lat: f64, lng: f64, idx: &BinaryIndex) -> AdminContext {
        let cell = cell_id_at_level(lat, lng, ADMIN_CACHE_LEVEL);
        self.cache
            .entry(cell)
            .or_insert_with(|| find_admin(lat, lng, idx))
            .clone()
    }
}

pub fn cell_id_at_level(lat: f64, lng: f64, level: u64) -> u64 {
    CellID::from(LatLng::from_degrees(lat, lng)).parent(level).0
}

fn cell_neighbors(cell_id: u64, level: u64) -> Vec<u64> {
    CellID(cell_id).all_neighbors(level).into_iter().map(|c| c.0).collect()
}

fn point_in_polygon(lat: f32, lng: f32, verts: &[NodeCoord]) -> bool {
    let n = verts.len();
    if n < 3 { return false; }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let xi = verts[i].lng; let yi = verts[i].lat;
        let xj = verts[j].lng; let yj = verts[j].lat;
        if ((yi > lat) != (yj > lat)) && lng < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}


pub fn find_admin(lat: f64, lng: f64, idx: &BinaryIndex) -> AdminContext {
    let cell = cell_id_at_level(lat, lng, ADMIN_CELL_LEVEL);
    let neighbors = cell_neighbors(cell, ADMIN_CELL_LEVEL);
    let all_polygons = idx.polygons();
    let all_vertices = idx.vertices();
    let mut best: [Option<(f32, &AdminPolygon)>; 12] = [None; 12];

    for c in std::iter::once(cell).chain(neighbors.into_iter()) {
        let offset = idx.lookup_admin_cell(c);
        idx.for_each_admin_entry(offset, |id| {
            let is_interior = (id & INTERIOR_FLAG) != 0;
            let poly = &all_polygons[(id & ID_MASK) as usize];
            let level = poly.admin_level as usize;
            if level >= 12 { return; }
            if let Some((best_area, _)) = best[level] {
                if poly.area >= best_area { return; }
            }
            let voff = poly.vertex_offset as usize;
            let vcnt = poly.vertex_count as usize;
            if is_interior || point_in_polygon(lat as f32, lng as f32, &all_vertices[voff..voff + vcnt]) {
                best[level] = Some((poly.area, poly));
            }
        });
    }

    let mut ctx = AdminContext::default();
    for level in 0..12usize {
        if let Some((_, poly)) = best[level] {
            let name = idx.get_string(poly.name_id).to_string();
            match poly.admin_level {
                2      => ctx.country = Some(name),
                4      => ctx.state   = Some(name),
                6      => ctx.county  = Some(name),
                8      => ctx.city    = Some(name),
                9 | 10 => ctx.suburb  = Some(name),
                _ => {}
            }
        }
    }

    if ctx.suburb.is_none() {
        ctx.suburb = nearest_place_node(lat, lng, idx);
    }

    ctx
}

fn nearest_place_node(lat: f64, lng: f64, idx: &BinaryIndex) -> Option<String> {
    let cell = cell_id_at_level(lat, lng, PLACE_CELL_LEVEL);
    let neighbors = cell_neighbors(cell, PLACE_CELL_LEVEL);
    let feats = idx.place_features();
    let cos_lat = lat.to_radians().cos();
    let mut best_dist = SUBURB_SNAP_DIST_SQ;
    let mut best_name: Option<String> = None;

    for c in std::iter::once(cell).chain(neighbors.into_iter()) {
        let offset = idx.lookup_place_cell(c);
        idx.for_each_place_entry(offset, |id| {
            let feat = &feats[id as usize];
            if feat.feature_type != 1 { return; }
            if feat.name_id == NO_DATA { return; }
            let dlat = (feat.lat as f64 - lat).to_radians();
            let dlng = (feat.lng as f64 - lng).to_radians();
            let dist = dlat * dlat + (dlng * cos_lat) * (dlng * cos_lat);
            if dist < best_dist {
                best_dist = dist;
                best_name = Some(idx.get_string(feat.name_id).to_string());
            }
        });
    }
    best_name
}

