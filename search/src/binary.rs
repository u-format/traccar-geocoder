use memmap2::Mmap;
use std::fs::File;

pub const NO_DATA: u32 = 0xFFFF_FFFF;
pub const INTERIOR_FLAG: u32 = 0x8000_0000;
pub const ID_MASK: u32 = 0x7FFF_FFFF;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WayHeader {
    pub node_offset: u32,
    pub node_count: u8,
    pub name_id: u32,
    pub postcode_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AdminPolygon {
    pub vertex_offset: u32,
    pub vertex_count: u16,
    pub name_id: u32,
    pub admin_level: u8,
    pub area: f32,
    pub country_code: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NodeCoord {
    pub lat: f32,
    pub lng: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlaceFeature {
    pub lat: f32,
    pub lng: f32,
    pub name_id: u32,
    pub feature_type: u8, 
}

pub struct BinaryIndex {
    pub admin_cells:    Mmap,
    pub admin_entries:  Mmap,
    pub admin_polygons: Mmap,
    pub admin_vertices: Mmap,
    pub place_cells:    Mmap,
    pub place_entries:  Mmap,
    pub street_ways:    Mmap,
    pub street_nodes:   Mmap,
    pub place_features: Mmap,
    pub strings:        Mmap,
}

impl BinaryIndex {
    pub fn load(dir: &str) -> Result<Self, String> {
        Ok(BinaryIndex {
            admin_cells:    mmap_file(&format!("{}/admin_cells.bin", dir))?,
            admin_entries:  mmap_file(&format!("{}/admin_entries.bin", dir))?,
            admin_polygons: mmap_file(&format!("{}/admin_polygons.bin", dir))?,
            admin_vertices: mmap_file(&format!("{}/admin_vertices.bin", dir))?,
            place_cells:    mmap_file(&format!("{}/place_cells.bin", dir))?,
            place_entries:  mmap_file(&format!("{}/place_entries.bin", dir))?,
            street_ways:    mmap_file(&format!("{}/street_ways.bin", dir))?,
            street_nodes:   mmap_file(&format!("{}/street_nodes.bin", dir))?,
            place_features: mmap_file(&format!("{}/place_features.bin", dir))?,
            strings:        mmap_file(&format!("{}/strings.bin", dir))?,
        })
    }

    pub fn get_string(&self, offset: u32) -> &str {
        if offset == NO_DATA { return ""; }
        let bytes = &self.strings[offset as usize..];
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        std::str::from_utf8(&bytes[..end]).unwrap_or("")
    }

    pub fn ways(&self) -> &[WayHeader] {
        unsafe {
            std::slice::from_raw_parts(
                self.street_ways.as_ptr() as *const WayHeader,
                self.street_ways.len() / std::mem::size_of::<WayHeader>(),
            )
        }
    }

    pub fn nodes(&self) -> &[NodeCoord] {
        unsafe {
            std::slice::from_raw_parts(
                self.street_nodes.as_ptr() as *const NodeCoord,
                self.street_nodes.len() / std::mem::size_of::<NodeCoord>(),
            )
        }
    }

    pub fn polygons(&self) -> &[AdminPolygon] {
        unsafe {
            std::slice::from_raw_parts(
                self.admin_polygons.as_ptr() as *const AdminPolygon,
                self.admin_polygons.len() / std::mem::size_of::<AdminPolygon>(),
            )
        }
    }

    pub fn vertices(&self) -> &[NodeCoord] {
        unsafe {
            std::slice::from_raw_parts(
                self.admin_vertices.as_ptr() as *const NodeCoord,
                self.admin_vertices.len() / std::mem::size_of::<NodeCoord>(),
            )
        }
    }

    pub fn place_features(&self) -> &[PlaceFeature] {
        unsafe {
            std::slice::from_raw_parts(
                self.place_features.as_ptr() as *const PlaceFeature,
                self.place_features.len() / std::mem::size_of::<PlaceFeature>(),
            )
        }
    }

    fn read_u16(data: &[u8], off: usize) -> u16 {
        u16::from_le_bytes([data[off], data[off + 1]])
    }

    fn read_u32(data: &[u8], off: usize) -> u32 {
        u32::from_le_bytes(data[off..off + 4].try_into().unwrap())
    }

    fn read_u64(data: &[u8], off: usize) -> u64 {
        u64::from_le_bytes(data[off..off + 8].try_into().unwrap())
    }

    fn lookup_cell(cells: &[u8], cell_id: u64) -> u32 {
        let entry_size = 12usize;
        let count = cells.len() / entry_size;
        if count == 0 { return NO_DATA; }
        let mut lo = 0usize;
        let mut hi = count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let mid_id = Self::read_u64(cells, mid * entry_size);
            if mid_id == cell_id {
                return Self::read_u32(cells, mid * entry_size + 8);
            } else if mid_id < cell_id {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        NO_DATA
    }

    fn for_each_entry(entries: &[u8], offset: u32, mut f: impl FnMut(u32)) {
        if offset == NO_DATA { return; }
        let off = offset as usize;
        if off + 2 > entries.len() { return; }
        let count = Self::read_u16(entries, off) as usize;
        let start = off + 2;
        if start + count * 4 > entries.len() { return; }
        for i in 0..count {
            f(Self::read_u32(entries, start + i * 4));
        }
    }

    pub fn lookup_admin_cell(&self, cell_id: u64) -> u32 {
        Self::lookup_cell(&self.admin_cells, cell_id)
    }

    pub fn for_each_admin_entry(&self, offset: u32, f: impl FnMut(u32)) {
        Self::for_each_entry(&self.admin_entries, offset, f);
    }

    pub fn lookup_place_cell(&self, cell_id: u64) -> u32 {
        Self::lookup_cell(&self.place_cells, cell_id)
    }

    pub fn for_each_place_entry(&self, offset: u32, f: impl FnMut(u32)) {
        Self::for_each_entry(&self.place_entries, offset, f);
    }
}

fn mmap_file(path: &str) -> Result<Mmap, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open {}: {}", path, e))?;
    unsafe { Mmap::map(&file).map_err(|e| format!("Failed to mmap {}: {}", path, e)) }
}
