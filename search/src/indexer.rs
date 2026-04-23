use std::collections::HashMap;
use tantivy::schema::*;
use tantivy::{Index, TantivyDocument};

use crate::admin::AdminCache;
use crate::binary::{BinaryIndex, NO_DATA};
use crate::tokenizer::normalize_turkish;

// --- Schema ---

pub struct SearchSchema {
    pub schema:       Schema,
    pub f_name:       Field,
    pub f_name_norm:  Field,
    pub f_context:    Field,
    pub f_display:    Field,
    pub f_lat:        Field,
    pub f_lon:        Field,
    pub f_kind:       Field,
    pub f_importance: Field,
}

pub fn build_schema() -> SearchSchema {
    let mut b = Schema::builder();

    let text_stored = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();

    let text_only = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("default")
                .set_index_option(IndexRecordOption::WithFreqsAndPositions),
        );

    let f_name      = b.add_text_field("name",      text_stored);
    let f_name_norm = b.add_text_field("name_norm", text_only.clone());
    let f_context   = b.add_text_field("context",   text_only);
    let f_display   = b.add_text_field("display",   STRING | STORED);
    let f_lat       = b.add_f64_field("lat",        STORED | FAST);
    let f_lon       = b.add_f64_field("lon",        STORED | FAST);
    let f_kind      = b.add_text_field("kind",      STRING | STORED);
    let f_importance = b.add_f64_field("importance", FAST);

    SearchSchema {
        schema: b.build(),
        f_name, f_name_norm, f_context, f_display,
        f_lat, f_lon, f_kind, f_importance,
    }
}

// --- Document helpers ---

fn make_doc(ss: &SearchSchema, name: &str, display: &str, lat: f64, lon: f64,
            kind: &str, context: &str, importance: f64) -> TantivyDocument {
    let name_norm = normalize_turkish(name);
    let mut doc = TantivyDocument::default();
    doc.add_text(ss.f_name,      &name_norm);
    doc.add_text(ss.f_name_norm, &name_norm);
    doc.add_text(ss.f_context,   &normalize_turkish(context));
    doc.add_text(ss.f_display,   display);
    doc.add_f64(ss.f_lat,        lat);
    doc.add_f64(ss.f_lon,        lon);
    doc.add_text(ss.f_kind,      kind);
    doc.add_f64(ss.f_importance, importance);
    doc
}

// --- Index building ---

pub fn build_index(idx: &BinaryIndex) -> Result<Index, tantivy::TantivyError> {
    let ss = build_schema();
    let index = Index::create_in_ram(ss.schema.clone());
    let mut writer = index.writer::<TantivyDocument>(64_000_000)?;
    let mut cache = AdminCache::new();

    eprintln!("[search] Indexing admin polygons...");
    index_admin_polygons(&ss, &mut writer, idx, &mut cache);

    eprintln!("[search] Indexing place features...");
    index_place_features(&ss, &mut writer, idx, &mut cache);

    eprintln!("[search] Indexing streets...");
    index_streets(&ss, &mut writer, idx, &mut cache);

    writer.commit()?;
    Ok(index)
}

fn index_admin_polygons(
    ss: &SearchSchema,
    writer: &mut tantivy::IndexWriter<TantivyDocument>,
    idx: &BinaryIndex,
    cache: &mut AdminCache,
) {
    let polygons = idx.polygons();
    let vertices = idx.vertices();

    let mut seen: HashMap<(u32, u8, (i16, i16)), f32> = HashMap::new();

    let mut count = 0usize;
    for poly in polygons {
        let level = poly.admin_level;
        if !matches!(level, 4 | 6 | 8 | 9 | 10) { continue; }
        if poly.name_id == NO_DATA { continue; }

        let voff = poly.vertex_offset as usize;
        let vcnt = std::cmp::min(poly.vertex_count as usize, 64);
        let (lat, lon) = polygon_centroid(&vertices[voff..voff + vcnt]);

        let cell = (lat as i16, lon as i16);
        let key = (poly.name_id, level, cell);
        if let Some(&prev_area) = seen.get(&key) {
            if poly.area >= prev_area { continue; }
        }
        seen.insert(key, poly.area);

        let name = idx.get_string(poly.name_id);
        if name.is_empty() { continue; }

        let kind = match level {
            4      => "state",
            6      => "county",
            8      => "city",
            9 | 10 => "suburb",
            _      => continue,
        };
        let importance = match level {
            8      => 4.0,
            6      => 3.0,
            4      => 2.5,
            9 | 10 => 1.5,
            _      => 1.0,
        };

        let ctx = cache.get(lat, lon, idx);
        let suffix = admin_display_suffix(level, &ctx);
        let display = if suffix.is_empty() { name.to_string() } else { format!("{}, {}", name, suffix) };
        let context = format!("{} {}", name, ctx.context_tokens());

        let _ = writer.add_document(make_doc(ss, name, &display, lat, lon, kind, &context, importance));
        count += 1;
    }
    eprintln!("[search]   {} admin polygon docs", count);
}

