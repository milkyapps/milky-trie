use rocksdb::{DBWithThreadMode, SingleThreaded};
use std::{collections::HashMap, iter::FusedIterator, sync::Arc};

pub struct Items(Vec<u8>);

impl std::fmt::Debug for Items {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Items").field(&self.0.len()).finish()
    }
}

pub struct ItemsStrIter<'a> {
    pos: usize,
    items: &'a Items,
}

impl<'a> Iterator for ItemsStrIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let slice = self.items.0.as_slice();

        let start = self.pos;
        let end = self.pos + 4;
        if end >= slice.len() {
            return None;
        }
        let len = &slice[start..end];
        let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
        self.pos += 4;

        let str = &self.items.0.as_slice()[self.pos..self.pos + len];
        self.pos += len;
        std::str::from_utf8(str).ok()
    }
}

impl<'a> FusedIterator for ItemsStrIter<'a> {}

impl Items {
    pub fn as_str(&self) -> ItemsStrIter<'_> {
        ItemsStrIter {
            pos: 0,
            items: self,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // allow value not being used. It is useful for debug
pub struct TrieNode {
    value: u8,
    next: [Option<u32>; 256],
}

impl Default for TrieNode {
    fn default() -> Self {
        Self {
            value: Default::default(),
            next: [None; 256],
        }
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub struct TrieData {
    qty: usize,
}

pub struct Trie {
    db: Arc<DBWithThreadMode<SingleThreaded>>,
    prefix: String,
    data: TrieData,
    cache: HashMap<usize, TrieNode>,
}

impl Trie {
    pub fn new(db: Arc<DBWithThreadMode<SingleThreaded>>, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        let data = Self::get_trie_data(&db, prefix.as_bytes());

        let mut s = Self {
            db,
            prefix,
            data,
            cache: HashMap::new(),
        };

        if s.cache_get_node_at(0).is_none() {
            s.cache_put_node_at(0, &TrieNode::default());
        }

        s
    }

    pub fn flush(&self) {
        let _ = self.db.flush_wal(true);
    }

    fn get_trie_data(db: &DBWithThreadMode<SingleThreaded>, prefix: &[u8]) -> TrieData {
        db.get(prefix)
            .unwrap()
            .map(|bytes| unsafe { *(bytes.as_ptr() as *const u8 as *const TrieData) })
            .unwrap_or_default()
    }

    fn set_trie_data(&self) {
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &self.data as *const TrieData as *const u8,
                std::mem::size_of::<TrieData>(),
            )
        };

        let _ = self.db.put(self.prefix.as_bytes(), bytes);
    }

    fn put_trie_node_at(&self, suffix: &[u8], node: &TrieNode) {
        let prefix = self.prefix.as_bytes();
        let mut root = [0u8; 1024];
        root[0..prefix.len()].clone_from_slice(prefix);
        root[prefix.len()..(prefix.len() + suffix.len())].clone_from_slice(suffix);
        let key = &root[0..(prefix.len() + suffix.len())];

        let bytes = unsafe {
            std::slice::from_raw_parts(
                node as *const TrieNode as *const u8,
                std::mem::size_of::<TrieNode>(),
            )
        };

        self.db.put(key, bytes).unwrap();
    }

    fn get_trie_node_at(&self, suffix: &[u8]) -> Option<TrieNode> {
        let prefix = self.prefix.as_bytes();
        let mut root = [0u8; 1024];
        root[0..prefix.len()].clone_from_slice(prefix);
        root[prefix.len()..(prefix.len() + suffix.len())].clone_from_slice(suffix);
        let key = &root[0..(prefix.len() + suffix.len())];

        let Ok(Some(bytes)) = self.db.get(key) else {
            return None;
        };

        let node = unsafe { *(bytes.as_ptr() as *const u8 as *const TrieNode) };
        Some(node)
    }

    fn cache_get_node_at(&mut self, n: usize) -> Option<TrieNode> {
        if let Some(node) = self.cache.get(&n) {
            return Some(*node);
        }

        let suffix = &n.to_le_bytes()[..];
        match self.get_trie_node_at(suffix) {
            Some(node) => {
                self.cache.insert(n, node);
                Some(node)
            }
            None => None,
        }
    }

    fn cache_put_node_at(&mut self, n: usize, node: &TrieNode) {
        *self.cache.entry(n).or_default() = *node;

        let suffix = &n.to_le_bytes()[..];
        self.put_trie_node_at(suffix, node);
    }

    fn get_value(&self, n: usize) -> Items {
        let mut root = [0u8; 1024];

        let prefix = self.prefix.as_bytes();
        root[0..prefix.len()].clone_from_slice(prefix);

        let n = n.to_le_bytes();
        let end = prefix.len() + n.len();
        root[prefix.len()..end].clone_from_slice(&n[..]);

        let suffix = b"/values";
        root[end..(end + suffix.len())].clone_from_slice(&suffix[..]);
        let key = &root[0..(end + suffix.len())];

        let v = if let Ok(Some(bytes)) = self.db.get(key) {
            bytes
        } else {
            vec![]
        };

        Items(v)
    }

    fn append_value(&self, n: usize, value: impl AsRef<[u8]>) {
        let mut root = [0u8; 1024];

        let prefix = self.prefix.as_bytes();
        root[0..prefix.len()].clone_from_slice(prefix);

        let n = n.to_le_bytes();
        let end = prefix.len() + n.len();
        root[prefix.len()..end].clone_from_slice(&n[..]);

        let suffix = b"/values";
        root[end..(end + suffix.len())].clone_from_slice(&suffix[..]);
        let key = &root[0..(end + suffix.len())];

        let value = value.as_ref();
        let mut bytes = if let Ok(Some(bytes)) = self.db.get(key) {
            bytes
        } else {
            Vec::with_capacity(value.len() + 8)
        };

        bytes.extend((value.len() as u32).to_le_bytes());
        bytes.extend(value);

        self.db.put(key, bytes.as_slice()).unwrap();
    }

    pub fn insert(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        let mut n = 0;
        let mut current = self.cache_get_node_at(0).unwrap();

        let bytes = key.as_ref();
        for byte in bytes {
            match current.next[*byte as usize] {
                Some(nextn) => {
                    n = nextn as usize;
                    current = self.cache_get_node_at(nextn as usize).unwrap();
                }
                None => {
                    self.data.qty += 1;
                    let nextn = self.data.qty;

                    current.next[*byte as usize] = Some(nextn as u32);
                    self.cache_put_node_at(n, &current);

                    let node = TrieNode {
                        value: *byte,
                        ..Default::default()
                    };
                    self.cache_put_node_at(nextn, &node);

                    n = nextn;
                    current = node;
                }
            };
        }

        self.set_trie_data();
        self.append_value(n, value)
    }

    pub fn get(&mut self, key: impl AsRef<[u8]>) -> Items {
        let mut n = 0;
        let mut current = self.cache_get_node_at(0).unwrap();

        let bytes = key.as_ref();
        for byte in bytes {
            match current.next[*byte as usize] {
                Some(nextn) => {
                    n = nextn;
                    current = self.cache_get_node_at(nextn as usize).unwrap();
                }
                None => return Items(vec![]),
            };
        }

        self.get_value(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_start_trie_from_scratch() {
        use rocksdb::DB;
        let path = "target/ok_start_trie_from_scratch";
        let _ = std::fs::remove_dir_all(path);
        let db = DB::open_default(path).unwrap();

        let mut t = Trie::new(Arc::new(db), "sometrie");

        t.insert("Item 1", b"42");
        t.insert("Item 2", b"43");

        // Get existing item
        let items = t.get("Item 1");
        assert!(items.as_str().count() == 1);
        assert!(matches!(items.as_str().next(), Some("42")));

        // Get item that do not exist
        let items = t.get("Item 3");
        assert!(items.as_str().count() == 0);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn ok_trie_restarting_from_store() {
        use rocksdb::DB;
        let path = "target/ok_trie_restarting_from_store";
        let _ = std::fs::remove_dir_all(path);

        {
            let db = DB::open_default(path).unwrap();
            let mut t = Trie::new(Arc::new(db), "sometrie");
            t.insert("Item 1", b"42");
            t.flush();
        }

        {
            let db = DB::open_default(path).unwrap();
            let mut t = Trie::new(Arc::new(db), "sometrie");

            // Get existing item
            let items = t.get("Item 1");
            dbg!(items.as_str().count());
            assert!(items.as_str().count() == 1);
            assert!(matches!(items.as_str().next(), Some("42")));

            // Get item that do not exist
            let items = t.get("Item 3");
            assert!(items.as_str().count() == 0);
        }

        let _ = std::fs::remove_dir_all(path);
    }
}
