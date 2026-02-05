//! Refine CLI
//!
//! 知识管理命令行工具

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemId, ItemRepository, ItemType};
use refine_core::refinement::{Conversation, Extractor};
use refine_core::search::{SearchEngine, SearchQuery};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "refine")]
#[command(about = "智能知识复用引擎 - 从 AI 对话中提炼知识", long_about = None)]
#[command(version)]
struct Cli {
    /// 数据库路径
    #[arg(long, default_value = "~/.refine/data.db")]
    db: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 从对话中提炼知识
    Extract {
        /// 从标准输入读取
        #[arg(long)]
        stdin: bool,
    },
    /// 搜索知识
    Search {
        /// 搜索关键词
        query: String,
        /// 限制结果数量
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// 列出所有知识
    List {
        /// 按类型过滤 (knowledge, skill, snippet)
        #[arg(short, long)]
        r#type: Option<String>,
        /// 限制数量
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// 显示知识详情
    Show {
        /// 知识 ID
        id: String,
    },
    /// 删除知识
    Delete {
        /// 知识 ID
        id: String,
    },
    /// 添加知识
    Add {
        /// 标题
        #[arg(short, long)]
        title: String,
        /// 摘要
        #[arg(short, long)]
        summary: String,
        /// 类型 (knowledge, skill, snippet)
        #[arg(long, default_value = "knowledge")]
        r#type: String,
    },
}

fn get_db_path(db: &str) -> PathBuf {
    if db.starts_with("~/") {
        dirs::home_dir()
            .map(|h| h.join(&db[2..]))
            .unwrap_or_else(|| PathBuf::from(db))
    } else {
        PathBuf::from(db)
    }
}

fn ensure_db_dir(path: &PathBuf) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn parse_item_type(s: &str) -> Option<ItemType> {
    match s.to_lowercase().as_str() {
        "knowledge" => Some(ItemType::Knowledge),
        "skill" => Some(ItemType::Skill),
        "snippet" => Some(ItemType::Snippet),
        _ => None,
    }
}

fn format_item(item: &Item, verbose: bool) -> String {
    if verbose {
        format!(
            "ID: {}\n类型: {:?}\n标题: {}\n摘要: {}\n标签: {}\n创建: {}\n---\n{}",
            item.id().as_str(),
            item.item_type(),
            item.title(),
            item.summary(),
            item.tags().iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", "),
            item.created_at().format("%Y-%m-%d %H:%M"),
            item.content()
        )
    } else {
        format!(
            "[{:?}] {} - {}",
            item.item_type(),
            item.id().as_str().chars().take(8).collect::<String>(),
            item.title()
        )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("refine=info,refine_core=info")
        .init();

    let cli = Cli::parse();
    let db_path = get_db_path(&cli.db);
    ensure_db_dir(&db_path)?;

    let store = SqliteStore::open(&db_path).context("打开数据库失败")?;
    let store = Arc::new(store);
    let engine = SearchEngine::new(store.clone());

    match cli.command {
        Commands::Extract { stdin } => {
            if stdin {
                let mut content = String::new();
                io::stdin().read_to_string(&mut content)?;

                let conv = Conversation::parse(&content).context("解析对话失败")?;
                println!("解析到 {} 条消息", conv.messages.len());

                let _extractor = Extractor::with_default_policy();
                // 注意：实际提炼需要 LLM，这里只是解析
                println!("对话解析成功，请配置 LLM API 进行提炼");
                println!("\n预览:");
                for (i, msg) in conv.messages.iter().take(3).enumerate() {
                    println!("  [{}] {:?}: {}...", i + 1, msg.role, &msg.content.chars().take(50).collect::<String>());
                }
            } else {
                println!("用法: cat conversation.txt | refine extract --stdin");
            }
        }

        Commands::Search { query, limit } => {
            let results = engine
                .search(SearchQuery::new(&query).with_limit(limit))
                .await?;

            if results.items.is_empty() {
                println!("未找到匹配的知识");
            } else {
                println!("找到 {} 条结果:\n", results.total);
                for hit in results.items {
                    println!("{}", format_item(&hit.item, false));
                }
            }
        }

        Commands::List { r#type, limit } => {
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
        }

        Commands::Show { id } => {
            let item_id = ItemId::from_str(&id);
            match store.find_by_id(&item_id).await? {
                Some(item) => println!("{}", format_item(&item, true)),
                None => println!("未找到 ID 为 {} 的知识", id),
            }
        }

        Commands::Delete { id } => {
            let item_id = ItemId::from_str(&id);
            if store.delete(&item_id).await? {
                println!("已删除: {}", id);
            } else {
                println!("未找到 ID 为 {} 的知识", id);
            }
        }

        Commands::Add { title, summary, r#type } => {
            let item_type = parse_item_type(&r#type).unwrap_or(ItemType::Knowledge);
            let item = match item_type {
                ItemType::Knowledge => Item::new_knowledge(&title, &summary),
                ItemType::Skill => Item::new_skill(&title, &summary),
                ItemType::Snippet => Item::new_snippet(&title, &summary),
            };

            store.save(&item).await?;
            println!("已添加: {} ({})", item.id().as_str(), item.title());
        }
    }

    Ok(())
}
