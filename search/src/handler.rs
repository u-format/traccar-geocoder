use axum::extract::Query as AxumQuery;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, TermQuery};
use tantivy::schema::{IndexRecordOption, Value};
use tantivy::{Index, IndexReader, ReloadPolicy, Term};

use crate::indexer::SearchSchema;
use crate::tokenizer::normalize_turkish;

pub struct SearchState {
    pub reader: IndexReader,
    pub ss:     Arc<SearchSchema>,
}

impl SearchState {
    pub fn new(index: &Index, ss: Arc<SearchSchema>) -> Result<Self, tantivy::TantivyError> {
        let reader = index.reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        Ok(SearchState { reader, ss })
    }
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub display_name: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(rename = "type")]
    pub kind: String,
}

pub async fn search_handler(
    AxumQuery(params): AxumQuery<SearchParams>,
    axum::extract::State(state): axum::extract::State<Arc<SearchState>>,
) -> Response {
    let q = match params.q {
        Some(q) if !q.trim().is_empty() => q,
        _ => return (StatusCode::BAD_REQUEST,
                     Json(serde_json::json!({"error": "missing query parameter q"}))
                    ).into_response(),
    };

    let results = match run_search(&state, q.trim(), 5) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[search] query error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "search error"}))
                   ).into_response();
        }
    };

    Json(results).into_response()
}

fn run_search(state: &SearchState, q: &str, limit: usize) -> tantivy::Result<Vec<SearchResult>> {
    let searcher = state.reader.searcher();
    let ss = &state.ss;

    let query = build_query(ss, q);
    let top_docs = searcher.search(&query, &tantivy::collector::TopDocs::with_limit(limit * 20))?;

    let query_tokens_norm: Vec<String> = q.split_whitespace()
        .map(|t| normalize_turkish(&t.to_lowercase()))
        .collect();
    let meaningful: Vec<&str> = query_tokens_norm.iter()
        .filter(|t| t.len() >= 3)
        .map(|t| t.as_str())
        .collect();

    let mut scored: Vec<(f64, SearchResult)> = top_docs
        .into_iter()
        .filter_map(|(score, addr)| {
            let doc: tantivy::TantivyDocument = searcher.doc(addr).ok()?;
            let display = doc.get_first(ss.f_display)?.as_str()?.to_string();
            let lat     = doc.get_first(ss.f_lat)?.as_f64()?;
            let lon     = doc.get_first(ss.f_lon)?.as_f64()?;
            let kind    = doc.get_first(ss.f_kind)?.as_str()?.to_string();
            let importance = doc.get_first(ss.f_importance)
                .and_then(|v| v.as_f64()).unwrap_or(1.0);

            let display_norm = normalize_turkish(&display.to_lowercase());
            let context_matches = query_tokens_norm.iter()
                .filter(|t| t.len() >= 4 && display_norm.contains(t.as_str()))
                .count();
            let all_match = !meaningful.is_empty()
                && meaningful.iter().all(|t| display_norm.contains(*t));
            let context_boost = if all_match {
                1.0 + context_matches as f64 + 3.0
            } else {
                1.0 + context_matches as f64
            };

            let final_score = score as f64 * importance * context_boost;
            Some((final_score, SearchResult { display_name: display, lat, lon, kind }))
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut seen = std::collections::HashSet::new();
    let mut out  = Vec::with_capacity(limit);
    for (_, result) in scored {
        if seen.insert(result.display_name.to_lowercase()) {
            out.push(result);
            if out.len() >= limit { break; }
        }
    }

    Ok(out)
}

fn build_query(ss: &SearchSchema, q: &str) -> BooleanQuery {
    let tokens_norm: Vec<String> = q.split_whitespace()
        .map(|t| normalize_turkish(&t.to_lowercase()))
        .collect();

    let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();

    for tok in &tokens_norm {
        clauses.push((Occur::Should, Box::new(TermQuery::new(
            Term::from_field_text(ss.f_name_norm, tok),
            IndexRecordOption::WithFreqs,
        ))));

        if tok.len() >= 4 {
            clauses.push((Occur::Should, Box::new(FuzzyTermQuery::new(
                Term::from_field_text(ss.f_name_norm, tok),
                1,
                true,
            ))));
        }

        clauses.push((Occur::Should, Box::new(TermQuery::new(
            Term::from_field_text(ss.f_context, tok),
            IndexRecordOption::WithFreqs,
        ))));
    }

    BooleanQuery::new(clauses)
}
