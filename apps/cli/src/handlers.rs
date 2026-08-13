use crate::cli::{Commands, IngestProvider};
use crate::ingest_sessions::{handle_ingest_sessions, IngestOptions};
use crate::insights::{handle_insights, InsightsOptions};
use crate::support::{build_llm_client_from_env, format_item, parse_item_type};
use anyhow::{Context, Result};
use refine_core::infra::{LlmClient, SqliteStore};
use refine_core::knowledge::{
    DocumentId, DocumentRepository, Item, ItemId, ItemRepository, ItemType, Source,
};
use refine_core::refinement::{
    extract_document_with_strict_defaults, persist_extracted_document, ExtractionPolicy,
    ItemExtractionInput,
};
use refine_core::search::{SearchEngine, SearchQuery};
use refine_core::session::SessionSource;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;

pub async fn run(
    command: Commands,
    store: Arc<SqliteStore>,
    engine: Arc<SearchEngine>,
    db_path: &Path,
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
            provider,
            limit,
            latest,
            dry_run,
            legacy_local_scan,
            retry_quarantined,
            backfill_session_metadata,
        } => {
            let provider = IngestProvider::resolve(provider, legacy_local_scan)?;
            let source_filter = source
                .as_deref()
                .map(|raw| {
                    parse_session_source(raw)
                        .with_context(|| format!("invalid session source {raw:?}"))
                })
                .transpose()?;
            if source_filter.is_some() && provider != IngestProvider::Local {
                anyhow::bail!(
                    "--source requires --provider local because remem does not expose a trustworthy Claude/Codex source"
                );
            }
            if legacy_local_scan {
                eprintln!("warning: --legacy-local-scan is deprecated; use --provider local");
            }
            if backfill_session_metadata && provider != IngestProvider::Local {
                anyhow::bail!("--backfill-session-metadata requires --provider local");
            }
            let llm_client = if dry_run || backfill_session_metadata {
                None
            } else {
                Some(build_llm_client_from_env()?)
            };
            let doc_store: Arc<dyn DocumentRepository> = store.clone();
            handle_ingest_sessions(
                IngestOptions {
                    source: source_filter,
                    provider,
                    limit,
                    latest,
                    dry_run,
                    retry_quarantined,
                    backfill_session_metadata,
                },
                db_path,
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
        Commands::Growth => {
            anyhow::bail!("'refine growth' has been removed. Use 'mirror dashboard' instead.");
        }
        Commands::Explore => {
            anyhow::bail!("'refine explore' has been removed. Use 'mirror score' instead.");
        }
        Commands::DeepInquiry => {
            anyhow::bail!("'refine deep-inquiry' has been removed. Use 'mirror score' instead.");
        }
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
    let items =
        extract_and_persist_cli_content(&content, store.as_ref(), llm_client.as_ref()).await?;
    println!("提炼完成：{} 条", items.len());
    for item in &items {
        println!(
            "  + [{}] {} ({})",
            format!("{:?}", item.item_type()).to_lowercase(),
            item.title(),
            item.id()
        );
    }

    Ok(())
}

async fn extract_and_persist_cli_content(
    content: &str,
    store: &SqliteStore,
    llm_client: &dyn LlmClient,
) -> Result<Vec<Item>> {
    let source = Source::new("cli");
    let input = ItemExtractionInput {
        source: "cli",
        title: None,
        raw_content: content,
        captured_at: None,
        policy: ExtractionPolicy::default(),
    };
    let aggregate = extract_document_with_strict_defaults(llm_client, &input, &source)
        .await
        .context("提炼失败")?;

    let doc_store: &dyn DocumentRepository = store;
    persist_extracted_document(doc_store, &aggregate)
        .await
        .context("保存提炼结果失败")?;
    Ok(aggregate.items)
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
    use super::{extract_and_persist_cli_content, parse_add_item_type};
    use async_trait::async_trait;
    use refine_core::error::InfraResult;
    use refine_core::infra::{LlmClient, SqliteStore};
    use refine_core::knowledge::{DocumentRepository, ItemRepository, ItemType};
    use tempfile::tempdir;

    struct FakeLlm;

    #[async_trait]
    impl LlmClient for FakeLlm {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            Ok(r#"{"items":[{"type":"knowledge","title":"CLI item","summary":"S","content":"C","tags":[]}]}"#.to_string())
        }
    }

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

    #[tokio::test]
    async fn cli_extract_persists_document_and_items_together() {
        let temp = tempdir().unwrap();
        let store = SqliteStore::open(&temp.path().join("refine.db")).unwrap();

        let items =
            extract_and_persist_cli_content("Human: hello\nAssistant: world", &store, &FakeLlm)
                .await
                .unwrap();

        assert_eq!(items.len(), 1);
        let doc_id = items[0].document_id().unwrap();
        assert!(DocumentRepository::find_by_id(&store, doc_id)
            .await
            .unwrap()
            .is_some());
        assert_eq!(ItemRepository::count_items(&store, None).await.unwrap(), 1);
    }
}