fn index_place_features(
    ss: &SearchSchema,
    writer: &mut tantivy::IndexWriter<TantivyDocument>,
    idx: &BinaryIndex,
    cache: &mut AdminCache,
) {
    let mut count = 0usize;
    for feat in idx.place_features() {
        if feat.feature_type == 0 { continue; }
        if feat.name_id == NO_DATA { continue; }
        let name = idx.get_string(feat.name_id);
        if name.is_empty() { continue; }

        let lat = feat.lat as f64;
        let lon = feat.lng as f64;
        let ctx = cache.get(lat, lon, idx);
        let suffix = ctx.display_suffix();
        let display = if suffix.is_empty() { name.to_string() } else { format!("{}, {}", name, suffix) };

        let (kind, importance) = match feat.feature_type {
            1 => ("suburb", 1.2),
            _ => ("poi",    1.0),
        };

        let _ = writer.add_document(make_doc(ss, name, &display, lat, lon, kind,
                                             &ctx.context_tokens(), importance));
        count += 1;
    }
    eprintln!("[search]   {} place/poi feature docs", count);
}

fn index_streets(
    ss: &SearchSchema,
    writer: &mut tantivy::IndexWriter<TantivyDocument>,
    idx: &BinaryIndex,
    cache: &mut AdminCache,
) {
    use crate::admin::cell_id_at_level;

    let ways = idx.ways();
    let nodes = idx.nodes();

    struct Acc { lat_sum: f64, lon_sum: f64, node_count: usize }
    let mut buckets: HashMap<(u32, u64), Acc> = HashMap::new();

    for way in ways {
        if way.name_id == NO_DATA { continue; }
        let off = way.node_offset as usize;
        let cnt = way.node_count as usize;
        if off + cnt > nodes.len() { continue; }
        let (lat, lon) = way_centroid(&nodes[off..off + cnt]);
        let city_cell = cell_id_at_level(lat, lon, 10);
        let acc = buckets.entry((way.name_id, city_cell)).or_insert(Acc { lat_sum: 0.0, lon_sum: 0.0, node_count: 0 });
        acc.lat_sum    += lat * cnt as f64;
        acc.lon_sum    += lon * cnt as f64;
        acc.node_count += cnt;
    }

    let mut count = 0usize;
    for ((name_id, _), acc) in &buckets {
        let lat = acc.lat_sum / acc.node_count as f64;
        let lon = acc.lon_sum / acc.node_count as f64;
        let name = idx.get_string(*name_id);
        if name.is_empty() { continue; }
        let ctx = cache.get(lat, lon, idx);
        let suffix = ctx.display_suffix();
        let display = if suffix.is_empty() { name.to_string() } else { format!("{}, {}", name, suffix) };
        let _ = writer.add_document(make_doc(ss, name, &display, lat, lon, "street",
                                             &ctx.context_tokens(), 1.0));
        count += 1;
    }
    eprintln!("[search]   {} street docs", count);
}

// --- Helpers ---

fn admin_display_suffix(level: u8, ctx: &crate::admin::AdminContext) -> String {
    let opts: [Option<&str>; 4] = match level {
        9 | 10 => [ctx.city.as_deref(), ctx.county.as_deref(), ctx.state.as_deref(), ctx.country.as_deref()],
        8      => [ctx.county.as_deref(), ctx.state.as_deref(), ctx.country.as_deref(), None],
        6      => [ctx.state.as_deref(), ctx.country.as_deref(), None, None],
        4      => [ctx.country.as_deref(), None, None, None],
        _      => return String::new(),
    };
    opts.iter().filter_map(|o| *o).collect::<Vec<_>>().join(", ")
}

fn way_centroid(nodes: &[crate::binary::NodeCoord]) -> (f64, f64) {
    let (lat_sum, lon_sum) = nodes.iter()
        .fold((0f64, 0f64), |(la, lo), n| (la + n.lat as f64, lo + n.lng as f64));
    let n = nodes.len() as f64;
    (lat_sum / n, lon_sum / n)
}

fn polygon_centroid(verts: &[crate::binary::NodeCoord]) -> (f64, f64) {
    way_centroid(verts)
}
