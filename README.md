# Mo Stock Watch

Mo Stock Watch 是一个轻量的 Windows 桌面 A 股持仓与交易账本小窗，用 Rust + egui/eframe 编写。它面向个人使用：本地维护多账户资产，自动刷新行情，记录交易流水，计算持仓与今日盈亏，并支持从券商截图校准持仓。

## 功能

- 本地维护 A 股持仓：代码、名称、数量、成本价。
- 多行情源刷新现价、昨收、涨跌幅：优先东方财富，失败后自动 fallback 到腾讯、新浪。
- 计算单票持仓盈亏、今日浮盈、总市值、总持仓盈亏、总今日浮盈。
- 记录买入、卖出、分红、费用、入金和出金，自动维护现金、成本和已实现盈亏。
- OCR 导入先展示新旧差异，确认后才校准持仓。
- 按交易所休市日历在 `09:20-11:30`、`13:00-15:00` 自动刷新。
- 到价、涨跌幅、当日亏损和高点回撤提醒，并调用 Windows 通知。
- 自选观察、个股详情、行情缓存和过期提示。
- 每日资产快照、月度收益、沪深 300 对比、最大回撤和仓位集中度。
- 保存前自动备份，保留最近 20 个版本并支持应用内恢复。
- 支持窗口置顶、刷新间隔、字号调整、编辑/保存持仓。
- 支持本地 Tesseract OCR 导入截图草稿。
- 支持 OpenAI 兼容接口的 AI OCR，优先 Responses API，失败后尝试 Chat Completions。
- API Key 使用 Windows 用户凭据加密保存，不再明文写入设置文件。
- 自动检查 GitHub Release。

## 快速运行

需要先安装：

- Rustup / Rust stable
- Visual Studio Build Tools 2022，并勾选 `Desktop development with C++`

开发运行：

```powershell
cargo run
```

Release 运行：

```powershell
cargo run --release
```

也可以双击项目根目录的脚本：

- `start-mo-stock-watch.bat`：启动固定的 `dist\mo-stock-watch.exe`；没有则构建 release。
- `run-release.bat`：构建并启动 release 版本。

如果出现 `link.exe not found` 或 `kernel32.lib not found`，通常是 MSVC Build Tools 或 Windows SDK 没装完整。

## 数据位置

应用数据默认保存在：

- `%APPDATA%\mo-stock-watch\portfolio.json`
- `%APPDATA%\mo-stock-watch\settings.json`
- `%APPDATA%\mo-stock-watch\credentials.dat`
- `%APPDATA%\mo-stock-watch\quote_cache.json`
- `%APPDATA%\mo-stock-watch\market_holidays.json`
- `%APPDATA%\mo-stock-watch\backups\`
- `%APPDATA%\mo-stock-watch\clipboard_ocr.png`

这些文件不在仓库里。移动源码目录不会丢持仓；换电脑时需要迁移上述数据文件。

## v0.2.1 数据安全修复

存档损坏会停止加载，不再回退示例持仓；保存有互斥锁、版本冲突检查和上一版恢复文件。缺失或过期行情不会写入资产快照。未知成交信息显示“待核”，不会等同于零收益。

截图维护使用 [单命令流程](docs/PORTFOLIO_OPS.md)，同时备份、更新持仓和派生账本、校验并重启。历史证据与真实成交分开；CSV 按需导出，不再每次重建 Excel。开发回归与研究局限见 [审计修复说明](docs/AUDIT_FIXES.md)。

## OCR

OCR 是可选能力。识别完成后会显示账户、原数量、新数量、原成本和新成本，只有点击确认才会写入持仓。默认不会删除截图中没有出现的股票。

本地 OCR 需要安装 Tesseract，并确保 `tesseract.exe` 在 PATH 中，或安装在常见目录：

- `C:\Program Files\Tesseract-OCR\tesseract.exe`
- `C:\Program Files (x86)\Tesseract-OCR\tesseract.exe`

AI OCR 在应用内填写 API Key、Base URL 和模型后使用。更多说明见 [docs/OCR.md](docs/OCR.md)。

## 开发

常用检查：

```powershell
cargo check
cargo test
```

`target/` 是 Rust 编译缓存和构建产物，可能达到数 GB，已被 `.gitignore` 排除，不会上传 GitHub。需要清理时运行：

```powershell
cargo clean
```

更多开发说明见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 仓库内容

```text
src/
  app.rs        主窗口状态、UI、交互和后台任务轮询
  ai.rs         OpenAI 兼容接口 OCR、模型列表、模型测试
  config.rs     本地配置和持仓文件读写
  main.rs       eframe 入口
  ocr.rs        Tesseract OCR 和文本解析
  portfolio.rs  持仓模型、市场推断、数据清洗
  quote.rs      东方财富行情获取和盈亏计算
docs/
  USAGE.md       使用说明
  LEDGER.md      交易流水、成本和盈亏口径
  OCR.md         OCR/AI OCR 配置说明
  DEVELOPMENT.md 开发和维护说明
  RELEASE.md     发布和迁移说明
```

## 注意

这是个人桌面辅助工具，不构成投资建议。行情数据来自第三方公开接口，可能延迟、失败或字段变化；交易决策请以券商和交易所数据为准。

## License

MIT License. See [LICENSE](LICENSE).
