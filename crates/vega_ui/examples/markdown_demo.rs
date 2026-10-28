use gpui_kit::component::{Root, text::TextView};
use gpui_kit::{App, Context, IntoElement, Render, Window, WindowOptions};

const MARKDOWN: &str = r#"# Markdown 列表展示

## 无序列表

- 第一项：普通文本
- 第二项：包含 **加粗** 和 *斜体*
- 第三项：包含 `行内代码`
- 第四项：[示例链接](https://example.com)
- 这是一段用于查看自动换行的长文本。调整窗口宽度，检查圆点与第一行正文的对齐，以及后续行是否保持一致的正文起点。

## 嵌套列表

- 第一层
  - 第二层
    - 第三层
- 回到第一层

## 有序列表与任务

1. 第一步
2. 第二步

- [ ] 待处理
- [x] 已处理

## 引用

> 这是一段引用，包含 **强调** 和 `代码`。

## 表格

| 名称 | 内容 |
| --- | --- |
| 列表 | 普通与嵌套 |
| 代码 | `let value = 42;` |

## 代码块

```rust
fn main() {
    let value = 42;
}
```
"#;

struct MarkdownDemo;

impl Render for MarkdownDemo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        TextView::markdown("markdown-demo", MARKDOWN)
            .selectable(true)
            .scrollable(true)
    }
}

fn main() {
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);
        let result = cx.open_window(WindowOptions::default(), |window, cx| {
            let demo = cx.new(|_| MarkdownDemo);
            cx.new(|cx| Root::new(demo, window, cx))
        });
        if result.is_err() {
            cx.quit();
        } else {
            cx.activate(true);
        }
    });
}
