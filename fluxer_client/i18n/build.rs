//! Parses the official Lingui `.po` catalogs into compact binary tables.
//!
//! Output (in OUT_DIR):
//! - `keys.zst`: zstd of (u32 idx len, idx, txt): sorted union of all message keys (`ctx\u{4}msgid` or `msgid`)
//!   and u32 LE offsets (n+1 entries).
//! - `<locale>.zst`: zstd of (u32 idx len, idx, blob); per-locale u32 LE triples (key index, start, end) into the
//!   blob, sorted by key index. Empty or source-identical translations are omitted.
//! - `locales.rs`: static `LOCALES` table.
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

fn unquote(s: &str) -> String {
    let s = s.trim();
    let inner = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s);
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Returns key -> msgstr (non-empty, differing from msgid).
fn parse_po(text: &str) -> BTreeMap<String, String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Field {
        None,
        Ctx,
        Id,
        Str,
    }
    let mut map = BTreeMap::new();
    let (mut ctx, mut id, mut st) = (None::<String>, String::new(), String::new());
    let mut field = Field::None;
    let mut have_id = false;
    let flush = |ctx: &mut Option<String>,
                 id: &mut String,
                 st: &mut String,
                 have_id: &mut bool,
                 map: &mut BTreeMap<String, String>| {
        if *have_id && !id.is_empty() && !st.is_empty() {
            let c = ctx.take();
            if *st != *id {
                let key = match c {
                    Some(c) => format!("{c}\u{4}{id}"),
                    None => id.clone(),
                };
                map.insert(key, std::mem::take(st));
            }
        }
        *ctx = None;
        id.clear();
        st.clear();
        *have_id = false;
    };
    for line in text.lines() {
        let line = line.trim_end();
        if line.starts_with('#') {
            continue;
        }
        if line.is_empty() {
            flush(&mut ctx, &mut id, &mut st, &mut have_id, &mut map);
            field = Field::None;
        } else if let Some(r) = line.strip_prefix("msgctxt ") {
            flush(&mut ctx, &mut id, &mut st, &mut have_id, &mut map);
            ctx = Some(unquote(r));
            field = Field::Ctx;
        } else if let Some(r) = line.strip_prefix("msgid ") {
            if field == Field::Str {
                flush(&mut ctx, &mut id, &mut st, &mut have_id, &mut map);
            }
            id = unquote(r);
            have_id = true;
            field = Field::Id;
        } else if let Some(r) = line.strip_prefix("msgstr ") {
            st = unquote(r);
            field = Field::Str;
        } else if line.starts_with('"') {
            let piece = unquote(line);
            match field {
                Field::Ctx => {
                    if let Some(c) = ctx.as_mut() {
                        c.push_str(&piece);
                    }
                }
                Field::Id => id.push_str(&piece),
                Field::Str => st.push_str(&piece),
                Field::None => {}
            }
        }
    }
    flush(&mut ctx, &mut id, &mut st, &mut have_id, &mut map);
    map
}

/// `u32 LE index length` + index bytes + blob bytes.
fn pack(idx: &[u8], blob: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + idx.len() + blob.len());
    v.extend((idx.len() as u32).to_le_bytes());
    v.extend_from_slice(idx);
    v.extend_from_slice(blob);
    v
}

fn compress(data: &[u8]) -> Vec<u8> {
    zstd::bulk::compress(data, 19).expect("zstd compress")
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=FLUXER_LOCALES_DIR");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let dir = match env::var("FLUXER_LOCALES_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => manifest.join("../../fluxer_app/src/features/i18n/locales"),
    };
    println!("cargo:rerun-if-changed={}", dir.display());
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    let mut codes: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read locales dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("messages.po").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    codes.sort();

    let mut catalogs: Vec<(String, BTreeMap<String, String>)> = Vec::new();
    let mut all_keys: BTreeMap<String, u32> = BTreeMap::new();
    for code in &codes {
        let path = dir.join(code).join("messages.po");
        println!("cargo:rerun-if-changed={}", path.display());
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let map = parse_po(&text);
        for k in map.keys() {
            all_keys.entry(k.clone()).or_insert(0);
        }
        catalogs.push((code.clone(), map));
    }
    let mut keys_txt = String::new();
    let mut keys_idx: Vec<u8> = Vec::new();
    for (i, (k, v)) in all_keys.iter_mut().enumerate() {
        *v = i as u32;
        keys_idx.extend((keys_txt.len() as u32).to_le_bytes());
        keys_txt.push_str(k);
    }
    keys_idx.extend((keys_txt.len() as u32).to_le_bytes());
    let raw_keys = pack(&keys_idx, keys_txt.as_bytes());
    fs::write(out.join("keys.zst"), compress(&raw_keys)).expect("write keys.zst");

    let mut rs = String::new();
    let _ = writeln!(rs, "static LOCALES: [LocaleTable; {}] = [", catalogs.len());
    for (code, map) in &catalogs {
        let mut blob = String::new();
        let mut idx: Vec<u8> = Vec::new();
        // BTreeMap iterates keys sorted, matching key indices order.
        for (k, v) in map {
            let ki = all_keys[k];
            let start = blob.len() as u32;
            blob.push_str(v);
            idx.extend(ki.to_le_bytes());
            idx.extend(start.to_le_bytes());
            idx.extend((blob.len() as u32).to_le_bytes());
        }
        fs::write(out.join(format!("{code}.zst")), compress(&pack(&idx, blob.as_bytes())))
            .expect("write locale zst");
        let _ = writeln!(
            rs,
            "    LocaleTable {{ code: {code:?}, zst: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{code}.zst\")), cell: OnceLock::new() }},"
        );
    }
    rs.push_str("];\n");
    fs::write(out.join("locales.rs"), rs).expect("write locales.rs");
}
