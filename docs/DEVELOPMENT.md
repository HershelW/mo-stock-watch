# 开发说明

## 技术栈

- Rust 2021
- eframe / egui
- reqwest blocking client
- serde / serde_json
- Tesseract 命令行 OCR
- OpenAI 兼容 API

## 常用命令

```powershell
cargo check
cargo test
cargo run
cargo run --release
cargo clean
```

当前没有业务单元测试，`cargo test` 主要用于确认项目能正常编译测试目标。

## 模块说明

- `src/main.rs`：创建 eframe 窗口。
- `src/app.rs`：应用状态、UI 渲染、用户交互、后台任务轮询。
- `src/portfolio.rs`：持仓模型、市场推断和数据标准化。
- `src/quote.rs`：东方财富行情请求、QuoteBook、盈亏计算。
- `src/config.rs`：持仓和设置的本地 JSON 读写。
- `src/ocr.rs`：Tesseract OCR 和启发式文本解析。
- `src/ai.rs`：OpenAI 兼容接口 OCR、模型列表、模型连通性测试。

## 构建缓存

`target/` 是 Rust 构建缓存，Windows debug 构建可能很大，尤其是：

- `target/debug/deps`
- `target/debug/incremental`
- `*.pdb`

这些已被 `.gitignore` 排除。需要释放空间时运行：

```powershell
cargo clean
```

## 数据和密钥

不要提交以下内容：

- `target/`
- `portfolio.json`
- `settings.json`
- API Key
- 截图缓存
- `*.pdb`
- `*.log`

用户数据默认在：

```text
%APPDATA%\mo-stock-watch\
```

## 后续可改进项

- 给 OCR 文本解析加测试样例。
- 将本地 Tesseract OCR 改成后台线程，避免 UI 卡顿。
- 真正接入透明度设置，或从设置里移除未使用字段。
- 在 UI 中显示行情接口返回的股票名称和更新时间。
- 增加导入前预览/确认弹窗，减少误保存概率。
