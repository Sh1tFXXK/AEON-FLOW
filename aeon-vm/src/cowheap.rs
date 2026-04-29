use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeapPageDelta {
    pub index: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct COWHeap {
    pub pages: Vec<[u8; PAGE_SIZE]>,
    pub dirty: BTreeSet<usize>,
    len: usize,
}

#[derive(Serialize, Deserialize)]
struct COWHeapWire {
    pages: Vec<Vec<u8>>,
    dirty: Vec<usize>,
    len: usize,
}

impl Serialize for COWHeap {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        COWHeapWire {
            pages: self.pages.iter().map(|page| page.to_vec()).collect(),
            dirty: self.dirty_page_indices(),
            len: self.len,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for COWHeap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = COWHeapWire::deserialize(deserializer)?;
        let mut pages = Vec::with_capacity(wire.pages.len());
        for page in wire.pages {
            if page.len() != PAGE_SIZE {
                return Err(serde::de::Error::custom(format!(
                    "heap page has {} bytes, expected {}",
                    page.len(),
                    PAGE_SIZE
                )));
            }
            let mut fixed = [0u8; PAGE_SIZE];
            fixed.copy_from_slice(&page);
            pages.push(fixed);
        }
        Ok(COWHeap {
            pages,
            dirty: wire.dirty.into_iter().collect(),
            len: wire.len,
        })
    }
}

impl COWHeap {
    pub fn new(len: usize) -> Self {
        let page_count = len.div_ceil(PAGE_SIZE);
        COWHeap {
            pages: vec![[0; PAGE_SIZE]; page_count],
            dirty: BTreeSet::new(),
            len,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut heap = COWHeap::new(bytes.len());
        heap.write(0, bytes).expect("new heap has enough space");
        heap.clear_dirty();
        heap
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, addr: usize) -> Result<u8, String> {
        self.check_range(addr, 1)?;
        Ok(self.pages[addr / PAGE_SIZE][addr % PAGE_SIZE])
    }

    pub fn set(&mut self, addr: usize, value: u8) -> Result<(), String> {
        self.check_range(addr, 1)?;
        let page = addr / PAGE_SIZE;
        self.pages[page][addr % PAGE_SIZE] = value;
        self.dirty.insert(page);
        Ok(())
    }

    pub fn read(&self, addr: usize, len: usize) -> Result<Vec<u8>, String> {
        self.check_range(addr, len)?;
        let mut out = Vec::with_capacity(len);
        let mut cursor = addr;
        let end = addr + len;
        while cursor < end {
            let page = cursor / PAGE_SIZE;
            let offset = cursor % PAGE_SIZE;
            let take = (PAGE_SIZE - offset).min(end - cursor);
            out.extend_from_slice(&self.pages[page][offset..offset + take]);
            cursor += take;
        }
        Ok(out)
    }

    pub fn write(&mut self, addr: usize, bytes: &[u8]) -> Result<(), String> {
        self.check_range(addr, bytes.len())?;
        let mut written = 0;
        while written < bytes.len() {
            let cursor = addr + written;
            let page = cursor / PAGE_SIZE;
            let offset = cursor % PAGE_SIZE;
            let take = (PAGE_SIZE - offset).min(bytes.len() - written);
            self.pages[page][offset..offset + take]
                .copy_from_slice(&bytes[written..written + take]);
            self.dirty.insert(page);
            written += take;
        }
        Ok(())
    }

    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len);
        for page in &self.pages {
            let remaining = self.len - out.len();
            if remaining == 0 {
                break;
            }
            out.extend_from_slice(&page[..remaining.min(PAGE_SIZE)]);
        }
        out
    }

    pub fn dirty_pages(&self) -> Vec<HeapPageDelta> {
        self.dirty
            .iter()
            .map(|index| HeapPageDelta {
                index: *index,
                bytes: self.pages[*index].to_vec(),
            })
            .collect()
    }

    pub fn dirty_page_indices(&self) -> Vec<usize> {
        self.dirty.iter().copied().collect()
    }

    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    pub fn apply_pages(&mut self, pages: &[HeapPageDelta]) -> Result<(), String> {
        for page in pages {
            if page.index >= self.pages.len() {
                return Err(format!("page {} out of range", page.index));
            }
            if page.bytes.len() != PAGE_SIZE {
                return Err(format!(
                    "page {} has {} bytes, expected {}",
                    page.index,
                    page.bytes.len(),
                    PAGE_SIZE
                ));
            }
            self.pages[page.index].copy_from_slice(&page.bytes);
            self.dirty.insert(page.index);
        }
        Ok(())
    }

    fn check_range(&self, addr: usize, len: usize) -> Result<(), String> {
        let end = addr
            .checked_add(len)
            .ok_or_else(|| format!("heap range overflows: addr={} len={}", addr, len))?;
        if end > self.len {
            return Err(format!("heap range {}..{} exceeds {}", addr, end, self.len));
        }
        Ok(())
    }
}
