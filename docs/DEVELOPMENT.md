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

当前单元测试覆盖行情解析、当日买入盈亏、交易日历、买卖现金/成本/已实现盈亏和截图校准。

数据维护回归（Python 3 标准库）：

```powershell
python -m unittest discover -s scripts -p "test_*.py" -v
```

研究会计回归需要 pandas：`python -m unittest discover -s analysis -p "test_*.py" -v`。无需训练模型或读取私人账本。CI 分别检查 Rust、维护脚本和研究会计；测试只使用合成数据。

隔离数据目录：设置 `MO_STOCK_APP_DIR`；`mo-stock-watch.exe --validate-data` 只验证，不开窗口。损坏数据不能初始化为示例账户；`portfolio.lock` 是跨进程互斥锁，崩溃遗留时确认没有写入进程后才能手工清理。

## 模块说明

- `src/main.rs`：创建 eframe 窗口。
- `src/app.rs`：应用状态、UI 渲染、用户交互、后台任务轮询。
- `src/portfolio.rs`：持仓模型、市场推断和数据标准化。
- `src/quote.rs`：东方财富行情请求、QuoteBook、盈亏计算。
- `src/config.rs`：持仓和设置的本地 JSON 读写。
- `src/calendar.rs`：交易日判断、内置休市安排和上交所日历更新。
- `src/credential.rs`：使用 Windows 用户凭据加密保护 AI API Key。
- `src/notification.rs`：Windows Toast 通知。
- `src/updater.rs`：GitHub Release 更新检查。
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

- 将本地 Tesseract OCR 改成后台线程，避免 UI 卡顿。
- 真正接入透明度设置，或从设置里移除未使用字段。
- 为交易流水增加 CSV 导入导出。
- 在取得稳定行业字段后增加行业集中度。
