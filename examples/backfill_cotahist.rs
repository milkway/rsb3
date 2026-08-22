//! Backfill de um ano do COTAHIST: download oficial → cache com
//! hash/manifesto → parse → Parquet no esquema V1.
//!
//! ```text
//! cargo run --release --example backfill_cotahist -- 2025 [dir-de-dados]
//! ```
//!
//! Sem o segundo argumento, usa `./data` (ZIP+TXT+manifesto e o Parquet).

use std::path::PathBuf;
use std::time::Instant;

use rsb3::cotahist::{self, Cache, Period, SchemaVersion};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let year: i32 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "2025".into())
        .parse()?;
    let dir = PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "data".into()));
    let cache = Cache { dir: dir.clone() };

    let t0 = Instant::now();
    let txt = cotahist::fetch(Period::Year(year), &cache)?;
    println!("fetch: {} em {:.1?}", txt.display(), t0.elapsed());

    let t1 = Instant::now();
    let batch = cotahist::parse(&txt)?;
    println!("parse: {} linhas em {:.1?}", batch.num_rows(), t1.elapsed());

    let out = dir.join(format!("{}.parquet", Period::Year(year).file_stem()));
    let t2 = Instant::now();
    cotahist::to_parquet(std::slice::from_ref(&batch), &out, SchemaVersion::V1)?;
    println!("parquet: {} em {:.1?}", out.display(), t2.elapsed());

    println!("total: {:.1?}", t0.elapsed());
    Ok(())
}
