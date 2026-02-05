# Refine - 测试策略

> 单元测试、集成测试、E2E 测试规范

---

## 1. 测试金字塔

```
          ┌─────────┐
          │  E2E    │  少量：关键用户流程
         ╱└─────────┘╲
        ╱             ╲
       ╱  ┌───────────┐ ╲
      ╱   │ 集成测试   │  ╲  中等：模块交互
     ╱    └───────────┘   ╲
    ╱                      ╲
   ╱    ┌─────────────────┐ ╲
  ╱     │    单元测试      │  ╲  大量：核心逻辑
 ╱      └─────────────────┘   ╲
╱───────────────────────────────╲
```

---

## 2. Rust 核心库测试

### 2.1 单元测试

每个模块在同文件中包含测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_creation() {
        let item = Item::new_knowledge("Title", "Summary");
        assert_eq!(item.title(), "Title");
        assert_eq!(item.summary(), "Summary");
    }
}
```

**运行**：
```bash
cargo test --package refine-core
```

---

### 2.2 测试覆盖范围

| 模块 | 测试重点 |
|------|----------|
| `knowledge/item.rs` | Item 创建、验证、更新 |
| `knowledge/types.rs` | ItemId、Tag、Source 验证 |
| `refinement/conversation.rs` | 对话解析、多格式支持 |
| `refinement/extractor.rs` | LLM 响应解析 |
| `search/query.rs` | 查询构建、分页 |
| `search/engine.rs` | 搜索结果聚合 |
| `infra/sqlite.rs` | CRUD、FTS 搜索 |
| `infra/llm.rs` | API 调用、错误处理 |

---

### 2.3 测试示例

```rust
// knowledge/item.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_knowledge() {
        let item = Item::new_knowledge("Rust Tips", "Performance best practices");
        assert!(matches!(item.item_type(), ItemType::Knowledge));
        assert!(!item.id().as_str().is_empty());
    }

    #[test]
    fn test_with_tags() {
        let item = Item::new_knowledge("Test", "Summary")
            .with_tags(vec![
                Tag::try_new("rust").unwrap(),
                Tag::try_new("performance").unwrap(),
            ]);
        assert_eq!(item.tags().len(), 2);
    }

    #[test]
    fn test_tag_validation() {
        assert!(Tag::try_new("valid-tag").is_some());
        assert!(Tag::try_new("valid_tag").is_some());
        assert!(Tag::try_new("中文标签").is_some());
        assert!(Tag::try_new("").is_none());  // 空
        assert!(Tag::try_new("a".repeat(51).as_str()).is_none());  // 太长
    }
}
```

---

### 2.4 异步测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sqlite_store() {
        let store = SqliteStore::in_memory().unwrap();
        let item = Item::new_knowledge("Test", "Summary");

        // 保存
        store.save(&item).await.unwrap();

        // 查询
        let found = store.find_by_id(item.id()).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title(), "Test");
    }

    #[tokio::test]
    async fn test_fts_search() {
        let store = SqliteStore::in_memory().unwrap();

        // 添加测试数据
        store.save(&Item::new_knowledge("Rust Error Handling", "Use thiserror")).await.unwrap();
        store.save(&Item::new_knowledge("Python Asyncio", "Async patterns")).await.unwrap();

        // 搜索
        let results = store.search_text("Rust", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].title().contains("Rust"));
    }
}
```

---

## 3. 集成测试

### 3.1 目录结构

```
packages/core/
├── src/
│   └── ...
└── tests/
    ├── integration_search.rs
    ├── integration_extract.rs
    └── common/
        └── mod.rs
```

---

### 3.2 集成测试示例

```rust
// tests/integration_search.rs
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemRepository};
use refine_core::search::{SearchEngine, SearchQuery};
use std::sync::Arc;

#[tokio::test]
async fn test_full_search_flow() {
    // 准备
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let engine = SearchEngine::new(store.clone());

    // 添加数据
    let items = vec![
        Item::new_knowledge("Rust Ownership", "Memory safety without GC"),
        Item::new_skill("Code Review", "Review code for quality"),
        Item::new_snippet("Error Handler", "fn handle_error() { ... }"),
    ];

    for item in &items {
        store.save(item).await.unwrap();
    }

    // 搜索
    let result = engine
        .search(SearchQuery::new("rust").with_limit(10))
        .await
        .unwrap();

    assert_eq!(result.total, 1);
    assert!(result.items[0].item.title().contains("Rust"));
}
```

