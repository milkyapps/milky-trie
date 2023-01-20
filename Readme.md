# Persistent Trie

Very simple implementation of a persistent trie using RocksDB.

```rust
use rocksdb::DB;
let path = "_path_for_rocksdb_storage";
let db = DB::open_default(path).unwrap();

let mut t = Trie::new(Arc::new(db), "sometrie");

t.insert("Item 1", b"42");
t.insert("Item 2", b"43");
let items = t.get("Item 1");
for item in items.as_str() {
    dbg!(item);
}
```

Keys can be anything that can be ref as `&[u8]`, which means keys can be
heteregeneous.

Values also only need to be ref as `&[u8]`. The interpretation of the value
is up to the client. Some helper methods exist to retrieve obvious cases like 
string slices, numbers etc...

When storing inside RocksDB, no assumption is made about flushing, so different configurations
will generate wildly different performance results. If RocksDB is being as lazy as possible, it is possible
to control `flush` calling `t.flush()` when needed.

## Performance

Performance is of course much worse than an in-memory trie (<https://github.com/sdleffler/qp-trie-rs>), but `insert` and `get` still achieve sub-millisecond performance.

```
Running benches/trie.rs

milky_trie::insert      time:   [61.783 µs 143.36 µs 327.94 µs]
Found 6 outliers among 100 measurements (6.00%)
  2 (2.00%) high mild
  4 (4.00%) high severe

milky_trie::get         time:   [8.2900 µs 8.3944 µs 8.5082 µs]
Found 7 outliers among 100 measurements (7.00%)
  6 (6.00%) high mild
  1 (1.00%) high severe

qp-trie::insert         time:   [3.6186 µs 3.6679 µs 3.7190 µs]
Found 11 outliers among 100 measurements (11.00%)
  2 (2.00%) low severe
  3 (3.00%) low mild
  3 (3.00%) high mild
  3 (3.00%) high severe

qp-trie::get            time:   [3.8719 µs 3.9413 µs 4.0607 µs]
Found 8 outliers among 100 measurements (8.00%)
  2 (2.00%) high mild
  6 (6.00%) high severe
```

## Todo

- [ ] Better testing
- [ ] Multi thread support apart from RocksDB configuration
- [ ] Support await/async
- [ ] Other storage support
- [ ] In memory support with performance on par of other tries
- [ ] Delete items
- [ ] Get with wildcard
