use crate::cli::Commands;
use crate::ingest_sessions::{handle_ingest_sessions, IngestOptions};
use crate::insights::{handle_insights, InsightsOptions};
use crate::support::{build_llm_client_from_env, format_item, parse_item_type};
use anyhow::{Context, Result};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{
    DocumentId, DocumentRepository, Item, ItemId, ItemRepository, ItemType, Source,
};
use refine_core::refinement::{apply_defaults, extract_items_with_llm, ExtractionPolicy};
use refine_core::search::{SearchEngine, SearchQuery};
use refine_core::session::SessionSource;
use std::io::{self, Read};
use std::sync::Arc;

pub async fn run(
    command: Commands,
    store: Arc<SqliteStore>,
    engine: Arc<SearchEngine>,
) -> Result<()> {
    match command {
        Commands::Extract { stdin } => handle_extract(stdin, store).await,
        Commands::Search { query, limit } => handle_search(&query, limit, engine).await,
        Commands::List { r#type, limit } => handle_list(r#type, limit, store).await,
        Commands::Show { id } => handle_show(&id, store).await,
        Commands::Delete { id } => handle_delete(&id, store).await,
        Commands::Add {
            title,
            summary,
            r#type,
        } => handle_add(&title, &summary, &r#type, store).await,
        Commands::IngestSessions {
            source,
            limit,
            dry_run,
        } => {
            let source_filter = source.as_deref().and_then(parse_session_source);
            let llm_client = if dry_run {
                None
            } else {
                Some(build_llm_client_from_env()?)
            };
            let item_store: Arc<dyn ItemRepository> = store.clone();
            let doc_store: Arc<dyn DocumentRepository> = store.clone();
            handle_ingest_sessions(
                IngestOptions {
                    source: source_filter,
                    limit,
                    dry_run,
                },
                item_store,
                doc_store,
                llm_client,
            )
            .await
        }
        Commands::Insights {
            period,
            prescription,
        } => {
            let llm_client = Some(build_llm_client_from_env()?);
            let item_store: Arc<dyn ItemRepository> = store.clone();
            let doc_store: Arc<dyn DocumentRepository> = store.clone();
            handle_insights(
                InsightsOptions {
                    period,
                    with_prescription: prescription,
                },
                item_store,
                doc_store,
                llm_client,
            )
            .await
        }
        Commands::Docs { limit } => handle_docs(limit, store).await,
        Commands::DocShow { id } => handle_doc_show(&id, store).await,
        Commands::DocSearch { query, limit } => handle_doc_search(&query, limit, store).await,
    }
}

async fn handle_extract(stdin: bool, store: Arc<SqliteStore>) -> Result<()> {
    if !stdin {
        println!("用法: cat conversation.txt | refine extract --stdin");
        return Ok(());
    }

    let mut content = String::new();
    io::stdin().read_to_string(&mut content)?;

    let llm_client = build_llm_client_from_env()?;
    let mut items =
        extract_items_with_llm(llm_client.as_ref(), &content, ExtractionPolicy::default())
            .await
            .context("提炼失败")?;
    let source = Source::new("cli");
    let doc_id = DocumentId::new();
    apply_defaults(&mut items, &source, &doc_id, &content);

    let item_store: &dyn ItemRepository = store.as_ref();
    println!("提炼完成：{} 条", items.len());
    for item in &items {
        item_store.save(item).await?;
        println!(
            "  + [{}] {} ({})",
            format!("{:?}", item.item_type()).to_lowercase(),
            item.title(),
            item.id()
        );
    }

    Ok(())
}

async fn handle_search(query: &str, limit: usize, engine: Arc<SearchEngine>) -> Result<()> {
    let results = engine
        .search(SearchQuery::new(query).with_limit(limit))
        .await?;

    if results.items.is_empty() {
        println!("未找到匹配的知识");
    } else {
        println!("找到 {} 条结果:\n", results.total);
        for hit in results.items {
            println!("{}", format_item(&hit.item, false));
        }
    }

    Ok(())
}

async fn handle_list(r#type: Option<String>, limit: usize, store: Arc<SqliteStore>) -> Result<()> {
    let items = if let Some(type_str) = r#type {
        let item_type = parse_item_type(&type_str).context("无效的类型")?;
        store.find_by_type(item_type).await?
    } else {
        store.find_all().await?
    };

    if items.is_empty() {
        println!("暂无知识");
    } else {
        println!("共 {} 条知识:\n", items.len());
        for item in items.into_iter().take(limit) {
            println!("{}", format_item(&item, false));
        }
    }

    Ok(())
}

async fn handle_show(id: &str, store: Arc<SqliteStore>) -> Result<()> {
    let item_store: &dyn ItemRepository = store.as_ref();
    let item_id = ItemId::from(id);
    match item_store.find_by_id(&item_id).await? {
        Some(item) => println!("{}", format_item(&item, true)),
        None => println!("未找到 ID 为 {} 的知识", id),
    }

    Ok(())
}

async fn handle_delete(id: &str, store: Arc<SqliteStore>) -> Result<()> {
    let item_store: &dyn ItemRepository = store.as_ref();
    let item_id = ItemId::from(id);
    if item_store.delete(&item_id).await? {
        println!("已删除: {}", id);
    } else {
        println!("未找到 ID 为 {} 的知识", id);
    }

    Ok(())
}

async fn handle_add(
    title: &str,
    summary: &str,
    raw_type: &str,
    store: Arc<SqliteStore>,
) -> Result<()> {
    let item_store: &dyn ItemRepository = store.as_ref();
    let item_type = parse_add_item_type(raw_type)?;
    let item = match item_type {
        ItemType::Knowledge => Item::new_knowledge(title, summary),
        ItemType::Skill => Item::new_skill(title, summary),
        ItemType::Snippet => Item::new_snippet(title, summary),
        ItemType::Observation => Item::new_observation(title, summary),
    };

    item_store.save(&item).await?;
    println!("已添加: {} ({})", item.id().as_str(), item.title());

    Ok(())
}

async fn handle_docs(limit: usize, store: Arc<SqliteStore>) -> Result<()> {
    let doc_store: &dyn DocumentRepository = store.as_ref();
    let total = doc_store.count().await?;
    let docs = doc_store.find_recent(0, limit).await?;

    if docs.is_empty() {
        println!("暂无文档");
    } else {
        println!("共 {} 篇文档:\n", total);
        for doc in &docs {
            let title = doc.title().unwrap_or("(无标题)");
            println!(
                "  {} | {} | {} | {}",
                doc.id().as_str().chars().take(8).collect::<String>(),
                title,
                doc.source(),
                doc.created_at().format("%Y-%m-%d %H:%M"),
            );
        }
    }

    Ok(())
}

async fn handle_doc_show(id: &str, store: Arc<SqliteStore>) -> Result<()> {
    let doc_store: &dyn DocumentRepository = store.as_ref();
    let item_store: &dyn ItemRepository = store.as_ref();
    let doc_id = DocumentId::from(id);
    let doc = doc_store.find_by_id(&doc_id).await?;

    let Some(doc) = doc else {
        println!("未找到 ID 为 {} 的文档", id);
        return Ok(());
    };

    let title = doc.title().unwrap_or("(无标题)");
    println!("ID: {}", doc.id());
    println!("标题: {}", title);
    println!("来源: {}", doc.source());
    println!("URL: {}", doc.url());
    println!("创建: {}", doc.created_at().format("%Y-%m-%d %H:%M"));
    println!("---");
    println!("{}", doc.raw_content());

    let items = item_store.find_by_document_id(&doc_id).await?;
    if !items.is_empty() {
        println!("\n关联知识 ({} 条):\n", items.len());
        for item in &items {
            println!("{}", format_item(item, false));
        }
    }

    Ok(())
}

async fn handle_doc_search(query: &str, limit: usize, store: Arc<SqliteStore>) -> Result<()> {
    let doc_store: &dyn DocumentRepository = store.as_ref();
    let total = doc_store.count_text_hits(query).await?;
    let docs = doc_store.search_text(query, 0, limit).await?;

    if docs.is_empty() {
        println!("未找到匹配的文档");
    } else {
        println!("找到 {} 篇匹配文档:\n", total);
        for doc in &docs {
            let title = doc.title().unwrap_or("(无标题)");
            println!(
                "  {} | {} | {}",
                doc.id().as_str().chars().take(8).collect::<String>(),
                title,
                doc.source(),
            );
        }
    }

    Ok(())
}

fn parse_session_source(raw: &str) -> Option<SessionSource> {
    match raw.to_lowercase().as_str() {
        "claude" | "claude-code" => Some(SessionSource::ClaudeCode),
        "codex" => Some(SessionSource::Codex),
        _ => None,
    }
}

fn parse_add_item_type(raw_type: &str) -> Result<ItemType> {
    parse_item_type(raw_type)
        .with_context(|| format!("无效的类型: {} (支持: knowledge, skill, snippet)", raw_type))
}

#[cfg(test)]
mod tests {
    use super::parse_add_item_type;
    use refine_core::knowledge::ItemType;

    #[test]
    fn parse_add_item_type_accepts_supported_values() {
        let cases = [
            ("knowledge", ItemType::Knowledge),
            ("skill", ItemType::Skill),
            ("snippet", ItemType::Snippet),
        ];

        for (raw, expected) in cases {
            let parsed = parse_add_item_type(raw).expect("expected supported type");
            assert_eq!(parsed, expected);
        }
    }

    #[test]
    fn parse_add_item_type_rejects_unknown_value() {
        let err = parse_add_item_type("invalid").expect_err("expected invalid type error");
        assert!(err.to_string().contains("无效的类型"));
    }
}
