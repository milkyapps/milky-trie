use std::{sync::{Arc, RwLock}, collections::HashMap};
use rocksdb::{DBWithThreadMode, SingleThreaded};

pub struct Items(Vec<u8>);

impl std::fmt::Debug for Items {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Items").field(&self.0.len()).finish()
    }
}

pub struct ItemsStrIter<'a> {
    pos: usize,
    items: &'a Items
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
        std::str::from_utf8(str).ok()
    }
}

impl Items {
    pub fn as_str(&self) -> ItemsStrIter<'_> {
        ItemsStrIter { pos: 0, items: self }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TrieNode {
    value: u8,
    next: [Option<u32>; 256]
}

impl Default for TrieNode {
    fn default() -> Self {
        Self { value: Default::default(), next: [None; 256] }
    }
}

pub struct Trie {
    db: Arc<DBWithThreadMode<SingleThreaded>>,
    prefix: String,
    qty: usize,
    cache: HashMap<usize, TrieNode>
}

impl Trie {
    pub fn new(db: Arc<DBWithThreadMode<SingleThreaded>>, prefix: impl Into<String>) -> Self {
        let mut s = Self {
            db,
            prefix: prefix.into(),
            qty: 0,
            cache: HashMap::new()
        };

        let root = TrieNode::default();
        s.cache_put_node_at(0, &root);

        s
    }

    pub fn flush(&self) {
        let _ = self.db.flush_wal(true);
    }

    pub fn put_trie_node_at(&self, suffix: &[u8], node: &TrieNode) {
        let prefix = self.prefix.as_bytes();
        let mut root = [0u8;1024];
        root[0..prefix.len()].clone_from_slice(prefix);
        root[prefix.len()..(prefix.len() + suffix.len())].clone_from_slice(suffix);
        let key = &root[0..(prefix.len() + suffix.len())]; 
        
        let value = unsafe { 
            std::slice::from_raw_parts(
                node as *const TrieNode as *const u8, 
                std::mem::size_of::<TrieNode>()
            )
        };

        self.db.put(key, value).unwrap();
    }

    pub fn get_trie_node_at(&self, suffix: &[u8]) -> Option<TrieNode> {
        let prefix = self.prefix.as_bytes();
        let mut root = [0u8;1024];
        root[0..prefix.len()].clone_from_slice(prefix);
        root[prefix.len()..(prefix.len() + suffix.len())].clone_from_slice(suffix);
        let key = &root[0..(prefix.len() + suffix.len())]; 
        
        let Ok(Some(value)) = self.db.get(key) else {
            return None;
        };

        let node = unsafe {
            *(value.as_ptr() as *const u8 as *const TrieNode)
        };
        Some(node)
    }

    fn cache_get_node_at(&mut self, n: usize) -> Option<TrieNode> {
        if let Some(node) = self.cache.get(&n) {
            return Some(node.clone());
        }

        let suffix = &n.to_le_bytes()[..];
        match self.get_trie_node_at(suffix) {
            Some(node) => {
                self.cache.insert(n, node);
                Some(node)
            },
            None => None,
        }
    }

    fn cache_put_node_at(&mut self, n: usize, node: &TrieNode) {
        *self.cache.entry(n).or_default() = node.clone();

        let suffix = &n.to_le_bytes()[..];
        self.put_trie_node_at(suffix, node);
    }

    pub fn get_value(&self, n: usize) -> Items {
        let mut root = [0u8;1024];

        let prefix = self.prefix.as_bytes();
        root[0..prefix.len()].clone_from_slice(prefix);
        
        let n = n.to_le_bytes();
        let end = prefix.len() + n.len();
        root[prefix.len()..end].clone_from_slice(&n[..]);
        
        let suffix = b"/values";        
        root[end..(end + suffix.len())].clone_from_slice(&suffix[..]);
        let key = &root[0..(end + suffix.len())]; 

        let v = if let Ok(Some(bytes)) = self.db.get(key)  {
            bytes
        } else {
            vec![]
        };

        Items(v)
    }

    pub fn append_value(&self, n: usize, value: &[u8]) {
        let mut root = [0u8;1024];

        let prefix = self.prefix.as_bytes();
        root[0..prefix.len()].clone_from_slice(prefix);
        
        let n = n.to_le_bytes();
        let end = prefix.len() + n.len();
        root[prefix.len()..end].clone_from_slice(&n[..]);
        
        let suffix = b"/values";        
        root[end..(end + suffix.len())].clone_from_slice(&suffix[..]);
        let key = &root[0..(end + suffix.len())]; 

        let mut bytes = if let Ok(Some(bytes)) = self.db.get(key)  {
            bytes
        } else {
            Vec::with_capacity(value.len() + 8)
        };

        bytes.extend((value.len() as u32).to_le_bytes());
        bytes.extend(value);

        self.db.put(key, bytes.as_slice()).unwrap();
    }

    pub fn insert(&mut self, key: impl AsRef<str>, value: &[u8]) {
        let mut n = 0;
        let mut current = self.cache_get_node_at(0).unwrap();

        let bytes = key.as_ref().as_bytes();
        for byte in bytes {
            match current.next[*byte as usize] {
                Some(nextn) => {
                    n = nextn as usize;
                    current = self.cache_get_node_at(nextn as usize).unwrap();
                },
                None => {
                    self.qty += 1;
                    let nextn = self.qty; 

                    current.next[*byte as usize] = Some(nextn as u32);
                    self.cache_put_node_at(n, &current);

                    let mut node = TrieNode::default();
                    node.value = *byte;
                    self.cache_put_node_at(nextn, &node);

                    n = nextn;
                    current = node;
                },
            };
        }

        self.append_value(n, value)
    }

    pub fn get(&mut self, key: &str) -> Items {
        let mut n = 0;
        let mut current = if let Some(root) = self.cache_get_node_at(0) {
            root
        } else {
            let root = TrieNode::default();
            self.cache_put_node_at(0, &root);
            root
        };

        let bytes = key.as_bytes();
        for byte in bytes {
            let (nextn, nextnode) = match current.next[*byte as usize] {
                Some(node) => {
                    (
                        node as usize,
                        self.cache_get_node_at(node as usize).unwrap()
                    )
                },
                None => {
                    return Items(vec![])
                },
            };
            n = nextn;
            current = nextnode;
        }

        self.get_value(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_insert_and_get() {
        use rocksdb::DB;
        let path = "_path_for_rocksdb_storage";
        let _ = std::fs::remove_dir_all(path);

        let db = DB::open_default(path).unwrap();
        let mut t = Trie::new(Arc::new(db), "sometrie");

        t.insert("Daniel", b"37");
        t.insert("Danielle", b"37");
        let items = t.get("Daniel");
        for item in items.as_str() {
            dbg!(item);
        }
    }
}