---

## 4. 前端测试

### 4.1 工具链

| 工具 | 用途 |
|------|------|
| Vitest | 单元测试 |
| React Testing Library | 组件测试 |
| Playwright | E2E 测试 |

---

### 4.2 组件测试示例

```typescript
// src/components/__tests__/Spotlight.test.tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { Spotlight } from '../Spotlight'

describe('Spotlight', () => {
  it('renders when open', () => {
    render(<Spotlight isOpen={true} onClose={() => {}} />)
    expect(screen.getByPlaceholderText(/搜索/)).toBeInTheDocument()
  })

  it('calls onClose when pressing Escape', () => {
    const onClose = vi.fn()
    render(<Spotlight isOpen={true} onClose={onClose} />)

    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalled()
  })

  it('navigates results with arrow keys', () => {
    render(<Spotlight isOpen={true} onClose={() => {}} />)

    const input = screen.getByPlaceholderText(/搜索/)
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    // 验证选中状态
  })
})
```

---

### 4.3 Hook 测试

```typescript
// src/lib/__tests__/store.test.ts
import { renderHook, act } from '@testing-library/react'
import { useStore } from '../store'

describe('useStore', () => {
  it('updates search query', () => {
    const { result } = renderHook(() => useStore())

    act(() => {
      result.current.setQuery('rust')
    })

    expect(result.current.query).toBe('rust')
  })
})
```

---

## 5. E2E 测试

### 5.1 Playwright 配置

```typescript
// playwright.config.ts
export default {
  testDir: './e2e',
  use: {
    baseURL: 'http://localhost:7788',
  },
}
```

---

### 5.2 E2E 测试示例

```typescript
// e2e/search.spec.ts
import { test, expect } from '@playwright/test'

test('search flow', async ({ page }) => {
  await page.goto('/')

  // 打开 Spotlight
  await page.keyboard.press('Meta+k')

  // 搜索
  await page.fill('[placeholder*="搜索"]', 'rust')
  await page.waitForSelector('[data-testid="search-result"]')

  // 选择结果
  await page.keyboard.press('Enter')

  // 验证详情页
  await expect(page.locator('h1')).toContainText('Rust')
})
```

---

## 6. Mock 策略

### 6.1 Rust Mock

```rust
// 使用 mockall
use mockall::predicate::*;
use mockall::mock;

mock! {
    LlmClient {}

    #[async_trait]
    impl LlmClient for LlmClient {
        async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String>;
    }
}

#[tokio::test]
async fn test_extractor_with_mock() {
    let mut mock = MockLlmClient::new();
    mock.expect_complete()
        .returning(|_, _| Ok(r#"{"items": []}"#.to_string()));

    let extractor = Extractor::new(Box::new(mock));
    // 测试...
}
```

---

### 6.2 前端 Mock

```typescript
// 使用 MSW
import { rest } from 'msw'
import { setupServer } from 'msw/node'

const server = setupServer(
  rest.get('http://localhost:19527/health', (req, res, ctx) => {
    return res(ctx.json({ success: true }))
  })
)

beforeAll(() => server.listen())
afterEach(() => server.resetHandlers())
afterAll(() => server.close())
```

---

## 7. CI 配置

```yaml
# .github/workflows/test.yml
name: Test

on: [push, pull_request]

jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace

  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: oven-sh/setup-bun@v1
      - run: bun install
      - run: bun test
```

---

## 8. 运行测试

```bash
# 全部 Rust 测试
cargo test --workspace

# 单个包
cargo test --package refine-core

# 带输出
cargo test -- --nocapture

# 前端测试
cd apps/desktop/ui && bun test

# E2E 测试
cd apps/desktop/ui && bun run e2e
```
