mod admin;
mod binary;
mod handler;
mod indexer;
mod tokenizer;

use axum::routing::get;
use axum::Router;
use handler::{search_handler, SearchState};
use indexer::build_schema;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data_dir  = args.get(1).map(|s| s.as_str()).unwrap_or("output-dir");
    let bind_addr = args.get(2).map(|s| s.as_str()).unwrap_or("0.0.0.0:3001");

    eprintln!("[search] Loading binary index from {}...", data_dir);
    let idx = binary::BinaryIndex::load(data_dir)
        .unwrap_or_else(|e| { eprintln!("[search] {}", e); std::process::exit(1); });

    eprintln!("[search] Building tantivy index...");
    let ss = Arc::new(build_schema());
    let tantivy_index = indexer::build_index(&idx)
        .unwrap_or_else(|e| { eprintln!("[search] index build failed: {}", e); std::process::exit(1); });

    let state = Arc::new(
        SearchState::new(&tantivy_index, ss)
            .unwrap_or_else(|e| { eprintln!("[search] reader failed: {}", e); std::process::exit(1); })
    );

    let app = Router::new()
        .route("/search", get(search_handler))
        .with_state(state);

    eprintln!("[search] Listening on {}...", bind_addr);
    let listener = tokio::net::TcpListener::bind(bind_addr).await
        .unwrap_or_else(|e| { eprintln!("[search] bind failed: {}", e); std::process::exit(1); });
    axum::serve(listener, app).await.unwrap();
}
